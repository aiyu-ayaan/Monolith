//! Storage buckets (doc 13): named containers of objects per app.
//!
//! Layout under `<data>/apps/<app>/files/`: `files.db` holds bucket config and one row per object
//! (`bucket`, `key`, blob id, size, mime, sha-256 etag); the bytes live in `<bucket>/<blob id>`.
//! Bucket config is mirrored in memory, so the upload path reads no config from the DB.
//!
//! v1 limits: one blob per object version (not content-addressed or deduplicated), local disk
//! only, declared MIME types (no sniffing), no thumbnails, no per-record file fields, not part
//! of backups. Bucket usage counts scan the object index (admin views only).

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use monolith_auth::hex;
use monolith_store::id::{encode_string, new_id};
use monolith_store::{
    AppStore, Error as StoreError, MigrationPlan, Query, SqliteConfig, SqliteStore, Value,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::now_us;
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult, FieldError};
use crate::schema::valid_name;
use crate::system::{Registry, sys_delete, sys_write};

pub const DEFAULT_MAX_SIZE: u64 = 100 << 20;
pub const MAX_OBJECT_SIZE: u64 = 10 << 30;
pub const MAX_KEY: usize = 512;
const MAX_MIME_RULES: usize = 32;
const MAX_LIST: usize = 1000;

const DDL: &str = "
CREATE TABLE IF NOT EXISTS buckets (
  name TEXT PRIMARY KEY, public INTEGER NOT NULL, max_size INTEGER NOT NULL,
  mime TEXT NOT NULL, created INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS objects (
  bucket TEXT NOT NULL, key TEXT NOT NULL, blob TEXT NOT NULL, size INTEGER NOT NULL,
  mime TEXT NOT NULL, etag TEXT NOT NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL,
  PRIMARY KEY (bucket, key)
) STRICT, WITHOUT ROWID;
";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BucketIn {
    pub name: String,
    /// Anyone who knows an object's key can read it. Listing and writes still need a credential.
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub max_size: Option<u64>,
    /// `image/png`, `image/*`; empty allows every type.
    #[serde(default)]
    pub allowed_mime: Vec<String>,
}

