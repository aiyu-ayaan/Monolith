//! Storage buckets (doc 13): named containers of objects per app.
//!
//! Layout under `<data>/apps/<app>/files/`: `files.db` holds bucket config and usage counters, one
//! row per object (`bucket`, `key`, blob hash, size, MIME) and a reference count per blob; the
//! bytes live content-addressed in `blobs/<h[0..2]>/<blake3 hex>`, shared by every object with the
//! same content in the app (dedupe). A blob is deleted when its last reference goes.
//!
//! Uploads are hashed (BLAKE3) while streaming to a temp file; the real MIME type is sniffed from
//! the first bytes. Bucket config is mirrored in memory, so the upload path reads no config from the
//! DB; usage counters are updated in the same commit as the object row.
//!
//! Image thumbnails of declared sizes are made on demand on a bounded pool and cached beside the
//! blob (`thumbs/<hash>/<size>.jpg|png`); they go away with the blob.
//!
//! Limits: local disk only, metadata commits are serialized per app.

use futures_core::Stream as _;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use futures_util::StreamExt;
use monolith_store::id::{encode_string, new_id};
use monolith_store::{
    AppStore, Error as StoreError, MigrationPlan, Mutation, Query, SqliteConfig, SqliteStore,
    Value, WriteOp,
};
use object_store::path::Path as ObjPath;
use object_store::{GetOptions, GetRange, ObjectStore, PutPayload, WriteMultipart};
use serde::{Deserialize, Serialize};

use crate::app::now_us;
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult, FieldError};
use crate::schema::valid_name;
use crate::system::{Registry, ensure_columns, sys_delete, sys_write};
use crate::thumbs::{self, Spec};

pub const DEFAULT_MAX_SIZE: u64 = 100 << 20;
pub const MAX_OBJECT_SIZE: u64 = 10 << 30;
pub const MAX_KEY: usize = 512;
const MAX_MIME_RULES: usize = 32;
const MAX_THUMB_SIZES: usize = 16;
/// Largest image fetched from a remote store to make a thumbnail.
const REMOTE_THUMB_SOURCE_MAX: u64 = 64 << 20;
const MAX_LIST: usize = 1000;
/// Bytes of an upload kept to sniff its type.
const SNIFF_BYTES: usize = 8192;
/// An unreferenced blob file younger than this may belong to a commit in flight.
const GC_GRACE: std::time::Duration = std::time::Duration::from_secs(600);

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
CREATE TABLE IF NOT EXISTS blobs (
  hash TEXT PRIMARY KEY, size INTEGER NOT NULL, refs INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
";

/// Columns added after the first layout: `(name, declaration)`.
const BUCKET_COLUMNS: [(&str, &str); 4] = [
    ("thumbs", "TEXT"),
    ("max_bytes", "INTEGER"),
    ("objects", "INTEGER NOT NULL DEFAULT 0"),
    ("bytes", "INTEGER NOT NULL DEFAULT 0"),
];

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
    /// Total bytes the bucket may hold (logical size, before dedupe); omit for no limit.
    #[serde(default)]
    pub max_bytes: Option<u64>,
    /// Thumbnail sizes clients may request with `?thumb=`: `100x100` (crop), `0x300` / `300x0`
    /// (keep aspect), `400x400f` (fit inside). Only these are generated.
    #[serde(default)]
    pub thumb_sizes: Vec<String>,
}

