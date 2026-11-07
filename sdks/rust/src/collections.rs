use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::errors::Result;

/// A default dynamic record model containing standard system metadata and arbitrary fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: String,
    #[serde(default)]
    pub created: Option<serde_json::Value>,
    #[serde(default)]
    pub updated: Option<serde_json::Value>,
    #[serde(default, rename = "_v")]
    pub version: Option<u64>,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Query options for listing collection records.
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub filter: Option<String>,
    pub sort: Option<String>,
    pub expand: Option<String>,
    pub fields: Option<String>,
    pub total: Option<String>,
}

impl ListOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn page(mut self, page: usize) -> Self {
        self.page = Some(page);
        self
    }

    pub fn per_page(mut self, per_page: usize) -> Self {
        self.per_page = Some(per_page);
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }

    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub fn sort(mut self, sort: impl Into<String>) -> Self {
        self.sort = Some(sort.into());
        self
    }

    pub fn expand(mut self, expand: impl Into<String>) -> Self {
        self.expand = Some(expand.into());
        self
    }

    pub fn fields(mut self, fields: impl Into<String>) -> Self {
        self.fields = Some(fields.into());
        self
    }

    pub fn total(mut self, total: impl Into<String>) -> Self {
        self.total = Some(total.into());
        self
    }

    /// Converts options into key-value query pairs.
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();
        if let Some(page) = self.page {
            params.push(("page".into(), page.to_string()));
        }
        if let Some(per_page) = self.per_page {
            params.push(("perPage".into(), per_page.to_string()));
        }
        if let Some(limit) = self.limit {
            params.push(("limit".into(), limit.to_string()));
        }
        if let Some(cursor) = &self.cursor {
            params.push(("cursor".into(), cursor.clone()));
        }
        if let Some(filter) = &self.filter {
            params.push(("filter".into(), filter.clone()));
        }
        if let Some(sort) = &self.sort {
            params.push(("sort".into(), sort.clone()));
        }
        if let Some(expand) = &self.expand {
            params.push(("expand".into(), expand.clone()));
        }
        if let Some(fields) = &self.fields {
            params.push(("fields".into(), fields.clone()));
        }
        if let Some(total) = &self.total {
            params.push(("total".into(), total.clone()));
        }
        params
    }
}

/// Paginated list of collection records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResult<T> {
    pub items: Vec<T>,
    #[serde(default, alias = "nextCursor")]
    pub next: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Result of a collection record count operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountResult {
    #[serde(default, alias = "count")]
    pub total: u64,
    #[serde(default)]
    pub exact: bool,
}

/// Client for interacting with a specific collection's records.
#[derive(Clone)]
pub struct Collection<T = Record> {
    client: Client,
    name: String,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Collection<T> {
    /// Creates a new collection client.
    pub fn new(client: Client, name: impl Into<String>) -> Self {
        Self {
            client,
            name: name.into(),
            _marker: PhantomData,
        }
    }

    /// Returns the collection name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Converts this collection client to a strongly-typed model `U`.
    pub fn with_type<U>(&self) -> Collection<U> {
        Collection {
            client: self.client.clone(),
            name: self.name.clone(),
            _marker: PhantomData,
        }
    }

    fn base_path(&self) -> String {
        let app =
            url::form_urlencoded::byte_serialize(self.client.app().as_bytes()).collect::<String>();
        let coll = url::form_urlencoded::byte_serialize(self.name.as_bytes()).collect::<String>();
        format!("/api/v1/apps/{app}/collections/{coll}/records")
    }
}

impl<T: DeserializeOwned> Collection<T> {
    /// Lists records in this collection matching the query options.
    pub async fn list(&self, options: Option<&ListOptions>) -> Result<ListResult<T>> {
        let path = self.base_path();
        let query_owned = options.map(|o| o.to_query_params()).unwrap_or_default();
        let query_ref: Vec<(&str, &str)> = query_owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.client.get(&path, Some(&query_ref)).await
    }

    /// Retrieves a single record by its ID.
    pub async fn get_one(&self, id: &str) -> Result<T> {
        let enc_id = url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>();
        let path = format!("{}/{}", self.base_path(), enc_id);
        self.client.get(&path, None).await
    }

    /// Retrieves a single record by its ID with relation expansion and field projection options.
    pub async fn get_one_with_options(
        &self,
        id: &str,
        expand: Option<&str>,
        fields: Option<&str>,
    ) -> Result<T> {
        let enc_id = url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>();
        let path = format!("{}/{}", self.base_path(), enc_id);
        let mut query = Vec::new();
        if let Some(exp) = expand {
            query.push(("expand", exp));
        }
        if let Some(f) = fields {
            query.push(("fields", f));
        }
        self.client
            .get(&path, if query.is_empty() { None } else { Some(&query) })
            .await
    }

    /// Creates a new record in this collection.
    pub async fn create<B: Serialize>(&self, data: &B) -> Result<T> {
        let path = self.base_path();
        self.client.post(&path, data).await
    }

    /// Updates (patches) an existing record by its ID.
    pub async fn update<B: Serialize>(&self, id: &str, data: &B) -> Result<T> {
        let enc_id = url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>();
        let path = format!("{}/{}", self.base_path(), enc_id);
        self.client.patch(&path, data).await
    }

    /// Replaces (upserts/puts) an existing record by its ID.
    pub async fn replace<B: Serialize>(&self, id: &str, data: &B) -> Result<T> {
        let enc_id = url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>();
        let path = format!("{}/{}", self.base_path(), enc_id);
        self.client.put(&path, data).await
    }

    /// Deletes a record by its ID.
    pub async fn delete(&self, id: &str) -> Result<()> {
        let enc_id = url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>();
        let path = format!("{}/{}", self.base_path(), enc_id);
        self.client.delete(&path).await
    }

    /// Counts records in this collection, optionally filtered.
    pub async fn count(&self, filter: Option<&str>) -> Result<u64> {
        let path = format!("{}:count", self.base_path());
        let query = filter.map(|f| vec![("filter", f)]);
        let res: CountResult = self.client.get(&path, query.as_deref()).await?;
        Ok(res.total)
    }
}
