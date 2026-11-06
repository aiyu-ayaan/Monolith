# 16 — Performance Engineering

Performance is the first requirement. This doc sets budgets, the techniques used to
meet them, the crate choices, and the anti-patterns banned in code review.

## 1. Budgets (targets — validated by [17](17-benchmark-plan.md), not yet measured)

Reference box: 4 vCPU / 8 GB RAM VPS, NVMe. Also tracked on the 8-core dev machine.

| Operation | p50 | p99 | Throughput target |
|-----------|-----|-----|-------------------|
| `GET /health` | < 0.1 ms | < 0.5 ms | — |
| Record view by id (public rule) | < 0.3 ms | < 2 ms | ≥ 60k req/s |
| Record view by id (RowLocal rule, user token) | < 0.4 ms | < 3 ms | ≥ 50k req/s |
| List 50 rows, 2 predicates, keyset | < 1 ms | < 5 ms | ≥ 15k req/s |
| Create record (single, group commit) | < 2 ms | < 10 ms | ≥ 20k writes/s per app |
| Batch insert 100 rows | < 5 ms | < 20 ms | ≥ 100k rows/s per app |
| Realtime: 1 write → 10k matching subscribers (RowLocal) | — | delivery < 50 ms | — |
| Analytics ingest | — | 202 in < 1 ms | ≥ 200k events/s |
| Sync pull 1000 changes | < 10 ms | < 30 ms | — |
| Idle realtime connection memory | — | — | ≤ 8 KB each |
| Server RSS with 20 apps open, no load | — | — | ≤ 150 MB |
| Cold start to serving | — | — | < 200 ms (excluding migrations) |

Every number is a **target to be tested**. The first benchmark run may show some are
unrealistic; budgets are then revised in this doc with the measured data.

## 2. Techniques by layer

### HTTP
- `axum` on `hyper` 1.x, HTTP/1.1 keep-alive + HTTP/2 (h2c behind proxy, TLS via
  `rustls` when terminating directly).
- `mimalloc` as global allocator (fewer allocation stalls under multi-thread load;
  verify against glibc and jemalloc in bench).
- Avoid per-request `String` allocs: route params as `&str`, `smallvec`/`compact_str`
  for small collections/strings, `bytes::Bytes` for bodies.
- Body limits enforced while streaming.
- Precompressed static assets; dynamic compression only above 1 KB.

### JSON
- Input: `serde` derive into typed structs where the shape is fixed; for records, a
  schema-driven decoder that validates while parsing (`sonic-rs` or `simd-json` —
  benchmark both against `serde_json`).
- Output: direct writer from SQLite values to bytes (see [11 §4](11-api-and-openapi.md)).
  Field-name prefixes (`"title":`) precomputed per collection schema version.

### Database (SQLite)
- Dedicated writer thread + group commit ([03 §4](03-architecture.md)).
- Reader thread pool, each with a warmed `prepare_cached` statement cache
  (capacity ≥ 256 statements).
- `mmap_size` for shared page cache; modest `cache_size` per connection.
- Integer µs timestamps, STRICT typed columns, UUIDv7 BLOB keys, junction tables.
- Keyset pagination; no default `COUNT`.
- Rules compiled to stable parameterized SQL; `EXISTS` semi-joins; no `DISTINCT`.
- `PRAGMA optimize` periodically; slow query log (> 50 ms) with plan.
- Checkpoints in idle windows.
- Interrupt long reads via progress handler at `read_timeout`.

### Caching
- Schema, compiled rules, compiled filters: `Arc` swapped atomically per schema version.
- Auth: in-memory API key cache, auth record cache (TTL + change-bus invalidation),
  revocation set.
- No generic HTTP response cache in v1 (correctness with rules is hard); `ETag`/`304`
  for single records and files.

### Realtime
- Subscription index by collection/record; in-memory predicates; serialize once per
  audience; bounded per-connection queues; coalesced writes ([07](07-realtime.md)).

