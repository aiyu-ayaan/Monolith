# Monolith — Design Docs (Inspection / Analysis Phase)

Monolith is a self-hosted, single-container backend suite (database + auth + realtime +
sync + files + analytics) written in Rust. One server powers the whole app library
(any current or future project) with per-app isolation, an OpenAPI-first
"plug and play" surface, and native SDKs for direct code integration.

**Performance is the #1 requirement.** Every design decision in these docs is judged
first by latency, throughput and memory per connection, then by features.

Status: **analysis phase**. No code yet. These docs are the source of truth for the
build phase. Reference implementation studied: PocketBase v0.40.6
(commit `7f2773ec234b62d0b80ca47c508397cb3d0da3ea`, 2026-10-26).

## Reading order

| # | Doc | What it answers |
|---|-----|-----------------|
| 00 | [Vision & scope](00-vision-and-scope.md) | What we build, for whom, what we do not build |
| 01 | [PocketBase teardown](01-pocketbase-teardown.md) | How PocketBase actually works, from source |
| 02 | [PocketBase bottlenecks](02-pocketbase-bottlenecks.md) | Where PocketBase loses performance and our fix for each |
| 03 | [Architecture](03-architecture.md) | Process/thread model, control plane vs data plane, request path |
| 04 | [Database engine evaluation](04-database-engine-evaluation.md) | SQLite vs Postgres vs Turso vs KV vs DuckDB; decision + bake-off |
| 05 | [Data model & storage](05-data-model-and-storage.md) | Schemas, IDs, field types, relations, indexes, migrations |
| 06 | [Rules engine](06-rules-engine.md) | Access-rule language and its compiler (SQL + in-memory) |
| 07 | [Realtime](07-realtime.md) | SSE/WebSocket live data, subscription index, fan-out |
| 08 | [Authentication](08-authentication.md) | Users, sessions, tokens, OAuth2, OTP, MFA, API keys |
| 09 | [Sync & backup](09-sync-and-backup.md) | Offline-first sync protocol, change feed, app data backup |
| 10 | [Analytics](10-analytics.md) | Event ingestion, rollups, query API |
| 11 | [API surface & OpenAPI](11-api-and-openapi.md) | REST, dynamic OpenAPI 3.1, MCP for AI agents, runtime tokens |
| 12 | [SDKs & integration](12-sdks-and-integration.md) | Plug-and-play vs embedded crate vs client SDKs |
| 13 | [Files](13-files.md) | Uploads, storage backends, thumbnails |
| 14 | [Extensibility](14-extensibility.md) | Hooks, WASM plugins, custom routes, cron |
| 15 | [Admin UI](15-admin-ui.md) | Framework choice (SolidJS) and UI architecture |
| 16 | [Performance engineering](16-performance-engineering.md) | Budgets, techniques, crate choices, anti-patterns |
| 17 | [Benchmark plan](17-benchmark-plan.md) | What we measure, how, pass/fail gates |
| 18 | [Deployment (one container)](18-deployment.md) | Docker image, volumes, config, ops |
| 19 | [Security](19-security.md) | Threat model and hardening |
| 20 | [Roadmap](20-roadmap.md) | Phased build plan with exit criteria |
| 21 | [Repository structure](21-repository-structure.md) | Monorepo layout, API contract, user docs, npm/Android/other SDK packages |
| 22 | [Agent skills & debuggability](22-agent-skills-and-debugging.md) | Skills pack for all AI agents, debug endpoints, MCP debug tools |
| 23 | [Brand & logo](23-brand.md) | Keystone M logo, Monolith Red, lockups, favicon set, usage rules |

## Key decisions at a glance

| Area | Decision | Doc |
|------|----------|-----|
| Language / runtime | Rust (stable), Tokio multi-thread runtime, `axum` HTTP | 03 |
| Primary DB | **Pending bake-off.** Lead candidate: embedded SQLite, one DB file per app, one writer thread per file with group commit. Fallback: co-located PostgreSQL | 04 |
| Analytics store | Embedded DuckDB (columnar) behind a Cargo feature, SQLite rollups as fallback | 04, 10 |
| IDs | UUIDv7 as 16-byte BLOB (client-generatable offline), 26-char base32 on the wire | 05 |
| Multi-relations | Junction tables with indexes, not JSON arrays | 05 |
| Rules | Compiled once per schema version to (a) SQL fragments and (b) in-memory predicates | 06 |
| Realtime | SSE + WebSocket, subscriptions indexed by collection, rule eval in memory, serialize once per audience | 07 |
| Auth tokens | Short-lived signed access token (Ed25519/HS256) + rotating refresh token; no DB hit on hot path | 08 |
| Sync | Per-app monotonic change sequence + HLC per row, cursor pull/push protocol | 09 |
| Plug and play | Per-app dynamic OpenAPI 3.1 + MCP endpoint + runtime-created scoped API keys | 11 |
| Admin UI | SolidJS + Vite, prebuilt, brotli-precompressed, embedded in binary | 15 |
| Packaging | One static binary in one Docker image (`distroless`/`scratch`), one data volume | 18 |
