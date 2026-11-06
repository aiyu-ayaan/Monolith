//! HTTP surface for `api/openapi/monolith.v1.yaml` (Phase 1 subset).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::{Bytes, HttpBody};
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use monolith_core::app::now_us;
use monolith_core::codec::format_us;
use monolith_core::{
    AppInfo, Auth, BatchItem, BatchKind, CollectionDef, CoreError, ExplainQueryReq, ExplainRuleReq,
    ListParams, Principal, ReadOpts, RecordOut, Registry, ReqKind, RequestLog, TotalMode, Value,
    WriteOpts,
};
use monolith_store::id::{decode as decode_id, encode_string, new_id};
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

#[derive(Clone)]
struct ErrorDetail(String);

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
            CoreError::PreconditionFailed(_) => (
                StatusCode::PRECONDITION_FAILED,
                "precondition-failed",
                "Precondition failed",
            ),
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
            "detail": &detail,
            "requestId": rid,
        });
        if let CoreError::Validation(errs) = &self.0 {
            body["errors"] = json!(errs);
        }
        let mut res = (status, body.to_string()).into_response();
        res.extensions_mut().insert(ErrorDetail(detail));
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

fn record_response(status: StatusCode, out: RecordOut) -> Response {
    let etag = format!("\"{}\"", out.version);
    if out.not_modified {
        let mut res = StatusCode::NOT_MODIFIED.into_response();
        if let Ok(v) = HeaderValue::from_str(&etag) {
            res.headers_mut().insert(header::ETAG, v);
        }
        return res;
    }
    let mut res = json_bytes(status, out.json);
    if let Ok(v) = HeaderValue::from_str(&etag) {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
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

/// Classifies `rest` (path after `/api/v1/apps/{app}/`) → kind + collection for metrics.
fn classify<'a>(method: &axum::http::Method, rest: &'a str) -> (ReqKind, Option<&'a str>) {
    let mut seg = rest.split('/');
    let first = seg.next().unwrap_or("");
    let coll = match (first, seg.next(), seg.next()) {
        ("collections", Some(c), Some(r)) if r.starts_with("records") => c,
        ("batch", None, None) => return (ReqKind::Write, None),
        _ => return (ReqKind::Other, None),
    };
    let read = matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD);
    (
        if read { ReqKind::Read } else { ReqKind::Write },
        Some(coll),
    )
}

