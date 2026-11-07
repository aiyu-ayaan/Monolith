use monolith_core::{AuthConfigIn, CoreError, RegisterIn, Registry, UserEdit};
use serde_json::{Value as J, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn cfg(v: J) -> AuthConfigIn {
    serde_json::from_value(v).unwrap()
}

fn reg_in(email: &str, pw: &str) -> RegisterIn {
    RegisterIn {
        email: email.into(),
        password: pw.into(),
        name: "Ada".into(),
    }
}

async fn setup(app: &str) -> (tempfile::TempDir, Registry) {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app(app).await.unwrap();
    (dir, reg)
}

/// Fake OIDC provider: discovery, token and userinfo endpoints on a local port.
async fn oidc(email: &str, verified: bool) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let (b2, email) = (base.clone(), email.to_owned());
    tokio::spawn(async move {
        loop {
            let Ok((mut c, _)) = l.accept().await else {
                return;
            };
            let (b, email) = (b2.clone(), email.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = c.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_owned();
                let body = match path.as_str() {
                    "/.well-known/openid-configuration" => json!({
                        "authorization_endpoint": format!("{b}/authorize"),
                        "token_endpoint": format!("{b}/token"),
                        "userinfo_endpoint": format!("{b}/userinfo")}),
                    "/token" => {
                        assert!(req.contains("code_verifier="), "PKCE verifier must be sent");
                        assert!(req.contains("code=the-code"));
                        json!({"access_token": "at-1"})
                    }
                    "/userinfo" => {
                        assert!(req.to_ascii_lowercase().contains("authorization: bearer at-1"));
                        json!({"sub": "sub-42", "email": email, "email_verified": verified, "name": "Grace"})
                    }
                    _ => json!({}),
                }
                .to_string();
                let _ = c
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len())
                            .as_bytes(),
                    )
                    .await;
            });
        }
    });
    base
}

