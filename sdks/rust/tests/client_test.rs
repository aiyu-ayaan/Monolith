use axum::extract::{Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use monolith_client::{BatchOp, Client, Error, FieldProblem, ListOptions, Problem};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[test]
fn test_problem_rfc9457_serialization() {
    let problem = Problem::new(
        "https://monolith.dev/errors/validation",
        "Validation failed",
        400,
    )
    .with_detail("One or more fields failed validation")
    .with_request_id("req_xyz123")
    .with_errors(vec![
        FieldProblem::new("email", "invalid_format", "Must be a valid email address"),
        FieldProblem::new("age", "out_of_range", "Must be at least 18"),
    ]);

    let json_str = serde_json::to_string(&problem).unwrap();
    let deserialized: Problem = serde_json::from_str(&json_str).unwrap();

    assert_eq!(
        deserialized.problem_type,
        "https://monolith.dev/errors/validation"
    );
    assert_eq!(deserialized.title, "Validation failed");
    assert_eq!(deserialized.status, 400);
    assert_eq!(
        deserialized.detail.as_deref(),
        Some("One or more fields failed validation")
    );
    assert_eq!(deserialized.request_id.as_deref(), Some("req_xyz123"));
    assert_eq!(
        deserialized.get_field_error("email"),
        Some("Must be a valid email address")
    );
    assert_eq!(
        deserialized.get_field_error("age"),
        Some("Must be at least 18")
    );
    assert_eq!(deserialized.get_field_error("nonexistent"), None);
}

#[test]
fn test_list_options_query_params() {
    let opts = ListOptions::new()
        .limit(25)
        .page(2)
        .per_page(25)
        .filter("status = 'active'")
        .sort("-created")
        .fields("id,title")
        .expand("author")
        .total("exact")
        .cursor("cur_abc");

    let params = opts.to_query_params();
    let map: HashMap<String, String> = params.into_iter().collect();

    assert_eq!(map.get("limit").map(String::as_str), Some("25"));
    assert_eq!(map.get("page").map(String::as_str), Some("2"));
    assert_eq!(map.get("perPage").map(String::as_str), Some("25"));
    assert_eq!(
        map.get("filter").map(String::as_str),
        Some("status = 'active'")
    );
    assert_eq!(map.get("sort").map(String::as_str), Some("-created"));
    assert_eq!(map.get("fields").map(String::as_str), Some("id,title"));
    assert_eq!(map.get("expand").map(String::as_str), Some("author"));
    assert_eq!(map.get("total").map(String::as_str), Some("exact"));
    assert_eq!(map.get("cursor").map(String::as_str), Some("cur_abc"));
}

#[test]
fn test_client_builder_defaults() {
    let client = Client::builder("http://localhost:8090/")
        .app("myapp")
        .api_key("mk_test123")
        .header("X-Custom", "custom_val")
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(client.base_url(), "http://localhost:8090");
    assert_eq!(client.app(), "myapp");
    assert_eq!(client.api_key().as_deref(), Some("mk_test123"));
    assert_eq!(client.token(), None);

    client.set_token(Some("tok_abc"));
    assert_eq!(client.token().as_deref(), Some("tok_abc"));

    client.set_token(None::<String>);
    assert_eq!(client.token(), None);
}

// ---------------- Mock Server & End-to-End Tests ----------------

async fn spawn_mock_server() -> (String, tokio::task::JoinHandle<()>) {
    let router = Router::new()
        // System install
        .route(
            "/api/v1/system/install",
            get(|| async { Json(json!({ "required": true })) }).post(
                |Json(body): Json<serde_json::Value>| async move {
                    if body.get("token").and_then(|t| t.as_str()) == Some("valid_token") {
                        (
                            StatusCode::CREATED,
                            Json(json!({ "token": "installed_su_token", "expires": "2026-12-31" })),
                        )
                    } else {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "type": "https://monolith.dev/errors/bad-request",
                                "title": "Invalid install token",
                                "status": 400,
                                "detail": "The provided token was invalid",
                            })),
                        )
                    }
                },
            ),
        )
        // System login
        .route(
            "/api/v1/system/auth/login",
            post(|Json(body): Json<serde_json::Value>| async move {
                if body.get("email").and_then(|e| e.as_str()) == Some("admin@example.com") {
                    (
                        StatusCode::OK,
                        Json(json!({ "token": "su_jwt_token", "expires": "2026-12-31" })),
                    )
                } else {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "type": "https://monolith.dev/errors/unauthorized",
                            "title": "Unauthorized",
                            "status": 401,
                            "detail": "Invalid credentials",
                        })),
                    )
                }
            }),
        )
        // App collection records
        .route(
            "/api/v1/apps/{app}/collections/{collection}/records",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                let limit = q
                    .get("limit")
                    .and_then(|l| l.parse::<usize>().ok())
                    .unwrap_or(10);
                let items: Vec<serde_json::Value> = (0..limit.min(2))
                    .map(|i| {
                        json!({
                            "id": format!("rec_{i}"),
                            "created": "2026-11-07T00:00:00Z",
                            "updated": "2026-11-07T00:00:00Z",
                            "_v": 1,
                            "title": format!("Task {i}"),
                        })
                    })
                    .collect();
                Json(json!({
                    "items": items,
                    "next": "next_cur_123",
                    "total": 42
                }))
            })
            .post(|Json(body): Json<serde_json::Value>| async move {
                (
                    StatusCode::CREATED,
                    Json(json!({
                        "id": "new_rec_1",
                        "created": "2026-11-07T00:00:00Z",
                        "updated": "2026-11-07T00:00:00Z",
                        "_v": 1,
                        "title": body.get("title").unwrap_or(&json!("untitled")),
                    })),
                )
            }),
        )
        .route(
            "/api/v1/apps/{app}/collections/{collection}/records:count",
            get(|| async { Json(json!({ "total": 99, "exact": true })) }),
        )
        .route(
            "/api/v1/apps/{app}/collections/{collection}/records/{id}",
            get(
                |Path((_app, _coll, id)): Path<(String, String, String)>| async move {
                    if id == "not_found" {
                        return (
                            StatusCode::NOT_FOUND,
                            Json(json!({
                                "type": "https://monolith.dev/errors/not-found",
                                "title": "Not Found",
                                "status": 404,
                                "detail": "Record not found",
                            })),
                        );
                    }
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": id,
                            "created": "2026-11-07T00:00:00Z",
                            "updated": "2026-11-07T00:00:00Z",
                            "_v": 1,
                            "title": "Existing Task",
                        })),
                    )
                },
            )
            .patch(
                |Path((_app, _coll, id)): Path<(String, String, String)>,
                 Json(body): Json<serde_json::Value>| async move {
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": id,
                            "created": "2026-11-07T00:00:00Z",
                            "updated": "2026-11-07T01:00:00Z",
                            "_v": 2,
                            "title": body.get("title").unwrap_or(&json!("patched")),
                        })),
                    )
                },
            )
            .put(
                |Path((_app, _coll, id)): Path<(String, String, String)>,
                 Json(body): Json<serde_json::Value>| async move {
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": id,
                            "created": "2026-11-07T00:00:00Z",
                            "updated": "2026-11-07T01:00:00Z",
                            "_v": 2,
                            "title": body.get("title").unwrap_or(&json!("replaced")),
                        })),
                    )
                },
            )
            .delete(
                |Path((_app, _coll, id)): Path<(String, String, String)>| async move {
                    if id == "locked" {
                        (
                            StatusCode::FORBIDDEN,
                            Json(json!({
                                "type": "https://monolith.dev/errors/forbidden",
                                "title": "Forbidden",
                                "status": 403,
                                "detail": "Record is locked",
                            })),
                        )
                    } else {
                        (StatusCode::NO_CONTENT, Json(json!(null)))
                    }
                },
            ),
        )
        // Storage buckets & objects
        .route(
            "/api/v1/apps/{app}/buckets/{bucket}/objects",
            get(|| async {
                Json(json!({
                    "items": [{
                        "key": "avatar.png",
                        "size": 1024,
                        "mime": "image/png",
                        "etag": "abcd1234ef",
                        "created": "2026-11-07T00:00:00Z",
                        "updated": "2026-11-07T00:00:00Z"
                    }],
                    "nextCursor": null
                }))
            }),
        )
        .route(
            "/api/v1/apps/{app}/buckets/{bucket}/objects/{*key}",
            get(
                |Path((_app, _bucket, key)): Path<(String, String, String)>| async move {
                    (
                        StatusCode::OK,
                        [("content-type", "application/octet-stream")],
                        format!("content-of-{key}"),
                    )
                },
            )
            .put(
                |Path((_app, _bucket, key)): Path<(String, String, String)>,
                 headers: HeaderMap,
                 body: axum::body::Bytes| async move {
                    let mime = headers
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("application/octet-stream");
                    (
                        StatusCode::CREATED,
                        Json(json!({
                            "key": key,
                            "size": body.len(),
                            "mime": mime,
                            "etag": "etag_12345",
                            "created": "2026-11-07T00:00:00Z",
                            "updated": "2026-11-07T00:00:00Z"
                        })),
                    )
                },
            )
            .delete(|| async { StatusCode::NO_CONTENT }),
        )
        // Batch
        .route(
            "/api/v1/apps/{app}/batch",
            post(|Json(body): Json<serde_json::Value>| async move {
                let ops = body
                    .get("ops")
                    .and_then(|o| o.as_array())
                    .cloned()
                    .unwrap_or_default();
                let results: Vec<serde_json::Value> = ops
                    .iter()
                    .enumerate()
                    .map(|(i, _)| json!({ "id": format!("batch_res_{i}") }))
                    .collect();
                Json(json!({ "results": results }))
            }),
        );

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (url, handle)
}

