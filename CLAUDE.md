# CLAUDE.md — Monolith

Monolith is a self-hosted, single-container backend suite in Rust: database with
access rules, auth, realtime, offline sync, files, analytics, and backups. One server
serves many apps. Inspired by PocketBase and Firebase.

**Current phase: Phase 1 (core data plane) nearly done; first Phase 2 slices landed (per-app auth config, realtime v1 over SSE, backups v1, API keys v1, storage buckets v1). Phase 0 bake-off runs and the M1–M6 gates are pending (benchmarks need the owner's OK).**
The design docs in `development/docs/` are the source of truth. Read
`development/docs/README.md` first, then `development/TODO.md` for where work stands.

## Prime directive: performance first

Every change is judged first by latency, throughput, and memory, then by features.
- Do not add a feature that regresses a benchmark gate in
  `development/docs/17-benchmark-plan.md` without an explicit decision recorded in the docs.
- Follow the banned hot-path patterns in `development/docs/16-performance-engineering.md §4`.
- Claims about speed need a benchmark or must be labelled as a target or hypothesis.
  Never present unmeasured numbers as results.

## Architecture in one paragraph

One Rust binary (Tokio + axum). Control plane in `/data/system.db`. Each app gets its
own data directory and DB file (`/data/apps/<app>/data.db`). Each app DB has one
dedicated writer thread with group commit and a reader thread pool. Every commit writes
a gap-free `seq` to the `_changes` log and publishes it on an in-process change bus. The
bus feeds realtime fan-out, sync, webhooks, and analytics. Rules compile once per schema
version into parameterized SQL and in-memory predicates. See
`development/docs/03-architecture.md`.

## Key decisions (do not silently change)

| Decision | Doc |
|----------|-----|
| Primary DB engine is **pending the Phase 0 bake-off**. The lead is SQLite with one file per app; the fallback is Postgres co-located in the same container. Keep all code above the `AppStore` trait engine-agnostic. | 04 |
| Analytics in DuckDB (Cargo feature `analytics-duckdb`), with SQLite rollups as the fallback | 04, 10 |
| IDs: UUIDv7 as 16-byte BLOB, 26-char lowercase Crockford base32 on the wire | 05 |
| Multi-relations use junction tables, not JSON arrays. Dates are INTEGER µs UTC. Tables are STRICT | 05 |
| Keyset pagination by default; no default COUNT | 11 |
| Access token verification needs no DB lookup; refresh tokens and API keys are stored as hashes | 08 |
| Realtime does not run per-subscriber SQL for RowLocal rules and serializes once per audience | 07 |
| Admin UI: SolidJS + Vite + TypeScript, precompressed and embedded | 15 |
| Deployment: one Docker image, one `/data` volume | 18 |
| Hooks: Rust (embedded) and WASM plugins; no JS VM in v1 | 14 |

Change a decision only by updating its doc (and ADR status) in the same change.

## Repository layout

Full tree and packaging rules: `development/docs/21-repository-structure.md`.

```
package.json, pnpm-workspace.yaml  JS/TS workspace root (ui, sdks/typescript, web examples)
api/           language-agnostic contract: openapi, asyncapi, jsonschema, mcp, conformance vectors
docs/          user-facing API docs (rest, websocket, kotlin, typescript, auth, rules) and guides
agent-skills/  SKILL.md pack for AI agents (overview, rest, realtime, sync, rules, auth-keys, sdks, debug); installed via `monolith agent-init`
sdks/          typescript (npm), kotlin (core KMP + android AAR), dart, swift, python, rust
crates/        Rust server workspace (monolith-{server,core,store,rules,realtime,auth,sync,analytics,files,openapi,plugins,embed})
ui/            SolidJS admin UI -> ui/dist (embedded)
examples/      curl, android-kotlin, web-typescript, python-script
tools/         codegen, release scripts, conformance runner
bench/         load harness, scenarios, results
docker/        Dockerfile, compose
development/   design docs (source of truth) and TODO.md
```

Debuggability is a feature (`development/docs/22`): every API change that adds an error or rule path needs request-id tracing, explain support, and the matching `agent-skills/` update.

Contract-first: change `api/` and conformance vectors before the server or any SDK.
REST and WebSocket must stay complete without an SDK.

## Conventions (for the build phase)

- Rust stable, edition 2024, `rust-toolchain.toml` pinned. `cargo fmt`, and
  `cargo clippy --all-targets -- -D warnings` must pass.
- Never block Tokio workers. SQLite, Argon2, and image work run on dedicated threads
  or pools.
- Use bounded channels everywhere. Backpressure becomes `429`/`503`, not memory growth.
- No `serde_json::Value` for record data in hot paths. Build no SQL with `format!`
  per request.
- Errors: `thiserror` in libraries, `anyhow` only in binaries and tests. API errors use
  RFC 9457 problem+json.
- Every rule-engine change runs the differential test suite (SQL compiler vs
  predicate compiler).
- Performance-sensitive crates (`store`, `rules`, `realtime`) need micro-benchmarks for
  new hot-path code.
- Tests: unit tests next to code, integration tests in `crates/*/tests`, SDK
  conformance tests against a real server container.
- JS/TS monorepo managed with `pnpm` workspaces (`ui`, `sdks/typescript`, `examples/web-typescript`).
  Root `pnpm install` sets up all JS/TS projects.
- Run every project task through root `pnpm` scripts, Rust included (`pnpm rust:*`
  wrap cargo). Add a script to `package.json` instead of documenting a raw command.
- Commit at each completed stage of work (one logical change per commit). Use
  Conventional Commits.
- Never add AI co-author trailers (no `Co-Authored-By:` lines) to commits or PRs.

## Commands

```bash
pnpm install                          # set up all JS/TS projects (`pnpm setup` is a pnpm built-in, not our script)

# Everyday
pnpm dev                              # backend + admin UI together (tools/dev.mjs); Ctrl-C stops both
pnpm dev:server                       # backend only (env: MONOLITH_DATA, MONOLITH_ADDR);
                                      # first start prints a one-time superuser setup link
pnpm dev:ui                           # admin UI only (Vite :5173, proxies /api to :8090)
pnpm build                            # JS/TS packages + Rust debug build
pnpm build:release                    # JS/TS packages + Rust release build (lto=fat)
pnpm start                            # build the UI, run the release server
pnpm test                             # JS/TS tests + cargo test
pnpm lint                             # Rust fmt check + clippy -D warnings + UI typecheck
pnpm fmt                              # cargo fmt

# Granular
pnpm build:js | build:ui | build:sdk  # JS/TS builds (all | admin UI | SDK)
pnpm test:js | test:sdk               # JS/TS tests (all | SDK)
pnpm typecheck:ui                     # tsc --noEmit for the admin UI
pnpm rust:build | rust:release        # cargo build (debug | release)
pnpm rust:test                        # cargo test --workspace
pnpm rust:lint | rust:fmt             # fmt check + clippy | format
pnpm rust:bench:build                 # compile benches only
pnpm rust:bench                       # run benches (ask the owner before running on the host)
pnpm bench:engine                     # engine bake-off harness (ask the owner before running on the host)
pnpm rust:deny                        # cargo-deny (needs `cargo install cargo-deny`)
pnpm clean                            # remove dist and node_modules in all JS/TS packages
```

## Reference material

- The PocketBase source analysis used commit `7f2773ec` (v0.40.6). Findings are in
  `development/docs/01-pocketbase-teardown.md` and `02-pocketbase-bottlenecks.md`.
  Reuse its good ideas: the collections and rules model, the delete dry-cache, the
  transactional batch, and the separate logs DB.
- Open questions that need owner input: `development/docs/20-roadmap.md`.
