# Monolith TODO

Source of truth for design: `docs/README.md`. Phases: `docs/20-roadmap.md`.

## Done — Analysis phase

- [x] Clone and study PocketBase v0.40.6 source (commit `7f2773ec`)
- [x] Teardown of PocketBase: DB setup, write/read path, rules, realtime, auth (`docs/01`)
- [x] Bottleneck list B1–B23 with Monolith fixes (`docs/02`)
- [x] Architecture: control/data plane, thread model, group commit, change bus (`docs/03`)
- [x] DB engine evaluation: SQLite vs Postgres vs Turso vs KV vs DuckDB (`docs/04`)
- [x] Data model, IDs, field types, change log, migrations (`docs/05`)
- [x] Rules engine design (`docs/06`)
- [x] Realtime design (`docs/07`)
- [x] Auth, tokens, API keys (`docs/08`)
- [x] Sync protocol and backup (`docs/09`)
- [x] Analytics (`docs/10`)
- [x] API surface, OpenAPI, MCP (`docs/11`)
- [x] SDKs and integration modes (`docs/12`)
- [x] Files, extensibility, admin UI framework choice (SolidJS) (`docs/13`–`15`)
- [x] Performance engineering and benchmark plan (`docs/16`, `17`)
- [x] Deployment, security, roadmap (`docs/18`–`20`)
- [x] Root `CLAUDE.md` and `README.md` (architecture, data model, API quick-start)
- [x] Repository structure: `api/` contract, `docs/` user API docs, `sdks/` (npm, Android/Kotlin, Dart, Swift, Python, Rust) (`docs/21`)
- [x] Common JS/TS project setup with pnpm workspaces (`ui/`, `sdks/typescript/`, `examples/web-typescript/`)

## Blocked on owner answers (`docs/20` open questions)

- [ ] Q1: Client stacks of your first apps (Flutter / native Android / web) → SDK priority
- [ ] Q2: Production hardware (vCPU, RAM, ARM vs x86)
- [ ] Q3: Durability default (`synchronous=NORMAL` vs `FULL`)
- [ ] Q4: Per-app admins in v1?
- [ ] Q5: Private use only, or public multi-tenant later?

## Next — Phase 0 (spikes and bake-off)

- [x] Create Cargo workspace skeleton + `rust-toolchain.toml`, CI (fmt, clippy, test, deny); Rust tasks via `pnpm rust:*`
- [x] Create `bench/harness` engine harness (S1 point read, S3 concurrent inserts; `pnpm bench:engine`)
- [ ] Extend harness: S2, S4–S8, realtime/sync scenarios
- [ ] PocketBase baseline P1–P7 in Docker (ask before running on host) — validates B1–B10
- [x] `AppStore` trait + SQLite prototype: writer thread, group commit, reader pool (`crates/monolith-store`)
- [x] SQLite prototype follow-ups: writer panic recovery, bounded reader wait (`read_timeout`), idle WAL checkpoint task (`crates/monolith-store`)
- [ ] SQLite compile flags (`LIBSQLITE3_FLAGS`)
- [ ] Postgres co-located prototype over Unix socket
- [ ] Verify Turso MVCC maturity (Q-engine); include only if usable
- [x] Micro-benchmark code: UUIDv7 encode/decode, group commit on/off, `WITHOUT ROWID` vs rowid, point read (`benches/store.rs`, not yet run)
- [ ] Micro-benchmarks still to write: JSON row writer vs `serde_json`/`sonic-rs`, page size, mimalloc vs jemalloc
- [ ] Analytics bake-off A1–A3: DuckDB vs SQLite rollups
- [ ] Docker image: musl vs glibc, DuckDB static link feasibility
- [ ] Mark ADR-001 (`docs/04`) Accepted with measured numbers; revise `docs/16` budgets

Phase 0 still open: benchmark runs (owner approval needed before running on the host),
Postgres prototype, PocketBase baseline, DuckDB A1–A3, Turso check, Docker image.

