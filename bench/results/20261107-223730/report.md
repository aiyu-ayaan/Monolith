# Monolith load results 20261107-223730

git `fe5c831` · Intel(R) Core(TM) i5-10200H CPU @ 2.40GHz (8 threads, 31.2 GB) · kernel 7.0.0-42-generic · oha 1.16.0
server pinned cpus 0, oha pinned 2-7; warm 3s, measure 15s x 3; seed 1000000 rows; loopback, no Docker limits.

| scenario | conns | units/s (median) | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | server CPU % | RSS MB | success |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| H0 health (floor) | 64 | 54453 | 1.22 | 1.42 | 1.68 | 2.29 | 7.6 | 98 | 163 | 100.00% |
| M1 view by id, public | 64 | 35772 | 1.75 | 2.12 | 2.87 | 4.29 | 10.1 | 99 | 164 | 100.00% |
| M2* view, row rule status='open' | 64 | 36205 | 1.73 | 2.11 | 2.76 | 3.59 | 8.9 | 99 | 164 | 100.00% |
| M3 list keyset limit=50 | 64 | 10179 | 6.16 | 7.08 | 9.72 | 12.38 | 20.3 | 100 | 166 | 100.00% |
| M3b list limit=50 filter n>50000 | 64 | 448 | 140.08 | 153.26 | 195.85 | 237.93 | 252.4 | 100 | 178 | 100.00% |
| M4 create single (c=64) | 64 | 23965 | 2.55 | 3.51 | 4.17 | 22.95 | 54.7 | 98 | 178 | 100.00% |
| M4b create single (c=256) | 256 | 23608 | 10.58 | 15.56 | 23.94 | 34.78 | 46.5 | 98 | 184 | 100.00% |
| M5 batch 100 creates (c=16) | 16 | 87370 | 17.33 | 21.15 | 50.27 | 81.55 | 86.4 | 98 | 185 | 100.00% |
| M1 open-loop @17886qps | 64 | 17883 | 0.58 | 0.84 | 1.38 | 3.16 | 14.1 | 0 | 886 | 100.00% |
