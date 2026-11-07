//! Per-app blob backend (doc 13 §1): local disk (default) or an S3-compatible bucket.
//!
//! Only blob bytes move to the object store; object metadata (`files.db`) and thumbnails stay on the
//! server's disk. The config lives in `system.db` (`settings.storage:<app>`); the secret access key
//! is stored as plain JSON like the OAuth client secrets (encryption at rest is Phase 3) and is never
//! returned. Saving an S3 config writes, reads and deletes a probe object first, so a wrong key,
//! region or endpoint fails at once instead of on the first upload.

use std::sync::Arc;

use monolith_store::{AppStore, Query, Value};
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjPath;
use object_store::prefix::PrefixStore;
use serde::{Deserialize, Serialize};

use crate::app::now_us;
use crate::error::{CoreError, CoreResult, FieldError};
use crate::files::{Backend, Files};
use crate::system::{Registry, sys_write};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    /// For S3-compatible services (MinIO, R2, B2, Wasabi); omit for AWS.
    pub endpoint: Option<String>,
    /// Key prefix inside the bucket, so several apps or services can share one.
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub allow_http: bool,
    /// `https://host/bucket/key` instead of `https://bucket.host/key`; most self-hosted services need it.
    pub path_style: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredStorage {
    pub s3: Option<S3Config>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct S3In {
    pub bucket: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub prefix: String,
    pub access_key_id: String,
    /// Omit to keep the stored secret.
    #[serde(default)]
    pub secret_access_key: Option<String>,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default)]
    pub path_style: bool,
}

