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
    ApiKey, ApiKeyIn, AppInfo, Auth, BatchItem, BatchKind, CollectionDef, CoreError,
    ExplainQueryReq, ExplainRuleReq, ListParams, Principal, ReadOpts, RecordOut, Registry, ReqKind,
    RequestLog, TotalMode, Value, WriteOpts,
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

fn bearer(headers: &HeaderMap) -> ApiResult<Option<&str>> {
    let Some(v) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    v.to_str()
        .ok()
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|t| Some(t.trim()))
        .ok_or(ApiError(CoreError::Unauthorized))
}

/// Bearer token → superuser; no header → anonymous; a bad token is a 401, never a downgrade.
/// API keys (`mk_...`) are app-bound, so routes outside an app reject them.
fn principal(reg: &Registry, headers: &HeaderMap) -> ApiResult<Principal> {
    let Some(token) = bearer(headers)? else {
        return Ok(Principal::default());
    };
    if token.starts_with("mk_") {
        return Err(ApiError(CoreError::Unauthorized));
    }
    reg.verify_superuser(token)
        .map(|_| Principal::superuser())
        .ok_or(ApiError(CoreError::Unauthorized))
}

/// Who calls an app route. A key is checked against its app and scopes with [`Caller::need`].
struct Caller {
    p: Principal,
    key: Option<Arc<ApiKey>>,
}

impl Caller {
    /// Fails with 403 unless the credential may do `perm` (e.g. `records:read`) on `target`.
    /// Superusers and anonymous callers pass: rules decide for anonymous ones.
    fn need(&self, perm: &str, target: Option<&str>) -> ApiResult<()> {
        match &self.key {
            Some(k) if !k.allows(perm, target) => Err(ApiError(CoreError::Forbidden(
                "the API key lacks the scope for this operation",
            ))),
            _ => Ok(()),
        }
    }
}

/// Resolves the credential for a route of `app`. A key with `records:admin` bypasses collection
/// rules; any other key is anonymous to the rules (no `@request.key` yet).
fn caller(reg: &Registry, headers: &HeaderMap, app: &str) -> ApiResult<Caller> {
    match bearer(headers)? {
        Some(t) if t.starts_with("mk_") => {
            let k = reg
                .verify_api_key(t)
                .ok_or(ApiError(CoreError::Unauthorized))?;
            if k.app != app {
                return Err(ApiError(CoreError::Forbidden(
                    "the API key belongs to another app",
                )));
            }
            let p = if k.admin() {
                Principal::superuser()
            } else {
                Principal::default()
            };
            Ok(Caller { p, key: Some(k) })
        }
        _ => Ok(Caller {
            p: principal(reg, headers)?,
            key: None,
        }),
    }
}