/// A bucket's config as held in memory.
#[derive(Debug)]
pub struct Bucket {
    pub name: String,
    pub public: bool,
    pub max_size: u64,
    pub allowed_mime: Vec<String>,
    pub max_bytes: Option<u64>,
    pub thumb_sizes: Vec<String>,
    pub created_us: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketInfo {
    pub name: String,
    pub public: bool,
    pub max_size: u64,
    pub allowed_mime: Vec<String>,
    pub max_bytes: Option<u64>,
    pub thumb_sizes: Vec<String>,
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
    if b.thumb_sizes.len() > MAX_THUMB_SIZES
        || b.thumb_sizes.iter().any(|t| Spec::parse(t).is_none())
        || b.thumb_sizes.iter().collect::<HashSet<_>>().len() != b.thumb_sizes.len()
    {
        return Err(invalid(
            "thumbSizes",
            "invalid_format",
            format!(
                "up to {MAX_THUMB_SIZES} distinct sizes like 100x100, 0x300 or 400x400f (sides up to {})",
                thumbs::MAX_SIDE
            ),
        ));
    }
    if b.max_bytes == Some(0) {
        return Err(invalid("maxBytes", "out_of_range", "at least 1 byte"));
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

/// Where blob bytes live. Metadata (`files.db`) and thumbnails stay on local disk either way.
#[derive(Clone)]
pub enum Backend {
    Local,
    /// An S3-compatible bucket (or any `object_store`), keys `blobs/<h[0..2]>/<hash>`.
    Remote(Arc<dyn ObjectStore>),
}

fn obj_path(hash: &str) -> ObjPath {
    ObjPath::from(format!("blobs/{}/{hash}", &hash[..2]))
}

fn remote_io(e: object_store::Error) -> CoreError {
    match e {
        object_store::Error::NotFound { .. } => CoreError::NotFound("object"),
        e => CoreError::Internal(format!("object store: {e}")),
    }
}

/// An open blob, readable in ranges.
pub enum Blob {
    Local(File),
    Remote {
        store: Arc<dyn ObjectStore>,
        path: ObjPath,
    },
}

/// Chunks of a blob range, ready for an HTTP body.
pub struct BlobStream(StreamInner);

enum StreamInner {
    Local(tokio::sync::mpsc::Receiver<std::io::Result<bytes::Bytes>>),
    Remote(futures_util::stream::BoxStream<'static, std::io::Result<bytes::Bytes>>),
}

impl futures_core::Stream for BlobStream {
    type Item = std::io::Result<bytes::Bytes>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match &mut self.0 {
            StreamInner::Local(rx) => rx.poll_recv(cx),
            StreamInner::Remote(s) => s.poll_next_unpin(cx),
        }
    }
}

impl Blob {
    /// `len` bytes from `offset`. Local files are read in 256 KiB chunks on a blocking thread with
    /// at most 4 chunks buffered, so a slow client applies backpressure to the disk; remote blobs
    /// are streamed from the object store with a range request.
    pub async fn read(self, offset: u64, len: u64) -> CoreResult<BlobStream> {
        match self {
            Blob::Local(mut file) => {
                let (tx, rx) = tokio::sync::mpsc::channel(4);
                tokio::task::spawn_blocking(move || {
                    use std::io::{Seek, SeekFrom};
                    if offset > 0
                        && let Err(e) = file.seek(SeekFrom::Start(offset))
                    {
                        let _ = tx.blocking_send(Err(e));
                        return;
                    }
                    let mut left = len;
                    while left > 0 {
                        let mut buf = vec![0u8; left.min(256 * 1024) as usize];
                        match file.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                buf.truncate(n);
                                left -= n as u64;
                                if tx.blocking_send(Ok(bytes::Bytes::from(buf))).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = tx.blocking_send(Err(e));
                                break;
                            }
                        }
                    }
                });
                Ok(BlobStream(StreamInner::Local(rx)))
            }
            Blob::Remote { store, path } => {
                let opts = GetOptions {
                    range: Some(GetRange::Bounded(offset..offset + len)),
                    ..GetOptions::default()
                };
                let res = store.get_opts(&path, opts).await.map_err(remote_io)?;
                Ok(BlobStream(StreamInner::Remote(
                    res.into_stream()
                        .map(|r| r.map_err(std::io::Error::other))
                        .boxed(),
                )))
            }
        }
    }

    /// The whole blob in memory: for tests and small objects.
    pub async fn read_all(self, len: u64) -> CoreResult<Vec<u8>> {
        let mut s = self.read(0, len).await?;
        let mut out = Vec::new();
        while let Some(c) =
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut s).poll_next(cx)).await
        {
            out.extend_from_slice(&c.map_err(io)?);
        }
        Ok(out)
    }
}

/// One app's storage: its `files.db`, bucket cache and blob directory.
pub struct Files {
    dir: PathBuf,
    backend: Backend,
    store: SqliteStore,
    buckets: RwLock<HashMap<String, Arc<Bucket>>>,
    /// Serializes metadata commits (read old object, write, release blobs) so counters and
    /// reference counts never race. Held for a few statements, never during byte transfer.
    commit: tokio::sync::Mutex<()>,
    /// Bounds concurrent thumbnail jobs across all apps.
    thumb_pool: Arc<tokio::sync::Semaphore>,
    /// One lock per thumbnail being made, so concurrent requests for it share one job.
    thumb_flight: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// A streaming upload being written to a temp file. Feed it from a blocking thread.
pub struct Upload {
    file: File,
    tmp: PathBuf,
    hasher: blake3::Hasher,
    head: Vec<u8>,
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
        if self.head.len() < SNIFF_BYTES {
            let n = (SNIFF_BYTES - self.head.len()).min(chunk.len());
            self.head.extend_from_slice(&chunk[..n]);
        }
        self.hasher.update(chunk);
        self.file.write_all(chunk).map_err(io)
    }

    /// Makes the bytes durable, settles the content type and returns what [`Files::commit`] needs.
    /// The sniffed type wins over the declared one; the bucket's type list is checked against it.
    pub fn finish(mut self) -> CoreResult<Finished> {
        let declared = std::mem::take(&mut self.mime);
        let mime = match infer::get(&self.head) {
            Some(t) if declared.split(';').next().map(str::trim) != Some(t.mime_type()) => {
                t.mime_type().to_owned()
            }
            _ => declared,
        };
        if !mime_ok(&self.bucket.allowed_mime, &mime) {
            return Err(CoreError::BadRequest(format!(
                "content type '{mime}' is not allowed in bucket '{}'",
                self.bucket.name
            )));
        }
        self.file.sync_data().map_err(io)?;
        self.done = true;
        Ok(Finished {
            tmp: std::mem::take(&mut self.tmp),
            hash: self.hasher.finalize().to_hex().to_string(),
            size: self.size,
            bucket: self.bucket.clone(),
            key: std::mem::take(&mut self.key),
            mime,
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
    hash: String,
    size: u64,
    bucket: Arc<Bucket>,
    key: String,
    mime: String,
}

impl Drop for Finished {
    fn drop(&mut self) {
        // After a successful commit the file was renamed or removed; this only cleans failures.
        let _ = std::fs::remove_file(&self.tmp);
    }
}

/// What a collection pass found.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcReport {
    /// Unreferenced blob files deleted.
    pub removed_files: u64,
    pub removed_bytes: u64,
    /// Blobs the database references but whose file is gone (data loss; reported, not repaired).
    pub missing_files: u64,
    /// Thumbnail directories of blobs that no longer exist.
    pub removed_thumb_dirs: u64,
}

fn mutation(sql: &str, params: Vec<Value>) -> Mutation {
    sys_write(sql, [0; 16], params).mutations.remove(0)
}

fn write_op(mutations: Vec<Mutation>) -> WriteOp {
    WriteOp {
        actor: None,
        mutations,
    }
}

/// Hard link, or a copy when the two paths are on different file systems.
pub(crate) fn link_or_copy(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) => std::fs::copy(from, to).map(drop),
    }
}