/// A bucket's config as held in memory.
#[derive(Debug)]
pub struct Bucket {
    pub name: String,
    pub public: bool,
    pub max_size: u64,
    pub allowed_mime: Vec<String>,
    pub created_us: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketInfo {
    pub name: String,
    pub public: bool,
    pub max_size: u64,
    pub allowed_mime: Vec<String>,
    pub created: String,
    pub objects: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub mime: String,
    /// SHA-256 of the content, hex.
    pub etag: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPage {
    pub items: Vec<ObjectInfo>,
    pub next_cursor: Option<String>,
}

fn invalid(field: &str, code: &'static str, msg: impl Into<String>) -> CoreError {
    CoreError::Validation(vec![FieldError::new(field, code, msg)])
}

fn io(e: std::io::Error) -> CoreError {
    CoreError::Internal(e.to_string())
}

fn join(e: tokio::task::JoinError) -> CoreError {
    CoreError::Internal(e.to_string())
}

/// 1..=512 bytes of UTF-8 without control characters, backslashes, a leading or doubled `/`,
/// or `.`/`..` segments. Keys never reach the filesystem (blobs are named by id), but the same
/// rules keep URLs and exports unambiguous.
pub fn valid_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= MAX_KEY
        && !k.chars().any(|c| c.is_control() || c == '\\')
        && k.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

fn mime_ok(rules: &[String], mime: &str) -> bool {
    if rules.is_empty() {
        return true;
    }
    let base = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    rules.iter().any(|r| match r.strip_suffix("/*") {
        Some(group) => base.split('/').next() == Some(group),
        None => *r == base,
    })
}

fn valid_mime(m: &str) -> bool {
    m.len() <= 255
        && m.bytes().all(|b| (0x20..0x7f).contains(&b))
        && m.split(';').next().is_some_and(|t| {
            t.trim().split_once('/').is_some_and(|(a, b)| {
                !a.is_empty() && !b.is_empty() && !b.contains('/') && !t.contains(' ')
            })
        })
}

fn validate_bucket(b: &BucketIn) -> CoreResult<()> {
    if !valid_name(&b.name) {
        return Err(invalid(
            "name",
            "invalid_format",
            "lowercase letters, digits and _, starting with a letter",
        ));
    }
    if b.max_size.is_some_and(|m| m == 0 || m > MAX_OBJECT_SIZE) {
        return Err(invalid(
            "maxSize",
            "out_of_range",
            format!("1 to {MAX_OBJECT_SIZE} bytes"),
        ));
    }
    if b.allowed_mime.len() > MAX_MIME_RULES
        || !b.allowed_mime.iter().all(|m| {
            m == &m.to_ascii_lowercase()
                && (m
                    .strip_suffix("/*")
                    .is_some_and(|g| !g.is_empty() && !g.contains('/'))
                    || valid_mime(m) && !m.contains(';'))
        })
    {
        return Err(invalid(
            "allowedMime",
            "invalid_format",
            "up to 32 lowercase types like image/png or image/*",
        ));
    }
    Ok(())
}

fn text(v: &Value) -> String {
    match v {
        Value::Text(t) => t.to_string(),
        _ => String::new(),
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        _ => 0,
    }
}

fn object_of(r: &[Value]) -> ObjectInfo {
    ObjectInfo {
        key: text(&r[0]),
        size: int(&r[1]).max(0) as u64,
        mime: text(&r[2]),
        etag: text(&r[3]),
        created: format_us(int(&r[4])),
        updated: format_us(int(&r[5])),
    }
}

const OBJ_COLS: &str = "key, size, mime, etag, created, updated";

/// One app's storage: its `files.db`, bucket cache and blob directory.
pub struct Files {
    dir: PathBuf,
    store: SqliteStore,
    buckets: RwLock<HashMap<String, Arc<Bucket>>>,
    /// Serializes metadata commits (read old blob, upsert, delete) so overwrites never orphan a blob.
    commit: tokio::sync::Mutex<()>,
}

/// A streaming upload being written to a temp file. Feed it from a blocking thread.
pub struct Upload {
    file: File,
    tmp: PathBuf,
    hasher: Sha256,
    size: u64,
    max: u64,
    bucket: Arc<Bucket>,
    key: String,
    mime: String,
    done: bool,
}

impl Upload {
    pub fn write(&mut self, chunk: &[u8]) -> CoreResult<()> {
        self.size += chunk.len() as u64;
        if self.size > self.max {
            return Err(CoreError::TooLarge(format!(
                "object over the bucket limit of {} bytes",
                self.max
            )));
        }
        self.hasher.update(chunk);
        self.file.write_all(chunk).map_err(io)
    }

    /// Makes the bytes durable and returns what [`Files::commit`] needs.
    pub fn finish(mut self) -> CoreResult<Finished> {
        self.file.sync_data().map_err(io)?;
        self.done = true;
        Ok(Finished {
            tmp: std::mem::take(&mut self.tmp),
            etag: hex(&std::mem::take(&mut self.hasher).finalize()),
            size: self.size,
            bucket: self.bucket.clone(),
            key: std::mem::take(&mut self.key),
            mime: std::mem::take(&mut self.mime),
        })
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        if !self.done {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

/// A fully received upload waiting for its metadata commit.
pub struct Finished {
    tmp: PathBuf,
    etag: String,
    size: u64,
    bucket: Arc<Bucket>,
    key: String,
    mime: String,
}

impl Drop for Finished {
    fn drop(&mut self) {
        // After a successful commit the file was renamed away; this only cleans failures.
        let _ = std::fs::remove_file(&self.tmp);
    }
}

impl Files {
    async fn open(dir: PathBuf) -> CoreResult<Files> {
        let d = dir.clone();
        let store = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(d.join(".tmp")).map_err(io)?;
            // Leftovers of uploads cut off by a crash.
            if let Ok(rd) = std::fs::read_dir(d.join(".tmp")) {
                rd.flatten().for_each(|e| {
                    let _ = std::fs::remove_file(e.path());
                });
            }
            let cfg = SqliteConfig {
                readers: 2,
                ..SqliteConfig::default()
            };
            SqliteStore::open(&d.join("files.db"), &cfg).map_err(CoreError::from)
        })
        .await
        .map_err(join)??;
        store
            .apply_schema(MigrationPlan {
                statements: vec![DDL.into()],
            })
            .await?;
        let rows = store
            .query(Query {
                sql: "SELECT name, public, max_size, mime, created FROM buckets".into(),
                params: vec![],
            })
            .await?;
        let buckets = rows
            .iter()
            .map(|r| {
                let b = Bucket {
                    name: text(&r[0]),
                    public: int(&r[1]) != 0,
                    max_size: int(&r[2]).max(0) as u64,
                    allowed_mime: serde_json::from_str(&text(&r[3])).unwrap_or_default(),
                    created_us: int(&r[4]),
                };
                (b.name.clone(), Arc::new(b))
            })
            .collect();
        Ok(Files {
            dir,
            store,
            buckets: RwLock::new(buckets),
            commit: tokio::sync::Mutex::new(()),
        })
    }

    /// Cached bucket config; no DB access.
    pub fn bucket(&self, name: &str) -> CoreResult<Arc<Bucket>> {
        self.buckets
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
            .ok_or(CoreError::NotFound("bucket"))
    }

    fn blob_path(&self, bucket: &str, blob: &str) -> PathBuf {
        self.dir.join(bucket).join(blob)
    }

    fn info(b: &Bucket, objects: u64, bytes: u64) -> BucketInfo {
        BucketInfo {
            name: b.name.clone(),
            public: b.public,
            max_size: b.max_size,
            allowed_mime: b.allowed_mime.clone(),
            created: format_us(b.created_us),
            objects,
            bytes,
        }
    }

    pub async fn list_buckets(&self) -> CoreResult<Vec<BucketInfo>> {
        let rows = self
            .store
            .query(Query {
                sql: "SELECT bucket, count(*), sum(size) FROM objects GROUP BY bucket".into(),
                params: vec![],
            })
            .await?;
        let usage: HashMap<String, (u64, u64)> = rows
            .iter()
            .map(|r| (text(&r[0]), (int(&r[1]) as u64, int(&r[2]).max(0) as u64)))
            .collect();
        let mut out: Vec<_> = self
            .buckets
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|b| {
                let (n, bytes) = usage.get(&b.name).copied().unwrap_or_default();
                Self::info(b, n, bytes)
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub async fn bucket_info(&self, name: &str) -> CoreResult<BucketInfo> {
        let b = self.bucket(name)?;
        let rows = self
            .store
            .query(Query {
                sql: "SELECT count(*), coalesce(sum(size), 0) FROM objects WHERE bucket = ?".into(),
                params: vec![Value::Text(name.into())],
            })
            .await?;
        let (n, bytes) = rows
            .first()
            .map_or((0, 0), |r| (int(&r[0]) as u64, int(&r[1]) as u64));
        Ok(Self::info(&b, n, bytes))
    }

    /// Creates a bucket (`create`) or replaces the config of an existing one.
    pub async fn put_bucket(&self, input: BucketIn, create: bool) -> CoreResult<BucketInfo> {
        validate_bucket(&input)?;
        let _g = self.commit.lock().await;
        let old = self.bucket(&input.name).ok();
        match (&old, create) {
            (Some(_), true) => {
                return Err(CoreError::Conflict(format!(
                    "bucket '{}' exists",
                    input.name
                )));
            }
            (None, false) => return Err(CoreError::NotFound("bucket")),
            _ => {}
        }
        let b = Bucket {
            name: input.name,
            public: input.public,
            max_size: input.max_size.unwrap_or(DEFAULT_MAX_SIZE),
            allowed_mime: input.allowed_mime,
            created_us: old.as_ref().map_or_else(now_us, |o| o.created_us),
        };
        let mime = serde_json::to_string(&b.allowed_mime)
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let dir = self.dir.join(&b.name);
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(dir))
            .await
            .map_err(join)?
            .map_err(io)?;
        self.store
            .write(sys_write(
                "INSERT INTO buckets (name, public, max_size, mime, created) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(name) DO UPDATE SET public = excluded.public, max_size = excluded.max_size, mime = excluded.mime",
                new_id(),
                vec![
                    Value::Text(b.name.as_str().into()),
                    Value::Int(i64::from(b.public)),
                    Value::Int(b.max_size as i64),
                    Value::Text(mime.into()),
                    Value::Int(b.created_us),
                ],
            ))
            .await?;
        let name = b.name.clone();
        self.buckets
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.clone(), Arc::new(b));
        drop(_g);
        self.bucket_info(&name).await
    }

    /// A non-empty bucket needs `force`, which deletes its objects too.
    pub async fn delete_bucket(&self, name: &str, force: bool) -> CoreResult<()> {
        let _g = self.commit.lock().await;
        self.bucket(name)?;
        let rows = self
            .store
            .query(Query {
                sql: "SELECT 1 FROM objects WHERE bucket = ? LIMIT 1".into(),
                params: vec![Value::Text(name.into())],
            })
            .await?;
        if !rows.is_empty() && !force {
            return Err(CoreError::Conflict(
                "the bucket has objects; delete them or pass force=true".into(),
            ));
        }
        for sql in [
            "DELETE FROM objects WHERE bucket = ?",
            "DELETE FROM buckets WHERE name = ?",
        ] {
            match self
                .store
                .write(sys_delete(sql, vec![Value::Text(name.into())]))
                .await
            {
                Ok(_) | Err(StoreError::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.buckets
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        let dir = self.dir.join(name);
        tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .await
        .map_err(join)?
        .map_err(io)
    }

    /// Validates the target and opens a temp file for the body.
    pub async fn begin_upload(&self, bucket: &str, key: &str, mime: &str) -> CoreResult<Upload> {
        let b = self.bucket(bucket)?;
        if !valid_key(key) {
            return Err(CoreError::BadRequest(
                "object key: 1 to 512 bytes, no empty, '.' or '..' segments, no control characters or backslashes".into(),
            ));
        }
        if !valid_mime(mime) || !mime_ok(&b.allowed_mime, mime) {
            return Err(CoreError::BadRequest(format!(
                "content type '{mime}' is not allowed in bucket '{bucket}'"
            )));
        }
        let tmp = self.dir.join(".tmp").join(encode_string(&new_id()));
        let t = tmp.clone();
        let file = tokio::task::spawn_blocking(move || File::create(t))
            .await
            .map_err(join)?
            .map_err(io)?;
        Ok(Upload {
            file,
            tmp,
            hasher: Sha256::new(),
            size: 0,
            max: b.max_size,
            bucket: b,
            key: key.to_owned(),
            mime: mime.to_owned(),
            done: false,
        })
    }

    /// Publishes a received upload under its key, replacing any previous object.
    pub async fn commit(&self, up: Finished) -> CoreResult<ObjectInfo> {
        let blob = encode_string(&new_id());
        let dest = self.blob_path(&up.bucket.name, &blob);
        let (src, d) = (up.tmp.clone(), dest.clone());
        tokio::task::spawn_blocking(move || std::fs::rename(src, d))
            .await
            .map_err(join)?
            .map_err(io)?;
        let now = now_us();
        let _g = self.commit.lock().await;
        let res = async {
            // The bucket may have been deleted while the body was uploading.
            if self.bucket(&up.bucket.name).is_err() {
                return Err(CoreError::NotFound("bucket"));
            }
            let old = self
                .store
                .query(Query {
                    sql: "SELECT blob, created FROM objects WHERE bucket = ? AND key = ?".into(),
                    params: vec![
                        Value::Text(up.bucket.name.as_str().into()),
                        Value::Text(up.key.as_str().into()),
                    ],
                })
                .await?;
            let created = old.first().map_or(now, |r| int(&r[1]));
            self.store
                .write(sys_write(
                    "INSERT INTO objects (bucket, key, blob, size, mime, etag, created, updated) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(bucket, key) DO UPDATE SET \
                     blob = excluded.blob, size = excluded.size, mime = excluded.mime, \
                     etag = excluded.etag, updated = excluded.updated",
                    new_id(),
                    vec![
                        Value::Text(up.bucket.name.as_str().into()),
                        Value::Text(up.key.as_str().into()),
                        Value::Text(blob.as_str().into()),
                        Value::Int(up.size as i64),
                        Value::Text(up.mime.as_str().into()),
                        Value::Text(up.etag.as_str().into()),
                        Value::Int(created),
                        Value::Int(now),
                    ],
                ))
                .await?;
            Ok((old.first().map(|r| text(&r[0])), created))
        }
        .await;
        drop(_g);
        match res {
            Ok((old, created)) => {
                if let Some(old) = old {
                    let p = self.blob_path(&up.bucket.name, &old);
                    tokio::task::spawn_blocking(move || std::fs::remove_file(p));
                }
                Ok(ObjectInfo {
                    key: up.key.clone(),
                    size: up.size,
                    mime: up.mime.clone(),
                    etag: up.etag.clone(),
                    created: format_us(created),
                    updated: format_us(now),
                })
            }
            Err(e) => {
                let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(dest)).await;
                Err(e)
            }
        }
    }

    async fn find(&self, bucket: &str, key: &str) -> CoreResult<(ObjectInfo, String)> {
        let rows = self
            .store
            .query(Query {
                sql: format!("SELECT {OBJ_COLS}, blob FROM objects WHERE bucket = ? AND key = ?")
                    .into(),
                params: vec![Value::Text(bucket.into()), Value::Text(key.into())],
            })
            .await?;
        rows.first()
            .map(|r| (object_of(r), text(&r[6])))
            .ok_or(CoreError::NotFound("object"))
    }

    pub async fn object_info(&self, bucket: &str, key: &str) -> CoreResult<ObjectInfo> {
        self.bucket(bucket)?;
        Ok(self.find(bucket, key).await?.0)
    }

    /// Metadata plus the open blob. The file stays readable even if the object is replaced meanwhile.
    pub async fn open_object(&self, bucket: &str, key: &str) -> CoreResult<(ObjectInfo, File)> {
        self.bucket(bucket)?;
        let (info, blob) = self.find(bucket, key).await?;
        let p = self.blob_path(bucket, &blob);
        let f = tokio::task::spawn_blocking(move || File::open(p))
            .await
            .map_err(join)?
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => CoreError::NotFound("object"),
                _ => io(e),
            })?;
        Ok((info, f))
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> CoreResult<()> {
        self.bucket(bucket)?;
        let _g = self.commit.lock().await;
        let (_, blob) = self.find(bucket, key).await?;
        self.store
            .write(sys_delete(
                "DELETE FROM objects WHERE bucket = ? AND key = ?",
                vec![Value::Text(bucket.into()), Value::Text(key.into())],
            ))
            .await?;
        drop(_g);
        let p = self.blob_path(bucket, &blob);
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(p)).await;
        Ok(())
    }

    /// Keyset page ordered by key. `cursor` is the last key of the previous page.
    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> CoreResult<ObjectPage> {
        self.bucket(bucket)?;
        let limit = limit.clamp(1, MAX_LIST);
        // Binary collation: every key with this prefix sorts below prefix + U+10FFFF.
        let upper = format!("{prefix}\u{10FFFF}");
        let rows = self
            .store
            .query(Query {
                sql: format!(
                    "SELECT {OBJ_COLS} FROM objects WHERE bucket = ?1 AND key >= ?2 AND key < ?3 \
                     AND key > ?4 ORDER BY key LIMIT ?5"
                )
                .into(),
                params: vec![
                    Value::Text(bucket.into()),
                    Value::Text(prefix.into()),
                    Value::Text(upper.into()),
                    Value::Text(cursor.unwrap_or("").into()),
                    Value::Int(limit as i64 + 1),
                ],
            })
            .await?;
        let mut items: Vec<_> = rows.iter().map(|r| object_of(r)).collect();
        let next_cursor = (items.len() > limit).then(|| {
            items.truncate(limit);
            items[limit - 1].key.clone()
        });
        Ok(ObjectPage { items, next_cursor })
    }
}

impl Registry {
    /// The app's storage, opened on first use.
    pub async fn files(&self, app: &str) -> CoreResult<Arc<Files>> {
        if let Some(f) = self
            .files
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(app)
        {
            return Ok(f.clone());
        }
        let _guard = self.open_lock.lock().await;
        if let Some(f) = self
            .files
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(app)
        {
            return Ok(f.clone());
        }
        self.app_info(app).await?;
        let f = Arc::new(Files::open(self.app_dir(app).join("files")).await?);
        self.files
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(app.to_owned(), f.clone());
        Ok(f)
    }
}
