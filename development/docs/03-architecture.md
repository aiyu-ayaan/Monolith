# 03 — Architecture

## 1. One process, one container

```
                         ┌──────────────────────── monolith (single Rust binary) ───────────────────────┐
 clients ──HTTP/1.1,h2──▶│ axum router ─ tower layers (trace, limit, cors, compression, auth-extract)     │
 SDKs / curl / AI agents │   ├─ /api/v1/apps/{app}/...   data plane handlers                               │
 MCP clients            │   ├─ /api/v1/system/...       control plane handlers                            │
                         │   ├─ /api/v1/apps/{app}/realtime   SSE / WebSocket                              │
                         │   ├─ /mcp/{app}               MCP endpoint                                      │
                         │   └─ /_/                      admin UI (embedded, precompressed)                │
                         │                                                                                 │
                         │  Control plane (system.db)    Data plane: one AppRuntime per app (lazy-loaded)  │
                         │  apps, superusers, keys,      ┌─────────────── AppRuntime "myapp" ─────────┐ │
                         │  global settings, audit       │ schema cache (Arc, versioned)                 │ │
                         │                               │ compiled rules + prepared SQL                 │ │
                         │                               │ Writer thread ─▶ data.db  (group commit)      │ │
                         │                               │ Reader pool  ─▶ data.db  (N read conns)       │ │
                         │                               │ change bus (broadcast of committed changes)   │ │
                         │                               │ realtime hub (subscription index)             │ │
                         │                               │ analytics ingester ─▶ analytics store         │ │
                         │                               │ log sink ─▶ logs.db                            │ │
                         │                               └───────────────────────────────────────────────┘ │
                         │  Shared services: file store (local/S3), thumbnailer pool, mailer, cron,         │
                         │  backup scheduler, metrics, WASM plugin host                                     │
                         └─────────────────────────────────────────────────────────────────────────────────┘
                                                   │
                                     /data volume (one mount)
```

The final DB engine is decided by the bake-off in [04](04-database-engine-evaluation.md).
This doc describes the **lead design (embedded SQLite)**. The storage boundary in §6
keeps the Postgres fallback possible without rewriting the HTTP, rules, realtime or
sync layers.

## 2. Control plane vs data plane

| | Control plane | Data plane |
|---|---|---|
| Storage | `/data/system.db` | `/data/apps/<app_id>/data.db`, `logs.db`, `analytics/`, `files/` |
| Content | apps, superusers, global settings, API key index, backup catalog, audit log | collections, records, auth users, sessions, change log, app settings |
| Write volume | Tiny | All user traffic |
| Lifecycle | Always open | `AppRuntime` opened on first request, closed after idle TTL (configurable, default never for "pinned" apps) |

Why a file per app:
- **Write scaling.** SQLite allows one writer per database file. Ten apps = ten
  independent writers, running in parallel on different cores.
- **Isolation.** A runaway migration or a huge import in one app cannot block another.
- **Backup/restore granularity.** Snapshot or restore one app without touching others.
- **Deletion.** Dropping an app = drop its directory.

## 3. Thread model

| Component | Runs on | Why |
|-----------|---------|-----|
| HTTP, realtime connections, JSON encode/decode | Tokio multi-thread runtime (worker per core) | Async I/O, tens of thousands of idle connections cheaply |
| SQLite **writer** (one per app DB) | Dedicated OS thread, owns one `rusqlite::Connection` | SQLite calls block; a dedicated thread avoids stalling Tokio workers and keeps the connection's statement cache hot |
| SQLite **readers** | Fixed pool of OS threads (default `min(cores*2, 16)` per hot app), each owning a connection | Blocking reads off the async runtime; per-thread prepared-statement caches |
| Analytics ingest | Dedicated thread per app (or shared, configurable) | Bulk appends |
| Thumbnails, password hashing (Argon2id) | Bounded `rayon`/blocking pool with semaphore | CPU-heavy; must not starve request handling |
| Cron, backups | Tokio tasks that hand work to the writer/blocking pools | |

Communication: requests send a `WriteOp` over a bounded MPSC channel to the writer and
await a `oneshot` reply. Readers are reached the same way (a request → reader
channel), or via `spawn_blocking` with a checked-out connection; the bake-off picks
whichever measures faster.

## 4. Write path (group commit)

```
handler ──validate input (no DB)──▶ build WriteOp{ app, collection, mutation, auth ctx, reply_tx }
        ──▶ writer queue (bounded, backpressure → 503 + Retry-After when full)

writer thread loop:
    op = queue.recv_blocking()
    batch = [op] + queue.try_recv() up to MAX_BATCH (default 256) or MAX_BATCH_WAIT (default 0–1 ms)
    BEGIN IMMEDIATE
      for op in batch:
          SAVEPOINT s
          run rule check (compiled), uniqueness/FK checks, INSERT/UPDATE/DELETE (prepared, cached)
          write change-log row (seq, collection, record id, op, hlc, changed columns)
          ok  → RELEASE s, stash result
          err → ROLLBACK TO s, stash error           (one bad op never poisons the batch)
    COMMIT
    for each op: reply_tx.send(result)
    publish committed changes to change bus (seq order)
```