/// Echoes a valid client `X-Request-Id` or mints one; problem bodies carry it too.
/// Also feeds the request trace ring buffer and the traffic metrics.
async fn request_id(State(reg): St, req: Request, next: Next) -> Response {
    let rid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| valid_request_id(s))
        .map_or_else(|| encode_string(&new_id()), str::to_owned);
    let su = principal(&reg, req.headers()).is_ok_and(|p| p.superuser);
    let (method, path) = (req.method().clone(), req.uri().path().to_owned());
    let app_name = path
        .strip_prefix("/api/v1/apps/")
        .and_then(|s| s.split('/').next())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let bytes_in = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .unwrap_or(0);
    let now = now_us();
    let ts = format_us(now);
    let start = Instant::now();
    let mut res = REQUEST_ID.scope(rid.clone(), next.run(req)).await;
    let duration_us = start.elapsed().as_micros() as u64;
    let status = res.status().as_u16();
    tracing::debug!(request_id = %rid, %method, %path, status, us = duration_us);
    let bytes_out = res.body().size_hint().exact().unwrap_or(0);
    let rest = app_name
        .as_deref()
        .and_then(|a| {
            path.strip_prefix("/api/v1/apps/")?
                .strip_prefix(a)?
                .strip_prefix('/')
        })
        .unwrap_or("");
    let (kind, coll) = classify(&method, rest);
    // The stats endpoints are polled by the admin UI; counting them would drown real traffic.
    let counted = !path.ends_with("/stats");
    if counted {
        reg.metrics
            .record_request(now, kind, status, duration_us, bytes_in, bytes_out);
    }
    if let Some(app) = app_name.as_deref().and_then(|n| reg.loaded_app(n)) {
        if counted {
            app.metrics
                .record_request(now, kind, status, duration_us, bytes_in, bytes_out);
            if kind == ReqKind::Read
                && status < 400
                && let Some(c) = coll.and_then(|c| app.schema().get(c).ok().map(|c| c.cid))
            {
                app.metrics.record_coll_read(c);
            }
        }
        let error = res.extensions().get::<ErrorDetail>().map(|e| e.0.clone());
        app.record_log(RequestLog {
            request_id: rid.clone(),
            timestamp: ts,
            method: method.to_string(),
            path,
            status,
            duration_us,
            superuser: su,
            error,
        });
    }
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
        .route("/api/v1/system/install", get(install_status).post(install))
        .route("/api/v1/system/superusers", post(create_superuser))
        .route("/api/v1/system/apps", get(list_apps).post(create_app))
        .route("/api/v1/system/apps/{app}", get(get_app).delete(delete_app))
        .route(
            "/api/v1/apps/{app}/collections",
            get(list_collections).post(create_collection),
        )
        .route("/api/v1/apps/{app}/collections:diff", post(diff_collection))
        .route(
            "/api/v1/apps/{app}/collections/{collection}",
            get(get_collection)
                .put(replace_collection)
                .delete(delete_collection),
        )
        .route(records, get(list_records).post(create_record))
        .route(&format!("{records}:count"), get(count_records))
        .route(
            &format!("{records}/{{id}}"),
            get(view_record)
                .patch(update_record)
                .put(upsert_record)
                .delete(delete_record),
        )
        .route("/api/v1/apps/{app}/batch", post(batch))
        .route("/api/v1/apps/{app}/rules:explain", post(explain_rule))
        .route("/api/v1/apps/{app}/query:explain", post(explain_query))
        .route("/api/v1/apps/{app}/logs", get(list_logs))
        .route("/api/v1/apps/{app}/logs/slow", get(list_slow_logs))
        .route("/api/v1/apps/{app}/stats", get(app_stats))
        .route("/api/v1/system/stats", get(system_stats))
        .route(
            "/_",
            get(|| async { axum::response::Redirect::permanent("/_/") }),
        )
        .route("/_/", get(|| ui_asset(Path(String::new()))))
        .route("/_/{*path}", get(ui_asset))
        .fallback(|| async { ApiError(CoreError::NotFound("route")) })
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(reg.clone(), request_id))
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

async fn install_status(State(reg): St) -> ApiResult<Response> {
    let required = reg.install_required().await?;
    Ok(axum::Json(json!({"required": required})).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    token: String,
    email: String,
    password: String,
}

async fn install(State(reg): St, body: Result<Bytes, BytesRejection>) -> ApiResult<Response> {
    let i: Install = parse_json(body)?;
    let (token, exp) = reg.install(&i.token, &i.email, &i.password).await?;
    let out = json!({"token": token, "expires": format_us(exp)});
    Ok((StatusCode::CREATED, axum::Json(out)).into_response())
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

async fn diff_collection(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let def: CollectionDef = parse_json(body)?;
    let statements = reg.app(&app).await?.diff_collection(def)?;
    Ok(axum::Json(json!({"statements": statements})).into_response())
}

// ---------- records ----------

fn parse_dry_run(q: &HashMap<String, String>) -> ApiResult<bool> {
    match q.get("dry_run").map(String::as_str) {
        None | Some("false" | "0" | "") => Ok(false),
        Some("true" | "1") => Ok(true),
        Some(_) => Err(ApiError(CoreError::BadRequest(
            "dry_run must be true or false".into(),
        ))),
    }
}

fn parse_if_match(headers: &HeaderMap) -> ApiResult<Option<u64>> {
    let Some(v) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let s = v
        .to_str()
        .map_err(|_| ApiError(CoreError::PreconditionFailed("invalid If-Match header")))?
        .trim()
        .trim_matches('"');
    s.parse::<u64>()
        .map(Some)
        .map_err(|_| ApiError(CoreError::PreconditionFailed("invalid If-Match header")))
}

fn parse_if_none_match(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::IF_NONE_MATCH)?
        .to_str()
        .ok()?
        .trim()
        .trim_matches('"')
        .parse::<u64>()
        .ok()
}

fn parse_total(q: &HashMap<String, String>, default: TotalMode) -> ApiResult<TotalMode> {
    match q.get("total").map(String::as_str) {
        None | Some("") => Ok(default),
        Some("none") => Ok(TotalMode::None),
        Some("exact") => Ok(TotalMode::Exact),
        Some("estimate") => Ok(TotalMode::Estimate),
        Some(_) => Err(ApiError(CoreError::BadRequest(
            "total must be none, exact or estimate".into(),
        ))),
    }
}

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
        expand: q.get("expand").cloned(),
        total: parse_total(&q, TotalMode::None)?,
    };
    let out = reg.app(&app).await?.list_records(&p, &coll, &lp).await?;
    Ok(json_bytes(StatusCode::OK, out))
}

