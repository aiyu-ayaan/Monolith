use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use reqwest::Method;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::client::Client;
use crate::errors::{Error, Problem, Result};

/// Refresh this many seconds before the access token expires.
const REFRESH_SKEW_SECS: i64 = 30;

/// A session as the server returns it (and as session stores keep it).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthSession {
    pub token: String,
    /// Superuser logins: legacy expiry field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<serde_json::Value>,
    /// App users: single-use refresh token (30 days).
    #[serde(
        default,
        rename = "refreshToken",
        skip_serializing_if = "Option::is_none"
    )]
    pub refresh_token: Option<String>,
    /// App users: RFC 3339 expiry of the access token.
    #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Who is signed in, as seen by [`AuthService::on_change`].
#[derive(Debug, Clone, PartialEq)]
pub enum AuthState {
    /// Before the stored session was looked at.
    Unknown,
    SignedOut,
    /// Carries the user object.
    SignedIn(Option<serde_json::Value>),
}

/// Public sign-in methods of an app (`GET auth/providers`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PublicAuth {
    pub app: String,
    #[serde(rename = "emailPassword")]
    pub email_password: EmailPasswordConfig,
    pub providers: Vec<ProviderInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EmailPasswordConfig {
    pub enabled: bool,
    #[serde(rename = "allowSignup")]
    pub allow_signup: bool,
    #[serde(rename = "minPasswordLength")]
    pub min_password_length: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(rename = "startPath")]
    pub start_path: String,
}

/// Where the app-user session lives between runs.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Option<AuthSession>;
    fn save(&self, session: &AuthSession);
    fn clear(&self);
}

/// Default store: the session lives as long as the process.
#[derive(Default)]
pub struct MemorySessionStore(Mutex<Option<AuthSession>>);

impl SessionStore for MemorySessionStore {
    fn load(&self) -> Option<AuthSession> {
        self.0.lock().ok()?.clone()
    }
    fn save(&self, session: &AuthSession) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(session.clone());
        }
    }
    fn clear(&self) {
        if let Ok(mut g) = self.0.lock() {
            *g = None;
        }
    }
}

/// JSON file for scripts and CLIs. Written atomically, mode 0600 on Unix (it holds a refresh token).
/// Same file format as the Python SDK.
pub struct FileSessionStore {
    path: PathBuf,
}

impl FileSessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl SessionStore for FileSessionStore {
    fn load(&self) -> Option<AuthSession> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let s: AuthSession = serde_json::from_str(&text).ok()?;
        (!s.token.is_empty()).then_some(s)
    }

    fn save(&self, session: &AuthSession) {
        use std::io::Write;
        let Ok(json) = serde_json::to_vec(session) else {
            return;
        };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let written = opts
            .open(&tmp)
            .and_then(|mut f| f.write_all(&json))
            .and_then(|_| std::fs::rename(&tmp, &self.path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Session state shared by every clone of a [`Client`].
pub(crate) struct AuthCore {
    store: Arc<dyn SessionStore>,
    session: RwLock<Option<AuthSession>>,
    /// Held across a refresh call: concurrent callers wait, then see the new token.
    refresh_lock: tokio::sync::Mutex<()>,
    state: watch::Sender<AuthState>,
}

impl AuthCore {
    pub(crate) fn new(store: Arc<dyn SessionStore>) -> Self {
        let session = store.load();
        let state = match &session {
            Some(s) => AuthState::SignedIn(s.user.clone()),
            None => AuthState::SignedOut,
        };
        Self {
            store,
            session: RwLock::new(session),
            refresh_lock: tokio::sync::Mutex::new(()),
            state: watch::Sender::new(state),
        }
    }

    pub(crate) fn session(&self) -> Option<AuthSession> {
        self.session.read().ok()?.clone()
    }

    fn set_session(&self, s: Option<AuthSession>) {
        if let Ok(mut g) = self.session.write() {
            *g = s;
        }
    }

    /// Drops the in-memory session (a bearer token was set by hand).
    pub(crate) fn forget(&self) {
        if self.session().is_some() {
            self.set_session(None);
            self.state.send_replace(AuthState::SignedOut);
        }
    }
}

fn expiry_unix(s: &AuthSession) -> Option<i64> {
    parse_rfc3339(s.expires_at.as_deref()?)
}

fn needs_refresh(s: &AuthSession) -> bool {
    s.refresh_token.is_some() && expiry_unix(s).is_some_and(|e| e - now_unix() <= REFRESH_SKEW_SECS)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `2026-11-08T10:00:00.123456Z` or with a `+hh:mm` offset, to unix seconds.
pub(crate) fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut i = 19;
    if b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    let offset = match *b.get(i)? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let off = num(i + 1..i + 3)? * 3600 + num(i + 4..i + 6)? * 60;
            if sign == b'+' { off } else { -off }
        }
        _ => return None,
    };
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss - offset)
}

