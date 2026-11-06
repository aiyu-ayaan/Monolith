//! HTTP surface for `api/openapi/monolith.v1.yaml` (Phase 1 subset).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use monolith_core::codec::format_us;
use monolith_core::{
    AppInfo, BatchItem, BatchKind, CollectionDef, CoreError, ListParams, Principal, Registry,
};
use monolith_store::id::{encode_string, new_id};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

/// doc 05 §10: max non-file request body.
pub const MAX_BODY: usize = 1 << 20;

tokio::task_local! {
    static REQUEST_ID: String;
}

pub struct ApiError(CoreError);

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        ApiError(e)
    }
}

type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, title) = match &self.0 {
            CoreError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad-request", "Bad request"),
            CoreError::Validation(_) => {
                (StatusCode::BAD_REQUEST, "validation", "Validation failed")
            }
            CoreError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", "Unauthorized"),
            CoreError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden", "Forbidden"),
            CoreError::NotFound(_) => (StatusCode::NOT_FOUND, "not-found", "Not found"),
            CoreError::Conflict(_) => (StatusCode::CONFLICT, "conflict", "Conflict"),
            CoreError::TooLarge(_) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload-too-large",
                "Payload too large",
            ),
            CoreError::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy", "Busy"),
            CoreError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Internal error",
            ),
        };
        let rid = REQUEST_ID.try_with(Clone::clone).unwrap_or_default();
        let detail = match &self.0 {
            CoreError::Internal(m) => {
                tracing::error!(request_id = %rid, error = %m, "internal error");
                "internal error; quote the request id when reporting".to_owned()
            }
            e => e.to_string(),
        };
        let mut body = json!({
            "type": format!("https://monolith.dev/errors/{code}"),
            "title": title,
            "status": status.as_u16(),
            "detail": detail,
            "requestId": rid,
        });
        if let CoreError::Validation(errs) = &self.0 {
            body["errors"] = json!(errs);
        }
        let mut res = (status, body.to_string()).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if status == StatusCode::SERVICE_UNAVAILABLE {
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        res
    }
}

fn json_bytes(status: StatusCode, body: Vec<u8>) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

fn body_bytes(b: Result<Bytes, BytesRejection>) -> ApiResult<Bytes> {
    b.map_err(|e| {
        ApiError(if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            CoreError::TooLarge(format!("request body over {MAX_BODY} bytes"))
        } else {
            CoreError::BadRequest(format!("request body: {e}"))
        })
    })
}

fn parse_json<T: DeserializeOwned>(b: Result<Bytes, BytesRejection>) -> ApiResult<T> {
    let b = body_bytes(b)?;
    serde_json::from_slice(&b)
        .map_err(|e| ApiError(CoreError::BadRequest(format!("invalid JSON body: {e}"))))
}

fn valid_request_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Echoes a valid client `X-Request-Id` or mints one; problem bodies carry it too.
async fn request_id(req: Request, next: Next) -> Response {
    let rid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| valid_request_id(s))
        .map_or_else(|| encode_string(&new_id()), str::to_owned);
    let (method, path) = (req.method().clone(), req.uri().path().to_owned());
    let start = Instant::now();
    let mut res = REQUEST_ID.scope(rid.clone(), next.run(req)).await;
    tracing::debug!(request_id = %rid, %method, %path, status = res.status().as_u16(), us = start.elapsed().as_micros() as u64);
    if let Ok(v) = HeaderValue::from_str(&rid) {
        res.headers_mut().insert("x-request-id", v);
    }
    res
}

/// Bearer token → superuser; no header → anonymous; a bad token is a 401, never a downgrade.
fn principal(reg: &Registry, headers: &HeaderMap) -> ApiResult<Principal> {
    let Some(v) = headers.get(header::AUTHORIZATION) else {
        return Ok(Principal::default());
    };
    let token = v
        .to_str()
        .ok()
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError(CoreError::Unauthorized))?;
    reg.verify_superuser(token.trim())
        .map(|_| Principal::superuser())
        .ok_or(ApiError(CoreError::Unauthorized))
}

fn require_su(reg: &Registry, headers: &HeaderMap) -> ApiResult<()> {
    if principal(reg, headers)?.superuser {
        Ok(())
    } else {
        Err(ApiError(CoreError::Unauthorized))
    }
}

type St = State<Arc<Registry>>;

pub fn router(reg: Arc<Registry>) -> Router {
    let records = "/api/v1/apps/{app}/collections/{collection}/records";
    Router::new()
        .route(
            "/api/v1/system/health",
            get(|| async { axum::Json(json!({"status": "ok"})) }),
        )
        .route("/api/v1/system/auth/login", post(login))
        .route("/api/v1/system/superusers", post(create_superuser))
        .route("/api/v1/system/apps", get(list_apps).post(create_app))
        .route("/api/v1/system/apps/{app}", get(get_app).delete(delete_app))
        .route(
            "/api/v1/apps/{app}/collections",
            get(list_collections).post(create_collection),
        )
        .route(
            "/api/v1/apps/{app}/collections/{collection}",
            get(get_collection)
                .put(replace_collection)
                .delete(delete_collection),
        )
        .route(records, get(list_records).post(create_record))
        .route(
            &format!("{records}/{{id}}"),
            get(view_record).patch(update_record).delete(delete_record),
        )
        .route("/api/v1/apps/{app}/batch", post(batch))
        .fallback(|| async { ApiError(CoreError::NotFound("route")) })
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(request_id))
        .with_state(reg)
}

// ---------- system ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    email: String,
    password: String,
}

