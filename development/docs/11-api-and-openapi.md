# 11 — API Surface, OpenAPI & MCP ("Plug and Play")

## 1. Principles

- One versioned REST API: `/api/v1/...`. JSON by default; MessagePack via `Accept`.
- **Per-app, schema-aware OpenAPI 3.1** generated at runtime, so any client generator,
  API tool or AI agent can integrate without an SDK.
- **MCP endpoint per app** exposing the same operations as tools for AI harnesses.
- Errors use RFC 9457 `application/problem+json`.
- Every list endpoint is cursor-paginated; totals are opt-in.

## 2. Route map

### Data plane (per app) — base `/api/v1/apps/{app}`

```
GET    /collections/{c}/records                 list (filter, sort, cursor, limit, fields, expand, total)
POST   /collections/{c}/records                 create
GET    /collections/{c}/records/{id}            view
PATCH  /collections/{c}/records/{id}            update (If-Match: _v for optimistic concurrency)
PUT    /collections/{c}/records/{id}            upsert
DELETE /collections/{c}/records/{id}            delete
POST   /batch                                   transactional batch (≤500 ops)
GET    /collections/{c}/records:count           count with filter (exact or estimate)

POST   /auth/{c}/sign-up | /auth/{c}/password | /auth/{c}/oauth2/{provider} | /auth/{c}/otp/request | /auth/{c}/otp/verify
POST   /auth/{c}/magic-link | /auth/{c}/anonymous | /auth/{c}/refresh | /auth/{c}/logout
POST   /auth/{c}/mfa/verify | /auth/{c}/password-reset/request | /confirm | /verify-email/request | /confirm
GET    /auth/{c}/methods | /auth/me | /auth/sessions        DELETE /auth/sessions/{sid}
GET    /.well-known/jwks.json

GET    /realtime  (SSE)   GET /realtime/ws   POST /realtime/{conn}/subscriptions   POST /channels/{name}
GET    /sync/pull         POST /sync/push     GET /sync/bootstrap
POST   /files/{c}/{id}/{field}  GET /files/{c}/{id}/{filename}?thumb=200x200  POST /files/token
POST   /analytics/events  POST /analytics/query  GET /analytics/{preset}
GET    /openapi.json      GET /docs (Scalar/Redoc UI)    POST /mcp (MCP streamable HTTP)
```

### App management (superuser or `schema:*` keys)

```
GET|POST /collections   GET|PATCH|DELETE /collections/{c}   POST /collections:import   GET /collections:export
GET|POST /keys          DELETE /keys/{id}
GET|PATCH /settings     GET /stats (?counts=estimate|exact) GET /logs   GET /logs/slow   GET /logs/stats
GET|POST /backups       POST /backups/{id}/restore   GET /backups/{id}/download
GET|POST /webhooks      GET /cron   POST /cron/{id}/run
```

### Control plane — base `/api/v1/system` (superuser)

```
GET|POST /apps      GET|PATCH|DELETE /apps/{app}     POST /apps/{app}/clone
GET|PATCH /settings GET /health  GET /stats  GET /metrics (Prometheus, separate port optional)
POST /superusers    POST /auth/login  POST /auth/mfa
```

PocketBase-compatible shim (optional feature `pb-compat`): maps
`/api/collections/{c}/records` paths and query params to v1 so existing PocketBase
SDK code can point at Monolith with minimal changes.

## 3. List query contract

| Param | Example | Notes |
|-------|---------|-------|
| `filter` | `status = 'active' && created > @today` | Rules language ([06](06-rules-engine.md)) |
| `sort` | `-created,id` | Final tie-breaker `id` added automatically |
| `limit` | `50` | Default 50, max 1000 |
| `cursor` | `eyJ...` | Opaque, encodes last sort-key tuple + schema version; keyset `WHERE (a,b) > (?,?)` |
| `fields` | `id,title,author.name` | Projected in SQL (only requested columns read) |
| `expand` | `author,tags` | One batched query per relation per level |
| `total` | `none` (default) \| `estimate` \| `exact` | `exact` = `COUNT(*)` with same filter; `estimate` from stats |
| `page` | `3` | Allowed only with `offset=true` (admin UI); discouraged |

Response:

```json
{ "items":[...], "next":"eyJ...", "total":null }
```

## 4. Response encoding (performance)

- Rows are serialized directly from SQLite column values into a reusable
  `BytesMut` buffer (custom writer, `itoa`/`ryu` for numbers, pre-escaped field-name
  fragments per collection). No `serde_json::Value` tree.
- Compression: `zstd` or `br` for responses > 1 KB when the client accepts; `gzip`
  fallback. Level tuned for speed (zstd 1–3).
- `ETag` = hash of `(_v of rows, query shape)` for single-record GETs → `304`.

## 5. Dynamic OpenAPI 3.1

`GET /api/v1/apps/{app}/openapi.json` (cached per schema version, `ETag`):

- One schema object per collection: `Task`, `TaskCreate`, `TaskUpdate` with exact
  field types, enums for `select`, formats (`email`, `uri`, `date-time`), `readOnly`
  system fields, required fields from field options.
- Typed operations per collection (`listTasks`, `getTask`, `createTask`, ...) with
  `filter`/`sort` described including allowed field names (as enum hints).
- Security schemes: `bearerAuth` (user token), `apiKey` (bearer `mk_...`).
- Spec filtered by the caller's credentials: an API key only sees collections and
  operations its scopes allow (`?for=key` default), superusers see everything.
- Static base spec for system endpoints is generated at compile time with `utoipa`;
  the dynamic per-collection part is merged at runtime.

Uses: generate typed clients (`openapi-generator`, `orval`, `openapi-typescript`),
import into Postman/Bruno/Insomnia, feed to AI tool-calling ("function calling from
OpenAPI").

## 6. MCP endpoint (AI harness integration)

`POST /api/v1/apps/{app}/mcp` — Model Context Protocol over streamable HTTP, auth via
API key with `mcp` scope.

| MCP tool | Maps to |
|----------|---------|
| `list_collections` | schema read (filtered by scopes) |
| `describe_collection` | field list, rules summary, example filter |
| `query_records` | list with filter/sort/limit/fields |
| `get_record`, `create_record`, `update_record`, `delete_record` | CRUD (scope-gated) |
| `count_records` | count |
| `analytics_query` | analytics query API |
| `run_backup` | backup (scope `backup:run`) |
| `trace_request`, `explain_rule`, `explain_query`, `list_slow_queries`, `inspect_realtime`, `sync_status`, `check_health`, `diff_schema`, `tail_logs` | debug surface, scope `debug:read` (see [22](22-agent-skills-and-debugging.md)) |

MCP resources: the agent skills pack ([22](22-agent-skills-and-debugging.md)), collection schemas as JSON Schema; app README (owner-editable markdown
describing the data for agents). All tool calls go through the same rules and scopes
as REST, are rate-limited and logged with the key id.

## 7. Webhooks (outbound)

- Triggers: record events (collection + filter), auth events, backup results.
- Delivery: queue in `_webhook_deliveries`, worker with exponential backoff, HMAC-SHA256
  signature header, timeout 10 s, max attempts 10, dead-letter view in admin UI.
- Driven from the change bus after commit, never inside the writer transaction.

## 8. Versioning

- `/api/v1` stays backward compatible; breaking changes → `/api/v2` with overlap.
- Per-app schema changes are reflected in the OpenAPI spec immediately; SDK codegen can
  pin a schema version via `ETag`.