// Refresh machinery: called by `Client::send`, so every request path shares it.
impl Client {
    /// Stores `s` as the current session: bearer token, memory, store, listeners.
    pub(crate) fn apply_session(&self, s: AuthSession) {
        let core = self.auth_core();
        self.set_bearer(Some(s.token.clone()));
        core.store.save(&s);
        let state = AuthState::SignedIn(s.user.clone());
        core.set_session(Some(s));
        core.state.send_replace(state);
    }

    pub(crate) fn clear_session(&self) {
        let core = self.auth_core();
        self.set_bearer(None);
        core.store.clear();
        core.set_session(None);
        core.state.send_replace(AuthState::SignedOut);
    }

    /// Refreshes an app-user session whose access token is about to expire.
    pub(crate) async fn ensure_fresh(&self) -> Result<()> {
        let core = self.auth_core();
        if !core.session().is_some_and(|s| needs_refresh(&s)) {
            return Ok(());
        }
        let _guard = core.refresh_lock.lock().await;
        // Another task may have refreshed while we waited.
        let Some(s) = core.session().filter(needs_refresh) else {
            return Ok(());
        };
        match self.do_refresh(&s).await {
            Ok(_) => Ok(()),
            // Still valid for a few seconds: let the request try; fail only when really expired.
            Err(_) if expiry_unix(&s).is_some_and(|x| x > now_unix()) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// True when the caller should retry once with the new token.
    pub(crate) async fn refresh_after_unauthorized(&self, used: &str) -> bool {
        let core = self.auth_core();
        let _guard = core.refresh_lock.lock().await;
        let Some(s) = core.session() else {
            return false;
        };
        if s.refresh_token.is_none() {
            return false;
        }
        if s.token != used {
            return true; // someone refreshed already
        }
        self.do_refresh(&s).await.is_ok()
    }

    /// Caller holds `refresh_lock`.
    async fn do_refresh(&self, s: &AuthSession) -> Result<AuthSession> {
        let Some(rt) = s.refresh_token.as_deref() else {
            return Err(Error::config("no refresh token"));
        };
        let body = serde_json::json!({ "refreshToken": rt });
        let res: Result<AuthSession> = self
            .json_request(Method::POST, &auth_path(self, "refresh"), &body, false)
            .await;
        match res {
            Ok(mut new) => {
                if new.user.is_none() {
                    new.user = s.user.clone();
                }
                self.apply_session(new.clone());
                Ok(new)
            }
            Err(e) => {
                // 400/401: the refresh token is rejected, revoked or already used.
                if matches!(e.status(), Some(400 | 401)) {
                    self.clear_session();
                }
                Err(e)
            }
        }
    }
}

fn auth_path(client: &Client, tail: &str) -> String {
    let app = url::form_urlencoded::byte_serialize(client.app().as_bytes()).collect::<String>();
    format!("/api/v1/apps/{app}/auth/{tail}")
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

    fn is_system(&self) -> bool {
        self.collection == "_superusers" || self.collection == "system"
    }

    fn path(&self, tail: &str) -> String {
        auth_path(&self.client, tail)
    }

    /// Returns the currently active bearer token, if any.
    pub fn token(&self) -> Option<String> {
        self.client.token()
    }

    /// Sets or updates the active authentication token in the client (no refresh token).
    pub fn set_token(&self, token: Option<impl Into<String>>) {
        self.client.set_token(token);
    }

    /// Signs out on this device only: clears the token and the stored session, no network call.
    /// Use [`logout`](Self::logout) to also revoke the refresh token on the server.
    pub fn sign_out(&self) {
        self.client.clear_session();
    }

    /// Signs out locally, then revokes the refresh token on the server.
    pub async fn logout(&self) -> Result<()> {
        let refresh = self
            .client
            .auth_core()
            .session()
            .and_then(|s| s.refresh_token);
        self.client.clear_session();
        if let Some(rt) = refresh {
            let body = serde_json::json!({ "refreshToken": rt });
            self.client
                .json_request::<_, ()>(Method::POST, &self.path("logout"), &body, false)
                .await?;
        }
        Ok(())
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
    /// For `_superusers` or `system`, this uses the system login endpoint (`/api/v1/system/auth/login`)
    /// and keeps a plain token. For other collections it is [`sign_in`](Self::sign_in).
    pub async fn sign_in_with_password(&self, email: &str, password: &str) -> Result<AuthSession> {
        if !self.is_system() {
            return self.sign_in(email, password).await;
        }
        let body = serde_json::json!({ "email": email, "password": password });
        let session: AuthSession = self.client.post("/api/v1/system/auth/login", &body).await?;
        self.set_token(Some(&session.token));
        Ok(session)
    }

    // ---- app users -------------------------------------------------------------

    /// Public: email/password flags and the enabled OAuth providers.
    pub async fn providers(&self) -> Result<PublicAuth> {
        self.client.get(&self.path("providers"), None).await
    }

    /// Creates an account and signs it in.
    pub async fn sign_up(
        &self,
        email: &str,
        password: &str,
        name: Option<&str>,
    ) -> Result<AuthSession> {
        let mut body = serde_json::json!({ "email": email, "password": password });
        if let Some(n) = name {
            body["name"] = n.into();
        }
        self.session_call("register", &body).await
    }

    pub async fn sign_in(&self, email: &str, password: &str) -> Result<AuthSession> {
        let body = serde_json::json!({ "email": email, "password": password });
        self.session_call("login", &body).await
    }

    async fn session_call(&self, tail: &str, body: &serde_json::Value) -> Result<AuthSession> {
        let session: AuthSession = self
            .client
            .json_request(Method::POST, &self.path(tail), body, false)
            .await?;
        self.client.apply_session(session.clone());
        Ok(session)
    }

    /// Forces a refresh (rotation). Concurrent callers share one refresh.
    pub async fn refresh(&self) -> Result<AuthSession> {
        let core = self.client.auth_core();
        let seen = core.session();
        let _guard = core.refresh_lock.lock().await;
        let Some(s) = core.session() else {
            return Err(Error::config("not signed in"));
        };
        // Someone refreshed while we waited: that counts.
        if seen.is_some_and(|x| x.token != s.token) {
            return Ok(s);
        }
        self.client.do_refresh(&s).await
    }

    /// The signed-in user, if any (from the last server response).
    pub fn user(&self) -> Option<serde_json::Value> {
        self.client.auth_core().session().and_then(|s| s.user)
    }

    /// Current auth state.
    pub fn state(&self) -> AuthState {
        self.client.auth_core().state.borrow().clone()
    }

    /// Stream of [`AuthState`] changes (`changed().await`, `borrow()`), starting with the current value.
    pub fn on_change(&self) -> watch::Receiver<AuthState> {
        self.client.auth_core().state.subscribe()
    }

    /// The current user, fetched from the server (superuser services: `/api/v1/system/me`).
    pub async fn me(&self) -> Result<serde_json::Value> {
        if self.is_system() {
            return self.client.get("/api/v1/system/me", None).await;
        }
        let user: serde_json::Value = self.client.get(&self.path("me"), None).await?;
        self.store_user(&user);
        Ok(user)
    }

    pub async fn update_profile(&self, name: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "name": name });
        let user: serde_json::Value = self.client.patch(&self.path("me"), &body).await?;
        self.store_user(&user);
        Ok(user)
    }

    fn store_user(&self, user: &serde_json::Value) {
        if let Some(mut s) = self.client.auth_core().session() {
            s.user = Some(user.clone());
            self.client.apply_session(s);
        }
    }

    /// The server ends every session of the user, so on success this signs out locally; sign in again.
    pub async fn change_password(&self, old_password: &str, new_password: &str) -> Result<()> {
        let body = serde_json::json!({ "oldPassword": old_password, "newPassword": new_password });
        self.client
            .post::<_, ()>(&self.path("password"), &body)
            .await?;
        self.client.clear_session();
        Ok(())
    }

    /// A token valid right now: refreshes first when it is about to expire.
    pub async fn access_token(&self) -> Result<Option<String>> {
        self.client.ensure_fresh().await?;
        Ok(self.client.token())
    }

    // ---- OAuth for servers, scripts and desktop apps ------------------------------

    /// URL to open in a browser. `redirect_uri` must be in the app's allowed redirects.
    pub fn provider_sign_in_url(&self, provider: &str, redirect_uri: &str) -> Result<String> {
        let p = url::form_urlencoded::byte_serialize(provider.as_bytes()).collect::<String>();
        let url = self.client.build_url(
            &self.path(&format!("oauth/{p}/start")),
            Some(&[("redirectUri", redirect_uri)]),
        )?;
        Ok(url.into())
    }

    /// Finishes a provider sign-in from the redirect URL, its `#fragment`, or a bare `token=...&...` string.
    /// A `#error=` fragment becomes an [`Error::Api`] with status 0 and the provider error as problem type.
    pub async fn handle_redirect(&self, url_or_fragment: &str) -> Result<AuthSession> {
        let fragment = match url_or_fragment.split_once('#') {
            Some((_, f)) => f,
            None if url_or_fragment.starts_with("token=")
                || url_or_fragment.starts_with("error=") =>
            {
                url_or_fragment
            }
            None => "",
        };
        let mut token = None;
        let mut session = AuthSession::default();
        let mut error = None;
        let mut description = None;
        for (k, v) in url::form_urlencoded::parse(fragment.as_bytes()) {
            let v = v.into_owned();
            match &*k {
                "token" => token = Some(v),
                "refreshToken" => session.refresh_token = Some(v),
                "expiresAt" => session.expires_at = Some(v),
                "error" => error = Some(v),
                "error_description" => description = Some(v),
                _ => {}
            }
        }
        if let Some(code) = error {
            let detail = description.unwrap_or_else(|| code.clone());
            return Err(Error::api(
                0,
                Problem::new(code, "Provider sign-in failed", 0).with_detail(detail),
            ));
        }
        let Some(token) = token.filter(|t| !t.is_empty()) else {
            return Err(Error::api(
                0,
                Problem::new("invalid_redirect", "No session in redirect", 0),
            ));
        };
        session.token = token.clone();

        // The fragment carries no user: fetch it with the new token before announcing SignedIn.
        let core = self.client.auth_core();
        let prev_bearer = self.client.token();
        let prev_session = core.session();
        core.set_session(Some(session.clone()));
        self.client.set_bearer(Some(token));
        match self
            .client
            .get::<serde_json::Value>(&self.path("me"), None)
            .await
        {
            Ok(user) => {
                session.user = Some(user);
                // A refresh during the call may have rotated the tokens.
                if let Some(cur) = core.session() {
                    session.token = cur.token;
                    session.refresh_token = cur.refresh_token;
                    session.expires_at = cur.expires_at;
                }
                self.client.apply_session(session.clone());
                Ok(session)
            }
            Err(e) => {
                core.set_session(prev_session);
                self.client.set_bearer(prev_bearer);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_rfc3339;

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2026-11-08T10:00:00.123456Z"),
            Some(1_794_132_000)
        );
        assert_eq!(
            parse_rfc3339("2026-11-08T12:00:00+02:00"),
            Some(1_794_132_000)
        );
        assert_eq!(parse_rfc3339("nope"), None);
    }
}