#[tokio::test]
async fn test_auth_service() {
    let (url, _handle) = spawn_mock_server().await;
    let client = Client::builder(&url).app("testapp").build().unwrap();

    // 1. Check install status
    let required = client.auth().install_required().await.unwrap();
    assert!(required);

    // 2. Perform install
    let session = client
        .auth()
        .install("valid_token", "admin@example.com", "pass")
        .await
        .unwrap();
    assert_eq!(session.token, "installed_su_token");
    assert_eq!(client.token().as_deref(), Some("installed_su_token"));

    // 3. System sign-in
    let su_auth = client.auth_collection("_superusers");
    let session2 = su_auth
        .sign_in_with_password("admin@example.com", "pass")
        .await
        .unwrap();
    assert_eq!(session2.token, "su_jwt_token");
    assert_eq!(client.token().as_deref(), Some("su_jwt_token"));

    // 4. Sign-in failure with Problem error
    let err = su_auth
        .sign_in_with_password("wrong@example.com", "badpass")
        .await
        .unwrap_err();

    match err {
        Error::Api {
            status, problem, ..
        } => {
            assert_eq!(status, 401);
            let p = problem.unwrap();
            assert_eq!(p.problem_type, "https://monolith.dev/errors/unauthorized");
            assert_eq!(p.detail.as_deref(), Some("Invalid credentials"));
        }
        other => panic!("Expected Api error, got {other:?}"),
    }

    // 5. Sign out
    su_auth.sign_out();
    assert_eq!(client.token(), None);
}

