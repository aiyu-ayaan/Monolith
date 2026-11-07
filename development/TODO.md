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
- [x] Realtime v1 (SSE, whole-collection topics, per-audience encode, bounded queues, shutdown close); see `docs/07 §9`
- [x] Backups v1 (manual snapshot, list, download, delete, restore into a new app); see `docs/09` "Implemented today"
- [x] Admin UI: Logs, Access rules, Realtime monitor and Backups pages (sidebar items that stay 'Soon': Offline sync, Webhooks, Plugins)
- [x] API keys v1: per-app, hashed, scoped, in-memory verification; Admin UI page (`docs/08 §7`)
- [x] Storage buckets v1: private/public buckets, streamed upload, Range/ETag download, listing; Admin UI page (`docs/13 §6`)
- [ ] API keys follow-ups: `@request.key.*` in rules, IP allowlist, per-key rate limit, publishable keys, last-used,
      remaining scopes (`users:manage`, `analytics:*`, `schema:*`, `backup:run`, `mcp`), `monolith keys` CLI
- [ ] Key/storage hardening (from the known limitations): key id in request logs, `_changes` pruning or a no-log write path for
      `files.db`/`system.db`, orphan-blob GC, per-bucket/app quotas, storage bytes in HTTP metrics, key update endpoint,
      conformance vectors + MCP tools + SDK methods + `docs/api/rest.md` for both features
- [ ] Storage follow-ups: S3 backend + presigned URLs, rule-based/per-user access, file fields on records, blake3
      content addressing + dedupe, MIME sniffing, thumbnails, quotas, orphan GC, files in backups, upload/download benchmark
- [ ] Phase 2 auth on top of that config: users (list/create/disable), password login + user tokens, OAuth code exchange per provider
- [ ] Realtime follow-ups: capture the old/new row in the writer (store crate) so events and deletes carry the true row;
      `since` replay from `_changes`; WebSocket + channels; filtered and per-record topics; `fields`/`expand`; `leave` events;
      relational view rules via the batched SQL check (`docs/07 §5`); benchmark S7; micro-benchmarks for the hub (`CLAUDE.md`: realtime needs them)
- [ ] Backups follow-ups: schedule + retention (`docs/09` B2), zstd archive, S3 target, WAL shipping, in-place restore, files in the archive,
      `monolith backup` CLI, encryption (B4), streamed download without buffering in the admin UI
- [ ] Sidebar items still marked "Soon": Offline sync (`docs/09` part A),
      Webhooks and Plugins (`docs/14`; webhooks need an HTTP client, plugins a WASM runtime)
- [ ] Rules follow-ups: `:length` and `:each` modifiers, `~` with a field on the right side, `@request.query.*`, `@request.key.*`
- [ ] Remaining Phase 1 follow-ups: superuser TOTP + token revocation, docs/api/rest.md + rules.md,
      TanStack virtual grid + CodeMirror rule editor, `json` fields as JSONB
- [x] Ported PocketBase rules vectors (`api/conformance/vectors/rules.json`, run by `crates/monolith-core/tests/rules_vectors.rs`);
      fixed `''`≡NULL on text, `%` wildcards in `~`, added date/number macros; `known_gaps`: `:length`/`:each`, `~` with a field on the right
- [ ] Phase 1 exit: M1–M6 gates (need owner OK to run benchmarks)

## Known limitations of what shipped (read before relying on it)

Authentication (per app)
- Configuration only: there are no end-user accounts, password login, user tokens or OAuth redirects yet, so nobody can sign in.
- Client secrets are stored as plain JSON in `system.db` (encryption at rest is Phase 3). They are never returned by the API.
- The redirect URI shown in the UI uses the browser's current origin; register your public URL in production.

Realtime (SSE)
- Whole-collection topics over SSE only; no WebSocket, channels, filters, `since` replay (clients refetch after reconnect).
- An event holds the row as it is at dispatch time, so quick writes to one row can arrive as one event.
- Non-superusers get `delete` events only when the view rule is public, and cannot subscribe when the view rule is relational.
- A client 256 frames behind is dropped. Not benchmarked: scenario S7 and the 50k-connection target are unmeasured.
- Per-event cost: one `bind` + in-memory rule check per subscriber (not cached per audience yet).

