# 01 — PocketBase Teardown (from source)

Source: `github.com/pocketbase/pocketbase` at commit `7f2773ec` (v0.40.6, 2026-10-26).
All paths below are relative to that repository. Facts marked **[src]** were read
directly from source in this analysis. Facts marked **[inferred]** are conclusions
drawn from the code, not measured behaviour. No benchmark was run (decision: skip
for now, see [17](17-benchmark-plan.md) for the plan to run it later).

## 1. Shape of the codebase

| Package | Size (non-test Go LOC) | Role |
|---------|------|------|
| `core/` | ~22.4k | App, DB access, collections, records, fields, hooks, migrations, settings, backups |
| `apis/` | ~8.7k | HTTP handlers, middlewares (auth, CORS, gzip, rate limit, body limit), realtime |
| `tools/` | ~17.5k | Router, filter parser glue, subscriptions broker, filesystem (local/S3), auth providers, cron, mailer, logger, store |
| `plugins/` | ~5.6k | `jsvm` (goja JS hooks), `migratecmd`, self-update |
| `ui/` | JS | Superuser dashboard, embedded into the binary |

Key dependencies **[src]** (`go.mod`): `modernc.org/sqlite` v1.60.1 (C SQLite
machine-translated to pure Go, no cgo), `pocketbase/dbx` (query builder),
`ganigeorgiev/fexpr` (filter expression parser), `golang-jwt/jwt/v5`, `dop251/goja`
(JS VM), `disintegration/imaging` (thumbnails), `golang.org/x/oauth2`.

## 2. Process model

- Single Go process. `net/http` server, goroutine per request.
- Two SQLite files under `pb_data/`: `data.db` (app data) and `auxiliary.db` (logs and
  other non-critical data) **[src]** `core/base.go:1258`, `core/base.go:1318`.
- Each file is opened **twice** **[src]** `core/base.go:1258-1290`:
  - *concurrent* pool for reads: `DataMaxOpenConns=120`, `DataMaxIdleConns=15`
    (aux: 20/3) **[src]** `core/base.go:34-37`.
  - *nonconcurrent* pool for writes: `SetMaxOpenConns(1)`. All writes funnel through
    one connection, so Go-level serialization replaces SQLite lock contention.
- Default query timeout 30 s **[src]** `core/base.go:38`.

## 3. SQLite configuration **[src]** `core/db_connect.go:14`

```
busy_timeout(10000)        -- first, so WAL switch can wait on other connections
journal_mode(WAL)
journal_size_limit(200000000)
synchronous(NORMAL)        -- no fsync per commit in WAL; fsync at checkpoint
foreign_keys(ON)
temp_store(MEMORY)
cache_size(-32000)         -- 32 MB page cache per connection
_defensive=1
```

No `mmap_size`, no explicit `wal_autocheckpoint` tuning, no `page_size` change.

Lock handling **[src]** `core/db_retry.go:43`: on `database is locked` / `table is
locked`, sleep and retry up to 12 times with back-off 50→1000 ms.

## 4. Data model

- **Collections** are rows in `_collections` with JSON field definitions; each
  collection maps to one real table. Types: `base`, `auth`, `view` **[src]**
  `core/collection_model.go:24-26`. View collections are SQL views.
- **Field types** **[src]**: `text, email, url, number, bool, date, autodate, select,
  file, relation, json, editor, password, geoPoint`.
- **Column storage** **[src]** (`ColumnType()` in `core/field_*.go`):
  - `id`: `TEXT PRIMARY KEY DEFAULT ('r'||lower(hex(randomblob(7))))`; app-generated
    IDs are 15 chars `[a-z0-9]` **[src]** `core/field_text.go:167`,
    `core/collection_model.go:969`. A `TEXT PRIMARY KEY` on a rowid table means a
    second B-tree (the PK autoindex) plus the rowid table **[inferred]**.
  - dates and autodates: `TEXT`.
  - number: `NUMERIC`.
  - multi `relation`, multi `file`, multi `select`: `JSON DEFAULT '[]'`.
  - `geoPoint`: `JSON {"lon":..,"lat":..}`.
- Records are held in memory as a dynamic field map with per-field getters/setters.
- Collections are cached in memory (`FindCachedCollectionByNameOrId`).

## 5. Write path **[src]** `core/db.go:265-330`

```
app.Save(model)
 └ OnModelCreate hook chain
    └ validate (field validators; may run SELECTs, e.g. unique checks)
    └ OnModelCreateExecute hook chain
       └ baseLockRetry( INSERT via nonconcurrent conn )   -- autocommit unless inside RunInTransaction
 └ OnModelAfterCreateSuccess → realtime broadcast, etc.
```

- Every standalone save is its own transaction (one commit per record).
- Batch API `POST /api/batch` runs many create/update/upsert/delete requests in one
  transaction; routes matched by regex **[src]** `apis/batch.go:36-61`. Limits come
  from `Settings.Batch` **[src]** `core/settings_model.go:450`.
