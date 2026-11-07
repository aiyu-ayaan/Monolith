//! App-user auth against a stateful axum fake server.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use monolith_client::{AuthState, Client, Error, FileSessionStore, Monolith, SessionStore};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const FAR: &str = "2999-01-01T00:00:00Z";
const PAST: &str = "2000-01-01T00:00:00Z";

#[derive(Default)]
struct St {
    n: u32,
    valid: HashSet<String>,
    refresh_tokens: HashSet<String>,
    refresh_calls: u32,
    logout_calls: u32,
    record_hits: u32,
    last_bearer: String,
    /// Sessions handed out from now on expire immediately (client sees an expired expiresAt).
    expired: bool,
    /// New access tokens are not accepted by the records route (to test the single retry).
    reject_new: bool,
    refresh_delay_ms: u64,
    name: String,
}

type Shared = Arc<Mutex<St>>;

fn session(st: &mut St) -> Value {
    st.n += 1;
    let (at, rt) = (format!("at{}", st.n), format!("rt{}", st.n));
    if !st.reject_new {
        st.valid.insert(at.clone());
    }
    st.refresh_tokens.insert(rt.clone());
    json!({
        "token": at, "refreshToken": rt,
        "expiresAt": if st.expired { PAST } else { FAR },
        "user": {"id": "u1", "email": "a@b.co", "name": st.name},
    })
}

fn problem(status: u16, title: &str) -> Response {
    (
        StatusCode::from_u16(status).unwrap(),
        [("content-type", "application/problem+json")],
        json!({"type": "about:blank", "title": title, "status": status}).to_string(),
    )
        .into_response()
}

fn bearer(h: &HeaderMap) -> String {
    h.get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string()
}

async fn spawn() -> (String, Shared) {
    let st: Shared = Arc::new(Mutex::new(St {
        name: "A".into(),
        ..Default::default()
    }));
    let base = "/api/v1/apps/{app}/auth";
    let router = Router::new()
        .route(
            &format!("{base}/providers"),
            get(|| async {
                Json(json!({
                    "app": "shop",
                    "emailPassword": {"enabled": true, "allowSignup": true, "minPasswordLength": 8},
                    "providers": [{"id": "google", "name": "Google", "startPath": "x"}]
                }))
            }),
        )
        .route(
            &format!("{base}/register"),
            post(
                |State(st): State<Shared>, Json(b): Json<Value>| async move {
                    if b["email"] == "taken@b.co" {
                        return problem(409, "Email taken");
                    }
                    let s = session(&mut st.lock().unwrap());
                    (StatusCode::CREATED, Json(s)).into_response()
                },
            ),
        )
        .route(
            &format!("{base}/login"),
            post(
                |State(st): State<Shared>, Json(b): Json<Value>| async move {
                    if b["password"] != "secret123" {
                        return problem(401, "Invalid credentials");
                    }
                    Json(session(&mut st.lock().unwrap())).into_response()
                },
            ),
        )
        .route(
            &format!("{base}/refresh"),
            post(
                |State(st): State<Shared>, Json(b): Json<Value>| async move {
                    let delay = {
                        let mut g = st.lock().unwrap();
                        g.refresh_calls += 1;
                        g.refresh_delay_ms
                    };
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    let mut g = st.lock().unwrap();
                    let rt = b["refreshToken"].as_str().unwrap_or_default().to_string();
                    if !g.refresh_tokens.remove(&rt) {
                        return problem(401, "Invalid refresh token");
                    }
                    Json(session(&mut g)).into_response()
                },
            ),
        )
        .route(
            &format!("{base}/logout"),
            post(
                |State(st): State<Shared>, Json(b): Json<Value>| async move {
                    let mut g = st.lock().unwrap();
                    g.logout_calls += 1;
                    g.refresh_tokens
                        .remove(b["refreshToken"].as_str().unwrap_or_default());
                    StatusCode::NO_CONTENT
                },
            ),
        )
        .route(
            &format!("{base}/me"),
            get(|State(st): State<Shared>, h: HeaderMap| async move {
                let g = st.lock().unwrap();
                if !g.valid.contains(&bearer(&h)) {
                    return problem(401, "Unauthorized");
                }
                Json(json!({"id": "u1", "email": "a@b.co", "name": g.name})).into_response()
            })
            .patch(
                |State(st): State<Shared>, h: HeaderMap, Json(b): Json<Value>| async move {
                    let mut g = st.lock().unwrap();
                    if !g.valid.contains(&bearer(&h)) {
                        return problem(401, "Unauthorized");
                    }
                    g.name = b["name"].as_str().unwrap_or_default().to_string();
                    Json(json!({"id": "u1", "email": "a@b.co", "name": g.name})).into_response()
                },
            ),
        )
        .route(
            &format!("{base}/password"),
            post(
                |State(st): State<Shared>, h: HeaderMap, Json(b): Json<Value>| async move {
                    let mut g = st.lock().unwrap();
                    if !g.valid.contains(&bearer(&h)) {
                        return problem(401, "Unauthorized");
                    }
                    if b["newPassword"] == "short" {
                        return problem(400, "Bad password");
                    }
                    g.refresh_tokens.clear();
                    StatusCode::NO_CONTENT.into_response()
                },
            ),
        )
        .route(
            "/api/v1/apps/{app}/collections/{coll}/records",
            get(|State(st): State<Shared>, h: HeaderMap| async move {
                let mut g = st.lock().unwrap();
                g.record_hits += 1;
                let b = bearer(&h);
                g.last_bearer = b.clone();
                if !g.valid.contains(&b) {
                    return problem(401, "Unauthorized");
                }
                Json(json!({"items": []})).into_response()
            }),
        )
        .with_state(st.clone());
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, st)
}