#[tokio::test(flavor = "multi_thread")]
async fn email_password_lifecycle() {
    let (_d, reg) = setup("shop").await;
    // Public config reflects the defaults.
    let pa = reg.public_auth("shop").await.unwrap();
    assert!(pa.email_password.enabled && pa.email_password.allow_signup && pa.providers.is_empty());

    let s = reg
        .register_user("shop", reg_in("Ada@Example.com ", "correct horse"))
        .await
        .unwrap();
    assert_eq!(s.user.email.as_deref(), Some("ada@example.com"));
    assert_eq!(s.user.providers, ["password"]);
    // The access token authenticates for this app only.
    let p = reg.verify_user("shop", &s.token).unwrap();
    assert_eq!(p.auth.as_ref().unwrap().collection.as_ref(), "users");
    assert!(reg.verify_user("other", &s.token).is_none());
    assert!(reg.verify_superuser(&s.token).is_none());

    // Duplicate email, weak password and bad email are refused.
    assert!(matches!(
        reg.register_user("shop", reg_in("ada@example.com", "another one 1"))
            .await,
        Err(CoreError::Conflict(_))
    ));
    assert!(matches!(
        reg.register_user("shop", reg_in("b@example.com", "short"))
            .await,
        Err(CoreError::Validation(_))
    ));
    assert!(matches!(
        reg.register_user("shop", reg_in("nope", "long enough pw"))
            .await,
        Err(CoreError::Validation(_))
    ));

    // Sign in; wrong password and unknown email look the same.
    let again = reg
        .sign_in_user("shop", "ada@example.com", "correct horse")
        .await
        .unwrap();
    assert_eq!(again.user.id, s.user.id);
    assert!(matches!(
        reg.sign_in_user("shop", "ada@example.com", "wrong").await,
        Err(CoreError::Unauthorized)
    ));
    assert!(matches!(
        reg.sign_in_user("shop", "who@example.com", "correct horse")
            .await,
        Err(CoreError::Unauthorized)
    ));

    // Refresh rotates: the old refresh token dies.
    let r = reg.refresh_session("shop", &s.refresh_token).await.unwrap();
    assert!(matches!(
        reg.refresh_session("shop", &s.refresh_token).await,
        Err(CoreError::Unauthorized)
    ));
    assert!(matches!(
        reg.refresh_session("other", &r.refresh_token).await,
        Err(CoreError::Unauthorized)
    ));
    reg.sign_out_user("shop", &r.refresh_token).await.unwrap();
    assert!(matches!(
        reg.refresh_session("shop", &r.refresh_token).await,
        Err(CoreError::Unauthorized)
    ));

    // Changing the password ends all sessions and the old password stops working.
    let id = monolith_store::id::decode(&s.user.id).unwrap();
    reg.change_password("shop", &id, "correct horse", "brand new pass")
        .await
        .unwrap();
    assert!(matches!(
        reg.refresh_session("shop", &again.refresh_token).await,
        Err(CoreError::Unauthorized)
    ));
    assert!(
        reg.sign_in_user("shop", "ada@example.com", "correct horse")
            .await
            .is_err()
    );
    reg.sign_in_user("shop", "ada@example.com", "brand new pass")
        .await
        .unwrap();

    // Console: disable blocks sign-in and refresh; list pages; delete removes.
    let live = reg
        .sign_in_user("shop", "ada@example.com", "brand new pass")
        .await
        .unwrap();
    let u = reg
        .update_user(
            "shop",
            &s.user.id,
            UserEdit {
                disabled: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(u.disabled);
    assert!(
        reg.sign_in_user("shop", "ada@example.com", "brand new pass")
            .await
            .is_err()
    );
    assert!(
        reg.refresh_session("shop", &live.refresh_token)
            .await
            .is_err()
    );
    reg.create_user("shop", reg_in("bob@example.com", "bobs password"))
        .await
        .unwrap();
    let (page, next) = reg.list_users("shop", 1, None).await.unwrap();
    assert_eq!((page.len(), next.is_some()), (1, true));
    let (rest, none) = reg.list_users("shop", 1, next.as_deref()).await.unwrap();
    assert_eq!((rest.len(), none), (1, None));
    reg.delete_user("shop", &s.user.id).await.unwrap();
    assert_eq!(reg.list_users("shop", 10, None).await.unwrap().0.len(), 1);

    // Turning sign-up off closes registration but not sign-in.
    reg.put_auth_config(
        "shop",
        cfg(json!({"emailPassword":{"enabled":true,"allowSignup":false,"minPasswordLength":8}})),
    )
    .await
    .unwrap();
    assert!(matches!(
        reg.register_user("shop", reg_in("c@example.com", "long enough pw"))
            .await,
        Err(CoreError::Forbidden(_))
    ));
    reg.sign_in_user("shop", "bob@example.com", "bobs password")
        .await
        .unwrap();
    // Turning the method off hides it.
    reg.put_auth_config(
        "shop",
        cfg(json!({"emailPassword":{"enabled":false,"allowSignup":true,"minPasswordLength":8}})),
    )
    .await
    .unwrap();
    let pa = reg.public_auth("shop").await.unwrap();
    assert!(!pa.email_password.enabled && !pa.email_password.allow_signup);
}

#[tokio::test(flavor = "multi_thread")]
async fn oauth_oidc_flow_creates_links_and_guards_redirects() {
    let (_d, reg) = setup("app").await;
    let issuer = oidc("grace@example.com", true).await;
    reg.put_auth_config(
        "app",
        cfg(json!({
            "providers":[{"id":"oidc","enabled":true,"clientId":"cid","clientSecret":"sec","extra":{"issuer":issuer}},
                         {"id":"apple","enabled":true,"clientId":"a","clientSecret":"b","extra":{"teamId":"t","keyId":"k"}}],
            "allowedRedirects":["https://app.example.com/auth","myapp://auth*"]
        })),
    )
    .await
    .unwrap();

    // The sign-in screen lists working providers only (Apple has no exchange yet).
    let pa = reg.public_auth("app").await.unwrap();
    assert_eq!(pa.providers.len(), 1);
    assert_eq!(pa.providers[0].id, "oidc");
    assert_eq!(
        pa.providers[0].start_path,
        "/api/v1/apps/app/auth/oauth/oidc/start"
    );

    // Redirect guard.
    let origin = "https://api.example.com";
    assert!(
        reg.oauth_start("app", "oidc", "https://evil.example.com/", origin)
            .await
            .is_err()
    );
    assert!(
        reg.oauth_start("app", "github", "https://app.example.com/auth", origin)
            .await
            .is_err()
    );

    let url = reg
        .oauth_start("app", "oidc", "https://app.example.com/auth", origin)
        .await
        .unwrap();
    let u = reqwest::Url::parse(&url).unwrap();
    let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], "cid");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(
        q["redirect_uri"],
        "https://api.example.com/api/v1/apps/app/auth/oauth/oidc/callback"
    );

    // Wrong state is refused; the right one yields a redirect carrying the session.
    assert!(
        reg.oauth_callback("app", "oidc", Some("the-code"), Some("bogus"), None)
            .await
            .is_err()
    );
    let back = reg
        .oauth_callback("app", "oidc", Some("the-code"), Some(&q["state"]), None)
        .await
        .unwrap();
    assert!(
        back.starts_with("https://app.example.com/auth#token=u1."),
        "{back}"
    );
    let frag: std::collections::HashMap<_, _> = reqwest::Url::parse(&back)
        .unwrap()
        .fragment()
        .unwrap()
        .split('&')
        .map(|p| p.split_once('=').unwrap())
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    let token = urlencoding_decode(&frag["token"]);
    assert!(reg.verify_user("app", &token).is_some());
    // The state is single use.
    assert!(
        reg.oauth_callback("app", "oidc", Some("the-code"), Some(&q["state"]), None)
            .await
            .is_err()
    );

    // Second sign-in with the same identity is the same user.
    let url2 = reg
        .oauth_start("app", "oidc", "myapp://auth/done", origin)
        .await
        .unwrap();
    let st2 = reqwest::Url::parse(&url2)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let back2 = reg
        .oauth_callback("app", "oidc", Some("the-code"), Some(&st2), None)
        .await
        .unwrap();
    assert!(back2.starts_with("myapp://auth/done#token="));
    let (users, _) = reg.list_users("app", 10, None).await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].providers, ["oidc"]);
    assert_eq!(users[0].email.as_deref(), Some("grace@example.com"));

    // A provider error comes back as #error=, not as a session.
    let url3 = reg
        .oauth_start("app", "oidc", "https://app.example.com/auth", origin)
        .await
        .unwrap();
    let st3 = reqwest::Url::parse(&url3)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let back3 = reg
        .oauth_callback("app", "oidc", None, Some(&st3), Some("access_denied"))
        .await
        .unwrap();
    assert!(back3.contains("#error=oauth_failed"), "{back3}");
}