- Hooks are an ordered handler chain with priorities and an explicit `e.Next()`
  **[src]** `tools/hook/hook.go`. ~90 hook points exist (model, record, collection,
  request, mailer, realtime, settings, backup, bootstrap/serve/terminate).

## 6. Read path / list API **[src]** `tools/search/provider.go:230-362`

- Query string: `page`, `perPage` (default 30, max 1000), `sort` (max 8 exprs),
  `filter` (max 3500 chars, max 200 exprs), `skipTotal`, `expand`, `fields`.
- Pagination is **OFFSET-based** **[src]** `provider.go:332`.
- Unless `skipTotal=1`, a `COUNT(DISTINCT id)` query runs **in parallel** with the
  page query on every list call **[src]** `provider.go:300`.
- List queries use `SELECT DISTINCT` because rule/filter joins may duplicate rows.
- Parsed filter ASTs cached in a global map, max 500 entries **[src]**
  `tools/search/filter.go:114`. SQL is rebuilt per request from the AST.
- `expand` loads related records per level, max depth 6, via
  `FindRecordsByIds` batches **[src]** `core/record_query_expand.go`.
- `fields` picker trims JSON output after the record is built.

## 7. Rules engine

- Five rules per collection: `listRule, viewRule, createRule, updateRule,
  deleteRule`, plus `manageRule` for auth collections. `nil` = superuser only,
  `""` = public, otherwise a filter expression.
- Expression grammar via `fexpr`: operators `= != > >= < <= ~ !~` and "any-of"
  variants `?= ?!= ?> ...`; `&&`, `||`, parentheses; functions `geoDistance`,
  `strftime` **[src]** `tools/search/token_functions.go`.
- Identifiers resolved by `RecordFieldResolver` **[src]**
  `core/record_field_resolver*.go`:
  - record fields and dotted relation paths (`author.org.name`),
  - back-relations (`comments_via_post`),
  - `@request.auth.*`, `@request.body.*`, `@request.query.*`, `@request.headers.*`,
    `@request.context`, `@request.method`,
  - `@collection.<name>[:alias].*` cross-collection joins,
  - modifiers such as `:isset`, `:changed`, `:length`, `:each`, `:lower`.
- **Compilation target is SQL only.** Each relation hop adds a `LEFT JOIN`; each
  multi-relation hop adds `LEFT JOIN json_each(CASE WHEN json_valid(..) ...)`;
  multi-match semantics add a correlated `NOT EXISTS (SELECT ...)` subquery that
  repeats the whole join chain. See generated SQL in
  `core/record_field_resolver_test.go:162-412`.
- `CanAccessRecord` (used for view/update/delete and realtime) runs
  `SELECT 1 FROM <collection> <joins> WHERE id=? AND <rule> LIMIT 1`
  **[src]** `core/record_query.go:599-639`.

## 8. Realtime **[src]** `apis/realtime.go`, `tools/subscriptions/`

- Transport: **SSE** (`GET /api/realtime`). Client receives `PB_CONNECT` with a
  `clientId`, then sets subscriptions with `POST /api/realtime`
  (max 1000 subscriptions, each ≤2500 chars).
- Connection idle timeout 5 min, max lifetime 30 min (forced reconnect)
  **[src]** `realtime.go:69-70`.
- Subscription topics: `collection/*`, `collection/<id>`, `collection`, each with
  optional `?options={"query":{"filter":..,"expand":..,"fields":..}}`.
- Broadcast on every record create/update/delete **[src]** `realtime.go:594-...`:
  1. Snapshot **all** connected clients, split in chunks of 150, one goroutine per
     chunk (`errgroup`).
  2. For each client, for each of 6 topic prefixes, scan the client's subscription
     map with `strings.HasPrefix`.
  3. For each matching subscription: `CanAccessRecord` → **one SQL query per
     subscriber** (skipped only for superuser or empty rule). If the subscription has
     a `filter`, a **second SQL query** runs **[src]** `realtime.go:856-902`.
  4. Per subscriber: `record.Fresh()` clone, enrich hooks, optional `expand`
     (more queries), optional `fields` pick, **`json.Marshal` per subscriber**.
  5. Send via a new goroutine per message into an **unbuffered** channel
     **[src]** `tools/subscriptions/client.go:96`.
- Deletes: the message is computed *before* the delete (so rules can still see the
  row) and parked in a per-client "dry cache", then flushed after commit.
- Auth record changes/deletes update or drop the auth state of connected clients.

## 9. Auth **[src]** `core/record_tokens.go`, `core/record_query.go:483`, `apis/record_auth*.go`

- Auth collections are ordinary collections with system fields (`email`,
  `emailVisibility`, `verified`, `password`, `tokenKey`).