fn client(url: &str) -> Client {
    Monolith::init(url, "shop").unwrap()
}

fn status(e: &Error) -> Option<u16> {
    e.status()
}

#[tokio::test]
async fn providers_sign_up_in_out_and_state() {
    let (url, st) = spawn().await;
    let mono = client(&url);
    let auth = mono.auth();
    let mut rx = auth.on_change();
    assert_eq!(*rx.borrow(), AuthState::SignedOut);

    let p = auth.providers().await.unwrap();
    assert_eq!(p.providers[0].id, "google");
    assert_eq!(p.email_password.min_password_length, 8);

    let s = auth
        .sign_up("a@b.co", "secret123", Some("A"))
        .await
        .unwrap();
    assert!(rx.has_changed().unwrap());
    assert!(matches!(&*rx.borrow_and_update(), AuthState::SignedIn(Some(u)) if u["id"] == "u1"));
    assert_eq!(auth.user().unwrap()["id"], "u1");
    assert_eq!(
        auth.access_token().await.unwrap().as_deref(),
        Some(s.token.as_str())
    );

    auth.logout().await.unwrap();
    assert_eq!(st.lock().unwrap().logout_calls, 1);
    assert_eq!(auth.state(), AuthState::SignedOut);
    assert_eq!(auth.access_token().await.unwrap(), None);

    let s2 = auth.sign_in("a@b.co", "secret123").await.unwrap();
    mono.records("tasks").list(None).await.unwrap();
    assert_eq!(st.lock().unwrap().last_bearer, s2.token);

    // Local-only sign out keeps working without a network call.
    auth.sign_out();
    assert_eq!(auth.state(), AuthState::SignedOut);
    assert_eq!(st.lock().unwrap().logout_calls, 1);
}

