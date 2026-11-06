# 02 — PocketBase Bottlenecks and Monolith Fixes

Each finding below comes from source reading ([01](01-pocketbase-teardown.md)).
Impact statements are **analysis, not measurements**. The benchmark plan in
[17](17-benchmark-plan.md) includes a PocketBase baseline run to confirm or refute
each one before we claim a speed-up.

Severity: **H** = scales with users/subscribers, **M** = constant-factor cost,
**L** = edge case.

| # | Sev | PocketBase behaviour | Why it costs | Monolith fix | Doc |
|---|-----|----------------------|--------------|--------------|-----|
| B1 | H | Realtime: **one SQL query per subscriber per change** to evaluate the rule (`CanAccessRecord`), a second one if the subscription has a `filter` | A write with N subscribers triggers up to 2N queries on the read pool. 10k subscribers → 10–20k queries per write | Compile rules to in-memory predicates evaluated against the changed row; SQL fallback only for rules that need joins, and then one batched query per distinct (rule, auth-class) group | 06, 07 |
| B2 | H | Realtime: iterate **all clients** and scan each client's subscription map by string prefix | O(clients × subs) work per write, even for clients subscribed to other collections | Subscription index: `collection → record-id map + wildcard list`, O(matching subscribers) | 07 |
| B3 | H | Realtime: `json.Marshal` **per subscriber** | Same payload serialized N times | Group subscribers by *audience key* (rule outcome + `fields` + `expand` + visibility). Serialize once per group, share `Bytes` | 07 |
| B4 | M | Realtime: goroutine per message + unbuffered channel | Allocation and scheduler pressure; slow client creates goroutine pile-up | Bounded per-connection queue (ring buffer); on overflow drop connection with `resync` event; no task per message | 07 |
| B5 | M | Realtime: SSE forced reconnect every 30 min, idle drop at 5 min | Reconnect storms re-subscribe everything | Configurable; WebSocket alternative; resumable streams with `Last-Event-ID` = change `seq` | 07, 09 |
| B6 | H | Auth: **DB lookup on every authenticated request** to fetch the user and its `tokenKey` | +1 query per request, contends with real work | Stateless access token (≤15 min) verified with key material in memory; revocation via in-memory denylist / session-version cache | 08 |
| B7 | H | Writes: **one transaction per record** through a single connection with lock-retry sleeps | Commit overhead per row; retry sleeps 50–1000 ms add tail latency | Dedicated writer thread per DB file with **group commit**: drain queue, run N mutations in one transaction, reply to each. Short `busy_timeout`, no sleep-retry loop on the hot path | 03, 05 |
| B8 | H | Lists: **OFFSET pagination** | Page k costs O(k·perPage) row skips | Keyset (cursor) pagination by default; OFFSET kept for admin UI only | 11 |
| B9 | H | Lists: **COUNT(DISTINCT id) by default** on every list request | Full scan of the filtered set per list call | Totals opt-in (`total=exact|estimate|none`, default `none`); estimate from `sqlite_stat1` | 11 |
| B10 | H | Multi-relations stored as **JSON arrays**; rules/filters use `json_each` | No index can serve `json_each` joins; back-relation lookups scan the whole table | Junction table per multi-relation field `(owner_id, target_id, pos)` with both-direction indexes; single relations as indexed integer FK columns | 05 |
| B11 | M | `SELECT DISTINCT` + LEFT JOIN chains for relation rules, correlated `NOT EXISTS` for multi-match | Large intermediate results, repeated join chains | Compile relation hops to `EXISTS (SELECT 1 FROM junction ...)` semi-joins; no DISTINCT needed | 06 |
| B12 | M | Rule/filter → SQL rebuilt **per request** from cached AST | CPU per request; SQL text differs per request so no statement cache reuse | Compile to parameterized SQL once per (schema version, rule); use `prepare_cached` statements; bind only values | 06, 16 |
| B13 | M | IDs: 15-char random `TEXT PRIMARY KEY` on rowid tables | Two B-trees per table, random insert positions (page splits, cache misses), larger indexes | Time-ordered UUIDv7 as 16-byte BLOB (client-generatable for offline sync): append-mostly inserts, smaller keys; `WITHOUT ROWID` vs rowid layout chosen by bake-off | 05 |
| B14 | M | Dates stored as TEXT; numbers as NUMERIC | Bigger rows, string compares in indexes | Dates as INTEGER (µs since epoch); typed columns with `STRICT` tables | 05 |
| B15 | M | `modernc.org/sqlite` (C→Go translation) | Generally slower than native C SQLite **[inferred, unmeasured here]** | Native SQLite via `rusqlite` with `bundled` feature, compiled with tuned flags | 04 |
| B16 | M | Per-connection 32 MB cache × up to 120 read conns, no mmap | Memory scales with pool size; page cache duplicated per conn | Fewer read connections (≈ CPU cores × 2), `mmap_size` for shared OS page cache, tuned `cache_size` | 04, 16 |
| B17 | M | Rate limit: fixed window | Bursts at window edges | Sliding-window counter / GCRA (token bucket) in sharded in-memory map | 19 |
| B18 | M | JS hooks via goja VM pool | Interpreter speed, GC, pool contention | Rust hooks (embedded mode) and WASM plugins (precompiled, sandboxed) | 14 |
| B19 | M | Thumbnails in pure Go on the request path; OOM risk acknowledged by author | CPU spike + memory blowup on large images | Decode with size guard, `fast_image_resize` (SIMD), bounded worker pool, generate once and cache | 13 |
| B20 | L | `expand` and `fields` applied after full record build | Extra allocations for discarded fields | Project columns in SQL from `fields`; batch expand queries per relation | 11 |
| B21 | M | Single app per process | Running many apps = many processes, duplicate memory and ports | Multi-app control plane; one SQLite file + writer per app | 03 |
| B22 | M | Single data file → one writer for everything | All apps (if co-hosted) and all collections share one write lock | DB file per app; optionally per-app "hot" collections split into separate files | 03, 04 |
| B23 | L | Logs: 200-entry batches into `auxiliary.db` | Fine; keep the idea | Same, with bounded channel, drop-on-overflow counter, per-app log DB | 16 |

## The three big wins

1. **Realtime fan-out without per-subscriber SQL** (B1–B4). Expected to matter most
   for live apps: cost per write becomes O(matching subscribers) of
   cheap in-memory checks plus O(audience groups) serializations.
2. **Group-commit writer** (B7) plus integer IDs and junction tables (B10, B13): more
   writes per fsync/checkpoint, smaller and index-friendly tables.
3. **Zero-DB auth on the hot path** (B6) plus keyset pagination without default
   counts (B8, B9): fewer queries per request.

## What we deliberately copy

- Collections/rules mental model and PocketBase-compatible filter syntax (lowers the
  learning curve; makes migration from PocketBase possible).
- Delete "dry cache" approach for realtime deletes.
- Transactional batch endpoint.
- Separate DB for logs.
- Single binary with embedded UI.
