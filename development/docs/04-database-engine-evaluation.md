# 04 — Database Engine Evaluation (ADR-001)

Status: **Proposed. Final decision after the Phase 0 bake-off (§6).**
Constraint from owner: everything ships in **one Docker container**; the engine does
not have to be SQLite if another option is faster for this workload.

## 1. Workload we are choosing for

| Property | Expectation |
|----------|-------------|
| Tenancy | Many apps (10–100) on one node; each app has 1–50 collections |
| Read/write mix | Read-heavy overall (≈ 80/20), but bursts of small writes (location pings, sync pushes) |
| Row size | Small (100 B – 4 KB); files are not stored in the DB |
| Query shape | Point lookups by id, filtered lists with sort + limit, relation lookups, rule checks, change-feed scans by `seq` |
| Concurrency | Thousands of concurrent HTTP clients; tens of thousands of realtime connections (mostly idle) |
| Data size | MBs to tens of GBs per app; not TB-scale |
| Ops | One container, one volume, backups per app, no DBA |

The hot path is **many small transactions with simple predicates**. Engine
per-query overhead (IPC, protocol encode/decode, planning) dominates more than raw
scan speed.

## 2. Candidates

### A. SQLite (native C, embedded via `rusqlite` `bundled`), file per app — *lead*

- **How it fits:** in-process, no IPC; a point read is a function call. WAL gives
  unlimited concurrent readers alongside one writer per file.
- **Write concurrency:** one writer **per file**. We multiply writers with
  file-per-app (and optionally file-per-hot-collection), and amortize commits with
  group commit ([03 §4](03-architecture.md)).
- **Strengths:** lowest read latency of any option; zero ops; trivial per-app
  backup (`VACUUM INTO` / online backup API / WAL shipping with Litestream-style
  replication); `STRICT` tables, partial and expression indexes, `R*Tree` (geo),
  `FTS5` (search), JSON functions.
- **Risks:** a single very hot app with heavy write contention on one file is capped
  by one writer thread; long-running write transactions (migrations) block that app's
  writes; `ALTER TABLE` is limited (rebuild-table migrations needed).
- **Mitigations:** group commit; keep transactions short; move migrations to a
  shadow-table rebuild with chunked copy; split hot collections to their own file
  (each file gets its own writer; cross-file transactions are not supported, so this
  is opt-in per collection).
- **Upstream `BEGIN CONCURRENT` branch / `wal2`:** optimistic multi-writer at page
  level exists only on a non-mainline SQLite branch. Building against it is possible
  with the `bundled` source swapped, but adds maintenance burden. Kept as a later
  option, not a v1 dependency.

### B. PostgreSQL, co-located in the same container — *fallback*

- **How it fits:** run `postgres` as a second process in the container under a tiny
  supervisor (e.g. `s6-overlay` or our binary spawning and supervising it); connect
  over a Unix socket with `tokio-postgres` + pipelining.
- **Strengths:** true multi-writer MVCC (row-level locks), mature planner, rich types,
  `LISTEN/NOTIFY`, logical replication, schemas-per-app for tenancy.
- **Costs:** per-query IPC + protocol overhead on every read (Unix socket round trip
  plus parse/bind/execute) vs a function call; idle memory per process and per
  connection; vacuum tuning; container now runs two daemons (signals, health,
  upgrades of the data directory between major versions, `pg_dump`/`pg_basebackup`
  for backups); larger image.
- **When it wins:** a single app with sustained high-contention writes on the same
  tables (many writers updating overlapping rows) where SQLite's one writer per file
  saturates.

### C. Turso Database (Rust rewrite of SQLite, formerly "Limbo") / libSQL

- **Turso Database:** SQLite-compatible engine written in Rust with async I/O
  (io_uring) and an MVCC mode aimed at concurrent writers.
- **libSQL:** C fork of SQLite with extensions (embedded replicas, extra ALTER
  support).
- **Strengths:** potential concurrent writes without leaving the embedded model;
  pure Rust build (Turso).
