//! In-process HTTP tests (no socket): status codes, problem+json, request ids, auth.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use monolith_core::Registry;
use serde_json::{Value, json};
use tower::ServiceExt;

struct T {
    app: Router,
    token: String,
    _dir: tempfile::TempDir,
}

struct Res {
    status: StatusCode,
    ctype: String,
    rid: String,
    etag: String,
    body: Value,
}

impl T {
    async fn new() -> T {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registry::open(dir.path()).await.unwrap();
        reg.create_superuser("root@example.com", "a long password")
            .await
            .unwrap();
        let mut t = T {
            app: monolith_server::router(Arc::new(reg)),
            token: String::new(),
            _dir: dir,
        };
        let r = t
            .call(
                Method::POST,
                "/api/v1/system/auth/login",
                Some(json!({"email":"root@example.com","password":"a long password"})),
                false,
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        t.token = r.body["token"].as_str().unwrap().to_owned();
        t
    }

    async fn raw(&self, req: Request<Body>) -> Res {
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let header = |n: &str| {
            res.headers()
                .get(n)
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default()
        };
        let (ctype, rid, etag) = (
            header("content-type"),
            header("x-request-id"),
            header("etag"),
        );
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Res {
            status,
            ctype,
            rid,
            etag,
            body,
        }
    }

    async fn call(&self, method: Method, uri: &str, body: Option<Value>, su: bool) -> Res {
        let token = su.then(|| self.token.clone());
        self.call_with(method, uri, body, token.as_deref()).await
    }

    async fn call_with(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        bearer: Option<&str>,
    ) -> Res {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = bearer {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        let body = match body {
            Some(v) => {
                b = b.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        self.raw(b.body(body).unwrap()).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn end_to_end() {
    let t = T::new().await;
    let r = t
        .call(Method::GET, "/api/v1/system/health", None, false)
        .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (StatusCode::OK, Some("ok"))
    );
    assert_eq!(r.rid.len(), 26);

    // System endpoints need a superuser.
    let r = t
        .call(Method::GET, "/api/v1/system/apps", None, false)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.ctype, "application/problem+json");
    assert_eq!(r.body["type"], "https://monolith.dev/errors/unauthorized");
    assert_eq!(r.body["requestId"], r.rid);

    let r = t
        .call(
            Method::POST,
            "/api/v1/system/apps",
            Some(json!({"name":"ride"})),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = t
        .call(
            Method::POST,
            "/api/v1/system/apps",
            Some(json!({"name":"ride"})),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let r = t.call(Method::GET, "/api/v1/system/apps", None, true).await;
    assert_eq!(r.body["items"][0]["name"], "ride");

    let coll = json!({"name":"trips","fields":[{"name":"dest","type":"text","required":true},{"name":"seats","type":"number","onlyInt":true}],
                      "rules":{"list":"","view":"","create":"seats > 0"}});
    let r = t
        .call(
            Method::POST,
            "/api/v1/apps/ride/collections",
            Some(coll.clone()),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["version"], 1);
    let r = t
        .call(
            Method::POST,
            "/api/v1/apps/ride/collections",
            Some(coll),
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let base = "/api/v1/apps/ride/collections/trips/records";
    let r = t
        .call(
            Method::POST,
            base,
            Some(json!({"dest":"airport","seats":3})),
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    let r = t
        .call(
            Method::POST,
            base,
            Some(json!({"dest":"x","seats":0})),
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = t
        .call(Method::POST, base, Some(json!({"seats":"two"})), false)
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["type"], "https://monolith.dev/errors/validation");
    assert!(
        r.body["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["field"] == "dest" && e["code"] == "required")
    );

    let r = t
        .call(Method::GET, &format!("{base}/{id}"), None, false)
        .await;
    assert_eq!(
        (r.status, r.body["dest"].as_str()),
        (StatusCode::OK, Some("airport"))
    );
    // Update rule is null: anonymous gets 403, superuser succeeds.
    let r = t
        .call(
            Method::PATCH,
            &format!("{base}/{id}"),
            Some(json!({"seats":4})),
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = t
        .call(
            Method::PATCH,
            &format!("{base}/{id}"),
            Some(json!({"seats":4})),
            true,
        )
        .await;
    assert_eq!(
        (r.status, r.body["seats"].as_i64()),
        (StatusCode::OK, Some(4))
    );

    for i in 0..3 {
        t.call(
            Method::POST,
            base,
            Some(json!({"dest": format!("d{i}"), "seats": 1})),
            true,
        )
        .await;
    }
    let r = t
        .call(
            Method::GET,
            &format!("{base}?limit=2&sort=-seats&filter=seats%20%3E%3D%201"),
            None,
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["items"].as_array().unwrap().len(), 2);
    assert_eq!(r.body["items"][0]["seats"], 4);
    let next = r.body["next"].as_str().unwrap().to_owned();
    let r = t
        .call(
            Method::GET,
            &format!("{base}?limit=2&sort=-seats&filter=seats%20%3E%3D%201&cursor={next}"),
            None,
            false,
        )
        .await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 2);
    assert!(r.body["next"].is_null());
    let r = t
        .call(
            Method::GET,
            &format!("{base}?filter=nope%20%3D%201"),
            None,
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = t
        .call(Method::GET, &format!("{base}?limit=x"), None, false)
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = t
        .call(
            Method::POST,
            "/api/v1/apps/ride/batch",
            Some(
                json!({"ops":[{"op":"create","collection":"trips","body":{"dest":"b1","seats":2}},
                               {"op":"delete","collection":"trips","id":id}]}),
            ),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["results"].as_array().unwrap().len(), 2);
    let r = t
        .call(Method::GET, &format!("{base}/{id}"), None, false)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let r = t
        .call(Method::DELETE, "/api/v1/system/apps/ride", None, true)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = t.call(Method::GET, base, None, false).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn edges() {
    let t = T::new().await;
    // Valid client request ids are echoed, invalid ones replaced.
    let req = |rid: &str| {
        Request::get("/api/v1/system/health")
            .header("x-request-id", rid)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(t.raw(req("trace-123")).await.rid, "trace-123");
    assert_ne!(t.raw(req("bad id!")).await.rid, "bad id!");

    // A broken token is 401, not a silent anonymous downgrade.
    let req = Request::get("/api/v1/system/apps")
        .header("authorization", "Bearer nope")
        .body(Body::empty())
        .unwrap();
    assert_eq!(t.raw(req).await.status, StatusCode::UNAUTHORIZED);

    let r = t
        .call(
            Method::POST,
            "/api/v1/system/auth/login",
            Some(json!({"email":"root@example.com","password":"wrong"})),
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = t.call(Method::GET, "/api/v1/nope", None, false).await;
    assert_eq!(
        (r.status, r.ctype.as_str()),
        (StatusCode::NOT_FOUND, "application/problem+json")
    );

    // Body limit: 1 MiB.
    let big = vec![b' '; (1 << 20) + 1];
    let req = Request::post("/api/v1/system/auth/login")
        .body(Body::from(big))
        .unwrap();
    let r = t.raw(req).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        r.body["type"],
        "https://monolith.dev/errors/payload-too-large"
    );
    assert_eq!(r.ctype, "application/problem+json");

    let r = t
        .call(
            Method::POST,
            "/api/v1/system/superusers",
            Some(json!({"email":"two@example.com","password":"another long one"})),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
}

#[tokio::test(flavor = "multi_thread")]
async fn first_superuser_installer() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Arc::new(Registry::open(dir.path()).await.unwrap());
    let mut t = T {
        app: monolith_server::router(reg.clone()),
        token: String::new(),
        _dir: dir,
    };
    let install = "/api/v1/system/install";
    let r = t.call(Method::GET, install, None, false).await;
    assert_eq!(r.body, json!({"required": true}));
    let creds = |tok: &str| json!({"token": tok, "email": "Root@Example.com", "password": "a long password"});

    // No token issued yet: nothing can install.
    let r = t.call(Method::POST, install, Some(creds("x")), false).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let tok = reg.issue_install_token().await.unwrap().unwrap();
    assert_eq!(tok.len(), 64);
    let r = t
        .call(Method::POST, install, Some(creds(&tok[..63])), false)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // A validation error keeps the token usable.
    let short = json!({"token": tok, "email": "root@example.com", "password": "short"});
    let r = t.call(Method::POST, install, Some(short), false).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = t
        .call(Method::POST, install, Some(creds(&tok)), false)
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    t.token = r.body["token"].as_str().unwrap().to_owned();
    let r = t.call(Method::GET, "/api/v1/system/apps", None, true).await;
    assert_eq!(r.status, StatusCode::OK);

    // One-time: the token is spent and no new one is issued once a superuser exists.
    let r = t
        .call(Method::POST, install, Some(creds(&tok)), false)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reg.issue_install_token().await.unwrap(), None);
    let r = t.call(Method::GET, install, None, false).await;
    assert_eq!(r.body, json!({"required": false}));
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_ui_route() {
    let t = T::new().await;
    let res = t
        .app
        .clone()
        .oneshot(Request::get("/_/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // 200 when ui/dist is built, 404 with a hint otherwise; never a problem+json route miss.
    assert!(matches!(
        res.status(),
        StatusCode::OK | StatusCode::NOT_FOUND
    ));
    if res.status() == StatusCode::OK {
        assert!(
            res.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert_eq!(res.headers()["cache-control"], "no-cache");
        assert!(res.headers().get("content-encoding").is_none());
        // Precompressed sibling (when `pnpm build:ui` wrote one) is picked by Accept-Encoding.
        let br = t
            .app
            .clone()
            .oneshot(
                Request::get("/_/")
                    .header("accept-encoding", "gzip, br;q=0.9")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(br.status(), StatusCode::OK);
        assert_eq!(br.headers()["vary"], "accept-encoding");
        assert!(
            br.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert!(matches!(
            br.headers()
                .get("content-encoding")
                .map(|v| v.to_str().unwrap()),
            None | Some("gzip" | "br")
        ));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rest_followups_and_debug_surface() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"shop"})),
        true,
    )
    .await;
    t.call(
        Method::POST,
        "/api/v1/apps/shop/collections",
        Some(json!({
            "name":"users",
            "fields":[{"name":"name","type":"text","required":true}],
            "rules":{"list":"","view":""}
        })),
        true,
    )
    .await;
    t.call(
        Method::POST,
        "/api/v1/apps/shop/collections",
        Some(json!({
            "name":"orders",
            "fields":[
                {"name":"item","type":"text","required":true},
                {"name":"buyer","type":"relation","collection":"users"}
            ],
            "rules":{"list":"","view":"","create":"","update":"item != 'locked'","delete":""}
        })),
        true,
    )
    .await;

    let u = t
        .call(
            Method::POST,
            "/api/v1/apps/shop/collections/users/records",
            Some(json!({"name":"ada"})),
            true,
        )
        .await;
    let uid = u.body["id"].as_str().unwrap().to_owned();

    // PUT upsert creates (201) then updates (200) with ETag / If-Match / expand.
    let oid = monolith_store::id::encode_string(&monolith_store::id::new_id());
    let orders = "/api/v1/apps/shop/collections/orders/records";
    let r = t
        .call(
            Method::PUT,
            &format!("{orders}/{oid}?expand=buyer"),
            Some(json!({"item":"book","buyer":uid})),
            false,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["expand"]["buyer"]["name"], "ada");
    assert!(!r.etag.is_empty());
    let etag1 = r.etag.clone();

    // If-None-Match -> 304 Not Modified.
    let req304 = Request::get(format!("{orders}/{oid}"))
        .header("if-none-match", &etag1)
        .body(Body::empty())
        .unwrap();
    let r304 = t.raw(req304).await;
    assert_eq!(r304.status, StatusCode::NOT_MODIFIED);
    assert_eq!(r304.etag, etag1);

    // If-Match mismatch -> 412 Precondition Failed.
    let req412 = Request::patch(format!("{orders}/{oid}"))
        .header("content-type", "application/json")
        .header("if-match", "\"99999\"")
        .header("x-request-id", "stale-check-1")
        .body(Body::from(r#"{"item":"pen"}"#))
        .unwrap();
    let r412 = t.raw(req412).await;
    assert_eq!(r412.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        r412.body["type"],
        "https://monolith.dev/errors/precondition-failed"
    );

    // Dry-run write returns 200 without modifying the record.
    let r_dry = t
        .call(
            Method::PATCH,
            &format!("{orders}/{oid}?dry_run=true"),
            Some(json!({"item":"ghost"})),
            false,
        )
        .await;
    assert_eq!(
        (r_dry.status, r_dry.body["item"].as_str()),
        (StatusCode::OK, Some("ghost"))
    );
    let r_view = t
        .call(Method::GET, &format!("{orders}/{oid}"), None, false)
        .await;
    assert_eq!(r_view.body["item"], "book");

    // Count endpoint and list with total=exact & expand.
    let r_cnt = t
        .call(Method::GET, &format!("{orders}:count"), None, false)
        .await;
    assert_eq!(
        (r_cnt.status, r_cnt.body["total"].as_i64()),
        (StatusCode::OK, Some(1))
    );
    let r_list = t
        .call(
            Method::GET,
            &format!("{orders}?total=exact&expand=buyer"),
            None,
            false,
        )
        .await;
    assert_eq!(r_list.body["total"], 1);
    assert_eq!(r_list.body["items"][0]["expand"]["buyer"]["name"], "ada");

    // Debug endpoints: collections:diff, rules:explain, query:explain, logs, logs/slow.
    let r_diff = t
        .call(
            Method::POST,
            "/api/v1/apps/shop/collections:diff",
            Some(json!({
                "name":"users",
                "fields":[
                    {"name":"name","type":"text","required":true},
                    {"name":"bio","type":"text"}
                ]
            })),
            true,
        )
        .await;
    assert_eq!(r_diff.status, StatusCode::OK);
    assert_eq!(
        r_diff.body["statements"][0],
        "ALTER TABLE \"users\" ADD COLUMN \"bio\" TEXT"
    );

    let r_rule = t
        .call(
            Method::POST,
            "/api/v1/apps/shop/rules:explain",
            Some(json!({
                "collection":"orders",
                "op":"update",
                "record":{"item":"locked"}
            })),
            true,
        )
        .await;
    assert_eq!(r_rule.status, StatusCode::OK);
    assert_eq!(r_rule.body["class"], "rowLocal");
    assert_eq!(r_rule.body["verdict"], false);

    let r_qplan = t
        .call(
            Method::POST,
            "/api/v1/apps/shop/query:explain",
            Some(json!({"collection":"orders","filter":"item = 'book'","sort":"-item"})),
            true,
        )
        .await;
    assert_eq!(r_qplan.status, StatusCode::OK);
    assert_eq!(r_qplan.body["usesScan"], true);

    let r_logs = t
        .call(
            Method::GET,
            "/api/v1/apps/shop/logs?request_id=stale-check-1",
            None,
            true,
        )
        .await;
    assert_eq!(r_logs.status, StatusCode::OK);
    assert_eq!(r_logs.body["items"][0]["status"], 412);
    assert!(r_logs.body["items"][0]["error"].is_string());

    let r_slow = t
        .call(
            Method::GET,
            "/api/v1/apps/shop/logs/slow?limit=5",
            None,
            true,
        )
        .await;
    assert_eq!(r_slow.status, StatusCode::OK);
    assert!(!r_slow.body["items"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn stats_count_reads_writes_and_record_ops() {
    let t = T::new().await;
    let r = t
        .call(Method::GET, "/api/v1/system/stats", None, false)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"st"})),
        true,
    )
    .await;
    let coll = json!({"name":"notes","fields":[{"name":"body","type":"text"}],"rules":{"list":"","view":"","create":""}});
    let r = t
        .call(
            Method::POST,
            "/api/v1/apps/st/collections",
            Some(coll),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let recs = "/api/v1/apps/st/collections/notes/records";
    for i in 0..3 {
        let r = t
            .call(
                Method::POST,
                recs,
                Some(json!({"body": format!("n{i}")})),
                false,
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED);
    }
    let id = t.call(Method::GET, recs, None, false).await.body["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = t
        .call(Method::DELETE, &format!("{recs}/{id}"), None, true)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    t.call(Method::GET, &format!("{recs}/{id}"), None, false)
        .await; // 404

    // Record ops arrive through the change bus task; give it a moment.
    let mut s = Value::Null;
    for _ in 0..50 {
        s = t
            .call(
                Method::GET,
                "/api/v1/apps/st/stats?counts=exact",
                None,
                true,
            )
            .await
            .body;
        // Per-collection counters are fed from the change bus, a moment after the write returns.
        if s["traffic"]["total"]["deletes"] == 1 && s["collections"][0]["deletes"] == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let tot = &s["traffic"]["total"];
    assert_eq!(tot["reads"], 2, "{s}");
    assert_eq!(tot["writes"], 4, "{s}");
    assert_eq!(
        (tot["creates"].clone(), tot["deletes"].clone()),
        (json!(3), json!(1))
    );
    assert_eq!(tot["errors4xx"], 1);
    assert!(tot["bytesOut"].as_u64().unwrap() > 0);
    assert_eq!(s["traffic"]["minutes"].as_array().unwrap().len(), 60);
    assert_eq!(s["traffic"]["hours"].as_array().unwrap().len(), 48);
    let c = &s["collections"][0];
    assert_eq!(
        (c["name"].clone(), c["records"].clone()),
        (json!("notes"), json!(2))
    );
    assert_eq!(
        (
            c["reads"].clone(),
            c["creates"].clone(),
            c["deletes"].clone()
        ),
        (json!(1), json!(3), json!(1))
    );
    assert!(s["store"]["commits"].as_u64().unwrap() >= 4);
    assert!(s["dbBytes"].as_u64().unwrap() > 0);

    let r = t
        .call(Method::GET, "/api/v1/apps/st/stats?counts=nope", None, true)
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let g = t
        .call(Method::GET, "/api/v1/system/stats", None, true)
        .await
        .body;
    assert_eq!(g["appsTotal"], 1);
    assert_eq!(g["apps"][0]["name"], "st");
    assert_eq!(g["apps"][0]["traffic"]["writes"], 4);
    assert_eq!(g["traffic"]["total"]["creates"], 3);
    assert!(g["traffic"]["total"]["requests"].as_u64().unwrap() >= 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn app_auth_config_round_trip() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    let url = "/api/v1/apps/demo/auth/config";
    // Needs a superuser.
    assert_eq!(
        t.call(Method::GET, url, None, false).await.status,
        StatusCode::UNAUTHORIZED
    );
    let r = t.call(Method::GET, url, None, true).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["emailPassword"]["enabled"], true);
    assert!(r.body["providers"].as_array().unwrap().is_empty());
    assert!(r.body["catalog"].as_array().unwrap().len() >= 10);
    let put = json!({
        "emailPassword": {"enabled": true, "allowSignup": false, "minPasswordLength": 12},
        "providers": [
            {"id":"google","enabled":true,"clientId":"gid","clientSecret":"shh"},
            {"id":"github","enabled":false,"clientId":"hid"}
        ]
    });
    let r = t.call(Method::PUT, url, Some(put), true).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(!r.body.to_string().contains("shh"), "secret leaked");
    assert_eq!(r.body["providers"][0]["clientSecretSet"], true);
    assert_eq!(r.body["providers"][1]["clientSecretSet"], false);
    // Persisted, and an omitted secret keeps the stored one.
    let r = t
        .call(
            Method::PUT,
            url,
            Some(json!({"providers":[{"id":"google","enabled":true,"clientId":"gid2"}]})),
            true,
        )
        .await;
    assert_eq!(r.body["providers"][0]["clientId"], "gid2");
    assert_eq!(r.body["providers"][0]["clientSecretSet"], true);
    let r = t.call(Method::GET, url, None, true).await;
    assert_eq!(r.body["providers"].as_array().unwrap().len(), 1);
    // Validation: unknown provider, enabling without a secret.
    for bad in [
        json!({"providers":[{"id":"nope"}]}),
        json!({"providers":[{"id":"discord","enabled":true,"clientId":"x"}]}),
    ] {
        let r = t.call(Method::PUT, url, Some(bad), true).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{:?}", r.body);
    }
    assert_eq!(
        t.call(Method::GET, "/api/v1/apps/ghost/auth/config", None, true)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Deleting the app removes its config.
    t.call(Method::DELETE, "/api/v1/system/apps/demo", None, true)
        .await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    let r = t.call(Method::GET, url, None, true).await;
    assert!(r.body["providers"].as_array().unwrap().is_empty());
}

/// Next body chunk as text (SSE frames arrive one per chunk).
async fn chunk(body: &mut Body) -> Option<String> {
    use axum::body::HttpBody;
    let f = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        std::future::poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_frame(cx)),
    )
    .await
    .ok()??
    .ok()?;
    Some(String::from_utf8_lossy(&f.into_data().ok()?).into_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn realtime_sse_streams_changes() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    let c = t
        .call(
            Method::POST,
            "/api/v1/apps/demo/collections",
            Some(json!({"name":"posts","fields":[{"name":"title","type":"text"}],"rules":{"list":"","view":""}})),
            true,
        )
        .await;
    assert_eq!(c.status, StatusCode::CREATED, "{:?}", c.body);
    let url = "/api/v1/apps/demo/realtime?collections=posts";
    // No collections, unknown collection.
    assert_eq!(
        t.call(Method::GET, "/api/v1/apps/demo/realtime", None, true)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.call(
            Method::GET,
            "/api/v1/apps/demo/realtime?collections=nope",
            None,
            true
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let res = t
        .app
        .clone()
        .oneshot(
            Request::get(url)
                .header("authorization", format!("Bearer {}", t.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    let mut body = res.into_body();
    assert!(chunk(&mut body).await.unwrap().starts_with("event: hello"));
    t.call(
        Method::POST,
        "/api/v1/apps/demo/collections/posts/records",
        Some(json!({"title":"live"})),
        true,
    )
    .await;
    let ev = chunk(&mut body).await.unwrap();
    assert!(
        ev.contains("event: record")
            && ev.contains("\"op\":\"create\"")
            && ev.contains("\"title\":\"live\""),
        "{ev}"
    );
    let stats = t
        .call(Method::GET, "/api/v1/apps/demo/stats", None, true)
        .await;
    assert_eq!(stats.body["realtime"]["connections"], 1);
    assert!(stats.body["realtime"]["events"].as_u64().unwrap() >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn backups_create_list_download_restore_delete() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    t.call(
        Method::POST,
        "/api/v1/apps/demo/collections",
        Some(json!({"name":"posts","fields":[{"name":"title","type":"text"}],"rules":{"list":"","view":""}})),
        true,
    )
    .await;
    t.call(
        Method::POST,
        "/api/v1/apps/demo/collections/posts/records",
        Some(json!({"title":"kept"})),
        true,
    )
    .await;
    let url = "/api/v1/apps/demo/backups";
    assert_eq!(
        t.call(Method::GET, url, None, false).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        t.call(Method::GET, url, None, true).await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let b = t.call(Method::POST, url, None, true).await;
    assert_eq!(b.status, StatusCode::CREATED, "{:?}", b.body);
    let name = b.body["name"].as_str().unwrap().to_owned();
    assert!(b.body["bytes"].as_u64().unwrap() > 0);
    assert_eq!(
        t.call(Method::GET, url, None, true).await.body["items"][0]["name"],
        name.as_str()
    );

    // Download is the raw SQLite file.
    let res = t
        .app
        .clone()
        .oneshot(
            Request::get(format!("{url}/{name}"))
                .header("authorization", format!("Bearer {}", t.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    assert_eq!(bytes.len() as u64, b.body["bytes"].as_u64().unwrap());

    // Restore into a new app, then the data is there; a taken name conflicts.
    let r = t
        .call(
            Method::POST,
            "/api/v1/apps/demo/backups:restore",
            Some(json!({"backup": name, "newApp": "copy"})),
            true,
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let list = t
        .call(
            Method::GET,
            "/api/v1/apps/copy/collections/posts/records",
            None,
            true,
        )
        .await;
    assert_eq!(list.body["items"][0]["title"], "kept");
    let dup = t
        .call(
            Method::POST,
            "/api/v1/apps/demo/backups:restore",
            Some(json!({"backup": name, "newApp": "copy"})),
            true,
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT, "{:?}", dup.body);

    // Names are validated before touching the filesystem.
    for bad in ["..%2Fdata.db", "x.db", "b1.db"] {
        assert_eq!(
            t.call(Method::DELETE, &format!("{url}/{bad}"), None, true)
                .await
                .status,
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
    assert_eq!(
        t.call(Method::DELETE, &format!("{url}/{name}"), None, true)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(
        t.call(Method::GET, url, None, true).await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn api_keys_scopes_and_revocation() {
    let t = T::new().await;
    for app in ["demo", "other"] {
        t.call(
            Method::POST,
            "/api/v1/system/apps",
            Some(json!({"name": app})),
            true,
        )
        .await;
    }
    t.call(
        Method::POST,
        "/api/v1/apps/demo/collections",
        Some(json!({"name":"tasks","fields":[{"name":"title","type":"text"}],"rules":{}})),
        true,
    )
    .await;
    let keys = "/api/v1/apps/demo/api-keys";
    let rec = "/api/v1/apps/demo/collections/tasks/records";

    // Management is superuser-only; bad scopes are rejected.
    let new = |scopes: Value| Some(json!({"name":"ci","scopes":scopes}));
    assert_eq!(
        t.call(Method::POST, keys, new(json!(["records:admin"])), false)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let bad = t.call(Method::POST, keys, new(json!(["root"])), true).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{:?}", bad.body);

    let admin = t
        .call(Method::POST, keys, new(json!(["records:admin"])), true)
        .await;
    assert_eq!(admin.status, StatusCode::CREATED, "{:?}", admin.body);
    let secret = admin.body["key"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("mk_demo_"));
    let listed = t.call(Method::GET, keys, None, true).await;
    assert_eq!(listed.body["items"][0]["prefix"], &secret[..12]);
    assert!(
        !listed.body.to_string().contains(&secret),
        "secret is shown once"
    );

    // An admin key bypasses rules (the collection has no rules, so anonymous is denied).
    let anon = t.call(Method::GET, rec, None, false).await;
    assert_eq!(anon.status, StatusCode::FORBIDDEN);
    let w = t
        .call_with(Method::POST, rec, Some(json!({"title":"x"})), Some(&secret))
        .await;
    assert_eq!(w.status, StatusCode::CREATED, "{:?}", w.body);
    // ...but only inside its own app, and not on admin routes.
    let cross = t
        .call_with(
            Method::GET,
            "/api/v1/apps/other/collections/tasks/records",
            None,
            Some(&secret),
        )
        .await;
    assert_eq!(cross.status, StatusCode::FORBIDDEN);
    let adm = t
        .call_with(Method::GET, "/api/v1/system/apps", None, Some(&secret))
        .await;
    assert_eq!(adm.status, StatusCode::UNAUTHORIZED);
    let adm = t.call_with(Method::GET, keys, None, Some(&secret)).await;
    assert_eq!(adm.status, StatusCode::UNAUTHORIZED);

    // A scoped key is limited to its scope.
    let ro = t
        .call(Method::POST, keys, new(json!(["records:read:tasks"])), true)
        .await;
    let ro_secret = ro.body["key"].as_str().unwrap().to_owned();
    let r = t.call_with(Method::GET, rec, None, Some(&ro_secret)).await;
    assert_ne!(r.status, StatusCode::UNAUTHORIZED);
    let r = t
        .call_with(
            Method::POST,
            rec,
            Some(json!({"title":"y"})),
            Some(&ro_secret),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    assert!(r.body["detail"].as_str().unwrap().contains("scope"));
    let r = t
        .call_with(
            Method::GET,
            "/api/v1/apps/demo/realtime?collections=tasks",
            None,
            Some(&ro_secret),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "needs realtime:subscribe");

    // Revoking takes effect on the next request; garbage keys are 401.
    let id = admin.body["id"].as_str().unwrap();
    assert_eq!(
        t.call(Method::DELETE, &format!("{keys}/{id}"), None, true)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let gone = t.call_with(Method::GET, rec, None, Some(&secret)).await;
    assert_eq!(gone.status, StatusCode::UNAUTHORIZED);
    let fake = t
        .call_with(Method::GET, rec, None, Some("mk_demo_nope"))
        .await;
    assert_eq!(fake.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        t.call(Method::DELETE, &format!("{keys}/{id}"), None, true)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Deleting the app removes its keys.
    t.call(Method::DELETE, "/api/v1/system/apps/demo", None, true)
        .await;
    let r = t.call_with(Method::GET, rec, None, Some(&ro_secret)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

/// Raw request for storage routes: returns status, headers and body bytes.
async fn blob(
    t: &T,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    extra: &[(&str, &str)],
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    let res = t
        .app
        .clone()
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, headers, bytes.to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_buckets_auth_range_and_limits() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    let bk = "/api/v1/apps/demo/buckets";
    let su = Some(t.token.as_str());

    // Bucket management is superuser-only.
    let mk = |name: &str, public: bool, max: u64| {
        Some(json!({"name": name, "public": public, "maxSize": max}))
    };
    assert_eq!(
        t.call(Method::POST, bk, mk("priv", false, 5 << 20), false)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let r = t
        .call(Method::POST, bk, mk("priv", false, 5 << 20), true)
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        t.call(Method::POST, bk, mk("priv", false, 5 << 20), true)
            .await
            .status,
        StatusCode::CONFLICT
    );
    t.call(Method::POST, bk, mk("pub", true, 1 << 20), true)
        .await;
    assert_eq!(
        t.call(Method::GET, bk, None, true).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Upload a body larger than the JSON body limit (3 MiB), streamed.
    let data: Vec<u8> = (0..3 << 20).map(|i| (i % 251) as u8).collect();
    let url = format!("{bk}/priv/objects/dir/big.bin");
    let (s, _, _) = blob(&t, Method::PUT, &url, None, &[], data.clone()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = blob(
        &t,
        Method::PUT,
        &url,
        su,
        &[("content-type", "application/octet-stream")],
        data.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{}", String::from_utf8_lossy(&b));
    let info: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(info["size"], 3 << 20);

    // Private read: anonymous 401, superuser OK with validators, ETag revalidates.
    assert_eq!(
        blob(&t, Method::GET, &url, None, &[], vec![]).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (s, h, b) = blob(&t, Method::GET, &url, su, &[], vec![]).await;
    assert_eq!((s, b == data), (StatusCode::OK, true));
    assert_eq!(h["content-length"], (3 << 20).to_string().as_str());
    assert_eq!(h["x-content-type-options"], "nosniff");
    let etag = h["etag"].to_str().unwrap().to_owned();
    let (s, _, b) = blob(
        &t,
        Method::GET,
        &url,
        su,
        &[("if-none-match", &etag)],
        vec![],
    )
    .await;
    assert_eq!((s, b.len()), (StatusCode::NOT_MODIFIED, 0));
    let (s, _, b) = blob(&t, Method::HEAD, &url, su, &[], vec![]).await;
    assert_eq!((s, b.len()), (StatusCode::OK, 0));

    // Ranges.
    let (s, h, b) = blob(
        &t,
        Method::GET,
        &url,
        su,
        &[("range", "bytes=10-19")],
        vec![],
    )
    .await;
    assert_eq!(
        (s, b.as_slice()),
        (StatusCode::PARTIAL_CONTENT, &data[10..20])
    );
    assert_eq!(
        h["content-range"],
        format!("bytes 10-19/{}", 3 << 20).as_str()
    );
    let (s, _, b) = blob(&t, Method::GET, &url, su, &[("range", "bytes=-5")], vec![]).await;
    assert_eq!(
        (s, b.as_slice()),
        (StatusCode::PARTIAL_CONTENT, &data[data.len() - 5..])
    );
    let (s, _, _) = blob(
        &t,
        Method::GET,
        &url,
        su,
        &[("range", "bytes=99999999-")],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::RANGE_NOT_SATISFIABLE);

    // Keys: scoped read works, other bucket is 403, write scope is separate, other app is 403.
    let mkkey = |scopes: Value| {
        let t = &t;
        async move {
            let r = t
                .call(
                    Method::POST,
                    "/api/v1/apps/demo/api-keys",
                    Some(json!({"name":"k","scopes":scopes})),
                    true,
                )
                .await;
            r.body["key"].as_str().unwrap().to_owned()
        }
    };
    let rk = mkkey(json!(["files:read:priv"])).await;
    assert_eq!(
        blob(&t, Method::GET, &url, Some(&rk), &[], vec![]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        blob(
            &t,
            Method::GET,
            &format!("{bk}/pub/objects/x"),
            Some(&rk),
            &[],
            vec![]
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "public bucket skips the scope check"
    );
    assert_eq!(
        blob(&t, Method::PUT, &url, Some(&rk), &[], b"x".to_vec())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        blob(
            &t,
            Method::GET,
            &format!("{bk}/priv/objects"),
            Some(&rk),
            &[],
            vec![]
        )
        .await
        .0,
        StatusCode::OK
    );
    let wk = mkkey(json!(["files:write"])).await;
    assert_eq!(
        blob(
            &t,
            Method::PUT,
            &format!("{bk}/priv/objects/w.txt"),
            Some(&wk),
            &[("content-type", "text/plain")],
            b"hi".to_vec()
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        blob(&t, Method::GET, &url, Some(&wk), &[], vec![]).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        blob(&t, Method::POST, bk, Some(&wk), &[], vec![]).await.0,
        StatusCode::UNAUTHORIZED,
        "keys cannot manage buckets"
    );

    // Public bucket: anonymous read, but listing and writes need a credential; hostile types download.
    let html = format!("{bk}/pub/objects/page.html");
    let (s, _, _) = blob(
        &t,
        Method::PUT,
        &html,
        su,
        &[("content-type", "text/html")],
        b"<script>1</script>".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, h, _) = blob(&t, Method::GET, &html, None, &[], vec![]).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        h["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    assert_eq!(
        blob(
            &t,
            Method::GET,
            &format!("{bk}/pub/objects"),
            None,
            &[],
            vec![]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        blob(&t, Method::PUT, &html, None, &[], b"x".to_vec())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    // Size limit: declared and streamed (chunked has no content-length in this harness only when absent).
    let big = vec![0u8; (1 << 20) + 1];
    let (s, _, _) = blob(
        &t,
        Method::PUT,
        &format!("{bk}/pub/objects/big"),
        su,
        &[],
        big,
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        blob(
            &t,
            Method::GET,
            &format!("{bk}/pub/objects/big"),
            None,
            &[],
            vec![]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Listing, delete, bucket delete.
    let l = t
        .call(
            Method::GET,
            &format!("{bk}/priv/objects?prefix=dir/"),
            None,
            true,
        )
        .await;
    assert_eq!(l.body["items"][0]["key"], "dir/big.bin");
    assert_eq!(
        t.call(Method::DELETE, &url, None, true).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.call(Method::DELETE, &url, None, true).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.call(Method::DELETE, &format!("{bk}/priv"), None, true)
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        t.call(Method::DELETE, &format!("{bk}/priv?force=true"), None, true)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn api_key_limits_allowlist_update_and_logs() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    t.call(
        Method::POST,
        "/api/v1/apps/demo/collections",
        Some(json!({"name":"tasks","fields":[{"name":"title","type":"text"}],"rules":{"list":"","view":""}})),
        true,
    )
    .await;
    let keys = "/api/v1/apps/demo/api-keys";
    let rec = "/api/v1/apps/demo/collections/tasks/records";
    let mk = |extra: Value| {
        let mut b = json!({"name":"k","scopes":["records:read"]});
        b.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        Some(b)
    };
    let from = |secret: &str, ip: Option<&str>| {
        let mut r = Request::get(rec)
            .header("authorization", format!("Bearer {secret}"))
            .body(Body::empty())
            .unwrap();
        if let Some(ip) = ip {
            let addr: std::net::SocketAddr = format!("{ip}:5555").parse().unwrap();
            r.extensions_mut().insert(axum::extract::ConnectInfo(addr));
        }
        r
    };

    // Bad limits and networks are rejected.
    for bad in [
        json!({"rateLimitPerMin":0}),
        json!({"allowedIps":["10.0.0.0/33"]}),
    ] {
        assert_eq!(
            t.call(Method::POST, keys, mk(bad), true).await.status,
            StatusCode::BAD_REQUEST
        );
    }

    // Rate limit: 2 per minute, then 429 with Retry-After.
    let limited = t
        .call(Method::POST, keys, mk(json!({"rateLimitPerMin":2})), true)
        .await;
    assert_eq!(limited.body["rateLimitPerMin"], 2);
    let lk = limited.body["key"].as_str().unwrap().to_owned();
    for _ in 0..2 {
        assert_eq!(t.raw(from(&lk, None)).await.status, StatusCode::OK);
    }
    let res = t.app.clone().oneshot(from(&lk, None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        res.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );

    // Allowlist: unknown address and other networks are 403, a matching one passes.
    let ipk = t
        .call(
            Method::POST,
            keys,
            mk(json!({"allowedIps":["10.1.0.0/16"]})),
            true,
        )
        .await;
    let ik = ipk.body["key"].as_str().unwrap().to_owned();
    assert_eq!(t.raw(from(&ik, None)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        t.raw(from(&ik, Some("192.0.2.1"))).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.raw(from(&ik, Some("10.1.2.3"))).await.status,
        StatusCode::OK
    );
    // A client-sent address header is ignored.
    let mut spoof = from(&ik, Some("192.0.2.1"));
    spoof
        .headers_mut()
        .insert("x-monolith-client-ip", "10.1.2.3".parse().unwrap());
    assert_eq!(t.raw(spoof).await.status, StatusCode::FORBIDDEN);

    // Last use shows in the list; the request log names the key.
    let list = t.call(Method::GET, keys, None, true).await;
    let row = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == ipk.body["id"])
        .unwrap()
        .clone();
    assert!(row["lastUsed"].is_string());
    let logs = t
        .call(Method::GET, "/api/v1/apps/demo/logs?limit=50", None, true)
        .await;
    assert!(
        logs.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["key"] == ipk.body["id"])
    );

    // Update: clears the allowlist and raises scopes; the secret keeps working; unknown id is 404.
    let id = ipk.body["id"].as_str().unwrap();
    let up = t
        .call(
            Method::PUT,
            &format!("{keys}/{id}"),
            Some(json!({"name":"renamed","scopes":["records:read","realtime:subscribe"]})),
            true,
        )
        .await;
    assert_eq!(up.status, StatusCode::OK, "{:?}", up.body);
    assert_eq!(
        (
            up.body["name"].as_str(),
            up.body["allowedIps"].as_array().map(Vec::len)
        ),
        (Some("renamed"), Some(0))
    );
    assert!(up.body["lastUsed"].is_string(), "usage survives an update");
    assert_eq!(
        t.raw(from(&ik, Some("192.0.2.1"))).await.status,
        StatusCode::OK
    );
    assert_eq!(
        t.call(
            Method::PUT,
            &format!("{keys}/00000000000000000000000000"),
            Some(json!({"name":"x","scopes":["records:read"]})),
            true
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_dedupe_quota_and_gc_over_http() {
    let t = T::new().await;
    t.call(
        Method::POST,
        "/api/v1/system/apps",
        Some(json!({"name":"demo"})),
        true,
    )
    .await;
    let bk = "/api/v1/apps/demo/buckets";
    let r = t
        .call(
            Method::POST,
            bk,
            Some(json!({"name":"b","maxBytes":10,"allowedMime":["image/*"]})),
            true,
        )
        .await;
    assert_eq!(
        (r.body["maxBytes"].clone(), r.body["objects"].clone()),
        (json!(10), json!(0))
    );
    let su = Some(t.token.as_str());
    let png = b"\x89PNG\r\n\x1a\n\0\0".to_vec();
    // Declared octet-stream, real PNG: stored and served as image/png.
    let (s, _, b) = blob(
        &t,
        Method::PUT,
        &format!("{bk}/b/objects/a.png"),
        su,
        &[],
        png.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{}", String::from_utf8_lossy(&b));
    assert_eq!(
        serde_json::from_slice::<Value>(&b).unwrap()["mime"],
        "image/png"
    );
    let (_, h, _) = blob(
        &t,
        Method::GET,
        &format!("{bk}/b/objects/a.png"),
        su,
        &[],
        vec![],
    )
    .await;
    assert_eq!(h["content-type"], "image/png");
    // Quota: 10 bytes held, one more byte of a second object does not fit.
    let (s, _, _) = blob(
        &t,
        Method::PUT,
        &format!("{bk}/b/objects/b.png"),
        su,
        &[],
        [png.clone(), vec![0]].concat(),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let (s, _, _) = blob(
        &t,
        Method::PUT,
        &format!("{bk}/b/objects/x"),
        su,
        &[("content-type", "text/plain")],
        b"hi".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "text is not an allowed type");
    // GC is superuser-only and reports.
    assert_eq!(
        t.call(Method::POST, "/api/v1/apps/demo/storage:gc", None, false)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let g = t
        .call(Method::POST, "/api/v1/apps/demo/storage:gc", None, true)
        .await;
    assert_eq!(
        (
            g.status,
            g.body["removedFiles"].clone(),
            g.body["missingFiles"].clone()
        ),
        (StatusCode::OK, json!(0), json!(0))
    );
}
