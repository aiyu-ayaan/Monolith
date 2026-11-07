use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::auth::AuthService;
use crate::collections::{Collection, Record};
use crate::errors::{Error, Problem, Result};
use crate::files::{FilesService, ObjectInfo};

/// A single atomic operation in a batch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOp {
    pub op: String,
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl BatchOp {
    pub fn create(collection: impl Into<String>, body: serde_json::Value) -> Self {
        Self {
            op: "create".into(),
            collection: collection.into(),
            id: None,
            body: Some(body),
        }
    }

    pub fn update(
        collection: impl Into<String>,
        id: impl Into<String>,
        body: serde_json::Value,
    ) -> Self {
        Self {
            op: "update".into(),
            collection: collection.into(),
            id: Some(id.into()),
            body: Some(body),
        }
    }

    pub fn delete(collection: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            op: "delete".into(),
            collection: collection.into(),
            id: Some(id.into()),
            body: None,
        }
    }
}

/// A result item from an atomic batch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultItem {
    pub id: String,
}

/// The response payload from a batch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    pub results: Vec<BatchResultItem>,
}

struct ClientInner {
    base_url: Url,
    app: String,
    http: reqwest::Client,
    token: RwLock<Option<String>>,
    api_key: RwLock<Option<String>>,
    custom_headers: HeaderMap,
}

/// Builder for constructing a [`Client`].
pub struct ClientBuilder {
    base_url: String,
    app: String,
    token: Option<String>,
    api_key: Option<String>,
    custom_headers: HeaderMap,
    timeout: Option<Duration>,
}

impl ClientBuilder {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            app: "default".into(),
            token: None,
            api_key: None,
            custom_headers: HeaderMap::new(),
            timeout: None,
        }
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn app(mut self, app: impl Into<String>) -> Self {
        self.app = app.into();
        self
    }

    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn header(mut self, key: &str, value: &str) -> Result<Self> {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|e| Error::Config(format!("invalid header name: {e}")))?;
        let val = HeaderValue::from_str(value)
            .map_err(|e| Error::Config(format!("invalid header value: {e}")))?;
        self.custom_headers.insert(name, val);
        Ok(self)
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn build(self) -> Result<Client> {
        let mut raw_url = self.base_url.trim().to_string();
        if !raw_url.ends_with('/') {
            raw_url.push('/');
        }
        let parsed_url = Url::parse(&raw_url)?;

        let mut req_builder = reqwest::Client::builder();
        if let Some(t) = self.timeout {
            req_builder = req_builder.timeout(t);
        }
        let http = req_builder.build()?;

        let inner = ClientInner {
            base_url: parsed_url,
            app: self.app,
            http,
            token: RwLock::new(self.token),
            api_key: RwLock::new(self.api_key),
            custom_headers: self.custom_headers,
        };

        Ok(Client {
            inner: Arc::new(inner),
        })
    }
}

/// The Monolith API client.
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

