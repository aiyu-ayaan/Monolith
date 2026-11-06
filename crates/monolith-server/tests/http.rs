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
        let mut b = Request::builder().method(method).uri(uri);
        if su {
            b = b.header("authorization", format!("Bearer {}", self.token));
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
        if s["traffic"]["total"]["deletes"] == 1 {
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