## In progress — Phase 1 (core data plane)

Order of work: rules → contract → store tweaks → auth → core → server → admin UI (all landed).
Next: remaining Phase 1 follow-ups below, then the exit gates.

- [x] `crates/monolith-rules`: parser, type checker, SQL compiler (IS/IS NOT, LIKE ESCAPE,
      IN, EXISTS semi-joins for single relations up to 4 hops), in-memory predicate,
      classes Const/AuthOnly/RowLocal/Relational, differential suite (12k random rules),
      micro-bench (`benches/rules.rs`)
- [x] `api/`: OpenAPI v1 for health, superuser login, superusers, apps, collections,
      records (keyset list, CRUD), batch; RFC 9457 `error.json`; vectors `ids.json`, `filters.json`
- [x] `monolith-store`: `seq_slot`/`hlc_slot` stamping, `RETURNING` rows in `WriteResult`,
      zero affected rows → `NotFound` (rule-guarded writes), `Error::Constraint`
- [x] `crates/monolith-auth` (tests pass): Argon2id
      hash/verify, HMAC-SHA256 superuser token `v1.<sub hex>.<exp µs>.<mac hex>`
- [x] `crates/monolith-core`: schema + compiled templates per version, migration planner,
      schema-driven JSON codec, keyset list (NULL-aware cursors, plan cache), records CRUD + batch,
      `Registry` (system.db, apps, superusers, token key). 11 tests incl. filter vectors
- [x] `crates/monolith-server` (`pnpm dev:server`): axum routes per the OpenAPI contract,
      problem+json with request id, `X-Request-Id` echo, 1 MiB limit (413), bearer superuser auth,
      env config + first-superuser bootstrap, embedded admin UI at `/_/`. 3 in-process HTTP tests
- [x] Admin UI v0 (`ui/`, 18 KB gzip JS): login, apps, collection schema editor, records grid
      (filter, sort, cursor paging, JSON create/edit/delete). CI typechecks and builds it
- [x] `agent-skills/monolith-rest` + `monolith-rules`: "implemented today" sections
- [x] First superuser via one-time installer link (PocketBase-style): `GET/POST /api/v1/system/install`,
      256-bit token in memory, 1 h TTL, single use; env credential bootstrap removed
- [x] Admin UI redesign (PocketBase layout): icon rail, collections sidebar, records grid, schema and
      record drawers, typed record form, install page, light/dark tokens, no external assets (27 KB gzip JS)
- [x] Manual check in a browser: `pnpm dev`, open the printed setup link
- [x] `pnpm dev`: backend + admin UI together (`tools/dev.mjs`), root scripts restructured
- [x] `monolith superuser create <email>` CLI for headless setup (doc 18): password via
      no-echo prompt or piped stdin, never argv; safe next to a running server
- [x] Phase 1 REST & debug follow-ups: single-relation `expand` (up to 4 hops, 1 batched query per relation per level),
      `total=none|exact|estimate`, `GET .../records:count`, `If-Match`/`ETag`/`If-None-Match` (`412`/`304`),
      `PUT` upsert, `?dry_run=true` savepoint rollback, `POST .../collections:diff`, `POST .../rules:explain`,
      `POST .../query:explain`, request trace ring buffer (`GET .../logs?request_id=` & `GET .../logs/slow`)
- [x] Global and per-app traffic & store analytics (in-memory since start, per-minute 1h & per-hour 48h rings):
      `SqliteStore::stats`, `GET /api/v1/system/stats`, `GET /api/v1/apps/{app}/stats?counts=estimate|exact`,
      and Admin UI server + per-app analytics screens (`/#/analytics`, `/#/apps/{app}/analytics`) with SVG
      time-series charts, storage engine internals, per-collection counters, slow requests and recent errors