- Methods: password, OAuth2 (≈32 built-in providers + generic OIDC, see
  `tools/auth/`), email OTP, MFA (two methods within a window), impersonation,
  password reset, email verification and email change.
- Tokens are HS256 JWTs. Signing key = `record.tokenKey + collection.<type>Secret`.
  Changing `tokenKey` revokes all tokens of that user.
- Per request, `loadAuthToken` middleware calls `FindAuthRecordByToken`, which
  parses unverified claims, **loads the user record from the DB**, then verifies the
  signature **[src]** `apis/middlewares.go:184-209`, `core/record_query.go:483-533`.
  Result: at least one SELECT per authenticated request.
- `_authOrigins`, `_mfas`, `_otps`, `_externalAuths` system collections back the
  auth flows.

## 10. Other subsystems

| Subsystem | Notes **[src]** |
|-----------|-----------|
| Logs | `slog` handler batching 200 entries, written to `auxiliary.db` (`core/base.go:1540`) |
| Rate limiting | In-memory **fixed window** per IP/tag; author notes a todo to move to sliding window (`apis/middlewares_rate_limit.go:330-332`) |
| Files | Local or S3 (custom minimal S3 client), protected files via short-lived file tokens, thumbnails via pure-Go `imaging`, global max source pixels `250<<20` because thumbs are the main OOM risk (CHANGELOG v0.40.6) |
| Backups | `VACUUM INTO` temp copies of both DBs + `wal_checkpoint(TRUNCATE)`, then zip with `pb_data` files; restore replaces data dir and restarts |
| Cron | Built-in scheduler; jobs listed/run via `/api/crons` |
| SQL console | `POST /api/sql`, superuser only, ≤1000 rows, ≤3 min |
| Migrations | Go or JS migration files; `_migrations` table; automigrate from UI schema changes |
| Settings | Single JSON row; optional encryption via env key |
| Admin UI | Rewritten from Svelte to **Shablon** (author's own small reactive helper) + Vite, vanilla JS modules, `ui/dist` ≈3.3 MB, embedded via `embed.go`; supports UI extensions served under `/_/extensions` |
| JS hooks | goja VM pool (`plugins/jsvm/pool.go`), type definitions generated for editor autocomplete |

## 11. HTTP route map (v0.40.6) **[src]**

```
GET    /api/health
GET    /api/settings                         (superuser) PATCH /api/settings
POST   /api/settings/test/s3 | /test/email | /apple/generate-client-secret
GET    /api/collections  POST /api/collections  PUT /api/collections/import
GET|PATCH|DELETE /api/collections/{c}        DELETE /api/collections/{c}/truncate
GET    /api/collections/meta/scaffolds | /meta/oauth2-providers
POST   /api/collections/meta/dry-run-view
GET    /api/collections/{c}/records          POST same
GET|PATCH|DELETE /api/collections/{c}/records/{id}
GET    /api/collections/{c}/auth-methods
POST   /api/collections/{c}/auth-refresh | auth-with-password | auth-with-oauth2
POST   /api/collections/{c}/request-otp | auth-with-otp
POST   /api/collections/{c}/request-password-reset | confirm-password-reset
POST   /api/collections/{c}/request-verification | confirm-verification
POST   /api/collections/{c}/request-email-change | confirm-email-change
POST   /api/collections/{c}/impersonate/{id} (superuser)
POST   /api/batch
GET    /api/realtime  POST /api/realtime
POST   /api/files/token   GET /api/files/{c}/{recordId}/{filename}
GET    /api/backups  POST /api/backups  POST /api/backups/upload
GET|DELETE /api/backups/{key}  POST /api/backups/{key}/restore
GET    /api/logs  DELETE /api/logs  GET /api/logs/stats  GET /api/logs/{id}
GET    /api/crons  POST /api/crons/{id}
POST   /api/sql
GET    /_/...  (admin UI, extensions)
```

## 12. What PocketBase does well (keep)

- Single binary, zero external services, embedded UI. Very easy ops.
- Collections + rules model is simple and expressive; developers like it.
- One-writer connection pattern avoids `SQLITE_BUSY` storms.
- Delete broadcast "dry cache" correctly handles rules that need the pre-delete row.
- Batch endpoint with transactional semantics.
- Hook system with priorities and explicit `Next()`.
- Separate auxiliary DB for logs keeps log writes off the main writer.

## 13. Gaps relative to Monolith goals

- **Single-app.** One `pb_data` = one app. Multi-app means multiple processes.
- **No offline sync protocol.** Only realtime + REST. No change feed, no cursors.
- **No analytics** beyond request logs.
- **No generated OpenAPI spec**, no MCP, no long-lived scoped API keys
  (superuser tokens or impersonation tokens are the workaround).
- Performance limits listed in [02](02-pocketbase-bottlenecks.md).
