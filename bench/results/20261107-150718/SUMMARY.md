# Monolith load test log, 2026-11-07

First measured HTTP load run of the release server. Raw data: `results.json`, per-run `oha/*.json`, timeline `run.log`, server output `server.log`, scenario table `report.md`. Driver: `bench/scenarios/load.py` (`pnpm bench:load`).

## Setup

- Build: `cargo build --release --workspace` at git `c0011ae` (lto=fat profile of the repo).
- Hardware: Intel i5-10200H (4 cores / 8 threads), 31 GB RAM, Linux 7.0.0-42-generic. A desktop session and other apps were running (about 13 GB RAM in use before the run), so absolute numbers carry noise.
- Server pinned to CPUs 0-3, `oha` 1.16.0 pinned to CPUs 4-7 (these are SMT siblings of the same four cores, so the two sides still share core resources).
- Loopback, no Docker, no `--cpus`/`--memory` limits. Not the deployment shape in doc 17 §1.
- Dataset: 1,000,000 rows in `items` (fields: title, status, n), seeded through `POST /batch` with 500 ops per call at 72.7k rows/s. Two more collections: `gated` (row rule `status = 'open'`) and `sink` (write target, ended with 11.9M rows).
- Method: 5 s warm-up, 20 s measured, 3 repetitions per scenario, median reported. Closed loop (fixed connections) for max throughput; one open-loop run (constant arrival rate) for latency.
- All requests are anonymous against public rules, except the batch run, which sent a superuser bearer token.
- Engine: whatever `AppStore` default the repo ships (SQLite lead). Durability/fsync settings were not inspected or changed.

## Results (median of 3)

| scenario | conns | units/s | p50 ms | p99 ms | p99.9 ms | server CPU % | gate (doc 17 §4) |
|---|--:|--:|--:|--:|--:|--:|---|
| H0 health (floor, no DB) | 64 | 118,432 | 0.46 | 2.18 | 3.83 | 331 | n/a |
| M1 view by id, public | 64 | 65,646 | 0.90 | 2.44 | 3.93 | 338 | 60k/s met; p99 < 2 ms missed at saturation |
| M1 open loop at 32.8k req/s (50% of max) | 64 | 32,819 | 0.41 | 1.38 | 2.81 | n/a | p99 < 2 ms met (one of 3 reps hit 2.59) |
| M2* view with row rule `status = 'open'` | 64 | 63,465 | 0.94 | 2.52 | 4.09 | 338 | 50k/s and p99 < 3 ms met |
| M3 list limit=50, no filter | 64 | 16,300 | 3.79 | 7.63 | 11.30 | 374 | 15k/s met; p99 < 5 ms missed at saturation |
| M3b list limit=50, filter `n>50000` (no index) | 64 | 1,562 | 40.78 | 50.03 | 54.66 | 399 | no gate |
| M4 create single | 64 | 43,401 | 1.22 | 3.57 | 21.35 | 270 | 20k/s and p99 < 10 ms met |
| M4b create single | 256 | 47,985 | 4.94 | 12.45 | 29.91 | 320 | 20k/s met; p99 above 10 ms at 256 conns |
| M5 batch of 100 creates | 16 | 102,724 rows/s | 14.91 (per batch) | 45.28 | 48.31 | 140 | 100k rows/s met by 2.7% |

Units are requests/s, except M5, which counts rows/s (1,027 batch requests/s). Zero non-2xx responses in every counted run.

## Reading the numbers

- Read path: view by id reaches 65.6k req/s, which is 55% of the no-DB health floor (118k). Row rule evaluation costs about 3%.
- All gates for throughput pass on this box. The p99 gates for M1 and M3 are missed only in saturated closed-loop runs, where 64 connections queue on 4 pinned cores; the gate wording does not say which load model applies. M1 at half load meets its gate.
- M3 list scales with page size (50 rows per response, so about 815k rows/s serialized).
- M3b is the one real problem found: a filter matching 95% of rows with no index costs 40 ms per page and pins all four cores at 399% CPU, 10x slower than the unfiltered list. It needs an explain check (`query:explain`) to see whether the plan scans or sorts.
- Single creates: 43-48k/s, about 2x the 20k/s gate. Batch writes are the single-writer-thread ceiling at about 100k rows/s with one core busy (140% CPU).
- Writes with 64 connections, repetition 1 only: 13,301/s with a 1,106 ms max latency, then 45k/s and 60 ms max in repetitions 2 and 3. A one-second stall right after the seed and read phases; the cause is not diagnosed (a checkpoint or flush is a guess). The median hides it.
- Memory: RSS 54 MB after the first 20 s run (health), 165 MB after the second, 224 MB after the filtered lists, 234 MB after batches, and 1,084 MB sampled during the final open-loop run. It ended at 1,998 MB with the 1.56 GB data directory. The jump after 11.9M `sink` rows is not explained here and needs a heap/cache check before any soak test (M11).

## Not covered

- M6 multi-app scaling, M7 realtime fan-out, M8 idle connections, M9 sync, M10 mixed workload, M11 24 h soak, M12 backup under load. No harness for these exists yet.
- M2 with a real RowLocal rule on an authenticated user (`owner = @request.auth.id`); this run uses an anonymous row rule instead (hence the asterisk).
- PocketBase baseline (P1-P7), Docker CPU/memory limits, separate load-generator machine, 30 s/120 s phases from the plan (these runs used 5 s/20 s).
- Latency tails past p99.9 and disk write MB/s.

Treat the table as a first measurement on a shared dev box, not a published benchmark result.
