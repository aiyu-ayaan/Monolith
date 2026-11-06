# 09 — Offline Sync & App Data Backup

Two related needs from the vision:
1. **Online sync** across the app library: apps work offline and converge with the
   server (Firestore-style).
2. **Backup** of each app's data (e.g. a mobile app), on schedule and on demand, with
   restore.

## Part A — Sync

### A1. Model

- Server is the source of truth. Clients keep a local replica (SQLite on mobile via SDK,
  IndexedDB on web) of the collections and rows they are allowed to see.
- Every committed mutation has a per-app `seq` (gap-free, commit order) and a row-level
  `_hlc` (hybrid logical clock: 48-bit physical ms + 16-bit counter).
- Sync is opt-in per collection (`sync: on`). Rules apply to sync exactly as to REST.

### A2. Pull

```
GET /api/v1/apps/{app}/sync/pull?cursor=918000&collections=tasks,users&limit=1000
→ 200 {
    "changes": [
      {"seq":918001,"c":"tasks","id":"01j9..","op":"upsert","v":918001,"hlc":..,"data":{...}},
      {"seq":918005,"c":"tasks","id":"01j8..","op":"delete","v":918005}
    ],
    "cursor": 918005,
    "more": false
  }
```

- Server reads `_changes WHERE seq > cursor ORDER BY seq LIMIT n`, joins current row
  state (latest version only; intermediate versions are collapsed), filters by the
  pull rule (compiled `RowLocal` predicate in memory, batched SQL for `Relational`).
- Row-level visibility changes: if a row was visible before and is not now, the client
  gets `op: "leave"` (removes it from the local replica).
- Long-poll (`wait=25s`) or realtime `since` subscription for push-style delivery.
- `cursor` older than retention → `410 Gone` → SDK runs a **bootstrap** (paged
  keyset scan of each collection as of a snapshot `seq`, then pulls from that `seq`).

### A3. Push

```
POST /api/v1/apps/{app}/sync/push
{ "device":"dev_01..", "mutations":[
    {"mid":"m-1","c":"tasks","id":"01j9..","op":"insert","data":{...},"hlc":..},
    {"mid":"m-2","c":"tasks","id":"01j9..","op":"update","base_v":917990,"patch":{"status":"done"},"hlc":..}
]}
→ 200 { "results":[{"mid":"m-1","ok":true,"v":918010},
                   {"mid":"m-2","ok":true,"v":918011,"merged":true}], "cursor":918011 }
```

- Mutations are idempotent by `(device, mid)` (dedupe table, TTL 7 days) so retries
  after network loss are safe.
- The whole push is one `WriteOp` → executes in the writer as one transaction with a
  savepoint per mutation (partial success allowed, per-mutation result).
- IDs are UUIDv7 generated on the device ([05](05-data-model-and-storage.md)), so
  offline-created relations stay valid.

### A4. Conflict resolution (per collection policy)

| Policy | Behaviour | Use |
|--------|-----------|-----|
| `lww-field` *(default)* | Per-field last-writer-wins by HLC: patch fields with newer HLC than the stored field HLC win | Most app data |
| `lww-row` | Whole row LWW by HLC | Simple objects |
| `server-wins` | Reject if `base_v` ≠ current `_v`; client gets current row | Money, inventory |
| `custom` | WASM/Rust hook decides merge | Domain logic |
| `append-only` | Inserts only (timeseries) | GPS points, logs |

Per-field HLCs for `lww-field` are stored in a compact side column `_fhlc BLOB`
(only on sync-enabled collections).

### A5. SDK responsibilities

- Local outbox (pending mutations), local cursor, retry with backoff.
- Optimistic UI: apply locally first, reconcile on push result.
- Query local replica with the same filter syntax (SDK ships a small evaluator, or
  translates to local SQLite).
- Background sync triggers: app foreground, network regained, realtime event, timer.

## Part B — Backup & restore

### B1. Mechanisms

| Mechanism | How | RPO | Cost |
|-----------|-----|-----|------|
| Snapshot | SQLite online backup API (`sqlite3_backup_*`, step in chunks, yields to writer) or `VACUUM INTO` → compress (zstd) → local and/or S3 | Schedule (e.g. hourly) | Low; per app |
| Continuous WAL shipping | Ship WAL frames to S3 after each checkpoint segment (Litestream-style, built in) | Seconds | Moderate I/O |
| Logical export | NDJSON / JSON per collection + schema + files manifest | On demand | For migration/portability |
| Files | Content-addressed; incremental (only new hashes uploaded) | Same as snapshot | |

Snapshots include `data.db`, `logs.db` (optional), analytics store (optional),
schema/migrations, and a files manifest. Archive format: `tar.zst` with a manifest
(app id, schema version, `seq`, checksums).

### B2. Policies (per app)

```toml
[apps.myapp.backup]
schedule      = "0 * * * *"        # hourly snapshot
keep_hourly   = 24
keep_daily    = 14
keep_weekly   = 8
target        = "s3://backups/myapp/"
wal_shipping  = true               # continuous, point-in-time restore
include_files = true
encrypt       = true               # age/XChaCha20 with app backup key
```

### B3. Restore

- Restore snapshot or point-in-time (snapshot + WAL replay to a timestamp/`seq`)
  **into a new app** (`myapp-restore-2026-11-06`) or in place.
- In-place restore: app switches to maintenance (503 for that app only), files are
  swapped atomically, `AppRuntime` reloads. Other apps unaffected.
- Clients with cursors beyond the restored `seq` receive `410` → bootstrap resync.

### B4. Cross-app "backup hub" use case

Any external app (not built on Monolith) can push its data into a Monolith app for
safekeeping using the plug-and-play API: create an app `myapp-archive`, create a key
with `records:write`, then `POST /sync/push` or batch insert. The same backup policies
then protect that data.