- [x] Multi-relation junction tables (`_rel_<coll>_<field>`, `maxSelect > 1`), any-of `?=`/`?!=`/`?>`/`?>=`/`?<`/`?<=`/`?~`/`?!~`
      and all-of `=` operators across relation hops, multi-relation `expand`, and Admin UI schema/record drawer support
- [x] `@collection.<name>(:<alias>)?.<path>` cross-collection joins (up to 4 aliases, shared within `&&` scopes)
      and `<coll>_via_<field>` back-relations (over both single and multi relations) in rules, filters, and `expand`
- [x] `@request.body.<field>:isset` and `@request.body.<field>:changed` write-rule modifiers (with bitmask tracking in `Body` and `rules:explain`)
- [x] `fields` projection pushed into list SQL (system cols + sort keys kept for cursors; `expand` keeps full columns)
- [x] Precompressed admin UI assets: `tools/precompress.mjs` writes `.br` (q11) + `.gz` after `vite build`; server picks by
      `Accept-Encoding`, serves embedded bytes zero-copy (JS 98 KB raw → 29 KB br)
- [x] Per-app Authentication (Firebase-style): `GET|PUT /api/v1/apps/{app}/auth/config` (email/password + 11 OAuth/OIDC providers,
      write-only secrets, stored in system.db), Admin UI Authentication page (provider list, add/edit/remove, redirect URI)
- [x] Admin UI dashboards: landing = server tiles + per-app cards + traffic charts; project overview = tiles, collections, sign-in status, charts
- [x] Realtime v1 (SSE, whole-collection topics, per-audience encode, bounded queues, shutdown close); see `docs/07 §9`. Still to do:
      WebSocket + channels, filtered/record topics, `since` replay, row capture in the writer (deletes), benchmark S7
- [x] Backups v1 (manual snapshot, list, download, delete, restore into a new app; docs/09 'Implemented today'). Still: schedule + retention, zstd, S3, WAL shipping, in-place restore, CLI
- [x] Admin UI: Logs, Access rules, Realtime monitor and Backups pages (sidebar items that stay 'Soon': Storage, API keys, Offline sync, Webhooks, Plugins)
- [ ] Phase 2 auth on top of that config: users (list/create/disable), password login + user tokens, OAuth code exchange per provider
- [ ] Remaining Phase 1 follow-ups: superuser TOTP + token revocation, docs/api/rest.md + rules.md,
      TanStack virtual grid + CodeMirror rule editor, `json` fields as JSONB
- [x] Ported PocketBase rules vectors (`api/conformance/vectors/rules.json`, run by `crates/monolith-core/tests/rules_vectors.rs`);
      fixed `''`≡NULL on text, `%` wildcards in `~`, added date/number macros; `known_gaps`: `:length`/`:each`, `~` with a field on the right
- [ ] Phase 1 exit: M1–M6 gates (need owner OK to run benchmarks)

## Later phases (see `docs/20-roadmap.md`)

- [x] Agent skills pack scaffold (`agent-skills/`) and debug-surface design (`docs/22`)
- [ ] Build `tools/agent-pack` generator + `monolith agent-init` CLI; serve `/llms.txt`, `/agent/*`
- [x] Implement Phase 1 debug endpoints (trace ring buffer, rules:explain, query:explain, collections:diff, dry_run); realtime inspect and sync status in Phase 2–3
- [ ] Fill `api/openapi` + `api/asyncapi` + conformance vectors (contract first)
- [ ] Phase 2: auth, API keys, realtime, rate limiting, TS + Dart SDK v0
- [ ] Phase 3: sync, files, backups, pilot app
- [ ] Phase 3: encryption at rest + keyed backup/restore (`docs/09` B4); first add encrypted S1/S3 runs to the Phase 0 harness
- [ ] Phase 4: OpenAPI + MCP, analytics, webhooks, WASM plugins, full admin UI
- [ ] Phase 5: PocketBase importer, Kotlin/Swift SDKs, passkeys, replicas
