---
name: monolith-plugins
description: Run your own sandboxed WebAssembly code on Monolith record changes: ABI, install, fuel and memory limits, logs, debugging. Use when a webhook is too heavy or you need server-side logic without running another service.
---

# WASM plugins

Superuser API under `/api/v1/apps/{app}/plugins`. Admin UI: Plugins page. Runs after commit, in change order, at most once.

ABI v1 (core module): export `memory`, `alloc(len: i32) -> i32`, `on_event(ptr: i32, len: i32) -> i32`.
Optional import: `monolith.log(level: i32, ptr: i32, len: i32)` (0 debug, 1 info, 2 warn, 3 error). No other imports: no network, files or DB.
`on_event` receives the JSON envelope `{id, app, event, collection, seq, createdAt, record}` (same as webhooks, no hidden fields) and returns 0 for success.

```bash
curl -X POST "$BASE/api/v1/apps/$APP/plugins?name=audit&events=orders.*&fuel=100000000" \
  -H "Authorization: Bearer $SU_TOKEN" -H 'content-type: application/wasm' --data-binary @plugin.wasm
curl "$BASE/api/v1/apps/$APP/plugins/$ID/logs" -H "Authorization: Bearer $SU_TOKEN"
```

Rules of thumb:
- Build with any toolchain that emits a core module (Rust `wasm32-unknown-unknown`, TinyGo, AssemblyScript); export `alloc` and keep the buffer alive for the call.
- A fresh instance runs per event: keep no state in globals. Each call has a fuel budget (default 50 M instructions) and 16 MiB of memory.
- Errors (non-zero return, trap, out of fuel) are counted in `errors`/`lastError` and are not retried. Use a webhook when you need retries.
- A slow plugin delays the app's other after-commit work: keep it short, or raise `fuel` only as far as needed.
- Counters and logs are in memory and reset on restart. Missing calls: check `enabled`, the event pattern (`collection.op`), and `lastError`.
