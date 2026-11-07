use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::errors::Result;

/// Metadata describing a stored object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub mime: String,
    pub etag: String,
    pub created: String,
    pub updated: String,
}

/// A page of stored objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPage {
    pub items: Vec<ObjectInfo>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// Service for managing file uploads, downloads, and bucket storage.
#[derive(Clone)]
pub struct FilesService {
    client: Client,
}

impl FilesService {
    /// Creates a new [`FilesService`].
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    fn bucket_path(&self, bucket: &str) -> String {
        let app =
            url::form_urlencoded::byte_serialize(self.client.app().as_bytes()).collect::<String>();
        let bucket = url::form_urlencoded::byte_serialize(bucket.as_bytes()).collect::<String>();
        format!("/api/v1/apps/{app}/buckets/{bucket}/objects")
    }

    /// Constructs the public or authenticated URL for accessing an object.
    pub fn get_url(&self, bucket: &str, key: &str) -> String {
        self.get_url_with_thumb(bucket, key, None)
    }

    /// Constructs the URL for accessing an object with an optional thumbnail spec (e.g. `"100x100"`).
    pub fn get_url_with_thumb(&self, bucket: &str, key: &str, thumb: Option<&str>) -> String {
        let clean_key = key.trim_start_matches('/');
        let path = format!("{}/{}", self.bucket_path(bucket), clean_key);
        let query = thumb.map(|t| vec![("thumb", t)]);
        match self.client.build_url(&path, query.as_deref()) {
            Ok(url) => url.to_string(),
            Err(_) => format!("{}{path}", self.client.base_url()),
        }
    }

    /// Uploads an object with the specified key and content.
    pub async fn upload(
        &self,
        bucket: &str,
        key: &str,
        data: impl Into<Bytes>,
        content_type: Option<&str>,
    ) -> Result<ObjectInfo> {
        let clean_key = key.trim_start_matches('/');
        let path = format!("{}/{}", self.bucket_path(bucket), clean_key);
        self.client
            .upload_raw(&path, data.into(), content_type)
            .await
    }

    /// Downloads the raw bytes of an object.
    pub async fn download(&self, bucket: &str, key: &str) -> Result<Bytes> {
        self.download_with_thumb(bucket, key, None).await
    }

    /// Downloads the raw bytes of an object or its thumbnail.
    pub async fn download_with_thumb(
        &self,
        bucket: &str,
        key: &str,
        thumb: Option<&str>,
    ) -> Result<Bytes> {
        let clean_key = key.trim_start_matches('/');
        let path = format!("{}/{}", self.bucket_path(bucket), clean_key);
        let query = thumb.map(|t| vec![("thumb", t)]);
        self.client.download_raw(&path, query.as_deref()).await
    }

    /// Deletes an object by key.
    pub async fn delete(&self, bucket: &str, key: &str) -> Result<()> {
        let clean_key = key.trim_start_matches('/');
        let path = format!("{}/{}", self.bucket_path(bucket), clean_key);
        self.client.delete(&path).await
    }

    /// Lists objects in a bucket with optional prefix, limit, and cursor.
    pub async fn list(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<ObjectPage> {
        let path = self.bucket_path(bucket);
        let mut query = Vec::new();
        if let Some(p) = prefix {
            query.push(("prefix", p));
        }
        let limit_str;
        if let Some(l) = limit {
            limit_str = l.to_string();
            query.push(("limit", &limit_str));
        }
        if let Some(c) = cursor {
            query.push(("cursor", c));
        }
        self.client
            .get(&path, if query.is_empty() { None } else { Some(&query) })
            .await
    }
}