async fn count_records(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let mode = match parse_total(&q, TotalMode::Exact)? {
        TotalMode::None => TotalMode::Exact,
        m => m,
    };
    let filter = q.get("filter").map_or("", String::as_str);
    let (total, exact) = reg
        .app(&app)
        .await?
        .count_records(&p, &coll, filter, mode)
        .await?;
    Ok(axum::Json(json!({"total": total, "exact": exact})).into_response())
}

async fn create_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let opts = WriteOpts {
        dry_run: parse_dry_run(&q)?,
        if_match: None,
        expand: q.get("expand").cloned(),
    };
    let body = body_bytes(body)?;
    let out = reg
        .app(&app)
        .await?
        .create_record_with(&p, &coll, &body, &opts)
        .await?;
    Ok(record_response(StatusCode::CREATED, out))
}

async fn view_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let opts = ReadOpts {
        fields: q.get("fields").cloned(),
        expand: q.get("expand").cloned(),
        if_none_match: parse_if_none_match(&headers),
    };
    let out = reg
        .app(&app)
        .await?
        .view_record_with(&p, &coll, &id, &opts)
        .await?;
    Ok(record_response(StatusCode::OK, out))
}

async fn update_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
    Query(q): Query<HashMap<String, String>>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let opts = WriteOpts {
        dry_run: parse_dry_run(&q)?,
        if_match: parse_if_match(&headers)?,
        expand: q.get("expand").cloned(),
    };
    let body = body_bytes(body)?;
    let out = reg
        .app(&app)
        .await?
        .update_record_with(&p, &coll, &id, &body, &opts)
        .await?;
    Ok(record_response(StatusCode::OK, out))
}

async fn upsert_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
    Query(q): Query<HashMap<String, String>>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let opts = WriteOpts {
        dry_run: parse_dry_run(&q)?,
        if_match: parse_if_match(&headers)?,
        expand: q.get("expand").cloned(),
    };
    let body = body_bytes(body)?;
    let out = reg
        .app(&app)
        .await?
        .upsert_record_with(&p, &coll, &id, &body, &opts)
        .await?;
    let status = if out.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok(record_response(status, out))
}

async fn delete_record(
    State(reg): St,
    headers: HeaderMap,
    Path((app, coll, id)): Path<(String, String, String)>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let opts = WriteOpts {
        dry_run: parse_dry_run(&q)?,
        if_match: parse_if_match(&headers)?,
        expand: None,
    };
    reg.app(&app)
        .await?
        .delete_record_with(&p, &coll, &id, &opts)
        .await?;
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
    Query(q): Query<HashMap<String, String>>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let p = principal(&reg, &headers)?;
    let dry_run = parse_dry_run(&q)?;
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
    let ids = reg.app(&app).await?.batch_with(&p, &items, dry_run).await?;
    let results: Vec<_> = ids
        .iter()
        .map(|id| json!({"id": encode_string(id)}))
        .collect();
    Ok(axum::Json(json!({"results": results})).into_response())
}

// ---------- debug surface (doc 22 §2) ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthAsBody {
    id: String,
    #[serde(default = "default_users")]
    collection: String,
    #[serde(default)]
    claims: HashMap<String, serde_json::Value>,
}

fn default_users() -> String {
    "users".into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExplainRuleBody<'a> {
    collection: String,
    #[serde(default)]
    op: String,
    #[serde(default)]
    expr: Option<String>,
    #[serde(default)]
    auth_as: Option<AuthAsBody>,
    #[serde(default, borrow)]
    record: Option<&'a serde_json::value::RawValue>,
    #[serde(default, borrow)]
    body: Option<&'a serde_json::value::RawValue>,
}

