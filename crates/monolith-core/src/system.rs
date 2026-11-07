//! Control plane (`/data/system.db`): apps, superusers, server secrets (doc 03 §2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use monolith_auth::{
    Claims, hash_password, hex, random_bytes, sign_token, verify_password, verify_token,
};
use monolith_store::id::new_id;
use monolith_store::{
    AppStore, Error as StoreError, Id, MigrationPlan, Mutation, Op, Query, SqliteConfig,
    SqliteStore, Value, WriteOp,
};

use crate::app::{App, now_us};
use crate::auth_config::{AuthConfigIn, AuthConfigView, StoredAuth};
use crate::error::{CoreError, CoreResult};
use crate::metrics::Metrics;
use crate::schema::valid_name;

pub const SUPERUSER_TOKEN_TTL_US: i64 = 3600 * 1_000_000;
pub const MIN_PASSWORD: usize = 10;
/// Lifetime of the one-time installer token; a restart issues a fresh one.
pub const INSTALL_TOKEN_TTL_US: i64 = 3600 * 1_000_000;

const SYSTEM_DDL: &str = "
CREATE TABLE IF NOT EXISTS apps (name TEXT PRIMARY KEY, created INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS superusers (
  id BLOB PRIMARY KEY, email TEXT NOT NULL UNIQUE, hash TEXT NOT NULL, created INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value BLOB NOT NULL) STRICT;
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub name: String,
    pub created_us: i64,
}

pub struct Registry {
    dir: PathBuf,
    sys: SqliteStore,
    apps: RwLock<HashMap<String, Arc<App>>>,
    open_lock: tokio::sync::Mutex<()>,
    token_key: Vec<u8>,
    /// Verified against when the email is unknown, so timing does not reveal accounts.
    dummy_hash: String,
    /// One-time installer token `(token, expires µs)`, memory only (doc 18 §2).
    install: tokio::sync::Mutex<Option<(String, i64)>>,
    /// All HTTP traffic plus every app's record ops (in memory, since start).
    pub metrics: Arc<Metrics>,
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn blocking_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Internal(e.to_string())
}

fn sys_delete(sql: &str, params: Vec<Value>) -> WriteOp {
    let mut w = sys_write(sql, [0; 16], params);
    w.mutations[0].op = Op::Delete;
    w
}

fn sys_write(sql: &str, rid: Id, params: Vec<Value>) -> WriteOp {
    WriteOp {
        actor: None,
        mutations: vec![Mutation {
            op: Op::Insert,
            coll: 0,
            rid,
            fields: None,
            sql: sql.into(),
            params,
            seq_slot: None,
            hlc_slot: None,
            post_sql: Vec::new(),
        }],
    }
}