/// Recreates `src/blobs/**` under `dst` with [`link_or_copy`].
pub(crate) fn link_tree(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    for shard in std::fs::read_dir(src.join("blobs"))? {
        let shard = shard?;
        for blob in std::fs::read_dir(shard.path())? {
            let blob = blob?;
            link_or_copy(
                &blob.path(),
                &dst.join("blobs")
                    .join(shard.file_name())
                    .join(blob.file_name()),
            )?;
        }
    }
    Ok(())
}

/// `blobs/ab/abcdef…`
fn blob_rel(hash: &str) -> PathBuf {
    PathBuf::from("blobs").join(&hash[..2]).join(hash)
}

fn valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Files {
    pub(crate) async fn open(
        dir: PathBuf,
        backend: Backend,
        thumb_pool: Arc<tokio::sync::Semaphore>,
    ) -> CoreResult<Files> {
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
        let added = ensure_columns(&store, "buckets", &BUCKET_COLUMNS).await?;
        let rows = store
            .query(Query {
                sql: "SELECT name, public, max_size, mime, created, max_bytes, thumbs FROM buckets"
                    .into(),
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
                    max_bytes: match &r[5] {
                        Value::Int(i) => Some((*i).max(0) as u64),
                        _ => None,
                    },
                    thumb_sizes: serde_json::from_str(&text(&r[6])).unwrap_or_default(),
                };
                (b.name.clone(), Arc::new(b))
            })
            .collect();
        let f = Files {
            dir,
            backend,
            store,
            buckets: RwLock::new(buckets),
            commit: tokio::sync::Mutex::new(()),
            thumb_pool,
            thumb_flight: std::sync::Mutex::new(HashMap::new()),
        };
        f.migrate_legacy(added).await?;
        Ok(f)
    }

    /// Upgrades data written before blobs were content-addressed (`<bucket>/<id>` files) and
    /// before buckets had usage counters. Runs once; later opens find nothing to do.
    async fn migrate_legacy(&self, counters_added: bool) -> CoreResult<()> {
        let rows = self
            .store
            .query(Query {
                sql: "SELECT bucket, key, blob, size FROM objects WHERE length(blob) != 64".into(),
                params: vec![],
            })
            .await?;
        for r in &rows {
            let (bucket, key, id) = (text(&r[0]), text(&r[1]), text(&r[2]));
            let dir = self.dir.clone();
            let src = dir.join(&bucket).join(&id);
            let moved =
                tokio::task::spawn_blocking(move || -> std::io::Result<Option<(String, u64)>> {
                    let Ok(mut f) = File::open(&src) else {
                        return Ok(None);
                    };
                    let (mut h, mut buf, mut n) =
                        (blake3::Hasher::new(), vec![0u8; 256 * 1024], 0u64);
                    loop {
                        let k = f.read(&mut buf)?;
                        if k == 0 {
                            break;
                        }
                        h.update(&buf[..k]);
                        n += k as u64;
                    }
                    let hash = h.finalize().to_hex().to_string();
                    let dest = dir.join(blob_rel(&hash));
                    if dest.exists() {
                        std::fs::remove_file(&src)?;
                    } else {
                        std::fs::create_dir_all(dest.parent().unwrap_or(&dir))?;
                        std::fs::rename(&src, &dest)?;
                    }
                    Ok(Some((hash, n)))
                })
                .await
                .map_err(join)?
                .map_err(io)?;
            match moved {
                Some((hash, n)) => {
                    self.store
                        .write(write_op(vec![
                            mutation(
                                "INSERT INTO blobs (hash, size, refs) VALUES (?, ?, 1) \
                                 ON CONFLICT(hash) DO UPDATE SET refs = refs + 1",
                                vec![Value::Text(hash.as_str().into()), Value::Int(n as i64)],
                            ),
                            mutation(
                                "UPDATE objects SET blob = ?1, etag = ?1 WHERE bucket = ?2 AND key = ?3",
                                vec![
                                    Value::Text(hash.as_str().into()),
                                    Value::Text(bucket.as_str().into()),
                                    Value::Text(key.as_str().into()),
                                ],
                            ),
                        ]))
                        .await?;
                }
                // The bytes are gone: drop the row rather than serve a 404 for a listed object.
                None => {
                    self.store
                        .write(sys_delete(
                            "DELETE FROM objects WHERE bucket = ? AND key = ?",
                            vec![
                                Value::Text(bucket.as_str().into()),
                                Value::Text(key.as_str().into()),
                            ],
                        ))
                        .await?;
                }
            }
        }
        if counters_added || !rows.is_empty() {
            self.store
                .write(sys_write(
                    "UPDATE buckets SET \
                       objects = (SELECT count(*) FROM objects WHERE bucket = buckets.name), \
                       bytes = (SELECT coalesce(sum(size), 0) FROM objects WHERE bucket = buckets.name)",
                    [0; 16],
                    vec![],
                ))
                .await
                .map(drop)
                .or_else(|e| match e {
                    StoreError::NotFound => Ok(()),
                    e => Err(e),
                })?;
        }
        Ok(())
    }

    /// Whether any blob is referenced (objects exist).
    pub async fn has_blobs(&self) -> CoreResult<bool> {
        let rows = self
            .store
            .query(Query {
                sql: "SELECT 1 FROM blobs LIMIT 1".into(),
                params: vec![],
            })
            .await?;
        Ok(!rows.is_empty())
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

    fn blob_path(&self, hash: &str) -> PathBuf {
        self.dir.join(blob_rel(hash))
    }

    fn info(b: &Bucket, objects: u64, bytes: u64) -> BucketInfo {
        BucketInfo {
            name: b.name.clone(),
            public: b.public,
            max_size: b.max_size,
            allowed_mime: b.allowed_mime.clone(),
            max_bytes: b.max_bytes,
            thumb_sizes: b.thumb_sizes.clone(),
            created: format_us(b.created_us),
            objects,
            bytes,
        }
    }

    pub async fn list_buckets(&self) -> CoreResult<Vec<BucketInfo>> {
        let rows = self
            .store
            .query(Query {
                sql: "SELECT name, objects, bytes FROM buckets".into(),
                params: vec![],
            })
            .await?;
        let usage: HashMap<String, (u64, u64)> = rows
            .iter()
            .map(|r| {
                (
                    text(&r[0]),
                    (int(&r[1]).max(0) as u64, int(&r[2]).max(0) as u64),
                )
            })
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
        let (n, bytes) = self.usage(name).await?;
        Ok(Self::info(&b, n, bytes))
    }

    /// `(objects, bytes)` from the bucket's counters: one primary-key read.
    async fn usage(&self, name: &str) -> CoreResult<(u64, u64)> {
        let rows = self
            .store
            .query(Query {
                sql: "SELECT objects, bytes FROM buckets WHERE name = ?".into(),
                params: vec![Value::Text(name.into())],
            })
            .await?;
        rows.first()
            .map(|r| (int(&r[0]).max(0) as u64, int(&r[1]).max(0) as u64))
            .ok_or(CoreError::NotFound("bucket"))
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
            max_bytes: input.max_bytes,
            thumb_sizes: input.thumb_sizes,
            created_us: old.as_ref().map_or_else(now_us, |o| o.created_us),
        };
        let mime = serde_json::to_string(&b.allowed_mime)
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let sizes = serde_json::to_string(&b.thumb_sizes)
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        self.store
            .write(sys_write(
                "INSERT INTO buckets (name, public, max_size, mime, created, max_bytes, thumbs) \
                 VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(name) DO UPDATE SET public = excluded.public, \
                 max_size = excluded.max_size, mime = excluded.mime, max_bytes = excluded.max_bytes, \
                 thumbs = excluded.thumbs",
                new_id(),
                vec![
                    Value::Text(b.name.as_str().into()),
                    Value::Int(i64::from(b.public)),
                    Value::Int(b.max_size as i64),
                    Value::Text(mime.into()),
                    Value::Int(b.created_us),
                    b.max_bytes.map_or(Value::Null, |m| Value::Int(m as i64)),
                    Value::Text(sizes.into()),
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
        let held = self
            .store
            .query(Query {
                sql: "SELECT blob, count(*) FROM objects WHERE bucket = ? GROUP BY blob".into(),
                params: vec![Value::Text(name.into())],
            })
            .await?;
        if !held.is_empty() && !force {
            return Err(CoreError::Conflict(
                "the bucket has objects; delete them or pass force=true".into(),
            ));
        }
        let n = Value::Text(name.into());
        let mut ms: Vec<Mutation> = held
            .iter()
            .map(|r| {
                mutation(
                    "UPDATE blobs SET refs = refs - ? WHERE hash = ?",
                    vec![Value::Int(int(&r[1])), Value::Text(text(&r[0]).into())],
                )
            })
            .collect();
        ms.push(mutation(
            "DELETE FROM buckets WHERE name = ?",
            vec![n.clone()],
        ));
        let mut ms_op = write_op(ms);
        if !held.is_empty() {
            // The bucket row is deleted above; this runs first so a failure keeps both.
            ms_op
                .mutations
                .insert(0, mutation("DELETE FROM objects WHERE bucket = ?", vec![n]));
        }
        self.store.write(ms_op).await?;
        self.buckets
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        self.reap().await
    }

    /// Deletes blobs nothing references any more (row and file). Callers hold the commit lock,
    /// so two passes never delete the same row.
    async fn reap(&self) -> CoreResult<()> {
        let dead = self
            .store
            .query(Query {
                sql: "SELECT hash FROM blobs WHERE refs <= 0".into(),
                params: vec![],
            })
            .await?;
        if dead.is_empty() {
            return Ok(());
        }
        let hashes: Vec<String> = dead.iter().map(|r| text(&r[0])).collect();
        self.store
            .write(write_op(
                hashes
                    .iter()
                    .map(|h| {
                        mutation(
                            "DELETE FROM blobs WHERE hash = ? AND refs <= 0",
                            vec![Value::Text(h.as_str().into())],
                        )
                    })
                    .collect(),
            ))
            .await?;
        let valid: Vec<&String> = hashes.iter().filter(|h| valid_hash(h)).collect();
        if let Backend::Remote(store) = &self.backend {
            for h in &valid {
                match store.delete(&obj_path(h)).await {
                    Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                    Err(e) => return Err(remote_io(e)),
                }
            }
        }
        let local = matches!(self.backend, Backend::Local);
        let paths: Vec<PathBuf> = valid
            .iter()
            .filter(|_| local)
            .map(|h| self.blob_path(h))
            .collect();
        let thumb_dirs: Vec<PathBuf> = valid
            .iter()
            .map(|h| self.dir.join("thumbs").join(h))
            .collect();
        tokio::task::spawn_blocking(move || {
            paths.iter().for_each(|p| drop(std::fs::remove_file(p)));
            thumb_dirs
                .iter()
                .for_each(|d| drop(std::fs::remove_dir_all(d)));
        })
        .await
        .map_err(join)
    }

    /// Validates the target and opens a temp file for the body.
    pub async fn begin_upload(&self, bucket: &str, key: &str, mime: &str) -> CoreResult<Upload> {
        let b = self.bucket(bucket)?;
        if !valid_key(key) {
            return Err(CoreError::BadRequest(
                "object key: 1 to 512 bytes, no empty, '.' or '..' segments, no control characters or backslashes".into(),
            ));
        }
        if !valid_mime(mime) {
            return Err(CoreError::BadRequest(format!(
                "invalid content type '{mime}'"
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
            hasher: blake3::Hasher::new(),
            head: Vec::with_capacity(SNIFF_BYTES),
            size: 0,
            max: b.max_size,
            bucket: b,
            key: key.to_owned(),
            mime: mime.to_owned(),
            done: false,
        })
    }

    /// Publishes a received upload under its key, replacing any previous object. Identical
    /// content already in the app is stored once.
    pub async fn commit(&self, up: Finished) -> CoreResult<ObjectInfo> {
        let bucket = up.bucket.name.clone();
        if let Backend::Remote(store) = &self.backend {
            // The slow part runs outside the commit lock.
            self.put_remote(store, &up.hash, &up.tmp, up.size).await?;
        }
        let _g = self.commit.lock().await;
        let usage = self.usage(&bucket).await?; // also fails if the bucket vanished meanwhile
        let old = self
            .store
            .query(Query {
                sql: "SELECT blob, size, created FROM objects WHERE bucket = ? AND key = ?".into(),
                params: vec![
                    Value::Text(bucket.as_str().into()),
                    Value::Text(up.key.as_str().into()),
                ],
            })
            .await?;
        let (old_hash, old_size, created) = old.first().map_or((None, 0, None), |r| {
            (
                Some(text(&r[0])),
                int(&r[1]).max(0) as u64,
                Some(int(&r[2])),
            )
        });
        if let Some(cap) = up.bucket.max_bytes
            && usage.1 - old_size + up.size > cap
        {
            return Err(CoreError::TooLarge(format!(
                "bucket '{bucket}' would exceed its {cap} byte quota"
            )));
        }
        // Place the bytes first; an unreferenced blob is harmless, a missing one is not.
        let placed = match &self.backend {
            Backend::Local => {
                let (src, dest) = (up.tmp.clone(), self.blob_path(&up.hash));
                tokio::task::spawn_blocking(move || -> std::io::Result<bool> {
                    if dest.exists() {
                        return std::fs::remove_file(&src).map(|()| false);
                    }
                    std::fs::create_dir_all(dest.parent().unwrap_or(&dest))?;
                    std::fs::rename(&src, &dest).map(|()| true)
                })
                .await
                .map_err(join)?
                .map_err(io)?
            }
            // Uploaded before the lock; this finds it unless a delete reaped it in between.
            Backend::Remote(store) => {
                self.put_remote(store, &up.hash, &up.tmp, up.size).await?;
                false
            }
        };
        let now = now_us();
        let same = old_hash.as_deref() == Some(up.hash.as_str());
        let (dn, db) = (
            i64::from(old_hash.is_none()),
            up.size as i64 - old_size as i64,
        );
        let mut ms = Vec::with_capacity(4);
        if !same {
            ms.push(mutation(
                "INSERT INTO blobs (hash, size, refs) VALUES (?, ?, 1) \
                 ON CONFLICT(hash) DO UPDATE SET refs = refs + 1",
                vec![
                    Value::Text(up.hash.as_str().into()),
                    Value::Int(up.size as i64),
                ],
            ));
        }
        ms.push(mutation(
            "INSERT INTO objects (bucket, key, blob, size, mime, etag, created, updated) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?3, ?6, ?7) ON CONFLICT(bucket, key) DO UPDATE SET \
             blob = excluded.blob, size = excluded.size, mime = excluded.mime, \
             etag = excluded.etag, updated = excluded.updated",
            vec![
                Value::Text(bucket.as_str().into()),
                Value::Text(up.key.as_str().into()),
                Value::Text(up.hash.as_str().into()),
                Value::Int(up.size as i64),
                Value::Text(up.mime.as_str().into()),
                Value::Int(created.unwrap_or(now)),
                Value::Int(now),
            ],
        ));
        if let Some(h) = old_hash.as_ref().filter(|_| !same) {
            ms.push(mutation(
                "UPDATE blobs SET refs = refs - 1 WHERE hash = ?",
                vec![Value::Text(h.as_str().into())],
            ));
        }
        ms.push(mutation(
            "UPDATE buckets SET objects = objects + ?, bytes = bytes + ? WHERE name = ?",
            vec![
                Value::Int(dn),
                Value::Int(db),
                Value::Text(bucket.as_str().into()),
            ],
        ));
        if let Err(e) = self.store.write(write_op(ms)).await {
            if placed {
                let p = self.blob_path(&up.hash);
                let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(p)).await;
            }
            return Err(e.into());
        }
        if old_hash.is_some() && !same {
            self.reap().await?;
        }
        Ok(ObjectInfo {
            key: up.key.clone(),
            size: up.size,
            mime: up.mime.clone(),
            etag: up.hash.clone(),
            created: format_us(created.unwrap_or(now)),
            updated: format_us(now),
        })
    }

    /// Uploads a received file to the object store unless that content is already there.
    async fn put_remote(
        &self,
        store: &Arc<dyn ObjectStore>,
        hash: &str,
        tmp: &std::path::Path,
        size: u64,
    ) -> CoreResult<()> {
        const CHUNK: usize = 8 << 20;
        let path = obj_path(hash);
        match store.head(&path).await {
            Ok(_) => return Ok(()),
            Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => return Err(remote_io(e)),
        }
        let t = tmp.to_path_buf();
        if size as usize <= CHUNK {
            let data = tokio::task::spawn_blocking(move || std::fs::read(t))
                .await
                .map_err(join)?
                .map_err(io)?;
            return store
                .put(&path, PutPayload::from(data))
                .await
                .map(drop)
                .map_err(remote_io);
        }
        let upload = store.put_multipart(&path).await.map_err(remote_io)?;
        let mut w = WriteMultipart::new_with_chunk_size(upload, CHUNK);
        let mut file = Some(File::open(&t).map_err(io)?);
        while let Some(mut f) = file.take() {
            let (f, chunk) = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
                let mut buf = vec![0u8; CHUNK];
                let mut n = 0;
                while n < CHUNK {
                    let k = f.read(&mut buf[n..])?;
                    if k == 0 {
                        break;
                    }
                    n += k;
                }
                buf.truncate(n);
                Ok((f, buf))
            })
            .await
            .map_err(join)?
            .map_err(io)?;
            if chunk.is_empty() {
                break;
            }
            w.wait_for_capacity(2).await.map_err(remote_io)?;
            w.write(&chunk);
            file = Some(f);
        }
        w.finish().await.map(drop).map_err(remote_io)
    }

    async fn find(&self, bucket: &str, key: &str) -> CoreResult<ObjectInfo> {
        let rows = self
            .store
            .query(Query {
                sql: format!("SELECT {OBJ_COLS} FROM objects WHERE bucket = ? AND key = ?").into(),
                params: vec![Value::Text(bucket.into()), Value::Text(key.into())],
            })
            .await?;
        rows.first()
            .map(|r| object_of(r))
            .ok_or(CoreError::NotFound("object"))
    }

    pub async fn object_info(&self, bucket: &str, key: &str) -> CoreResult<ObjectInfo> {
        self.bucket(bucket)?;
        self.find(bucket, key).await
    }

    /// Metadata plus the blob. A local file stays readable even if the object is replaced meanwhile.
    pub async fn open_object(&self, bucket: &str, key: &str) -> CoreResult<(ObjectInfo, Blob)> {
        self.bucket(bucket)?;
        let info = self.find(bucket, key).await?;
        if !valid_hash(&info.etag) {
            return Err(CoreError::NotFound("object"));
        }
        let blob = match &self.backend {
            Backend::Local => {
                let p = self.blob_path(&info.etag);
                Blob::Local(
                    tokio::task::spawn_blocking(move || File::open(p))
                        .await
                        .map_err(join)?
                        .map_err(|e| match e.kind() {
                            std::io::ErrorKind::NotFound => CoreError::NotFound("object"),
                            _ => io(e),
                        })?,
                )
            }
            Backend::Remote(store) => Blob::Remote {
                store: store.clone(),
                path: obj_path(&info.etag),
            },
        };
        Ok((info, blob))
    }

    /// A cached or freshly made thumbnail of an image object, as `(info, file)`. The info is the
    /// thumbnail's: its size and type, and an ETag of `<blob hash>-<size>`. Only sizes the bucket
    /// declares are made, at most `cores/2` at a time server-wide, and one job per thumbnail however
    /// many requests wait for it.
    pub async fn thumbnail(
        &self,
        bucket: &str,
        key: &str,
        spec: &str,
    ) -> CoreResult<(ObjectInfo, Blob)> {
        let b = self.bucket(bucket)?;
        let sp = Spec::parse(spec)
            .filter(|_| b.thumb_sizes.iter().any(|t| t == spec))
            .ok_or_else(|| {
                CoreError::BadRequest(format!(
                    "thumbnail size '{spec}' is not enabled for bucket '{bucket}' (enabled: {})",
                    b.thumb_sizes.join(", ")
                ))
            })?;
        let mut info = self.find(bucket, key).await?;
        if !thumbs::supported(&info.mime) || !valid_hash(&info.etag) {
            return Err(CoreError::BadRequest(
                "thumbnails are made for png, jpeg, gif and webp images".into(),
            ));
        }
        let dir = self.dir.join("thumbs").join(&info.etag);
        let spec_owned = spec.to_owned();
        let open_cached = |dir: PathBuf| {
            let spec = spec_owned.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    [("jpg", "image/jpeg"), ("png", "image/png")]
                        .into_iter()
                        .find_map(|(ext, mime)| {
                            File::open(dir.join(format!("{spec}.{ext}")))
                                .ok()
                                .map(|f| (f, mime))
                        })
                })
                .await
                .map_err(join)
            }
        };
        let mut found = open_cached(dir.clone()).await?;
        if found.is_none() {
            let flight_key = format!("{}/{spec}", info.etag);
            let lock = self
                .thumb_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(flight_key.clone())
                .or_default()
                .clone();
            let _g = lock.lock().await;
            found = open_cached(dir.clone()).await?;
            if found.is_none() {
                let _permit = self
                    .thumb_pool
                    .acquire()
                    .await
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                let tmp = self.dir.join(".tmp").join(encode_string(&new_id()));
                // A remote source is fetched into a temp file first (images only, so bounded).
                let (src, fetched) = match &self.backend {
                    Backend::Local => (self.blob_path(&info.etag), false),
                    Backend::Remote(store) => {
                        if info.size > REMOTE_THUMB_SOURCE_MAX {
                            return Err(CoreError::TooLarge(format!(
                                "image over {REMOTE_THUMB_SOURCE_MAX} bytes cannot be thumbnailed from a remote store"
                            )));
                        }
                        let data = store
                            .get(&obj_path(&info.etag))
                            .await
                            .map_err(remote_io)?
                            .bytes()
                            .await
                            .map_err(remote_io)?;
                        let p = self.dir.join(".tmp").join(encode_string(&new_id()));
                        let w = p.clone();
                        tokio::task::spawn_blocking(move || std::fs::write(w, data))
                            .await
                            .map_err(join)?
                            .map_err(io)?;
                        (p, true)
                    }
                };
                let (d, spec_s) = (dir.clone(), spec.to_owned());
                let made =
                    tokio::task::spawn_blocking(move || -> CoreResult<(File, &'static str)> {
                        if !fetched && !src.exists() {
                            return Err(CoreError::NotFound("object"));
                        }
                        let made = thumbs::make(&src, sp, &tmp);
                        if fetched {
                            let _ = std::fs::remove_file(&src);
                        }
                        let mime = made.inspect_err(|_| drop(std::fs::remove_file(&tmp)))?;
                        let ext = if mime == "image/png" { "png" } else { "jpg" };
                        std::fs::create_dir_all(&d).map_err(io)?;
                        std::fs::rename(&tmp, d.join(format!("{spec_s}.{ext}"))).map_err(io)?;
                        Ok((
                            File::open(d.join(format!("{spec_s}.{ext}"))).map_err(io)?,
                            mime,
                        ))
                    })
                    .await
                    .map_err(join)?;
                let mut g = self.thumb_flight.lock().unwrap_or_else(|e| e.into_inner());
                if Arc::strong_count(&lock) <= 2 {
                    g.remove(&flight_key);
                }
                drop(g);
                found = Some(made?);
            }
        }
        let (file, mime) = found.ok_or(CoreError::NotFound("object"))?;
        let len = file.metadata().map_err(io)?.len();
        info.size = len;
        info.mime = mime.to_owned();
        info.etag = format!("{}-{spec}", info.etag);
        Ok((info, Blob::Local(file)))
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> CoreResult<()> {
        self.bucket(bucket)?;
        let _g = self.commit.lock().await;
        let info = self.find(bucket, key).await?;
        self.store
            .write(write_op(vec![
                mutation(
                    "DELETE FROM objects WHERE bucket = ? AND key = ?",
                    vec![Value::Text(bucket.into()), Value::Text(key.into())],
                ),
                mutation(
                    "UPDATE blobs SET refs = refs - 1 WHERE hash = ?",
                    vec![Value::Text(info.etag.as_str().into())],
                ),
                mutation(
                    "UPDATE buckets SET objects = objects - 1, bytes = bytes - ? WHERE name = ?",
                    vec![Value::Int(info.size as i64), Value::Text(bucket.into())],
                ),
            ]))
            .await?;
        self.reap().await
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

    /// The object-store version of the sweep: one flat listing of `blobs/` compared with the table.
    async fn gc_remote(&self, store: &Arc<dyn ObjectStore>, rep: &mut GcReport) -> CoreResult<()> {
        let rows = self
            .store
            .query(Query {
                sql: "SELECT hash FROM blobs".into(),
                params: vec![],
            })
            .await?;
        let known: HashSet<String> = rows.iter().map(|r| text(&r[0])).collect();
        let mut present = HashSet::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let mut list = store.list(Some(&ObjPath::from("blobs")));
        while let Some(m) = list.next().await {
            let m = m.map_err(remote_io)?;
            let Some(name) = m.location.filename().map(str::to_owned) else {
                continue;
            };
            if known.contains(&name) {
                present.insert(name);
            } else if now - m.last_modified.timestamp() > GC_GRACE.as_secs() as i64
                && store.delete(&m.location).await.is_ok()
            {
                rep.removed_files += 1;
                rep.removed_bytes += m.size;
            }
        }
        rep.missing_files = known.difference(&present).count() as u64;
        Ok(())
    }

    /// Removes thumbnail directories whose blob is gone (a crash between the blob delete and the
    /// thumbnail delete). Returns how many.
    async fn gc_thumbs(&self) -> CoreResult<u64> {
        let root = self.dir.join("thumbs");
        let names: Vec<(String, PathBuf)> = tokio::task::spawn_blocking(move || {
            let Ok(rd) = std::fs::read_dir(root) else {
                return Vec::new();
            };
            rd.flatten()
                .filter(|e| {
                    e.metadata()
                        .ok()
                        .and_then(|m| m.modified().ok()?.elapsed().ok())
                        .is_some_and(|a| a > GC_GRACE)
                })
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .collect()
        })
        .await
        .map_err(join)?;
        let mut n = 0;
        for (hash, path) in names {
            let rows = self
                .store
                .query(Query {
                    sql: "SELECT 1 FROM blobs WHERE hash = ?".into(),
                    params: vec![Value::Text(hash.as_str().into())],
                })
                .await?;
            if rows.is_empty()
                && tokio::task::spawn_blocking(move || std::fs::remove_dir_all(path).is_ok())
                    .await
                    .map_err(join)?
            {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Writes a consistent copy of the storage into `dest` (`files.db` plus `blobs/`). Blobs are
    /// immutable and never rewritten, so they are hard-linked (copied when linking is not possible):
    /// no bytes are duplicated. Metadata commits wait for the copy, uploads in flight do not.
    pub(crate) async fn snapshot_into(&self, dest: PathBuf) -> CoreResult<()> {
        let _g = self.commit.lock().await;
        let d = dest.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(d))
            .await
            .map_err(join)?
            .map_err(io)?;
        self.store.snapshot(dest.join("files.db")).await?;
        // Blobs in an object store are not copied: they have their own durability.
        if matches!(self.backend, Backend::Remote(_)) {
            return Ok(());
        }
        let rows = self
            .store
            .query(Query {
                sql: "SELECT hash FROM blobs WHERE refs > 0".into(),
                params: vec![],
            })
            .await?;
        let hashes: Vec<String> = rows
            .iter()
            .map(|r| text(&r[0]))
            .filter(|h| valid_hash(h))
            .collect();
        let src = self.dir.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            for h in hashes {
                let rel = blob_rel(&h);
                // A referenced blob without a file was already lost; the copy has the same hole.
                if src.join(&rel).exists() {
                    link_or_copy(&src.join(&rel), &dest.join(&rel))?;
                }
            }
            Ok(())
        })
        .await
        .map_err(join)?
        .map_err(io)
    }

    /// Deletes blob files no row references (left by a crash between placing a file and
    /// committing it) and reports referenced blobs whose file is missing. Files younger than ten
    /// minutes are skipped, since a commit may be about to reference them. Safe to run at any time.
    pub async fn gc(&self) -> CoreResult<GcReport> {
        let mut rep = GcReport::default();
        if let Backend::Remote(store) = &self.backend {
            self.gc_remote(store, &mut rep).await?;
            rep.removed_thumb_dirs = self.gc_thumbs().await?;
            return Ok(rep);
        }
        for p in 0..256u32 {
            let pre = format!("{p:02x}");
            let rows = self
                .store
                .query(Query {
                    sql: "SELECT hash FROM blobs WHERE hash >= ?1 AND hash < ?2".into(),
                    params: vec![
                        Value::Text(pre.as_str().into()),
                        Value::Text(format!("{pre}g").into()),
                    ],
                })
                .await?;
            let known: HashSet<String> = rows.iter().map(|r| text(&r[0])).collect();
            let dir = self.dir.join("blobs").join(&pre);
            let known2 = known.clone();
            let (removed, bytes, present) = tokio::task::spawn_blocking(move || {
                let (mut removed, mut bytes, mut present) = (0u64, 0u64, HashSet::new());
                let Ok(rd) = std::fs::read_dir(&dir) else {
                    return (0, 0, present);
                };
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if known2.contains(&name) {
                        present.insert(name);
                        continue;
                    }
                    let Ok(md) = e.metadata() else { continue };
                    let old = md
                        .modified()
                        .ok()
                        .and_then(|m| m.elapsed().ok())
                        .is_some_and(|a| a > GC_GRACE);
                    if old && std::fs::remove_file(e.path()).is_ok() {
                        removed += 1;
                        bytes += md.len();
                    }
                }
                (removed, bytes, present)
            })
            .await
            .map_err(join)?;
            rep.removed_files += removed;
            rep.removed_bytes += bytes;
            rep.missing_files += known.difference(&present).count() as u64;
        }
        rep.removed_thumb_dirs = self.gc_thumbs().await?;
        Ok(rep)
    }
}

impl Registry {
    /// The app's storage, opened on first use. The first open also starts one background `gc`.
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
        let f = {
            let backend = self.backend_for(app).await?;
            Arc::new(
                Files::open(
                    self.app_dir(app).join("files"),
                    backend,
                    self.thumb_pool.clone(),
                )
                .await?,
            )
        };
        self.files
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(app.to_owned(), f.clone());
        let bg = f.clone();
        let app = app.to_owned();
        tokio::spawn(async move {
            match bg.gc().await {
                Ok(r) if r.removed_files + r.missing_files > 0 => {
                    tracing::warn!(app, ?r, "storage gc");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(app, error = %e, "storage gc failed"),
            }
        });
        Ok(f)
    }
}
