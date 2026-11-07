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
- [x] Superuser management: `GET /api/v1/system/superusers`, `DELETE .../superusers/{id}` (409 for self or
      the last one, the count check is inside the DELETE), `GET /api/v1/system/me`. 1 HTTP test
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
- [x] Admin UI navigation rework + settings (`docs/15 §5`): grouped sidebar without duplicate entries, collections tree,
      user menu in the footer, Project settings (General, API keys, Users and permissions), Console settings at `#/settings`
- [x] Storage buckets v1: private/public buckets, streamed upload, Range/ETag download, listing; Admin UI page (`docs/13 §6`)
- [x] API key hardening: last-used, per-key rate limit (GCRA), IP/CIDR allowlist, key update, `NO_CHANGE_LOG` writes
- [ ] API keys follow-ups: `@request.key.*` in rules, publishable keys, rate limit for failed auth, remaining scopes (`users:manage`, `analytics:*`, `schema:*`, `backup:run`, `mcp`), `monolith keys` CLI
- [x] Storage internals: BLAKE3 content addressing + dedupe, reference counts, bucket quota, usage counters, orphan GC,
      MIME sniffing, migration of the first layout (`docs/13 §6`)
- [ ] Key/storage hardening (from the known limitations): per-app quota, conformance vectors + MCP tools + SDK methods + `docs/api/rest.md` for both features
- [x] Thumbnails (declared sizes, on demand, cached, bounded pool) (`docs/13 §6`)
- [ ] Thumbnail follow-ups: WebP/AVIF output by `Accept`, eager generation at upload, cache negative results, include thumbnails in quotas, resize benchmark
- [x] Files in backups (hard-linked blobs + `files.db` snapshot, restored into the new app)
- [x] S3-compatible blob backend per app (`object_store`; config + probe + UI), tested against the in-memory store only
- [ ] S3 follow-ups: run the suite against a real MinIO/S3 (needs the owner's OK and credentials), presigned-URL redirects, migrate blobs between
      backends, encrypt the stored secret, bucket-lifecycle docs, remote-aware backups (versioning / copy)
- [ ] Storage follow-ups: rule-based/per-user access, file fields on records, upload/download benchmark
- [ ] Phase 2 auth on top of that config: users (list/create/disable), password login + user tokens, OAuth code exchange per provider
- [x] Realtime v2: filtered and per-record topics, `snapshot`, `since` replay, `leave` events (`docs/07 §9`)
- [ ] Realtime follow-ups: capture the old/new row in the writer (store crate) so events and deletes carry the true row;
      WebSocket + channels; `fields`/`expand`; per-conn `seen` beyond 50 000;
      relational view rules via the batched SQL check (`docs/07 §5`); benchmark S7; micro-benchmarks for the hub (`CLAUDE.md`: realtime needs them)
- [ ] Backups follow-ups: schedule + retention (`docs/09` B2), zstd archive, S3 target, WAL shipping, in-place restore,
      `monolith backup` CLI, encryption (B4), a single-file archive that includes the storage, streamed download without buffering in the admin UI
- [x] Webhooks v1: record events, HMAC signature, retries, delivery log, test ping (`docs/11 §7`); Admin UI Webhooks page
- [ ] Webhook follow-ups: auth/backup events, per-hook filter, admin UI delivery detail, cron triggers, benchmark
- [x] WASM plugins v1: after-commit `on_event` handlers on wasmtime with fuel and memory caps (`docs/14 §6`); Admin UI Plugins page
- [ ] Plugin follow-ups: before-write hooks, custom routes, cron, `kv`/`http.fetch` host functions, capability manifest, component model, benchmark call overhead
- [ ] Sidebar items still marked "Soon": Offline sync (`docs/09` part A)
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
- No publishable keys. A lost secret cannot be shown again (only SHA-256 is stored); there is no rotate endpoint (create + revoke).
- Key names are not unique and there is no cap on keys per app. Expired keys stay listed (and in memory) until revoked or restart.
- `lastUsed` is saved once a minute, so a crash can lose up to a minute. The IP allowlist behind a reverse proxy only works with
  `MONOLITH_TRUST_PROXY=1` and a proxy that appends the real client address last in `X-Forwarded-For`.
- Single node: the in-memory key map is not shared; multi-node needs invalidation over the change bus.
- Scopes not enforced anywhere yet: `users:manage`, `analytics:*`, `schema:*`, `backup:run`, `mcp` (they are rejected on create).
- Auth failures are not rate limited, so a flood of bad keys costs one SHA-256 each and nothing else.

Storage
- Blobs on local disk (default) or an S3-compatible bucket. Local backups are hard links on the same disk (not a disaster-recovery copy; the
  download endpoint serves the database file only); S3-backed apps back up metadata only. No per-user access.
- Dedupe is per app (never across apps), and only by whole-file hash: no chunk-level dedupe.
- The reference counts can drift if the database is edited by hand or restored from a copy; `gc` fixes orphan files but does not
  recompute counts. Upload/download are not benchmarked.
- Admin UI download buffers the object in the browser; uploads show no progress and run one file at a time.
- Metadata commits are serialized per app by one mutex, so many small concurrent uploads queue on it (bytes are written in parallel,
  only the DB step waits). Each commit also costs an fsync of the blob plus a `files.db` write.
- Sniffing recognises binary formats only; HTML, SVG, JSON and other text keep the declared type, and a bucket that allows `text/*`
  accepts any text. SVG is never shown inline.
- Quotas: `maxSize` per object and `maxBytes` per bucket; there is no per-app total, so many buckets can still fill the disk.
- A crash between placing a blob file and the database commit leaves an orphan file until the next `gc` (first open of the app's
  storage, or `POST /storage:gc`); a crash after the commit but before the file delete leaves one the same way.
- Download: only a single `Range` (multi-range is served whole), no `If-Range`, `Last-Modified` or `If-Modified-Since`.
  `Cache-Control` is `no-cache` (always revalidates); no immutable caching because keys are mutable names.
- Upload answers 201 vs 200 by comparing `created` and `updated`, which could misreport a replace within the same microsecond.
- Object keys containing `%2F` are decoded to `/` by the router before validation; there is no way to store a literal `%2F`.
- Storage traffic is counted (object GET/HEAD as reads, PUT/DELETE as writes, with `Content-Length` as the byte count), but an upload
  sent with chunked encoding has no length up front, so its bytes in are 0 in the charts.
- Not covered by tests: concurrent overwrite of one key, crash recovery, multi-range and `If-Range`, bucket delete during an upload.

S3 backend
- Verified only with `object_store`'s in-memory store. Nothing has run against AWS S3, MinIO, R2 or B2: signing, endpoint styles, multipart
  limits and retry behaviour are the library's and untested here. Do not rely on it for production data before a real-service run.
- The location is locked once files exist (no blob migration). Deleting an app leaves its bucket contents. Backups hold metadata only.
- Every download is proxied through the server (no presigned redirects), so S3 egress and server bandwidth both count.
- `object_store` retries failed requests with backoff, so a wrong endpoint can make the probe on save take a long time before it errors.
- Thumbnails from a remote source download the whole image first (max 64 MiB). The secret key is stored unencrypted in `system.db`.

Contract and SDKs (keys and storage)
- No conformance vectors for scopes or object endpoints, no entries in `api/mcp/tools.json`, no TypeScript/Kotlin SDK methods,
  and no `docs/api/rest.md` section. REST works without an SDK, but the contract-first rule is only met by `api/openapi`.
- The generated app-specific agent skill (`monolith-app-<app>`) does not list buckets or keys yet.

Rules engine
- Known gaps in `api/conformance/vectors/rules.json` (`:length`, `:each`, field-valued `~`).
- Behaviour changed to match PocketBase: text `''` equals NULL for `=`/`!=`; `~` with an unescaped `%` is used as the LIKE pattern as written.

Admin UI
- Sidebar entries marked "Soon" do not link anywhere. The UI is not covered by automated tests; only typecheck and manual checks.
- No keyboard focus trap in drawers; no sidebar collapse. Settings has no change-password and no per-project roles
  (Project admin / Viewer are shown as planned).

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
