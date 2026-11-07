use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::errors::Result;

/// Authentication session information returned upon successful login or install.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct InstallStatusResponse {
    required: bool,
}

/// Authentication and session management service.
#[derive(Clone)]
pub struct AuthService {
    client: Client,
    collection: String,
}

impl AuthService {
    /// Creates a new [`AuthService`] scoped to a given collection (e.g. `"users"` or `"_superusers"`).
    pub fn new(client: Client, collection: impl Into<String>) -> Self {
        Self {
            client,
            collection: collection.into(),
        }
    }

    /// Returns the collection name this auth service is scoped to.
    pub fn collection(&self) -> &str {
        &self.collection
    }

    /// Returns the currently active bearer token, if any.
    pub fn token(&self) -> Option<String> {
        self.client.token()
    }

    /// Sets or updates the active authentication token in the client.
    pub fn set_token(&self, token: Option<impl Into<String>>) {
        self.client.set_token(token);
    }

    /// Clears the active authentication token from the client.
    pub fn sign_out(&self) {
        self.client.set_token(None::<String>);
    }

    /// Checks whether the server requires initial setup/installation.
    pub async fn install_required(&self) -> Result<bool> {
        let res: InstallStatusResponse = self.client.get("/api/v1/system/install", None).await?;
        Ok(res.required)
    }

    /// Performs initial superuser setup on a fresh server instance.
    pub async fn install(&self, token: &str, email: &str, password: &str) -> Result<AuthSession> {
        let body = serde_json::json!({
            "token": token,
            "email": email,
            "password": password,
        });
        let session: AuthSession = self.client.post("/api/v1/system/install", &body).await?;
        self.set_token(Some(&session.token));
        Ok(session)
    }

    /// Authenticates with an email and password.
    ///
    /// For `_superusers` or `system`, this uses the system login endpoint (`/api/v1/system/auth/login`).
    /// For other collections, it authenticates against the app's collection auth route.
    pub async fn sign_in_with_password(&self, email: &str, password: &str) -> Result<AuthSession> {
        let is_system = self.collection == "_superusers" || self.collection == "system";
        let path = if is_system {
            "/api/v1/system/auth/login".to_string()
        } else {
            let app_enc = url::form_urlencoded::byte_serialize(self.client.app().as_bytes())
                .collect::<String>();
            format!("/api/v1/apps/{app_enc}/auth/login")
        };

        let body = serde_json::json!({
            "email": email,
            "password": password,
        });

        let session: AuthSession = self.client.post(&path, &body).await?;
        self.set_token(Some(&session.token));
        Ok(session)
    }

    /// Retrieves information about the currently authenticated user/superuser.
    pub async fn me(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v1/system/me", None).await
    }
}
