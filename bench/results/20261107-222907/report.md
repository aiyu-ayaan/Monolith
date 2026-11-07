# Monolith load results 20261107-222907

git `fe5c831` · Intel(R) Core(TM) i5-10200H CPU @ 2.40GHz (8 threads, 31.2 GB) · kernel 7.0.0-42-generic · oha 1.16.0
server pinned cpus 0-3, oha pinned 4-7; warm 3s, measure 15s x 3; seed 1000000 rows; loopback, no Docker limits.

| scenario | conns | units/s (median) | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | server CPU % | RSS MB | success |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| H0 health (floor) | 64 | 125791 | 0.42 | 0.79 | 2.15 | 4.12 | 15.9 | 317 | 166 | 100.00% |
| M1 view by id, public | 64 | 71286 | 0.82 | 1.36 | 2.38 | 3.89 | 10.3 | 334 | 170 | 100.00% |
| M2* view, row rule status='open' | 64 | 71216 | 0.83 | 1.36 | 2.32 | 3.70 | 10.3 | 336 | 170 | 100.00% |
| M3 list keyset limit=50 | 64 | 21641 | 2.87 | 3.88 | 5.32 | 7.42 | 18.2 | 382 | 174 | 100.00% |
| M3b list limit=50 filter n>50000 | 64 | 1640 | 38.23 | 43.91 | 54.67 | 64.02 | 71.8 | 398 | 223 | 100.00% |
| M4 create single (c=64) | 64 | 46765 | 1.18 | 1.80 | 3.40 | 21.31 | 56.9 | 267 | 223 | 100.00% |
| M4b create single (c=256) | 256 | 51568 | 4.67 | 6.12 | 12.65 | 29.10 | 57.5 | 313 | 233 | 100.00% |
| M5 batch 100 creates (c=16) | 16 | 62634 | 13.84 | 15.47 | 878.43 | 1121.99 | 1122.7 | 82 | 234 | 100.00% |
| M1 open-loop @35643qps | 64 | 35638 | 0.42 | 0.72 | 1.31 | 2.54 | 27.9 | 0 | 1084 | 100.00% |