async fn login(State(reg): St, body: Result<Bytes, BytesRejection>) -> ApiResult<Response> {
    let c: Credentials = parse_json(body)?;
    let (token, exp) = reg.login(&c.email, &c.password).await?;
    Ok(axum::Json(json!({"token": token, "expires": format_us(exp)})).into_response())
}

async fn create_superuser(
    State(reg): St,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let c: Credentials = parse_json(body)?;
    let id = reg.create_superuser(&c.email, &c.password).await?;
    let out = json!({"id": encode_string(&id), "email": c.email.trim().to_ascii_lowercase()});
    Ok((StatusCode::CREATED, axum::Json(out)).into_response())
}

fn app_json(a: &AppInfo) -> serde_json::Value {
    json!({"name": a.name, "created": format_us(a.created_us)})
}

async fn list_apps(State(reg): St, headers: HeaderMap) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let items: Vec<_> = reg.list_apps().await?.iter().map(app_json).collect();
    Ok(axum::Json(json!({"items": items})).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewApp {
    name: String,
}

async fn create_app(
    State(reg): St,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let n: NewApp = parse_json(body)?;
    let a = reg.create_app(&n.name).await?;
    Ok((StatusCode::CREATED, axum::Json(app_json(&a))).into_response())
}

async fn get_app(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    Ok(axum::Json(app_json(&reg.app_info(&app).await?)).into_response())
}

async fn delete_app(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    reg.delete_app(&app).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------- collections ----------

fn collection_json(def: &CollectionDef, version: u64) -> ApiResult<serde_json::Value> {
    let mut v =
        serde_json::to_value(def).map_err(|e| ApiError(CoreError::Internal(e.to_string())))?;
    v["version"] = json!(version);
    Ok(v)
}

async fn list_collections(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let schema = reg.app(&app).await?.schema();
    let mut defs: Vec<_> = schema.collections.values().collect();
    defs.sort_by(|a, b| a.def.name.cmp(&b.def.name));
    let items = defs
        .iter()
        .map(|c| collection_json(&c.def, schema.version))
        .collect::<ApiResult<Vec<_>>>()?;
    Ok(axum::Json(json!({"items": items})).into_response())
}

async fn create_collection(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let def: CollectionDef = parse_json(body)?;
    let a = reg.app(&app).await?;
    let c = a.put_collection(def, true).await?;
    Ok((
        StatusCode::CREATED,
        axum::Json(collection_json(&c.def, a.schema().version)?),
    )
        .into_response())
}

async fn get_collection(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let schema = reg.app(&app).await?.schema();
    Ok(axum::Json(collection_json(&schema.get(&coll)?.def, schema.version)?).into_response())
}

async fn replace_collection(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let def: CollectionDef = parse_json(body)?;
    if def.name != coll {
        return Err(ApiError(CoreError::BadRequest(
            "body name must match the path".into(),
        )));
    }
    let a = reg.app(&app).await?;
    let c = a.put_collection(def, false).await?;
    Ok(axum::Json(collection_json(&c.def, a.schema().version)?).into_response())
}

async fn delete_collection(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    reg.app(&app).await?.delete_collection(&coll).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------- records ----------

async fn list_records(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let limit = match q.get("limit") {
        None => None,
        Some(l) => Some(
            l.parse()
                .map_err(|_| ApiError(CoreError::BadRequest("limit must be a number".into())))?,
        ),
    };
    let lp = ListParams {
        filter: q.get("filter").cloned().unwrap_or_default(),
        sort: q.get("sort").cloned().unwrap_or_default(),
        limit,
        cursor: q.get("cursor").cloned(),
        fields: q.get("fields").cloned(),
    };
    let out = reg.app(&app).await?.list_records(&p, &coll, &lp).await?;
    Ok(json_bytes(StatusCode::OK, out))
}

async fn create_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let body = body_bytes(body)?;
    let out = reg.app(&app).await?.create_record(&p, &coll, &body).await?;
    Ok(json_bytes(StatusCode::CREATED, out))
}

async fn view_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let out = reg.app(&app).await?.view_record(&p, &coll, &id).await?;
    Ok(json_bytes(StatusCode::OK, out))
}

async fn update_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let body = body_bytes(body)?;
    let out = reg
        .app(&app)
        .await?
        .update_record(&p, &coll, &id, &body)
        .await?;
    Ok(json_bytes(StatusCode::OK, out))
}

async fn delete_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    reg.app(&app).await?.delete_record(&p, &coll, &id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchBody<'a> {
    #[serde(borrow)]
    ops: Vec<BatchOp<'a>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchOp<'a> {
    op: String,
    collection: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default, borrow)]
    body: Option<&'a serde_json::value::RawValue>,
}

async fn batch(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let raw = body_bytes(body)?;
    let b: BatchBody<'_> = serde_json::from_slice(&raw)
        .map_err(|e| ApiError(CoreError::BadRequest(format!("invalid JSON body: {e}"))))?;
    let items = b
        .ops
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let kind = match o.op.as_str() {
                "create" => BatchKind::Create,
                "update" => BatchKind::Update,
                "delete" => BatchKind::Delete,
                _ => {
                    return Err(ApiError(CoreError::BadRequest(format!(
                        "ops[{i}].op must be create, update or delete"
                    ))));
                }
            };
            Ok(BatchItem {
                kind,
                collection: &o.collection,
                id: o.id.as_deref(),
                body: o.body.map(|r| r.get().as_bytes()),
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;
    let ids = reg.app(&app).await?.batch(&p, &items).await?;
    let results: Vec<_> = ids
        .iter()
        .map(|id| json!({"id": encode_string(id)}))
        .collect();
    Ok(axum::Json(json!({"results": results})).into_response())
}
