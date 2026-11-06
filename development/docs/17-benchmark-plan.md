# 17 — Benchmark Plan

Benchmarks decide engine choices ([04](04-database-engine-evaluation.md)) and guard
budgets ([16](16-performance-engineering.md)). No numbers in these docs are measured
yet; a PocketBase baseline run was deliberately skipped during the analysis phase and
is scheduled as step 1 below.

## 1. Rules

- Same hardware, same OS tuning, same dataset for every compared system.
- Load generator on a **separate machine or pinned to separate cores** (`taskset`) so
  it does not steal CPU from the server.
- Open-loop load for latency (constant arrival rate, e.g. `oha -q` / `wrk2`) to avoid
  coordinated omission; closed-loop only for max-throughput runs.
- Warm-up 30 s, measure 120 s, 3 repetitions, report median of runs plus p50/p90/p99/
  p99.9 and max, CPU %, RSS, disk write MB/s.
- All runs scripted in `bench/` and results committed as JSON + markdown summary with
  git SHA, hardware, kernel, config.
- Every system runs inside Docker with the same CPU/memory limits (`--cpus`,
  `--memory`), matching our single-container deployment.

## 2. Step 1 — PocketBase baseline (to validate [02](02-pocketbase-bottlenecks.md))

PocketBase v0.40.6 official binary in a container, same dataset:

| ID | Scenario | Confirms |
|----|----------|----------|
| P1 | View by id, public rule, 512 conns | baseline read path |
| P2 | View by id, auth token, rule `owner = @request.auth.id` | B6 cost (user load per request) |
| P3 | List page 1 vs page 500, default (with total) vs `skipTotal=1` | B8, B9 |
| P4 | Filter on multi-relation (`tags.name ?= 'x'`) at 100k / 1M rows | B10 |
| P5 | Single-record creates, 256 concurrent | B7 |
| P6 | Realtime: 1k / 5k / 10k SSE subscribers with RowLocal rule, 100 writes/s | B1–B4: delivery latency, CPU, DB query rate |
| P7 | Idle SSE connections memory at 10k / 50k | B4 |

## 3. Step 2 — Engine bake-off (Phase 0)

Scenarios S1–S8 in [04 §6](04-database-engine-evaluation.md) for SQLite / Postgres /
Turso, plus analytics A1–A3:

| ID | Scenario | Engines |
|----|----------|---------|
| A1 | Ingest 50M events at max rate | DuckDB appender vs SQLite batched |
| A2 | DAU per day over 90 days, group by platform | DuckDB raw vs SQLite rollup |
| A3 | Funnel of 3 events over 30 days | DuckDB vs SQLite |

Micro-benchmarks (`criterion`/`divan`) inside the engine harness: ID encode/decode,
JSON row writer vs `serde_json`, rule predicate eval, filter compile, allocator A/B.

## 4. Step 3 — Monolith vs budgets (every phase)

| ID | Scenario | Gate (from 16 §1) |
|----|----------|------------------|
| M1 | View by id, public | ≥ 60k req/s, p99 < 2 ms |
| M2 | View by id, RowLocal rule | ≥ 50k req/s, p99 < 3 ms |
| M3 | List keyset | ≥ 15k req/s, p99 < 5 ms |
| M4 | Create single | ≥ 20k/s per app, p99 < 10 ms |
| M5 | Batch 100 | ≥ 100k rows/s per app |
| M6 | Multi-app writes 20 apps | aggregate scales ≥ 0.7× linear up to core count |
| M7 | Realtime fan-out 10k subs | p99 delivery < 50 ms |
| M8 | Idle connections 50k | ≤ 8 KB each |
| M9 | Sync pull/push mixed, 1k devices | p99 < 30 ms |
| M10 | Mixed mobile-app workload (§6) | p99 < 25 ms at target rate |
| M11 | Soak 24 h at 60% of max | no RSS growth > 5%, no p99 drift > 10% |
| M12 | Backup during M10 | p99 impact < 20% |

Also: Monolith vs PocketBase on P1–P7 with identical data — published as a comparison
table once measured.

## 5. CI performance regression gate

- Nightly on a dedicated runner (fixed hardware): M1, M3, M4, M7 short versions (30 s).
- Fail if throughput drops > 5% or p99 rises > 10% vs the rolling median of the last 7
  nightly runs.
- Micro-benchmarks on every PR touching `monolith-store`, `monolith-rules`,
  `monolith-realtime` with `critcmp` diff in the PR comment.

## 6. Mobile-app-shaped mixed workload (M10)

| Share | Operation |
|------:|-----------|
| 35% | View task / user by id (RowLocal rules) |
| 20% | List my tasks (keyset, filter by user + status) |
| 15% | Insert location point into `timeseries` collection |
| 10% | Update task status |
| 10% | Sync pull (cursor) |
| 5% | Sync push (2–10 mutations) |
| 5% | Analytics batch (20 events) |
| background | 5k realtime connections subscribed to their active task |

Dataset: 100k users, 2M tasks, 50M location points, 20 collections.

## 7. Tooling

- `oha` (installed on the dev box during analysis via `cargo install oha`) for HTTP.
- Custom Rust load harness in `bench/harness` for realtime (SSE/WS), sync and mixed
  workloads (needs session state that HTTP-only tools cannot model).
- `perf`, `cargo flamegraph`, `heaptrack`/`bytehound` for profiling; `tokio-console`.