#[tokio::test]
async fn error_mapping() {
    let (url, _st) = spawn().await;
    let auth = client(&url).auth();
    let e = auth.sign_in("a@b.co", "wrong").await.unwrap_err();
    assert_eq!(status(&e), Some(401));
    assert_eq!(e.problem().unwrap().title, "Invalid credentials");
    assert_eq!(auth.state(), AuthState::SignedOut);
    let e = auth
        .sign_up("taken@b.co", "secret123", None)
        .await
        .unwrap_err();
    assert_eq!(status(&e), Some(409));
}

#[tokio::test]
async fn profile_and_password() {
    let (url, _st) = spawn().await;
    let auth = client(&url).auth();
    auth.sign_in("a@b.co", "secret123").await.unwrap();
    assert_eq!(auth.update_profile("Zed").await.unwrap()["name"], "Zed");
    assert_eq!(auth.user().unwrap()["name"], "Zed");
    assert_eq!(auth.me().await.unwrap()["name"], "Zed");
    let e = auth
        .change_password("secret123", "short")
        .await
        .unwrap_err();
    assert_eq!(status(&e), Some(400));
    assert!(matches!(auth.state(), AuthState::SignedIn(_)));
    auth.change_password("secret123", "newsecret123")
        .await
        .unwrap();
    assert_eq!(auth.state(), AuthState::SignedOut);
}

#[tokio::test]
async fn refresh_rotates_tokens() {
    let (url, st) = spawn().await;
    let auth = client(&url).auth();
    let first = auth.sign_in("a@b.co", "secret123").await.unwrap();
    let second = auth.refresh().await.unwrap();
    assert_ne!(first.token, second.token);
    assert_ne!(first.refresh_token, second.refresh_token);
    assert!(
        !st.lock()
            .unwrap()
            .refresh_tokens
            .contains(first.refresh_token.as_deref().unwrap())
    );
}

#[tokio::test]
async fn expired_token_is_refreshed_once_for_concurrent_requests() {
    let (url, st) = spawn().await;
    let mono = client(&url);
    {
        let mut g = st.lock().unwrap();
        g.expired = true; // login hands out an already expired access token
        g.refresh_delay_ms = 150;
    }
    mono.auth().sign_in("a@b.co", "secret123").await.unwrap();
    st.lock().unwrap().expired = false;

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let m = mono.clone();
        tasks.push(tokio::spawn(
            async move { m.records("tasks").list(None).await },
        ));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(st.lock().unwrap().refresh_calls, 1);
}

