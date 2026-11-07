# 14 — Extensibility: Hooks, Plugins, Cron

## 1. Options compared

| Option | Speed | Safety | Hot reload | Decision |
|--------|-------|--------|-----------|----------|
| Rust hooks (embedded crate) | Native | Trusted (same process) | No (rebuild) | **Yes** — embedded mode |
| WASM plugins (`wasmtime`, component model) | Near-native after AOT compile; ~µs call overhead | Sandboxed: memory/time/fuel limits, capability-based host API | Yes | **Yes** — binary/Docker mode |
| JavaScript (goja-style VM, `rquickjs`/`boa`) | Interpreter; GC | Sandboxed | Yes | **No in v1** (PocketBase's JSVM cost, B18). Revisit with QuickJS if users demand JS |
| Outbound webhooks | Network latency | Isolated | Yes | **Yes** — for after-commit integration |

WASM plugins can be written in Rust, TinyGo, AssemblyScript, C, or JS via
`javy`/ComponentizeJS (compiled ahead of time, not interpreted per request).

## 2. Hook points

| Phase | Hooks | Runs where | Budget |
|-------|-------|-----------|--------|
| Request | `on_request(route)`, custom routes | Tokio | request timeout |
| Before write | `before_create/update/delete(collection)` — mutate or reject | **inside writer transaction** | strict: default 5 ms CPU, no network |
| After commit | `after_create/update/delete(collection)` | Tokio task from change bus | async, may do network |
| Auth | `on_sign_up`, `on_sign_in`, `on_oauth_link`, custom claims | Tokio | request timeout |
| Realtime | `on_subscribe`, `on_channel_publish` | Tokio | 1 ms |
| Lifecycle | `on_app_open`, `on_schema_change`, `on_backup` | Tokio | — |
| Mail | `on_mail_send` (templating, provider override) | Tokio | — |

Before-write hooks run inside the writer, so a slow hook would slow every write of
that app. Enforcement: WASM fuel/epoch interruption at the budget; exceeding it aborts
that mutation with an error and increments a metric. Rust hooks in embedded mode are
trusted but measured and logged when over budget.

## 3. WASM host API (capabilities granted per plugin)

`record.get/set`, `db.query` (read-only, through rules or as service role if granted),
`http.fetch` (after-commit hooks only, allowlisted hosts), `kv` (per-plugin small store),
`log`, `mail.send`, `crypto` helpers, `env` (plugin config). Plugins declare required
capabilities in a manifest; superuser approves on install.

## 4. Cron

- Per-app cron jobs: run a WASM function, call a webhook, or run a built-in action
  (backup, purge timeseries older than X, rebuild rollups).
- Scheduler: single Tokio task with a timer wheel; jobs never overlap with themselves
  (skip or queue policy); run history kept in `logs.db`.
- Manual trigger via API/UI.

## 5. Custom routes

- Embedded mode: any `axum` router merged under `/api/custom/...` with access to
  `RequestCtx` (auth, app).
- WASM mode: plugin exports `handle(request) -> response`; mounted under
  `/api/v1/apps/{app}/fn/{name}` with rate limits and optional scope requirement.

## 6. Implemented today (`crates/monolith-core/src/plugins.rs`)

- Runtime: `wasmtime` (Cranelift), core modules only (the component model and WIT come later).
- ABI v1: the module exports `memory`, `alloc(len: i32) -> i32`, `on_event(ptr: i32, len: i32) -> i32` and may import only
  `monolith.log(level, ptr, len)` (0 debug, 1 info, 2 warn, 3 error). Any other import is refused at upload.
- Hook point: after commit, from the same dispatcher as webhooks (doc 11 §7). The host copies the webhook envelope
  (`{id, app, event, collection, seq, createdAt, record}`, no hidden fields) into guest memory and calls `on_event`; non-zero = error.
- Sandbox: fresh instance per call, fuel per call (default 50 M, set 1 000 to 5 G per plugin), 16 MiB memory cap, runs on the
  blocking pool behind a semaphore (half the cores). No network, files or DB access: no `http.fetch`, `kv`, `db.query` host functions yet.
- Delivery: at most once, in change order per app; an error or fuel trap is counted and kept as `lastError`, not retried.
  The dispatcher waits for each call, so a slow plugin delays that app's webhooks (bounded by the fuel budget).
- State: modules are stored in system.db (`plugins`, up to 8 MiB each, 20 per app) and compiled at start. Calls, errors and the last
  100 log lines live in memory and reset on restart.
- API (superuser): `POST /api/v1/apps/{app}/plugins?name=&events=&fuel=&enabled=` with the raw `.wasm` as body, `GET`, `PUT|DELETE .../{id}`,
  `GET .../{id}/logs`. Not yet: before-write hooks, custom routes (`/fn/{name}`), cron, capability manifest and approval, signed plugins.
- Not benchmarked: call overhead and compile time are unmeasured.
