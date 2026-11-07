//! Per-app snapshot backups (doc 09 part B, "Snapshot"): `VACUUM INTO` a consistent copy under
//! `<data>/backups/<app>/b<UTC timestamp>.db`. The writer keeps running while it is taken.
//!
//! v1: manual create, list, download, delete, and restore into a NEW app. Not yet: schedules,
//! retention, zstd, S3, WAL shipping, encryption, in-place restore. Backups outlive the app
//! on purpose: delete the files to discard them.

use std::path::PathBuf;

use monolith_store::AppStore;
use serde::Serialize;

use crate::app::now_us;
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult};
use crate::schema::valid_name;
use crate::system::{AppInfo, Registry};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// `b20261107134512123.db`
    pub name: String,
    pub bytes: u64,
    /// RFC 3339, taken from the name.
    pub created: String,
}

/// `b` + 17 digits (`YYYYMMDDhhmmssmmm`) + `.db`; the only names that reach the filesystem.
fn valid_backup(name: &str) -> bool {
    name.len() == 21
        && name.starts_with('b')
        && name.ends_with(".db")
        && name[1..18].bytes().all(|b| b.is_ascii_digit())
}

fn created_of(name: &str) -> String {
    let d = &name[1..18];
    format!(
        "{}-{}-{}T{}:{}:{}.{}Z",
        &d[0..4],
        &d[4..6],
        &d[6..8],
        &d[8..10],
        &d[10..12],
        &d[12..14],
        &d[14..17]
    )
}

fn io(e: std::io::Error) -> CoreError {
    match e.kind() {
        std::io::ErrorKind::NotFound => CoreError::NotFound("backup"),
        _ => CoreError::Internal(e.to_string()),
    }
}

impl Registry {
    fn backup_dir(&self, app: &str) -> PathBuf {
        self.data_dir().join("backups").join(app)
    }

    fn backup_path(&self, app: &str, name: &str) -> CoreResult<PathBuf> {
        if !valid_name(app) || !valid_backup(name) {
            return Err(CoreError::NotFound("backup"));
        }
        Ok(self.backup_dir(app).join(name))
    }

    /// Takes a snapshot of a loaded or idle app.
    pub async fn create_backup(&self, app: &str) -> CoreResult<BackupInfo> {
        let a = self.app(app).await?;
        let dir = self.backup_dir(app);
        let d = dir.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(d))
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?
            .map_err(io)?;
        let digits: String = format_us(now_us())
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        let name = format!("b{}.db", &digits[..17]);
        let info = a.store().snapshot(dir.join(&name)).await?;
        Ok(BackupInfo {
            created: created_of(&name),
            name,
            bytes: info.bytes,
        })
    }

    /// Newest first. Missing directory means no backups.
    pub async fn list_backups(&self, app: &str) -> CoreResult<Vec<BackupInfo>> {
        if !valid_name(app) {
            return Err(CoreError::NotFound("app"));
        }
        let dir = self.backup_dir(app);
        let mut out = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<BackupInfo>> {
            let mut v = Vec::new();
            for e in std::fs::read_dir(dir)? {
                let e = e?;
                let name = e.file_name().to_string_lossy().into_owned();
                if valid_backup(&name) {
                    v.push(BackupInfo {
                        created: created_of(&name),
                        bytes: e.metadata()?.len(),
                        name,
                    });
                }
            }
            Ok(v)
        })
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(Vec::new())
            } else {
                Err(e)
            }
        })
        .map_err(io)?;
        out.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(out)
    }

    pub async fn delete_backup(&self, app: &str, name: &str) -> CoreResult<()> {
        let p = self.backup_path(app, name)?;
        tokio::task::spawn_blocking(move || std::fs::remove_file(p))
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?
            .map_err(io)
    }

    /// Opens a backup for streaming: `(file, byte length)`.
    pub async fn open_backup(&self, app: &str, name: &str) -> CoreResult<(std::fs::File, u64)> {
        let p = self.backup_path(app, name)?;
        tokio::task::spawn_blocking(move || {
            let f = std::fs::File::open(p)?;
            let len = f.metadata()?.len();
            Ok((f, len))
        })
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?
        .map_err(io)
    }

    /// Creates app `new_app` from a backup of `app`. The new app starts from that snapshot's
    /// data and schema; the source app is untouched.
    pub async fn restore_backup(
        &self,
        app: &str,
        name: &str,
        new_app: &str,
    ) -> CoreResult<AppInfo> {
        let src = self.backup_path(app, name)?;
        if !valid_name(new_app) {
            return Err(CoreError::BadRequest(format!(
                "invalid app name '{new_app}'"
            )));
        }
        let dest_dir = self.app_dir(new_app);
        // Reserve the name first so two restores cannot race onto one directory.
        let info = self.reserve_app(new_app).await?;
        let copy = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&dest_dir)?;
            std::fs::copy(&src, dest_dir.join("data.db")).map(drop)
        })
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?;
        if let Err(e) = copy {
            let _ = self.delete_app(new_app).await;
            return Err(io(e));
        }
        self.app(new_app).await?;
        Ok(info)
    }
}
