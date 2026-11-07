# Monolith load test, 2026-11-07 (after tuning)

Second measured run, after the performance pass. First run and its log: [`../20261107-150718/SUMMARY.md`](../20261107-150718/SUMMARY.md).

- 4-core run: this folder (`results.json`, `run.log`, `oha/*.json`, `footprint.json`).
- 1-core run: [`../20261107-223730/`](../20261107-223730/).
- Chart: [`../throughput.svg`](../throughput.svg) (`pnpm bench:chart`).

## Setup

Same as the first run: release build, Intel i5-10200H (4 cores / 8 threads, 31 GB), Linux 7.0.0-42, `oha` 1.16.0, loopback, no Docker limits, 1,000,000 seeded rows, 3 s warm-up + 3 × 15 s, median. 4-core run: server on CPUs 0-3, `oha` on 4-7. 1-core run: server on CPU 0, `oha` on 2-7. The machine also ran a desktop session and Chrome (load average 8 with no benchmark running), so single runs vary by up to ±20%.

## Results (median of 3)

| scenario | 4 cores units/s | p50 / p99 ms | 1 core units/s | p50 / p99 ms | gate (doc 17 §4) |
|---|--:|--:|--:|--:|---|
| H0 health (no DB) | 125,791 | 0.42 / 2.15 | 54,453 | 1.22 / 1.68 | n/a |
| M1 view by id, public | 71,286 | 0.82 / 2.38 | 35,772 | 1.75 / 2.87 | 60k/s met on 4 cores; p99 < 2 ms missed at saturation |
| M1 open loop at 50% of max | 35,638 | 0.42 / 1.31 | 17,883 | 0.58 / 1.38 | p99 < 2 ms met |
| M2* view, row rule | 71,216 | 0.83 / 2.32 | 36,205 | 1.73 / 2.76 | met on 4 cores |
| M3 list 50 rows | 21,641 | 2.87 / 5.32 | 10,179 | 6.16 / 9.72 | 15k/s met on 4 cores; p99 < 5 ms just missed |
| M3b list 50, filter `n>50000` | 1,640 | 38.2 / 54.7 | 448 | 140 / 196 | no gate (see below) |
| M4 create single, 64 conns | 46,765 | 1.18 / 3.40 | 23,965 | 2.55 / 4.17 | met on both |
| M4b create single, 256 conns | 51,568 | 4.67 / 12.65 | 23,608 | 10.6 / 23.9 | throughput met |
| M5 batch of 100 (rows/s) | 62,634 | 13.8 / 878 | 87,370 | 17.3 / 50.3 | 100k rows/s missed (4-core run hit the write stall) |

Footprint (`bench/scenarios/footprint.py`, 1 core): 10.8 MB RSS at idle, 55.4 MB with 20 apps open (budget 150 MB), 0.0% idle CPU, cold start 30 ms on restart, 2.9 ms to open an app on first request.

## What changed since the first run

- **List pages +33%** (4 cores: 16.3k to 21.6k/s; 1 core: +26% in a back-to-back A/B). Dates were formatted with `format!` into a new `String` for each value, and every record has at least two (`created`, `updated`). `codec::write_us` now writes the fixed 27-byte form straight into the output buffer. A test checks it against the old formatter on 2,000+ values, including years 0 and 9999 and the fallback outside that range.
- **View by id**: 65.6k to 71.3k/s on 4 cores. This is within the run-to-run noise on this machine, so it is not claimed as a gain.
- **Allocator**: `mimalloc` was measured and not adopted. See doc 16 §2: +20% on 4 cores, but idle RSS went up by 3-4×.
- **Reader threads**: 4 vs 8 per app made no measurable difference, so the default (`2 × cores`, max 16) stays.
- **Middleware** (request log + metrics): turning it off changed throughput by less than the noise (≤5%) on 1 core, so it stays.

## Findings from the first run, resolved

- **"RSS grows to 2 GB"**: not a leak. `RssAnon` (heap) stays at 45-80 MB. The growth is `RssFile`: SQLite's `mmap_size` (1 GB per connection) maps the database file, and the kernel can drop those pages under memory pressure. On a small host this is page cache, not committed memory.
- **"Filtered list is 10× slower"**: an artifact of the test data. The seed writes `n = 0..999,999` in id order, and the list sorts by id, so `n > 50000` must skip the first 50,001 rows before it finds one match. A filter that matches early rows does not pay this. This was reasoned from the data, not profiled: confirm with `query:explain`. A real app with that access pattern needs an index on `n` and `sort=n`.

## Still open

- **Write stall, now explained** (follow-up the same day): the 0.8-2 s pauses are the SSD. The test data lived on an Intel SSDPEKNU512GZ, a QLC drive. During every stall, `/proc/diskstats` showed write latency of 150-1,050 ms at only 20-35 MB/s, with IO pressure "full" at about 50%, while the server averaged about 65 MB/s of writes. The writer thread sat in `folio_wait_bit_common` / `jbd2_log_wait_commit` (kernel disk waits) with 44-54 writes queued. A side fix landed: WAL checkpoints moved from the writer to a shared background thread (doc 04 §5). On a healthy disk that cut max single-create latency from 57-76 ms to 35-45 ms and batch p99 from 44 ms to 21 ms. When the drive itself stalls, writes still pause; the bounded writer queue absorbs that and then answers 503. A drive with steady write speed (TLC/MLC, or a VPS volume with guaranteed IOPS) avoids it.
- **p99 at saturation**: M1 and M3 miss their p99 gates only in closed-loop runs that hold 64 connections on 4 cores. At 50% load, M1 p99 is 1.31 ms.
- Not covered: M6-M12 (multi-app scaling, realtime, idle connections, sync, mixed, soak, backup under load), M2 with an authenticated `@request.auth.id` rule, a PocketBase baseline, Docker CPU/memory limits.