#[tokio::test]
async fn test_collection_crud() {
    let (url, _handle) = spawn_mock_server().await;
    let client = Client::builder(&url).app("testapp").build().unwrap();

    let tasks = client.records("tasks");

    // 1. Create
    let new_rec = tasks
        .create(&json!({ "title": "Buy groceries" }))
        .await
        .unwrap();
    assert_eq!(new_rec.id, "new_rec_1");
    assert_eq!(new_rec.fields.get("title"), Some(&json!("Buy groceries")));

    // 2. Get one
    let rec = tasks.get_one("rec_123").await.unwrap();
    assert_eq!(rec.id, "rec_123");
    assert_eq!(rec.fields.get("title"), Some(&json!("Existing Task")));

    // 3. Get one not found
    let err = tasks.get_one("not_found").await.unwrap_err();
    assert_eq!(err.status(), Some(404));

    // 4. Update
    let updated = tasks
        .update("rec_123", &json!({ "title": "Updated Task" }))
        .await
        .unwrap();
    assert_eq!(updated.fields.get("title"), Some(&json!("Updated Task")));

    // 5. Replace
    let replaced = tasks
        .replace("rec_123", &json!({ "title": "Replaced Task" }))
        .await
        .unwrap();
    assert_eq!(replaced.fields.get("title"), Some(&json!("Replaced Task")));

    // 6. Delete
    tasks.delete("rec_123").await.unwrap();

    // 7. Delete forbidden
    let del_err = tasks.delete("locked").await.unwrap_err();
    assert_eq!(del_err.status(), Some(403));

    // 8. List
    let list_res = tasks
        .list(Some(&ListOptions::new().limit(2)))
        .await
        .unwrap();
    assert_eq!(list_res.items.len(), 2);
    assert_eq!(list_res.next.as_deref(), Some("next_cur_123"));
    assert_eq!(list_res.total, Some(42));

    // 9. Count
    let count = tasks.count(None).await.unwrap();
    assert_eq!(count, 99);
}