#[tokio::test]
async fn retries_once_after_401_single_flight() {
    let (url, st) = spawn().await;
    let mono = client(&url);
    mono.auth().sign_in("a@b.co", "secret123").await.unwrap();
    {
        let mut g = st.lock().unwrap();
        g.valid.clear(); // server revoked the access token; refresh token still good
        g.refresh_delay_ms = 150;
    }
    let mut tasks = Vec::new();
    for _ in 0..6 {
        let m = mono.clone();
        tasks.push(tokio::spawn(
            async move { m.records("tasks").list(None).await },
        ));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(st.lock().unwrap().refresh_calls, 1);
}

#[tokio::test]
async fn retry_happens_only_once() {
    let (url, st) = spawn().await;
    let mono = client(&url);
    mono.auth().sign_in("a@b.co", "secret123").await.unwrap();
    {
        let mut g = st.lock().unwrap();
        g.valid.clear();
        g.reject_new = true; // the refreshed token is rejected too
    }
    let e = mono.records("tasks").list(None).await.unwrap_err();
    assert_eq!(status(&e), Some(401));
    let g = st.lock().unwrap();
    assert_eq!(g.record_hits, 2);
    assert_eq!(g.refresh_calls, 1);
}

#[tokio::test]
async fn dead_refresh_token_signs_out() {
    let (url, st) = spawn().await;
    let mono = client(&url);
    let auth = mono.auth();
    auth.sign_in("a@b.co", "secret123").await.unwrap();
    {
        let mut g = st.lock().unwrap();
        g.valid.clear();
        g.refresh_tokens.clear();
    }
    let e = mono.records("tasks").list(None).await.unwrap_err();
    assert_eq!(status(&e), Some(401));
    assert_eq!(auth.state(), AuthState::SignedOut);
    assert_eq!(mono.token(), None);
}

#[tokio::test]
async fn restore_from_file_store() {
    let (url, st) = spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("session.json");
    let build = || {
        Monolith::builder(&url, "shop")
            .session_file(&path)
            .build()
            .unwrap()
    };

    let s = build().auth().sign_in("a@b.co", "secret123").await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // New process: restored without a network call.
    let m2 = build();
    assert!(matches!(m2.auth().state(), AuthState::SignedIn(Some(_))));
    assert_eq!(m2.token().as_deref(), Some(s.token.as_str()));
    m2.records("tasks").list(None).await.unwrap();
    assert_eq!(st.lock().unwrap().refresh_calls, 0);

    // Stored access token expired: refreshed on first use and the file is updated.
    let mut stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    stored["expiresAt"] = json!(PAST);
    std::fs::write(&path, stored.to_string()).unwrap();
    let m3 = build();
    m3.records("tasks").list(None).await.unwrap();
    assert_eq!(st.lock().unwrap().refresh_calls, 1);
    let on_disk = FileSessionStore::new(&path).load().unwrap();
    assert_ne!(on_disk.token, s.token);
    assert_eq!(Some(on_disk.token), m3.token());

    m3.auth().sign_out();
    assert!(!path.exists());
    assert_eq!(build().auth().state(), AuthState::SignedOut);

    // Corrupt file: signed out, no panic.
    std::fs::write(&path, "{not json").unwrap();
    assert_eq!(build().auth().state(), AuthState::SignedOut);
}

#[tokio::test]
async fn provider_url_and_redirect() {
    let (url, st) = spawn().await;
    let auth = client(&url).auth();
    assert_eq!(
        auth.provider_sign_in_url("google", "http://127.0.0.1:9000/cb")
            .unwrap(),
        format!(
            "{url}/api/v1/apps/shop/auth/oauth/google/start?redirectUri=http%3A%2F%2F127.0.0.1%3A9000%2Fcb"
        )
    );

    let sess = session(&mut st.lock().unwrap());
    let frag = format!(
        "token={}&refreshToken={}&expiresAt={}",
        sess["token"].as_str().unwrap(),
        sess["refreshToken"].as_str().unwrap(),
        "2999-01-01T00%3A00%3A00Z"
    );
    for arg in [
        format!("http://127.0.0.1:9000/cb#{frag}"),
        format!("#{frag}"),
        frag.clone(),
    ] {
        auth.sign_out();
        let s = auth.handle_redirect(&arg).await.unwrap();
        assert_eq!(Some(s.token.as_str()), sess["token"].as_str());
        assert_eq!(s.refresh_token.as_deref(), sess["refreshToken"].as_str());
        assert_eq!(s.expires_at.as_deref(), Some(FAR));
        assert_eq!(auth.user().unwrap()["id"], "u1"); // fetched from /me
    }
}

#[tokio::test]
async fn redirect_errors() {
    let (url, _st) = spawn().await;
    let mono = client(&url);
    let auth = mono.auth();
    let e = auth
        .handle_redirect("http://x/cb#error=oauth_failed&error_description=email%20taken")
        .await
        .unwrap_err();
    let p = e.problem().unwrap();
    assert_eq!(p.problem_type, "oauth_failed");
    assert_eq!(p.detail.as_deref(), Some("email taken"));
    let e = auth.handle_redirect("http://x/cb").await.unwrap_err();
    assert_eq!(e.problem().unwrap().problem_type, "invalid_redirect");
    // A token the server rejects leaves the client signed out.
    let e = auth.handle_redirect("#token=bogus").await.unwrap_err();
    assert_eq!(status(&e), Some(401));
    assert_eq!(auth.state(), AuthState::SignedOut);
    assert_eq!(mono.token(), None);
}