- **Risk:** maturity. Status, feature parity (triggers, FTS5, R*Tree, `STRICT`, JSON)
  and durability guarantees of the MVCC mode **must be re-verified at decision time**;
  as of this writing we treat it as **not production-proven for our needs**.
- **Plan:** include in the bake-off as an experimental engine. If its MVCC mode is
  stable and wins on write contention, it can replace SQLite behind the same
  `AppStore` seam with minimal SQL dialect changes.

### D. Embedded KV / LSM (`redb`, `fjall`, RocksDB) + our own query layer

- **Strengths:** very high raw write throughput (LSM); full control over layout;
  pure-Rust options.
- **Costs:** we would have to build secondary indexes, a filter/sort executor,
  transactions across indexes, migrations, backup tooling and an admin SQL console.
  That is a database project, not a backend project.
- **Verdict:** **rejected** for the primary store. Could be used later for specific
  sub-systems (e.g. rate-limit counters, ephemeral presence) if profiling shows a need.

### E. DuckDB (embedded, columnar) — *analytics only*

- **Strengths:** vectorized columnar execution; aggregations over millions of events
  are orders of magnitude faster than row stores; compresses event data well;
  embedded (`duckdb` crate), so fits the one-container rule.
- **Costs:** large C++ dependency (build time, binary size); OLTP-style single-row
  writes are slow → must ingest via batched appender; one writer process per DB file.
- **Verdict:** **chosen for analytics** behind a Cargo feature `analytics-duckdb`
  (default on in the Docker image). Fallback feature uses SQLite with pre-aggregated
  rollup tables ([10](10-analytics.md)).

### F. Others considered

| Engine | Reason not chosen |
|--------|-------------------|
| SurrealDB / other multi-model DBs | Higher per-query overhead than SQLite for simple CRUD; less proven; we already own the API layer |
| MySQL / MariaDB | Same IPC cost as Postgres without Postgres' feature advantages for us |
| ClickHouse (for analytics) | Excellent at scale but heavy for a single container (memory, separate server); DuckDB covers our scale |
| MongoDB | Separate server, document model not needed; rules/relations easier in SQL |

## 3. Scoring (pre-bake-off, 1–5, weights reflect "performance first")

| Criterion | Weight | SQLite (A) | Postgres (B) | Turso MVCC (C) | KV+own layer (D) |
|-----------|-------:|-----------:|-------------:|---------------:|-----------------:|
| Read latency (p50/p99) | 25 | 5 | 3 | 5* | 5 |
| Write throughput, many apps | 15 | 5 | 4 | 5* | 5 |
| Write throughput, one hot app, contended | 15 | 3 | 5 | 4* | 4 |
| Memory per connection / idle footprint | 10 | 5 | 2 | 5* | 5 |
| Single-container ops simplicity | 10 | 5 | 2 | 5 | 4 |
| Per-app backup / restore | 10 | 5 | 3 | 4* | 2 |
| Maturity / correctness risk | 10 | 5 | 5 | 2 | 2 |
| Build effort for us | 5 | 5 | 4 | 4 | 1 |
| **Weighted total** | 100 | **4.70** | 3.50 | 4.40* | 3.95 |

`*` = unverified, depends on bake-off results.

## 4. Decision (provisional)

1. **Primary store: SQLite (native, `rusqlite` bundled), one file per app**, dedicated
   writer thread with group commit, reader thread pool, WAL.
2. **Analytics store: DuckDB** (feature-gated), SQLite rollups as fallback.
3. **Keep the `AppStore` seam** ([03 §6](03-architecture.md)) so Postgres or Turso can be
   added as an alternative engine per app if the bake-off or production data shows a
   hot app saturating its single writer.
4. Decision becomes **final** only when the bake-off gates below pass.

## 5. SQLite configuration (starting point, tuned in bake-off)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;          -- durability: survives app crash; last txns may roll back on power loss.
                                      -- per-app setting "durability=full" switches to FULL.