### Concurrency hygiene
- Never block a Tokio worker: SQLite, Argon2, image work go to dedicated threads/pools.
- Never hold a lock across `.await`.
- Bounded channels everywhere; backpressure turns into `429/503`, not memory growth.
- `parking_lot` locks; sharded maps for hot shared state.

### Build
- `release` profile: `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`.
  Keep `panic = "unwind"` for the server binary, because writer-thread restart and
  per-request panic catching depend on unwinding. `panic = "abort"` is allowed only
  for CLI tools.
- `target-cpu`: build `x86-64-v3` image variant plus a baseline `x86-64` image; ARM64
  (`neoverse`) image for ARM VPS.
- PGO (profile-guided optimization) with `cargo-pgo` using the bench workload, in CI
  for release builds; BOLT evaluated later.

## 3. Crate shortlist (verify latest stable versions at implementation)

| Need | Crate | Alternative to bench |
|------|-------|---------------------|
| Async runtime | `tokio` | — |
| HTTP | `axum`, `hyper`, `tower`, `tower-http` | — |
| SQLite | `rusqlite` (`bundled`, `blob`, `functions`, `hooks`) | `sqlite` crate; Turso crate (bake-off) |
| Postgres (fallback engine) | `tokio-postgres` + `deadpool-postgres` | `sqlx` |
| Analytics | `duckdb` | SQLite rollups |
| JSON | `serde`, `sonic-rs` | `simd-json`, `serde_json` |
| MessagePack | `rmp-serde` | — |
| Allocator | `mimalloc` | `tikv-jemallocator` |
| Cache | `moka` | `quick_cache` |
| Concurrent map | `papaya` | `dashmap`, `scc` |
| Tokens | `jsonwebtoken` or `jwt-simple` (EdDSA) | `pasetors` |
| Password hash | `argon2` | — |
| OAuth2/OIDC | `oauth2`, `openidconnect` | — |
| Rate limit | `governor` (GCRA) | custom |
| Files/S3 | `object_store` | `aws-sdk-s3` |
| Images | `image` (decode) + `fast_image_resize` | `libvips` bindings (bigger, C dep) |
| Hashing | `blake3` | — |
| Compression | `zstd`, `brotli`, `flate2` (zlib-rs backend) | — |
| WASM | `wasmtime` | — |
| OpenAPI | `utoipa` | `aide` |
| Mail | `lettre` | — |
| Cron | `croner` (parsing) + own scheduler | `tokio-cron-scheduler` |
| Embedding UI | `rust-embed` | `include_dir` |
| Metrics | `metrics` + `metrics-exporter-prometheus` | — |
| Tracing | `tracing`, `tracing-subscriber` (non-blocking writer) | — |
| CLI | `clap` | — |
| Config | `figment` or `config` | — |
| Bench (micro) | `criterion` / `divan` | — |
| Load gen | `oha` (HTTP), custom Rust harness for realtime/sync | `k6`, `wrk2` |

## 4. Banned in hot paths (code review checklist)

- `serde_json::Value` as an intermediate for record data.
- `format!` to build SQL per request (use compiled templates + binds).
- `Mutex` held across `.await`; `std::sync::Mutex` on contended shared state.
- Unbounded channels or `Vec` buffers fed by client input.
- `tokio::spawn` per realtime message or per log line.
- Per-request regex compilation; `Regex` must be `static`/`LazyLock`.
- Synchronous filesystem or SQLite calls on a Tokio worker thread.
- `clone()` of large records where `Arc`/borrow works.
- `OFFSET` pagination and default `COUNT(*)` in public endpoints.

## 5. Observability for performance

- Prometheus metrics: request latency histograms per route class and app, writer
  queue depth, batch size histogram, commit latency, reader wait time, statement cache
  hit rate, realtime connections/subscriptions/queue overflows, fan-out duration,
  analytics buffer depth, allocator stats.
- `tracing` spans with sampling; `tokio-console` support behind a feature flag.
- Continuous profiling endpoint (`pprof`-compatible via `pprof-rs`) for superusers.
- Performance regression gate in CI (see [17 §5](17-benchmark-plan.md)).