#[tokio::test(flavor = "multi_thread")]
async fn oauth_links_by_verified_email_only() {
    for (verified, ok) in [(true, true), (false, false)] {
        let (_d, reg) = setup("lk").await;
        let issuer = oidc("ada@example.com", verified).await;
        reg.put_auth_config(
            "lk",
            cfg(json!({"providers":[{"id":"oidc","enabled":true,"clientId":"c","clientSecret":"s","extra":{"issuer":issuer}}],
                       "allowedRedirects":["https://a.example.com/*"]})),
        )
        .await
        .unwrap();
        reg.register_user("lk", reg_in("ada@example.com", "correct horse"))
            .await
            .unwrap();
        let url = reg
            .oauth_start("lk", "oidc", "https://a.example.com/x", "http://h")
            .await
            .unwrap();
        let st = reqwest::Url::parse(&url)
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let back = reg
            .oauth_callback("lk", "oidc", Some("the-code"), Some(&st), None)
            .await
            .unwrap();
        assert_eq!(back.contains("#token="), ok, "verified={verified}: {back}");
        let (users, _) = reg.list_users("lk", 10, None).await.unwrap();
        assert_eq!(users.len(), 1, "never a duplicate account");
        assert_eq!(users[0].providers.contains(&"oidc".to_owned()), ok);
    }
}

fn urlencoding_decode(s: &str) -> String {
    reqwest::Url::parse(&format!("http://x/?k={s}"))
        .unwrap()
        .query_pairs()
        .next()
        .unwrap()
        .1
        .into_owned()
}