impl Client {
    /// Creates a new [`Client`] with default settings pointing to `base_url`.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        Self::builder(base_url).build()
    }

    /// Creates a [`ClientBuilder`] to configure the client.
    pub fn builder(base_url: impl Into<String>) -> ClientBuilder {
        ClientBuilder::new(base_url)
    }

    /// Returns the configured base URL without a trailing slash.
    pub fn base_url(&self) -> &str {
        self.inner.base_url.as_str().trim_end_matches('/')
    }

    /// Returns the target application identifier.
    pub fn app(&self) -> &str {
        &self.inner.app
    }

    /// Returns the current bearer token if set.
    pub fn token(&self) -> Option<String> {
        self.inner
            .token
            .read()
            .ok()
            .and_then(|t| t.as_ref().cloned())
    }

    /// Sets or clears the active session token.
    pub fn set_token(&self, token: Option<impl Into<String>>) {
        if let Ok(mut lock) = self.inner.token.write() {
            *lock = token.map(Into::into);
        }
    }

    /// Returns the configured API key if set.
    pub fn api_key(&self) -> Option<String> {
        self.inner
            .api_key
            .read()
            .ok()
            .and_then(|k| k.as_ref().cloned())
    }

    /// Sets or clears the API key.
    pub fn set_api_key(&self, key: Option<impl Into<String>>) {
        if let Ok(mut lock) = self.inner.api_key.write() {
            *lock = key.map(Into::into);
        }
    }

    /// Returns an [`AuthService`] scoped to the default `"users"` collection.
    pub fn auth(&self) -> AuthService {
        AuthService::new(self.clone(), "users")
    }

    /// Returns an [`AuthService`] scoped to a specific collection (e.g. `"_superusers"`).
    pub fn auth_collection(&self, collection: impl Into<String>) -> AuthService {
        AuthService::new(self.clone(), collection)
    }

    /// Returns a [`Collection`] client scoped to the specified collection name.
    pub fn collection<T>(&self, name: impl Into<String>) -> Collection<T> {
        Collection::new(self.clone(), name)
    }

    /// Returns a [`Collection<Record>`] client for standard record maps.
    pub fn records(&self, name: impl Into<String>) -> Collection<Record> {
        Collection::new(self.clone(), name)
    }

    /// Returns a [`FilesService`] for storage operations.
    pub fn files(&self) -> FilesService {
        FilesService::new(self.clone())
    }

    /// Executes an atomic batch of operations against `/api/v1/apps/{app}/batch`.
    pub async fn batch(&self, ops: &[BatchOp]) -> Result<BatchResponse> {
        let app_enc =
            url::form_urlencoded::byte_serialize(self.app().as_bytes()).collect::<String>();
        let path = format!("/api/v1/apps/{app_enc}/batch");
        let body = serde_json::json!({ "ops": ops });
        self.post(&path, &body).await
    }

    /// Builds a full [`Url`] from a relative path and optional query parameters.
    pub fn build_url(&self, path: &str, query: Option<&[(&str, &str)]>) -> Result<Url> {
        let clean = path.trim_start_matches('/');
        let mut u = self.inner.base_url.join(clean)?;
        if let Some(params) = query {
            let mut pairs = u.query_pairs_mut();
            for (k, v) in params {
                pairs.append_pair(k, v);
            }
        }
        Ok(u)
    }

    fn apply_headers(&self, req: &mut reqwest::Request) {
        let headers = req.headers_mut();

        for (k, v) in self.inner.custom_headers.iter() {
            if !headers.contains_key(k) {
                headers.insert(k.clone(), v.clone());
            }
        }

        if !headers.contains_key(AUTHORIZATION) {
            let token = self.token();
            let key = self.api_key();
            let auth_value = token.or(key);
            if let Some(auth) = auth_value
                && let Ok(val) = HeaderValue::from_str(&format!("Bearer {auth}"))
            {
                headers.insert(AUTHORIZATION, val);
            }
        }
    }

    async fn execute_get(&self, url: Url) -> Result<reqwest::Response> {
        let max_attempts = 4;

        for attempt in 0..max_attempts {
            let mut req = reqwest::Request::new(Method::GET, url.clone());
            self.apply_headers(&mut req);

            let res = match self.inner.http.execute(req).await {
                Ok(r) => r,
                Err(e) => {
                    if attempt < max_attempts - 1 {
                        let delay = Duration::from_millis(100 * (1 << attempt));
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(Error::Network(e));
                }
            };

            let status = res.status();

            if status == StatusCode::SERVICE_UNAVAILABLE && attempt < max_attempts - 1 {
                let delay = res
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| Duration::from_millis(100 * (1 << attempt)));
                tokio::time::sleep(delay).await;
                continue;
            }

            if !status.is_success() {
                return self.handle_error_response(res).await;
            }

            return Ok(res);
        }

        Err(Error::Config("Max request attempts exceeded".into()))
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Option<&[(&str, &str)]>,
    ) -> Result<T> {
        let url = self.build_url(path, query)?;
        let res = self.execute_get(url).await?;
        if res.status() == StatusCode::NO_CONTENT {
            return serde_json::from_str("null").map_err(Error::Serialization);
        }
        let data = res.json::<T>().await?;
        Ok(data)
    }

    pub async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = self.build_url(path, None)?;
        let bytes = serde_json::to_vec(body)?;
        let mut req = reqwest::Request::new(Method::POST, url);
        req.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.apply_headers(&mut req);
        *req.body_mut() = Some(reqwest::Body::from(bytes));

        let res = self.inner.http.execute(req).await?;
        let status = res.status();
        if !status.is_success() {
            return self.handle_error_response(res).await;
        }
        if status == StatusCode::NO_CONTENT {
            return serde_json::from_str("null").map_err(Error::Serialization);
        }
        let data = res.json::<T>().await?;
        Ok(data)
    }

    pub async fn put<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = self.build_url(path, None)?;
        let bytes = serde_json::to_vec(body)?;
        let mut req = reqwest::Request::new(Method::PUT, url);
        req.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.apply_headers(&mut req);
        *req.body_mut() = Some(reqwest::Body::from(bytes));

        let res = self.inner.http.execute(req).await?;
        let status = res.status();
        if !status.is_success() {
            return self.handle_error_response(res).await;
        }
        if status == StatusCode::NO_CONTENT {
            return serde_json::from_str("null").map_err(Error::Serialization);
        }
        let data = res.json::<T>().await?;
        Ok(data)
    }

    pub async fn patch<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.build_url(path, None)?;
        let bytes = serde_json::to_vec(body)?;
        let mut req = reqwest::Request::new(Method::PATCH, url);
        req.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.apply_headers(&mut req);
        *req.body_mut() = Some(reqwest::Body::from(bytes));

        let res = self.inner.http.execute(req).await?;
        let status = res.status();
        if !status.is_success() {
            return self.handle_error_response(res).await;
        }
        if status == StatusCode::NO_CONTENT {
            return serde_json::from_str("null").map_err(Error::Serialization);
        }
        let data = res.json::<T>().await?;
        Ok(data)
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = self.build_url(path, None)?;
        let mut req = reqwest::Request::new(Method::DELETE, url);
        self.apply_headers(&mut req);
        let res = self.inner.http.execute(req).await?;
        if !res.status().is_success() {
            return self.handle_error_response(res).await;
        }
        Ok(())
    }

    pub(crate) async fn upload_raw(
        &self,
        path: &str,
        data: bytes::Bytes,
        content_type: Option<&str>,
    ) -> Result<ObjectInfo> {
        let url = self.build_url(path, None)?;
        let mut req = reqwest::Request::new(Method::PUT, url);
        let mime = content_type.unwrap_or("application/octet-stream");
        if let Ok(val) = HeaderValue::from_str(mime) {
            req.headers_mut().insert(CONTENT_TYPE, val);
        }
        self.apply_headers(&mut req);
        *req.body_mut() = Some(reqwest::Body::from(data));

        let res = self.inner.http.execute(req).await?;
        if !res.status().is_success() {
            return self.handle_error_response(res).await;
        }
        let info = res.json::<ObjectInfo>().await?;
        Ok(info)
    }

    pub(crate) async fn download_raw(
        &self,
        path: &str,
        query: Option<&[(&str, &str)]>,
    ) -> Result<bytes::Bytes> {
        let url = self.build_url(path, query)?;
        let res = self.execute_get(url).await?;
        let bytes = res.bytes().await?;
        Ok(bytes)
    }

    async fn handle_error_response<T>(&self, res: reqwest::Response) -> Result<T> {
        let status = res.status();
        let status_code = status.as_u16();
        let request_id = res
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);

        let is_json = res
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("json"));

        let problem = if is_json {
            match res.json::<Problem>().await {
                Ok(mut p) => {
                    if p.request_id.is_none() {
                        p.request_id = request_id;
                    }
                    p
                }
                Err(_) => Problem {
                    problem_type: "http_error".into(),
                    title: status.canonical_reason().unwrap_or("HTTP Error").into(),
                    status: status_code,
                    detail: None,
                    request_id,
                    errors: None,
                },
            }
        } else {
            let text = res.text().await.unwrap_or_default();
            Problem {
                problem_type: "http_error".into(),
                title: status.canonical_reason().unwrap_or("HTTP Error").into(),
                status: status_code,
                detail: if text.is_empty() { None } else { Some(text) },
                request_id,
                errors: None,
            }
        };

        Err(Error::api(status_code, problem))
    }
}