impl Registry {
    /// Opens the data directory (creating it on first start).
    pub async fn open(dir: &Path) -> CoreResult<Registry> {
        let d = dir.to_owned();
        let sys = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(d.join("apps"))
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let cfg = SqliteConfig {
                readers: 2,
                ..SqliteConfig::default()
            };
            SqliteStore::open(&d.join("system.db"), &cfg).map_err(CoreError::from)
        })
        .await
        .map_err(blocking_err)??;
        sys.apply_schema(MigrationPlan {
            statements: vec![SYSTEM_DDL.into()],
        })
        .await?;
        let key = random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?;
        // Keeps the existing key on restart; NotFound means another start already set it.
        match sys
            .write(sys_write(
                "INSERT INTO settings (key, value) VALUES ('token_key', ?) ON CONFLICT DO NOTHING",
                [0; 16],
                vec![Value::Blob(key.into())],
            ))
            .await
        {
            Ok(_) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        let rows = sys
            .query(Query {
                sql: "SELECT value FROM settings WHERE key = 'token_key'".into(),
                params: vec![],
            })
            .await?;
        let Some(Value::Blob(token_key)) = rows.first().and_then(|r| r.first()) else {
            return Err(CoreError::Internal("missing token_key".into()));
        };
        let dummy_hash = tokio::task::spawn_blocking(|| hash_password("dummy password for timing"))
            .await
            .map_err(blocking_err)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(Registry {
            dir: dir.to_owned(),
            sys,
            apps: RwLock::new(HashMap::new()),
            open_lock: tokio::sync::Mutex::new(()),
            token_key: token_key.to_vec(),
            dummy_hash,
            install: tokio::sync::Mutex::new(None),
            metrics: Arc::new(Metrics::new()),
        })
    }

    pub(crate) fn app_dir(&self, name: &str) -> PathBuf {
        self.dir.join("apps").join(name)
    }

    // ---------- apps ----------

    pub async fn list_apps(&self) -> CoreResult<Vec<AppInfo>> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT name, created FROM apps ORDER BY name".into(),
                params: vec![],
            })
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| match (&r[0], &r[1]) {
                (Value::Text(n), Value::Int(c)) => Some(AppInfo {
                    name: n.to_string(),
                    created_us: *c,
                }),
                _ => None,
            })
            .collect())
    }

    pub async fn app_info(&self, name: &str) -> CoreResult<AppInfo> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT name, created FROM apps WHERE name = ?".into(),
                params: vec![Value::Text(name.into())],
            })
            .await?;
        match rows.first().map(|r| (&r[0], &r[1])) {
            Some((Value::Text(n), Value::Int(c))) => Ok(AppInfo {
                name: n.to_string(),
                created_us: *c,
            }),
            _ => Err(CoreError::NotFound("app")),
        }
    }

    pub async fn create_app(&self, name: &str) -> CoreResult<AppInfo> {
        if !valid_name(name) {
            return Err(CoreError::BadRequest(format!("invalid app name '{name}'")));
        }
        let now = now_us();
        self.sys
            .write(sys_write(
                "INSERT INTO apps (name, created) VALUES (?, ?)",
                new_id(),
                vec![Value::Text(name.into()), Value::Int(now)],
            ))
            .await
            .map_err(|e| match e {
                StoreError::Constraint(_) => CoreError::Conflict(format!("app '{name}' exists")),
                e => e.into(),
            })?;
        self.app(name).await?;
        Ok(AppInfo {
            name: name.to_owned(),
            created_us: now,
        })
    }

    /// Already-opened app, without touching disk.
    /// Ends every realtime stream so a graceful shutdown does not wait on them.
    pub fn close_realtime(&self) {
        for a in self.apps.read().unwrap_or_else(|e| e.into_inner()).values() {
            a.hub.close();
        }
    }

    pub fn loaded_app(&self, name: &str) -> Option<Arc<App>> {
        self.apps
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
    }

    /// The running app, opened on first use.
    pub async fn app(&self, name: &str) -> CoreResult<Arc<App>> {
        if let Some(a) = self.loaded_app(name) {
            return Ok(a);
        }
        let _guard = self.open_lock.lock().await;
        if let Some(a) = self
            .apps
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return Ok(a.clone());
        }
        if !valid_name(name) {
            return Err(CoreError::NotFound("app"));
        }
        self.app_info(name).await?;
        let dir = self.app_dir(name);
        tokio::task::spawn_blocking({
            let dir = dir.clone();
            move || std::fs::create_dir_all(dir)
        })
        .await
        .map_err(blocking_err)?
        .map_err(|e| CoreError::Internal(e.to_string()))?;
        let app = Arc::new(App::open(name, &dir.join("data.db"), self.metrics.clone()).await?);
        self.apps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_owned(), app.clone());
        Ok(app)
    }

    /// Irreversible: closes the app and removes its data directory.
    pub async fn delete_app(&self, name: &str) -> CoreResult<()> {
        let _guard = self.open_lock.lock().await;
        self.app_info(name).await?;
        self.apps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        self.sys
            .write(WriteOp {
                actor: None,
                mutations: vec![Mutation {
                    op: Op::Delete,
                    coll: 0,
                    rid: [0; 16],
                    fields: None,
                    sql: "DELETE FROM apps WHERE name = ?".into(),
                    params: vec![Value::Text(name.into())],
                    seq_slot: None,
                    hlc_slot: None,
                    post_sql: Vec::new(),
                }],
            })
            .await?;
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM settings WHERE key = ?",
                vec![Value::Text(format!("auth:{name}").into())],
            ))
            .await
        {
            Ok(_) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        let dir = self.app_dir(name);
        tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .await
        .map_err(blocking_err)?
        .map_err(|e| CoreError::Internal(e.to_string()))
    }

    // ---------- per-app auth configuration ----------

    async fn stored_auth(&self, app: &str) -> CoreResult<StoredAuth> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT value FROM settings WHERE key = ?".into(),
                params: vec![Value::Text(format!("auth:{app}").into())],
            })
            .await?;
        match rows.first().and_then(|r| r.first()) {
            Some(Value::Blob(b)) => serde_json::from_slice(b)
                .map_err(|e| CoreError::Internal(format!("auth config for {app}: {e}"))),
            _ => Ok(StoredAuth::default()),
        }
    }

    pub async fn auth_config(&self, app: &str) -> CoreResult<AuthConfigView> {
        self.app_info(app).await?;
        Ok(self.stored_auth(app).await?.view(app))
    }

    pub async fn put_auth_config(
        &self,
        app: &str,
        input: AuthConfigIn,
    ) -> CoreResult<AuthConfigView> {
        self.app_info(app).await?;
        let next = self.stored_auth(app).await?.apply(input)?;
        let bytes = serde_json::to_vec(&next).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [0; 16],
                vec![
                    Value::Text(format!("auth:{app}").into()),
                    Value::Blob(bytes.into()),
                ],
            ))
            .await?;
        Ok(next.view(app))
    }

    // ---------- superusers ----------

    pub async fn superuser_count(&self) -> CoreResult<i64> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT count(*) FROM superusers".into(),
                params: vec![],
            })
            .await?;
        Ok(match rows.first().and_then(|r| r.first()) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        })
    }

    pub async fn create_superuser(&self, email: &str, password: &str) -> CoreResult<Id> {
        let email = email.trim().to_ascii_lowercase();
        if !email.contains('@') || email.len() > 254 {
            return Err(CoreError::Validation(vec![crate::FieldError::new(
                "email",
                "invalid_format",
                "not an email address",
            )]));
        }
        if password.chars().count() < MIN_PASSWORD {
            return Err(CoreError::Validation(vec![crate::FieldError::new(
                "password",
                "too_short",
                format!("at least {MIN_PASSWORD} characters"),
            )]));
        }
        let pw = password.to_owned();
        let hash = tokio::task::spawn_blocking(move || hash_password(&pw))
            .await
            .map_err(blocking_err)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let id = new_id();
        self.sys
            .write(sys_write(
                "INSERT INTO superusers (id, email, hash, created) VALUES (?, ?, ?, ?)",
                id,
                vec![
                    Value::Blob(id.into()),
                    Value::Text(email.into()),
                    Value::Text(hash.into()),
                    Value::Int(now_us()),
                ],
            ))
            .await
            .map_err(|e| match e {
                StoreError::Constraint(_) => {
                    CoreError::Conflict("a superuser with this email exists".into())
                }
                e => e.into(),
            })?;
        Ok(id)
    }

    /// Issues the one-time installer token when no superuser exists (PocketBase-style
    /// setup URL). It lives in memory only and is printed to the server log, so only
    /// someone with console or log access can create the first superuser.
    pub async fn issue_install_token(&self) -> CoreResult<Option<String>> {
        let mut slot = self.install.lock().await;
        if self.superuser_count().await? > 0 {
            *slot = None;
            return Ok(None);
        }
        let t = hex(&random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?);
        *slot = Some((t.clone(), now_us() + INSTALL_TOKEN_TTL_US));
        Ok(Some(t))
    }

    /// True while no superuser exists.
    pub async fn install_required(&self) -> CoreResult<bool> {
        Ok(self.superuser_count().await? == 0)
    }

    /// Creates the first superuser with the installer token and signs them in.
    /// The token is consumed on success; any token is rejected once a superuser exists.
    pub async fn install(
        &self,
        token: &str,
        email: &str,
        password: &str,
    ) -> CoreResult<(String, i64)> {
        let mut slot = self.install.lock().await;
        let valid = matches!(&*slot, Some((t, exp))
            if now_us() < *exp && ct_eq(t.as_bytes(), token.as_bytes()));
        if !valid || self.superuser_count().await? > 0 {
            return Err(CoreError::Unauthorized);
        }
        let sub = self.create_superuser(email, password).await?;
        *slot = None;
        let exp_us = now_us() + SUPERUSER_TOKEN_TTL_US;
        Ok((sign_token(&self.token_key, Claims { sub, exp_us }), exp_us))
    }

    /// Returns `(token, expires µs)`; wrong email and wrong password look the same.
    pub async fn login(&self, email: &str, password: &str) -> CoreResult<(String, i64)> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT id, hash FROM superusers WHERE email = ?".into(),
                params: vec![Value::Text(email.trim().to_ascii_lowercase().into())],
            })
            .await?;
        let (id, hash) = match rows.first().map(|r| (&r[0], &r[1])) {
            Some((Value::Blob(id), Value::Text(h))) => {
                (<[u8; 16]>::try_from(&id[..]).ok(), h.to_string())
            }
            _ => (None, self.dummy_hash.clone()),
        };
        let pw = password.to_owned();
        let ok = tokio::task::spawn_blocking(move || verify_password(&pw, &hash))
            .await
            .map_err(blocking_err)?;
        match (ok, id) {
            (true, Some(sub)) => {
                let exp_us = now_us() + SUPERUSER_TOKEN_TTL_US;
                Ok((sign_token(&self.token_key, Claims { sub, exp_us }), exp_us))
            }
            _ => Err(CoreError::Unauthorized),
        }
    }

    /// No DB lookup (doc 08 §2).
    // shortcut: no revocation list yet; deleting a superuser takes effect when the token expires (1 h).
    pub fn verify_superuser(&self, token: &str) -> Option<Id> {
        verify_token(&self.token_key, token, now_us()).map(|c| c.sub)
    }
}