Backups
- Manual only, plain SQLite files, same disk as the data (not a disaster-recovery copy until S3 or an off-volume target exists).
- Restore creates a new app; there is no in-place restore. Backups survive deleting the app. Admin UI download buffers the file in the browser.

API keys
- Only `records:admin` bypasses rules; other keys are anonymous to the rules and narrowed by scope (no `@request.key.*` yet),
  so a `records:read` key sees no more than an anonymous caller does. `records:admin` is app-wide, not per collection.
- No IP allowlist, per-key rate limit, publishable keys or last-used time. A lost secret cannot be shown again (only SHA-256 is stored).
- Request logs and the trace ring buffer do not identify keys: key requests show `superuser: false` and no key id, so you cannot
  tell which key made a call. Needs a key id in `RequestLog` and the debug endpoints (`docs/22`).
- Key names are not unique and there is no cap on keys per app. Expired keys stay listed (and in memory) until revoked or restart.
- Changing a key's scopes or expiry means revoking and creating a new one (no update endpoint).
- Single node: the in-memory key map is not shared; multi-node needs invalidation over the change bus.
- Scopes not enforced anywhere yet: `users:manage`, `analytics:*`, `schema:*`, `backup:run`, `mcp` (they are rejected on create).
- Auth failures are not rate limited, so a flood of bad keys costs one SHA-256 each and nothing else.

Storage
- Local disk only; not in backups (restore creates an app without its files). No per-user access, thumbnails, quotas or S3.
- Not content-addressed: overwriting writes a new blob, so identical uploads are not deduplicated. MIME is as declared by the uploader.
- Bucket usage counts scan the object index (fine for the admin UI, not for huge buckets). Upload/download are not benchmarked.
- Admin UI download buffers the object in the browser; uploads show no progress and run one file at a time.
- Metadata commits are serialized per app by one mutex, so many small concurrent uploads queue on it (bytes are written in parallel,
  only the DB step waits). Each commit also costs an fsync of the blob plus a `files.db` write.
- Every metadata write adds a row to `_changes` in `files.db` (and bucket/key writes in `system.db`); nothing prunes it, so it grows
  with the number of uploads. The store crate needs a "no change log" write path for non-record tables.
- Crash windows leak data: a crash between the blob rename and the DB commit leaves an orphan blob; a crash before the old blob is
  deleted leaves the replaced one. Only `files/.tmp` is cleaned at open. Needs the orphan GC scan.
- Download: only a single `Range` (multi-range is served whole), no `If-Range`, `Last-Modified` or `If-Modified-Since`.
  `Cache-Control` is `no-cache` (always revalidates); no immutable caching because keys are mutable names.
- Upload answers 201 vs 200 by comparing `created` and `updated`, which could misreport a replace within the same microsecond.
- Object keys containing `%2F` are decoded to `/` by the router before validation; there is no way to store a literal `%2F`.
- HTTP metrics count storage requests as `other`; streamed uploads and downloads report 0 bytes in/out because the size is not
  known up front (only `Content-Length` is read), so the traffic charts under-count storage.
- Not covered by tests: concurrent overwrite of one key, crash recovery, multi-range and `If-Range`, bucket delete during an upload.
- Quotas: `maxSize` is per object only; there is no per-bucket or per-app total, so a key with `files:write` can fill the disk.

Contract and SDKs (keys and storage)
- No conformance vectors for scopes or object endpoints, no entries in `api/mcp/tools.json`, no TypeScript/Kotlin SDK methods,
  and no `docs/api/rest.md` section. REST works without an SDK, but the contract-first rule is only met by `api/openapi`.
- The generated app-specific agent skill (`monolith-app-<app>`) does not list buckets or keys yet.

Rules engine
- Known gaps in `api/conformance/vectors/rules.json` (`:length`, `:each`, field-valued `~`).
- Behaviour changed to match PocketBase: text `''` equals NULL for `=`/`!=`; `~` with an unescaped `%` is used as the LIKE pattern as written.

Admin UI
- Sidebar entries marked "Soon" do not link anywhere. The UI is not covered by automated tests; only typecheck and manual checks.
- No keyboard focus trap in drawers; no sidebar collapse; no Settings page.

Testing and performance
- None of the new code is benchmarked. Phase 1 gates M1–M6 and the Phase 0 bake-off are still pending the owner's OK to run on the host.
- `pnpm test` fails on a machine without installed JS dependencies (`vitest: not found` in `sdks/typescript`); run `pnpm install` first.

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