fn json_to_claim(v: &serde_json::Value) -> Option<Value> {
    match v {
        serde_json::Value::Null => Some(Value::Null),
        serde_json::Value::Bool(b) => Some(Value::Int(i64::from(*b))),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .or_else(|| n.as_f64().map(Value::Real)),
        serde_json::Value::String(s) => Some(Value::Text(s.as_str().into())),
        _ => None,
    }
}

async fn explain_rule(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let raw = body_bytes(body)?;
    let b: ExplainRuleBody<'_> = serde_json::from_slice(&raw)
        .map_err(|e| ApiError(CoreError::BadRequest(format!("invalid JSON body: {e}"))))?;
    let auth = match b.auth_as {
        None => None,
        Some(a) => {
            let id = decode_id(&a.id).ok_or_else(|| {
                ApiError(CoreError::BadRequest(
                    "authAs.id must be a valid record id".into(),
                ))
            })?;
            let claims = a
                .claims
                .iter()
                .filter_map(|(k, v)| Some((k.clone().into_boxed_str(), json_to_claim(v)?)))
                .collect();
            Some(Auth {
                id,
                collection: a.collection.into_boxed_str(),
                claims,
            })
        }
    };
    let out = reg.app(&app).await?.explain_rule(ExplainRuleReq {
        collection: b.collection,
        op: b.op,
        expr: b.expr,
        auth,
        record: b.record.map(|r| r.get().as_bytes().to_vec()),
        body: b.body.map(|r| r.get().as_bytes().to_vec()),
    })?;
    Ok(axum::Json(out).into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExplainQueryBody {
    collection: String,
    #[serde(default)]
    filter: String,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    superuser: bool,
}

async fn explain_query(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let b: ExplainQueryBody = parse_json(body)?;
    let out = reg
        .app(&app)
        .await?
        .explain_query(ExplainQueryReq {
            collection: b.collection,
            filter: b.filter,
            sort: b.sort,
            superuser: b.superuser,
        })
        .await?;
    Ok(axum::Json(out).into_response())
}

async fn list_logs(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let limit = match q.get("limit") {
        None => 100,
        Some(l) => l
            .parse::<usize>()
            .map_err(|_| ApiError(CoreError::BadRequest("limit must be a number".into())))?,
    };
    let items = reg
        .app(&app)
        .await?
        .list_logs(q.get("request_id").map(String::as_str), limit);
    Ok(axum::Json(json!({"items": items})).into_response())
}

async fn list_slow_logs(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let limit = match q.get("limit") {
        None => 50,
        Some(l) => l
            .parse::<usize>()
            .map_err(|_| ApiError(CoreError::BadRequest("limit must be a number".into())))?,
    };
    let items = reg.app(&app).await?.slow_logs(limit);
    Ok(axum::Json(json!({"items": items})).into_response())
}

async fn system_stats(State(reg): St, headers: HeaderMap) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    Ok(axum::Json(reg.system_stats().await?).into_response())
}

async fn app_stats(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let exact = match q.get("counts").map(String::as_str) {
        None | Some("estimate") => false,
        Some("exact") => true,
        Some(_) => {
            return Err(ApiError(CoreError::BadRequest(
                "counts must be estimate or exact".into(),
            )));
        }
    };
    Ok(axum::Json(reg.app(&app).await?.stats(exact).await?).into_response())
}

// ---------- admin UI (doc 15 §4) ----------

/// `ui/dist`, embedded in release builds and read from disk in debug builds.
// shortcut: served uncompressed; doc 15 wants brotli/gzip precompressed at build time.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist"]
#[allow_missing = true]
struct UiAssets;

async fn ui_asset(Path(path): Path<String>) -> Response {
    let path = if path.is_empty() {
        "index.html"
    } else {
        path.as_str()
    };
    let Some(file) = UiAssets::get(path) else {
        let msg = if UiAssets::get("index.html").is_none() {
            "admin UI not built: run `pnpm build:ui` and rebuild the server"
        } else {
            "no such UI asset"
        };
        return (StatusCode::NOT_FOUND, msg).into_response();
    };
    // Vite hashes asset names, so they never change; index.html must always revalidate.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mime = file.metadata.mimetype().to_owned();
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache.to_owned()),
        ],
        file.data.into_owned(),
    )
        .into_response()
}
