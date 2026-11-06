# 00 — Vision & Scope

## Problem

Every app in the library needs the same backend pieces: users and
login, a database with access rules, live updates, offline sync, file storage,
backup of user data, and usage analytics. Rebuilding these per app wastes time.
Hosted options (Firebase, Supabase) cost money, lock data in, and are not tuned for
one owner running many small-to-medium apps on one box.

## Product statement

Monolith is **one self-hosted server** that acts as the backend for **many apps**.
Each app is an isolated *project* with its own database, users, rules, files,
API keys and analytics. Apps integrate in one of three ways:

1. **Plug and play (no SDK).** Point any HTTP client, script, or AI agent at the
   app's OpenAPI 3.1 spec or MCP endpoint and authenticate with a runtime-created
   API key. Same spirit as `ntfy`: `curl -H "Authorization: Bearer mk_..." ...` works.
2. **Client SDK (Firebase-style).** Add the SDK to the app (TypeScript, Dart/Flutter,
   Kotlin, Swift). It provides auth, typed CRUD, realtime listeners and offline sync.
3. **Embedded crate (PocketBase-style).** Use Monolith as a Rust library inside a
   custom server, adding routes and hooks in Rust.

## Goals (ranked)

1. **Performance.** Lowest achievable latency and highest throughput per CPU core and
   per GB of RAM. Concrete budgets live in [16](16-performance-engineering.md) and
   gates in [17](17-benchmark-plan.md).
2. **Concurrency.** Many concurrent users per app, many apps per server, high
   read *and* write concurrency, tens of thousands of realtime connections.
3. **Multi-app isolation.** One app's load, bugs or schema changes cannot corrupt or
   stall another app.
4. **Ease of integration.** OpenAPI, MCP, SDKs, runtime tokens.
5. **Data ownership and backup.** Every app's data is exportable, restorable and
   continuously backed up.
6. **Single-container operations.** One image, one volume, one config file.

## Feature scope (v1)

| Area | In scope |
|------|----------|
| Projects (apps) | Create/delete apps at runtime; per-app settings, quotas, keys |
| Database | Collections (base, auth, view), typed fields, indexes, relations, migrations, import/export |
| Rules | list/view/create/update/delete/manage rules per collection, PocketBase-compatible syntax plus extensions |
| Auth | Password, OAuth2 (common providers + generic OIDC), email OTP, MFA, magic link, anonymous users, sessions, impersonation |
| API keys | Scoped, expiring, revocable keys created at runtime (UI/CLI/API) |
| Realtime | Record subscriptions with filters, presence-free broadcast channels, SSE and WebSocket |
| Sync | Offline-first change feed (pull/push), conflict resolution, per-device cursors |
| Files | Local disk or S3-compatible, signed URLs, thumbnails |
| Analytics | Event ingestion API, sessions/users/retention rollups, custom events, dashboard |
| Backups | Scheduled per-app snapshots, retention policy, S3 upload, point-in-time restore of an app |
| Extensibility | Server hooks (Rust in embedded mode, WASM plugins in binary mode), custom routes, cron |
| Admin UI | Manage apps, schema, records, rules, users, keys, logs, analytics, backups |
| Observability | Structured logs, request logs per app, Prometheus metrics, health checks |

## Non-goals (v1)

- Horizontal multi-node clustering / sharding across machines. One node scales
  vertically; replicas for read scaling and DR come later ([20](20-roadmap.md)).
- Arbitrary server-side SQL from clients. Superuser SQL console only.
- A JavaScript hook runtime as the primary extension model (cost: GC pauses and
  per-request VM pools; see [14](14-extensibility.md)).
- Full-text search engine beyond SQLite FTS5 (Tantivy integration is a later option).
- Payments, push notifications, email marketing. (Push may come later as a module.)

## First consumer: MyApp

App data (users, trips, locations, history) must be backed up and synced. It
drives these requirements:

- Location-heavy writes (many small inserts) → write path must batch efficiently.
- Mobile clients going offline → sync protocol with cursors and conflict handling.
- Live app state → realtime subscriptions with per-user rules.
- Geo queries → `geoPoint` field type and R*Tree-backed bounding-box index.

## Glossary

| Term | Meaning |
|------|---------|
| App / project | One isolated tenant of Monolith (e.g. a mobile app). Has its own data DB file(s) |
| Control plane | System-wide state: apps, superusers, global settings, API key registry |
| Data plane | Per-app state: collections, records, users, files, change log |
| Collection | A table with typed fields and rules (PocketBase term, kept for familiarity) |
| Record | A row in a collection |
| Rule | Filter expression that grants access for an operation |
| Writer | The single task that owns write access to one SQLite file |
| Change sequence (`seq`) | Monotonic per-app integer stamped on every committed mutation |