#[tokio::test]
async fn test_files_service() {
    let (url, _handle) = spawn_mock_server().await;
    let client = Client::builder(&url).app("testapp").build().unwrap();
    let files = client.files();

    // 1. URL formatting
    let url_str = files.get_url("avatars", "user.png");
    assert_eq!(
        url_str,
        format!("{url}/api/v1/apps/testapp/buckets/avatars/objects/user.png")
    );

    let thumb_url = files.get_url_with_thumb("avatars", "user.png", Some("100x100"));
    assert_eq!(
        thumb_url,
        format!("{url}/api/v1/apps/testapp/buckets/avatars/objects/user.png?thumb=100x100")
    );

    // 2. Upload
    let info = files
        .upload(
            "avatars",
            "user.png",
            b"fake-image-bytes".to_vec(),
            Some("image/png"),
        )
        .await
        .unwrap();
    assert_eq!(info.key, "user.png");
    assert_eq!(info.size, 16);
    assert_eq!(info.mime, "image/png");

    // 3. Download
    let bytes = files.download("avatars", "user.png").await.unwrap();
    assert_eq!(bytes.as_ref(), b"content-of-user.png");

    // 4. Delete
    files.delete("avatars", "user.png").await.unwrap();

    // 5. List
    let page = files.list("avatars", None, Some(10), None).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].key, "avatar.png");
}

#[tokio::test]
async fn test_batch_operations() {
    let (url, _handle) = spawn_mock_server().await;
    let client = Client::builder(&url).app("testapp").build().unwrap();

    let ops = vec![
        BatchOp::create("tasks", json!({ "title": "Batch Task 1" })),
        BatchOp::update("tasks", "task_2", json!({ "title": "Batch Task 2" })),
        BatchOp::delete("tasks", "task_3"),
    ];

    let res = client.batch(&ops).await.unwrap();
    assert_eq!(res.results.len(), 3);
    assert_eq!(res.results[0].id, "batch_res_0");
    assert_eq!(res.results[1].id, "batch_res_1");
    assert_eq!(res.results[2].id, "batch_res_2");
}
