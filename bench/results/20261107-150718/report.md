# Monolith load results 20261107-150718

git `c0011ae` · Intel(R) Core(TM) i5-10200H CPU @ 2.40GHz (8 threads, 31.2 GB) · kernel 7.0.0-42-generic · oha 1.16.0
server pinned cpus 0-3, oha pinned 4-7; warm 5s, measure 20s x 3; seed 1000000 rows; loopback, no Docker limits.

| scenario | conns | units/s (median) | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | server CPU % | RSS MB | success |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| H0 health (floor) | 64 | 118432 | 0.46 | 0.82 | 2.18 | 3.83 | 11.1 | 331 | 165 | 100.00% |
| M1 view by id, public | 64 | 65646 | 0.90 | 1.46 | 2.44 | 3.93 | 10.6 | 338 | 169 | 100.00% |
| M2* view, row rule status='open' | 64 | 63465 | 0.94 | 1.50 | 2.52 | 4.09 | 11.2 | 338 | 170 | 100.00% |
| M3 list keyset limit=50 | 64 | 16300 | 3.79 | 5.54 | 7.63 | 11.30 | 25.2 | 374 | 173 | 100.00% |
| M3b list limit=50 filter n>50000 | 64 | 1562 | 40.78 | 44.99 | 50.03 | 54.66 | 65.8 | 399 | 224 | 100.00% |
| M4 create single (c=64) | 64 | 43401 | 1.22 | 1.89 | 3.57 | 21.35 | 81.3 | 270 | 224 | 100.00% |
| M4b create single (c=256) | 256 | 47985 | 4.94 | 6.86 | 12.45 | 29.91 | 66.2 | 320 | 233 | 100.00% |
| M5 batch 100 creates (c=16) | 16 | 102724 | 14.91 | 16.54 | 45.28 | 48.31 | 82.4 | 140 | 234 | 100.00% |
| M1 open-loop @32823qps | 64 | 32819 | 0.41 | 0.72 | 1.38 | 2.81 | 31.3 | 0 | 1084 | 100.00% |