/// [`caller`] plus one scope check; the common case for a single-collection route.
fn app_principal(
    reg: &Registry,
    headers: &HeaderMap,
    app: &str,
    perm: &str,
    target: &str,
) -> ApiResult<Principal> {
    let c = caller(reg, headers, app)?;
    c.need(perm, Some(target))?;
    Ok(c.p)
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
        .route("/api/v1/apps/{app}/realtime", get(realtime))
        .route(
            "/api/v1/apps/{app}/backups",
            get(list_backups).post(create_backup),
        )
        .route("/api/v1/apps/{app}/backups:restore", post(restore_backup))
        .route(
            "/api/v1/apps/{app}/backups/{name}",
            get(download_backup).delete(delete_backup),
        )
        .route(
            "/api/v1/apps/{app}/auth/config",
            get(get_auth_config).put(put_auth_config),
        )
        .route(
            "/api/v1/apps/{app}/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route(
            "/api/v1/apps/{app}/api-keys/{id}",
            axum::routing::delete(revoke_api_key),
        )
        .route("/api/v1/system/stats", get(system_stats))
        .route(
            "/_",
            get(|| async { axum::response::Redirect::permanent("/_/") }),
        )
        .route(
            "/_/",
            get(|headers: HeaderMap| ui_asset(Path(String::new()), headers)),
        )
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
    let p = app_principal(&reg, &headers, &app, "records:read", &coll)?;
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
    let p = app_principal(&reg, &headers, &app, "records:read", &coll)?;
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
    let p = app_principal(&reg, &headers, &app, "records:write", &coll)?;
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
    let p = app_principal(&reg, &headers, &app, "records:read", &coll)?;
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
    let p = app_principal(&reg, &headers, &app, "records:write", &coll)?;
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
    let p = app_principal(&reg, &headers, &app, "records:write", &coll)?;
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
    let p = app_principal(&reg, &headers, &app, "records:write", &coll)?;
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
    let c = caller(&reg, &headers, &app)?;
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
    for it in &items {
        c.need("records:write", Some(it.collection))?;
    }
    let ids = reg
        .app(&app)
        .await?
        .batch_with(&c.p, &items, dry_run)
        .await?;
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

// ---------- realtime (SSE, doc 07) ----------

/// One SSE frame as bytes without copying: the `Arc<str>` is shared with every receiver.
struct FrameBytes(monolith_core::realtime::Frame);

impl AsRef<[u8]> for FrameBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Frames from the hub plus a heartbeat comment so proxies keep the stream open.
struct EventStream {
    sub: monolith_core::realtime::Subscription,
    hb: tokio::time::Interval,
}

impl futures_core::Stream for EventStream {
    type Item = Result<Bytes, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if let Poll::Ready(f) = self.sub.rx.poll_recv(cx) {
            return Poll::Ready(f.map(|f| Ok(Bytes::from_owner(FrameBytes(f)))));
        }
        match self.hb.poll_tick(cx) {
            Poll::Ready(_) => Poll::Ready(Some(Ok(Bytes::from_static(b": hb\n\n")))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// `GET /api/v1/apps/{app}/realtime?collections=a,b`: record changes as Server-Sent Events.
async fn realtime(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let c = caller(&reg, &headers, &app)?;
    c.need("realtime:subscribe", None)?;
    let names: Vec<String> = q
        .get("collections")
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    for n in &names {
        c.need("records:read", Some(n))?;
    }
    let sub = reg.app(&app).await?.subscribe(&c.p, &names)?;
    let mut hb = tokio::time::interval(std::time::Duration::from_secs(
        monolith_core::realtime::HEARTBEAT_S,
    ));
    hb.reset(); // the first tick would fire immediately
    let mut res = Response::new(axum::body::Body::from_stream(EventStream { sub, hb }));
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(res)
}

// ---------- backups (doc 09 part B) ----------

async fn list_backups(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    reg.app_info(&app).await?;
    Ok(axum::Json(json!({"items": reg.list_backups(&app).await?})).into_response())
}

async fn create_backup(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let b = reg.create_backup(&app).await?;
    Ok((StatusCode::CREATED, axum::Json(b)).into_response())
}

async fn delete_backup(
    State(reg): St,
    headers: HeaderMap,
    Path((app, name)): Path<(String, String)>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    reg.delete_backup(&app, &name).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreReq {
    backup: String,
    new_app: String,
}

async fn restore_backup(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let r: RestoreReq = parse_json(body)?;
    let a = reg.restore_backup(&app, &r.backup, &r.new_app).await?;
    Ok((StatusCode::CREATED, axum::Json(app_json(&a))).into_response())
}

/// Reads a file in 256 KiB chunks on a blocking thread; at most 4 chunks are buffered.
struct FileStream(tokio::sync::mpsc::Receiver<std::io::Result<Bytes>>);

impl futures_core::Stream for FileStream {
    type Item = std::io::Result<Bytes>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

async fn download_backup(
    State(reg): St,
    headers: HeaderMap,
    Path((app, name)): Path<(String, String)>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let (mut file, len) = reg.open_backup(&app, &name).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        loop {
            let mut buf = vec![0u8; 256 * 1024];
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    buf.truncate(n);
                    if tx.blocking_send(Ok(Bytes::from(buf))).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    break;
                }
            }
        }
    });
    let mut res = Response::new(axum::body::Body::from_stream(FileStream(rx)));
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sqlite3"),
    );
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{app}-{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

// ---------- per-app auth configuration ----------

async fn get_auth_config(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    Ok(axum::Json(reg.auth_config(&app).await?).into_response())
}

async fn put_auth_config(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let input = parse_json(body)?;
    Ok(axum::Json(reg.put_auth_config(&app, input).await?).into_response())
}

// ---------- API keys (doc 08 §4) ----------

async fn list_api_keys(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    Ok(axum::Json(json!({"items": reg.list_api_keys(&app).await?})).into_response())
}

/// The response carries the secret as `key`; it is never shown again.
async fn create_api_key(
    State(reg): St,
    headers: HeaderMap,
    Path(app): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    let input: ApiKeyIn = parse_json(body)?;
    let (info, secret) = reg.create_api_key(&app, input).await?;
    let mut out =
        serde_json::to_value(info).map_err(|e| ApiError(CoreError::Internal(e.to_string())))?;
    out["key"] = json!(secret);
    Ok((StatusCode::CREATED, axum::Json(out)).into_response())
}

async fn revoke_api_key(
    State(reg): St,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> ApiResult<Response> {
    require_su(&reg, &headers)?;
    reg.revoke_api_key(&app, &id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------- admin UI (doc 15 §4) ----------

/// `ui/dist`, embedded in release builds and read from disk in debug builds.
/// `tools/precompress.mjs` adds `.br`/`.gz` siblings at build time.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist"]
#[allow_missing = true]
struct UiAssets;

async fn ui_asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    let path = if path.is_empty() {
        "index.html"
    } else {
        path.as_str()
    };
    let Some(raw) = UiAssets::get(path) else {
        let msg = if UiAssets::get("index.html").is_none() {
            "admin UI not built: run `pnpm build:ui` and rebuild the server"
        } else {
            "no such UI asset"
        };
        return (StatusCode::NOT_FOUND, msg).into_response();
    };
    let accept = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let accepts = |enc: &str| {
        accept
            .split(',')
            .any(|t| t.split(';').next().is_some_and(|n| n.trim() == enc))
    };
    let (file, encoding) = [("br", ".br"), ("gzip", ".gz")]
        .into_iter()
        .filter(|(enc, _)| accepts(enc))
        .find_map(|(enc, ext)| UiAssets::get(&format!("{path}{ext}")).map(|f| (f, Some(enc))))
        .unwrap_or((raw.clone(), None));
    // Vite hashes asset names, so they never change; index.html must always revalidate.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut res = Response::new(match file.data {
        std::borrow::Cow::Borrowed(b) => axum::body::Body::from(b),
        std::borrow::Cow::Owned(v) => axum::body::Body::from(v),
    });
    let h = res.headers_mut();
    if let Ok(v) = header::HeaderValue::from_str(raw.metadata.mimetype()) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static(cache),
    );
    h.insert(
        header::VARY,
        header::HeaderValue::from_static("accept-encoding"),
    );
    if let Some(enc) = encoding {
        h.insert(
            header::CONTENT_ENCODING,
            header::HeaderValue::from_static(enc),
        );
    }
    res
}