PRAGMA busy_timeout = 5000;           -- only matters for external tools; our writer is the sole writer
PRAGMA foreign_keys = ON;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -16000;           -- 16 MB per connection (fewer connections than PocketBase)
PRAGMA mmap_size = 1073741824;        -- 1 GB address space; OS page cache shared across connections
PRAGMA wal_autocheckpoint = 4000;     -- pages; checkpoint also driven manually by an idle-time task
PRAGMA journal_size_limit = 134217728;
PRAGMA page_size = 4096;              -- set before first table; bake-off also tests 8192/16384
PRAGMA trusted_schema = OFF;
PRAGMA cell_size_check = OFF;
PRAGMA optimize = 0x10002;            -- on open; plain `PRAGMA optimize` periodically and on close
```

Compile-time flags for the bundled SQLite (via `LIBSQLITE3_FLAGS`):
`SQLITE_DQS=0`, `SQLITE_DEFAULT_MEMSTATUS=0`, `SQLITE_LIKE_DOESNT_MATCH_BLOBS`,
`SQLITE_MAX_EXPR_DEPTH=0`, `SQLITE_OMIT_DEPRECATED`, `SQLITE_OMIT_SHARED_CACHE`,
`SQLITE_USE_ALLOCA`, `SQLITE_ENABLE_FTS5`, `SQLITE_ENABLE_RTREE`,
`SQLITE_ENABLE_STAT4`, `SQLITE_THREADSAFE=2` (each connection used by one thread at a
time). Each flag is verified by the bake-off, not assumed.

Checkpointing: one background thread shared by all stores runs
`PRAGMA wal_checkpoint(PASSIVE)` on a short-lived connection. Each writer asks for it at
most every 100 ms while busy and once when idle. The writer keeps
`wal_autocheckpoint = 4000` as a backstop, because a PASSIVE pass alone never lets the WAL
restart under nonstop writes. Measured 2026-11-07: when the writer did all the
checkpointing, every queued write waited behind its fsyncs. With the background pass, on a
healthy disk, max single-create latency fell from 57-76 ms to 35-45 ms, batch p99 halved
(44 ms to 21 ms), peak write bursts fell by about a third, and the WAL stayed near 17 MB.
`TRUNCATE` during low traffic is not built yet.

## 6. Phase 0 bake-off (decides this ADR)

Run on the target hardware class (e.g. 4 vCPU / 8 GB VPS and the 8-core dev box).
Same Rust harness, same schema, engines behind `AppStore`.

| Scenario | Description | Metric |
|----------|-------------|--------|
| S1 point read | `GET` record by id, 1M rows, 512 concurrent | req/s, p50/p99 |
| S2 filtered list | 2 predicates + sort + limit 50, rule with auth join | req/s, p99 |
| S3 single-app writes | 1 app, 256 concurrent inserters, 1 KB rows | inserts/s, p99 commit latency |
| S4 contended updates | 1 app, 256 clients updating 1k hot rows | updates/s, p99, abort rate |
| S5 multi-app writes | 20 apps × 32 inserters | aggregate inserts/s |
| S6 mixed 80/20 | realistic mobile-app mix incl. change-feed reads | req/s at p99 < 25 ms |
| S7 memory | 10k idle realtime conns + 20 open apps | RSS |
| S8 backup | snapshot of a 5 GB app while S6 runs | duration, p99 impact |

Engines: SQLite (A) with group commit on/off; Postgres (B) 16+ co-located over Unix
socket, pipelined; Turso (C) MVCC mode if available. DuckDB vs SQLite rollups run
separately in the analytics bake-off ([10](10-analytics.md)).

**Gates to finalize SQLite:**
- S1/S2: SQLite ≥ Postgres throughput and ≤ Postgres p99.
- S3: ≥ 50k single-row inserts/s with group commit on the 8-core box (target, to be
  confirmed realistic in the first run).
- S4: SQLite p99 ≤ 2× Postgres p99. If worse, enable per-app engine choice (SQLite
  default, Postgres opt-in for "write-contended" apps).
- S7: RSS under 1 GB.

All numbers above are **targets**, not measured results.
