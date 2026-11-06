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
- [x] Root `CLAUDE.md`
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
- [ ] SQLite prototype follow-ups: writer restart on panic, bounded reader wait (`read_timeout`), idle WAL checkpoint task, `LIBSQLITE3_FLAGS` compile flags
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

Order of work: rules → contract → store tweaks → auth → core → server → admin UI.

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
- [ ] `crates/monolith-core` (WIP, files written, **not yet in `lib.rs`, not compiled**):
  - [x] `src/error.rs`: `CoreError` (maps to problem types), `FieldError`
  - [x] `src/schema.rs`: `CollectionDef`/`FieldDef`/`Rules`/`IndexDef` (serde), `validate_def`,
        `Schema::build` (compiles rules + SQL templates per version), `SchemaView` for `Schema`.
        Slots: 0 id, 1 created, 2 updated, 3 _v, 4.. user fields. Templates: view/insert/update/delete,
        each with a rule variant and a superuser variant (`"1"`). Insert = `INSERT … SELECT * FROM
        (SELECT ? AS …) AS t WHERE (rule) RETURNING …`; update uses `iif(?, ?, col)` per field.
  - [x] `src/codec.rs`: JSON writer (`write_record`, `format_us`), schema-driven body decoder
        (`decode_body`) with per-field validation
  - [x] `src/ddl.rs`: `plan(old, new)` → CREATE TABLE (STRICT, WITHOUT ROWID) / ADD/DROP COLUMN /
        index diff; type changes rejected (needs online rebuild); `sql_str` quoting
  - [x] `src/list.rs`: `parse_sort`, keyset `plan` (NULL-aware), cursor encode/decode (hex, FNV shape hash)
  - [ ] Next: write `src/lib.rs` (`mod` declarations + re-exports), fix compile errors, run unit tests
  - [ ] `src/app.rs`: `App { store: SqliteStore, schema: RwLock<Arc<Schema>>, schema_lock: tokio Mutex,
        list plan cache (bounded, clear on overflow) }`; open = create `_collections(name, cid, def)` and
        `_meta(schema_version)`, load schema; `put_collection` (validate → `Schema::build` → `ddl::plan` +
        upsert def + bump version via `apply_schema` → swap Arc); `delete_collection` (reject if referenced);
        records: create/view/update/delete/list/batch with `Principal { superuser, auth: Option<rules::Auth> }`
        (rule `None` → 403 for non-superusers; create rule rejection → 403; update/delete → 404);
        validate client ids (UUIDv7 version bits, timestamp in [now-30d, now+5min])
  - [ ] `src/system.rs`: `Registry` over `/data/system.db` (tables `apps`, `superusers`, `settings` with
        `token_key`), lazy app open via `spawn_blocking`, create/list/delete app (`/data/apps/<name>/data.db`),
        superuser create/login (Argon2 on blocking pool)
  - [ ] Integration tests in `crates/monolith-core/tests` (CRUD, rules, list paging with NULL sort keys,
        migrations, batch atomicity); check `api/conformance/vectors/*.json`
- [ ] `crates/monolith-server` (stub `main.rs` only): axum 0.8 routes per `api/openapi/monolith.v1.yaml`,
      problem+json errors, `X-Request-Id` middleware, bearer superuser token, env config
      (`MONOLITH_DATA`, `MONOLITH_ADDR`, first superuser from `MONOLITH_ADMIN_EMAIL`/`MONOLITH_ADMIN_PASSWORD`),
      `pnpm` script `server:dev`; HTTP integration tests
- [ ] Admin UI v0 (SolidJS): login, apps, collections editor, records grid
- [ ] Phase 1 follow-ups: multi-relation junction tables + `?=` operators, `@collection` joins,
      back-relations (`_via_`), `:isset`/`:changed`, expand, `fields` projection in SQL (now done at
      encode), `total=exact|estimate`, superuser TOTP, `rules:explain`/`query:explain`, `dry_run`,
      request tracing, agent-skills updates, docs/api/rest.md + rules.md
- [ ] Phase 1 exit: M1–M6 gates, ported PocketBase rules vectors

## Later phases (see `docs/20-roadmap.md`)

- [x] Agent skills pack scaffold (`agent-skills/`) and debug-surface design (`docs/22`)
- [ ] Build `tools/agent-pack` generator + `monolith agent-init` CLI; serve `/llms.txt`, `/agent/*`
- [ ] Implement debug endpoints (trace, rules:explain, query:explain, realtime inspect, sync status, dry_run) in Phase 1–2
- [ ] Fill `api/openapi` + `api/asyncapi` + conformance vectors (contract first)
- [ ] Phase 2: auth, API keys, realtime, rate limiting, TS + Dart SDK v0
- [ ] Phase 3: sync, files, backups, pilot app
- [ ] Phase 4: OpenAPI + MCP, analytics, webhooks, WASM plugins, full admin UI
- [ ] Phase 5: PocketBase importer, Kotlin/Swift SDKs, passkeys, replicas