fn default_region() -> String {
    "us-east-1".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageConfigIn {
    /// `local` or `s3`.
    pub backend: String,
    #[serde(default)]
    pub s3: Option<S3In>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3View {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key_set: bool,
    pub allow_http: bool,
    pub path_style: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfigView {
    pub backend: &'static str,
    pub s3: Option<S3View>,
}

impl StoredStorage {
    fn view(&self) -> StorageConfigView {
        StorageConfigView {
            backend: if self.s3.is_some() { "s3" } else { "local" },
            s3: self.s3.as_ref().map(|c| S3View {
                bucket: c.bucket.clone(),
                region: c.region.clone(),
                endpoint: c.endpoint.clone(),
                prefix: c.prefix.clone(),
                access_key_id: c.access_key_id.clone(),
                secret_access_key_set: !c.secret_access_key.is_empty(),
                allow_http: c.allow_http,
                path_style: c.path_style,
            }),
        }
    }
}

fn invalid(field: &str, code: &'static str, msg: impl Into<String>) -> CoreError {
    CoreError::Validation(vec![FieldError::new(field, code, msg)])
}

fn simple(s: &str, extra: &[char]) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

/// Validates `input` and merges it over `old` (the secret is kept when omitted).
fn merge(old: &StoredStorage, input: StorageConfigIn) -> CoreResult<StoredStorage> {
    match input.backend.as_str() {
        "local" => Ok(StoredStorage::default()),
        "s3" => {
            let i = input
                .s3
                .ok_or_else(|| invalid("s3", "required", "the s3 settings are required"))?;
            if !(3..=63).contains(&i.bucket.len()) || !simple(&i.bucket, &['.', '-', '_']) {
                return Err(invalid(
                    "s3.bucket",
                    "invalid_format",
                    "3 to 63 letters, digits, '.', '-' or '_'",
                ));
            }
            if i.region.is_empty() || i.region.len() > 32 || !simple(&i.region, &['-']) {
                return Err(invalid("s3.region", "invalid_format", "like us-east-1"));
            }
            if let Some(e) = &i.endpoint {
                let ok = (e.starts_with("https://") || (i.allow_http && e.starts_with("http://")))
                    && e.len() <= 300
                    && !e.chars().any(|c| c.is_control() || c == ' ');
                if !ok {
                    return Err(invalid(
                        "s3.endpoint",
                        "invalid_format",
                        "an https:// URL (http:// only with allowHttp)",
                    ));
                }
            }
            let prefix = i.prefix.trim_matches('/').to_owned();
            if prefix.len() > 200
                || prefix.split('/').any(|p| p == ".." || p == ".")
                || prefix.chars().any(|c| c.is_control() || c == '\\')
            {
                return Err(invalid(
                    "s3.prefix",
                    "invalid_format",
                    "up to 200 characters, no '.' or '..' segments",
                ));
            }
            if i.access_key_id.is_empty() || i.access_key_id.len() > 128 {
                return Err(invalid("s3.accessKeyId", "required", "1 to 128 characters"));
            }
            let secret = match (i.secret_access_key, old.s3.as_ref()) {
                (Some(s), _) if !s.is_empty() => s,
                (_, Some(o)) if !o.secret_access_key.is_empty() => o.secret_access_key.clone(),
                _ => {
                    return Err(invalid(
                        "s3.secretAccessKey",
                        "required",
                        "the secret access key is required",
                    ));
                }
            };
            if secret.len() > 256 {
                return Err(invalid(
                    "s3.secretAccessKey",
                    "too_long",
                    "at most 256 characters",
                ));
            }
            Ok(StoredStorage {
                s3: Some(S3Config {
                    bucket: i.bucket,
                    region: i.region,
                    endpoint: i.endpoint,
                    prefix,
                    access_key_id: i.access_key_id,
                    secret_access_key: secret,
                    allow_http: i.allow_http,
                    path_style: i.path_style,
                }),
            })
        }
        _ => Err(invalid("backend", "invalid_value", "local or s3")),
    }
}

fn build(c: &S3Config) -> CoreResult<Arc<dyn ObjectStore>> {
    let mut b = AmazonS3Builder::new()
        .with_bucket_name(&c.bucket)
        .with_region(&c.region)
        .with_access_key_id(&c.access_key_id)
        .with_secret_access_key(&c.secret_access_key)
        .with_allow_http(c.allow_http)
        .with_virtual_hosted_style_request(!c.path_style);
    if let Some(e) = &c.endpoint {
        b = b.with_endpoint(e);
    }
    let s = b
        .build()
        .map_err(|e| CoreError::BadRequest(format!("s3 settings: {e}")))?;
    Ok(if c.prefix.is_empty() {
        Arc::new(s)
    } else {
        Arc::new(PrefixStore::new(s, c.prefix.as_str()))
    })
}

/// Writes, reads and deletes a probe object.
async fn probe(store: &dyn ObjectStore) -> CoreResult<()> {
    let fail = |step: &str, e: object_store::Error| {
        CoreError::BadRequest(format!(
            "object store check failed while trying to {step}: {e}"
        ))
    };
    let path = ObjPath::from(format!(".monolith-probe/{}", now_us()));
    store
        .put(&path, "ok".into())
        .await
        .map_err(|e| fail("write", e))?;
    let got = store.get(&path).await.map_err(|e| fail("read", e))?;
    got.bytes().await.map_err(|e| fail("read", e))?;
    store.delete(&path).await.map_err(|e| fail("delete", e))
}

impl Registry {
    async fn stored_storage(&self, app: &str) -> CoreResult<StoredStorage> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT value FROM settings WHERE key = ?".into(),
                params: vec![Value::Text(format!("storage:{app}").into())],
            })
            .await?;
        match rows.first().and_then(|r| r.first()) {
            Some(Value::Blob(b)) => serde_json::from_slice(b)
                .map_err(|e| CoreError::Internal(format!("storage config for {app}: {e}"))),
            _ => Ok(StoredStorage::default()),
        }
    }

    pub async fn storage_config(&self, app: &str) -> CoreResult<StorageConfigView> {
        self.app_info(app).await?;
        Ok(self.stored_storage(app).await?.view())
    }

    /// Switches the app's blob backend. Existing blobs are not moved, so once the app holds objects
    /// the location (local, or bucket + prefix + endpoint) cannot change; credentials can.
    pub async fn put_storage_config(
        &self,
        app: &str,
        input: StorageConfigIn,
    ) -> CoreResult<StorageConfigView> {
        self.app_info(app).await?;
        let old = self.stored_storage(app).await?;
        let next = merge(&old, input)?;
        let place = |s: &StoredStorage| {
            s.s3.as_ref()
                .map(|c| (c.bucket.clone(), c.prefix.clone(), c.endpoint.clone()))
        };
        if place(&old) != place(&next) && self.files(app).await?.has_blobs().await? {
            return Err(CoreError::Conflict(
                "the app already holds files; blobs are not moved between backends, delete them first".into(),
            ));
        }
        if let Some(c) = &next.s3 {
            probe(build(c)?.as_ref()).await?;
        }
        let bytes = serde_json::to_vec(&next).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [0; 16],
                vec![
                    Value::Text(format!("storage:{app}").into()),
                    Value::Blob(bytes.into()),
                ],
            ))
            .await?;
        // The next use reopens storage with the new backend; requests in flight finish on the old one.
        self.files
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(app);
        Ok(next.view())
    }

    pub(crate) async fn backend_for(&self, app: &str) -> CoreResult<Backend> {
        Ok(match self.stored_storage(app).await?.s3 {
            Some(c) => Backend::Remote(build(&c)?),
            None => Backend::Local,
        })
    }

    /// Uses `store` for the app's blobs until the next restart or config change. For tests and
    /// embedding; the HTTP API configures S3 through [`Self::put_storage_config`].
    pub async fn use_object_store(
        &self,
        app: &str,
        store: Arc<dyn ObjectStore>,
    ) -> CoreResult<Arc<Files>> {
        self.app_info(app).await?;
        let _guard = self.open_lock.lock().await;
        let f = Arc::new(
            Files::open(
                self.app_dir(app).join("files"),
                Backend::Remote(store),
                self.thumb_pool.clone(),
            )
            .await?,
        );
        self.files
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(app.to_owned(), f.clone());
        Ok(f)
    }
}