Properties:
- One commit per batch instead of per row. Under load, batch size grows
  automatically; at low load latency is one transaction.
- `seq` is assigned inside the transaction so the change log is gap-free in commit
  order (needed by sync, [09](09-sync-and-backup.md)).
- After-commit hooks and realtime fan-out run **after** `COMMIT` on Tokio, never inside
  the writer loop.
- User-defined "before" hooks that must run inside the transaction are limited to
  Rust/WASM code with a strict time budget ([14](14-extensibility.md)).
- Explicit client transactions (batch API) become a single `WriteOp` holding many
  mutations, executed atomically inside one savepoint.

## 5. Read path

```
handler ──parse query (filter, sort, cursor, fields, expand) — cached by query shape
        ──resolve compiled rule for (collection, op) from schema cache
        ──build SQL from pre-compiled fragments, bind params
        ──reader pool: prepare_cached → step rows → serialize directly to JSON bytes
        ──(optional) expand: one batched query per relation per level
        ──response (compressed when large)
```

- Row → JSON is streamed straight from SQLite column values into the output buffer
  (no intermediate `serde_json::Value` maps).
- Reads in WAL mode never block on the writer.
- Read-your-writes: the reply to a write includes `seq`; a subsequent read with
  `?minSeq=` waits (bounded) until a reader snapshot includes that `seq`. With a single
  node and WAL, a read that starts after the write's commit already sees it, so this is
  mostly relevant for future replicas.

## 6. Storage boundary (engine-agnostic seam)

```rust
trait AppStore: Send + Sync {
    async fn write(&self, op: WriteOp) -> Result<WriteResult>;         // goes through writer
    async fn query(&self, q: CompiledQuery) -> Result<RowStream>;       // reader pool
    async fn changes_since(&self, seq: u64, limit: u32) -> Result<ChangePage>;
    async fn apply_schema(&self, plan: MigrationPlan) -> Result<()>;
    async fn snapshot(&self, dest: &Path) -> Result<SnapshotInfo>;
}
```

`CompiledQuery` carries SQL text in a small dialect enum (SQLite / Postgres) produced by
the rules/filter compiler. Everything above this trait (HTTP, auth, realtime, sync,
OpenAPI) is engine-independent.

## 7. Change bus

Every committed mutation produces a `ChangeEvent { app, seq, collection, id, op,
old_row?, new_row, hlc, actor }`. Consumers:

| Consumer | Uses |
|----------|------|
| Realtime hub | Fan-out to subscribers ([07](07-realtime.md)) |
| Sync | Long-poll / stream of `changes_since(seq)` ([09](09-sync-and-backup.md)) |
| After-commit hooks / webhooks | User code, outbound webhooks with retry |
| Analytics (optional) | Auto-events like `record.created` counts |
| Cache invalidation | Auth/session caches, schema cache |

Implementation: `tokio::sync::broadcast` per app with a sized ring; slow consumers that
lag fall back to reading the persisted change log by `seq`, so no consumer can stall
the writer.

## 8. Crate layout (planned workspace)

```
crates/
  monolith-server     # binary: config, CLI, wiring, HTTP server
  monolith-core       # AppRuntime, schema, records, change bus, settings
  monolith-store      # AppStore trait + sqlite (and optional postgres) engines, writer, reader pool
  monolith-rules      # filter/rule parser, type checker, SQL + predicate compilers
  monolith-realtime   # subscription index, SSE/WS transports
  monolith-auth       # users, sessions, tokens, OAuth2/OIDC, OTP, MFA, API keys
  monolith-sync       # change log, sync protocol, conflict resolution
  monolith-analytics  # ingest, rollups, query
  monolith-files      # storage backends, thumbnails
  monolith-openapi    # dynamic spec generation, MCP tool mapping
  monolith-plugins    # hooks registry, WASM host (wasmtime)
  monolith-embed      # public crate for "use Monolith as a library"
ui/                   # SolidJS admin UI (built to ui/dist, embedded)
sdks/                 # ts, dart, kotlin, swift
bench/                # load generators, scenarios, result storage
```

## 9. Configuration

Order of precedence: CLI flags > env vars (`MONOLITH_*`) > `/data/monolith.toml` >
defaults. Runtime-changeable settings (SMTP, OAuth providers, rate limits, API keys,
quotas) live in DB and are editable from the admin UI and the API without restart.

## 10. Failure behaviour

| Failure | Behaviour |
|---------|-----------|
| Writer queue full | 503 with `Retry-After`; metric `writer_queue_full_total` |
| Reader pool saturated | Wait up to `read_timeout` (default 2 s), then 503 |
| Slow realtime client | Queue overflow → connection closed with `resync` hint |
| App DB corrupted / missing | That app returns 503; others unaffected; alert in admin UI |
| Panic in handler | Caught by tower layer → 500, logged with request id |
| Panic in writer thread | Writer restarted; in-flight batch rolled back and replied with errors |
