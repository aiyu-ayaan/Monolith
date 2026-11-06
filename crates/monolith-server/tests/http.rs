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
        let (ctype, rid) = (header("content-type"), header("x-request-id"));
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
