# 20 — Roadmap

Each phase ends with exit criteria. A phase is not done until its benchmark gates pass.

## Phase 0 — Spikes & bake-off (decide the engine)

- Cargo workspace skeleton, CI (fmt, clippy, tests, deny), bench harness.
- PocketBase baseline P1–P7 ([17 §2](17-benchmark-plan.md)).
- `AppStore` prototype on SQLite (writer + group commit, reader pool) and Postgres
  (co-located, Unix socket); Turso MVCC if usable.
- Micro-benchmarks: JSON writer, UUIDv7 BLOB vs alternatives, `WITHOUT ROWID` vs rowid,
  page size, allocator, `sonic-rs` vs `serde_json`.
- DuckDB vs SQLite analytics A1–A3; musl vs glibc image.

**Exit:** ADR-001 ([04](04-database-engine-evaluation.md)) marked Accepted with
measured numbers; budgets in [16](16-performance-engineering.md) revised with data.

## Phase 1 — Core data plane

- Control plane (`system.db`), apps CRUD, superuser auth (+TOTP).
- Collections (base/auth/view/timeseries), field types, migrations planner,
  junction tables, indexes.
- Rules parser, type checker, SQL compiler, predicate compiler + differential tests.
- Records REST: CRUD, keyset list, fields, expand, batch.
- Change log; response JSON writer; error model.
- Admin UI v0: login, apps, collections editor, records grid.

**Exit:** M1–M6 gates pass; PocketBase rules test vectors (ported) pass.

## Phase 2 — Auth, keys, realtime

- User auth: password, OAuth2/OIDC, OTP, magic link, anonymous, MFA, sessions.
- API keys with scopes; per-app JWKS.
- Realtime hub: SSE + WS, subscription index, in-memory rule eval, channels, resume.
- Rate limiting; request logs (`logs.db`).
- TS SDK v0 (auth, CRUD, realtime). Dart SDK v0.

**Exit:** M7, M8 gates; auth security review checklist complete.

## Phase 3 — Sync, files, backups

- Sync pull/push, conflict policies, device registry, bootstrap.
- Files (local + S3), thumbnails.
- Backups: snapshots, WAL shipping, restore (new app / in place), policies.
- Encryption at rest (opt-in, gated on the encrypted S1/S3 benchmark) and keyed
  backup/restore: KEK/DEK envelope, age-encrypted archives ([09 B4](09-sync-and-backup.md)).
- SDK offline sync (TS IndexedDB/SQLite, Dart SQLite).
- Pilot app integration.

**Exit:** M9, M12 gates; a pilot app runs on Monolith in staging with offline sync.

## Phase 4 — Plug and play, analytics, extensibility

- Dynamic OpenAPI 3.1, docs UI, codegen CLI.
- MCP endpoint.
- Analytics ingest, rollups, query API, dashboards.
- Webhooks, cron, WASM plugins, embedded crate public API.
- Admin UI complete (all screens in [15 §5](15-admin-ui.md)).

**Exit:** M10, M11 gates; external tool (Postman, an AI agent via MCP) integrates with
no SDK.

## Phase 5 — Hardening & later

- PocketBase importer + `pb-compat` shim.
- Kotlin and Swift SDKs.
- Passkeys, push notifications module, feature flags.
- Read replicas / standby node via WAL shipping; multi-node realtime.
- Optional per-app Postgres or Turso engine (if bake-off/production shows a need).
- Vector field type; Tantivy full-text option.

## Open questions (to resolve before or during Phase 0)

| # | Question | Owner input needed? |
|---|----------|--------------------|
| Q1 | Client stacks of your first apps (Flutter? native? web?) — sets SDK priority | Yes |
| Q2 | Target hardware for production (VPS size, ARM vs x86) | Yes |
| Q3 | Durability default: `synchronous=NORMAL` (fast, may lose last commits on power loss) vs `FULL` | Yes |
| Q4 | Should app admins (non-superuser, per-app) exist in v1? | Yes |
| Q5 | Turso MVCC maturity at decision time | No — verify in Phase 0 |
| Q6 | Public multi-tenant SaaS later, or private use only? Affects quotas/billing design | Yes |
