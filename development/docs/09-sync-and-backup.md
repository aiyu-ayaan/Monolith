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

### B4. Encryption at rest and keys (feature scope, Phase 3)

Status: **scoped, not built.** Goal: a stolen `/data` volume, disk image or backup
archive reveals no app data, and backup/restore works across servers with keys the
operator controls.

**Engine.** Page-level encryption inside SQLite, so WAL, snapshots and temp files are
covered too. Candidates: SQLCipher (AES-256-CBC + HMAC-SHA512 per page) and
SQLite3MultipleCiphers (ChaCha20-Poly1305, SQLCipher-compatible mode). Both work with
rusqlite. Pages decrypt only when they enter the page cache, so cached reads cost nothing
extra. **Hypothesis, not measured:** 5–15% on cold reads and commits. The Phase 0
harness runs S1/S3 with encryption on and off before this ships; until then it stays
opt-in and off by default (performance-first, CLAUDE.md). On the Postgres fallback,
use volume encryption (LUKS/dm-crypt); community Postgres has no TDE.

**Key hierarchy (envelope encryption).**

| Key | What | Stored |
|-----|------|--------|
| KEK (master key) | 256-bit, wraps every DEK | Never in `/data`. A file mounted from outside the volume (`--master-key-file`, e.g. a Docker/K8s secret on tmpfs) or a KMS (later). Not an env var |
| System DEK | Encrypts `system.db` | `/data/keys/system.dek`, wrapped by the KEK (XChaCha20-Poly1305) |
| App DEK | One per app, encrypts `apps/<app>/data.db` | `system.db` table `app_keys(app, wrapped_dek, kek_id, created)` |
| Backup key | age X25519 key pair per server (or per app) | Public key on the server; private key shown **once** at creation, kept offline by the operator |

- Enabling: chosen at first start (`--encrypt`), shown in the installer page. A plain
  server converts with `monolith encrypt` (per app `sqlcipher_export`, app in
  maintenance). Never toggled silently.
- KEK rotation: re-wrap the DEKs only (fast, no data rewrite).
- DEK rotation: `PRAGMA rekey` per app (rewrites the file; maintenance window, 503 for
  that app only).
- Lost KEK and lost backup private key = unrecoverable data. The UI and docs must say so
  before enabling.

**Backup with keys.** A snapshot of an encrypted DB is already ciphertext under its
DEK. The archive (`tar.zst`) is then encrypted to the backup public key (age) and its
manifest carries the app DEK wrapped to the same recipient. The server can write
backups but cannot read old ones without the offline private key.

**Restore with keys.** The operator supplies the backup private key for the duration
of the restore only (upload or `--backup-identity` file; never persisted). The server
unwraps the DEK, re-wraps it with its own KEK, and restores into a new app or in place
(B3). This also moves an encrypted app to another server.

**Done when:** encrypted S1/S3 numbers recorded in doc 17; restore of an encrypted
snapshot onto a second server passes; wrong/missing key gives a clear error and leaves
the target untouched; key rotation tests for KEK and DEK.

### B5. Cross-app "backup hub" use case

Any external app (not built on Monolith) can push its data into a Monolith app for
safekeeping using the plug-and-play API: create an app `myapp-archive`, create a key
with `records:write`, then `POST /sync/push` or batch insert. The same backup policies
then protect that data.

## Implemented today (backups v1, `crates/monolith-core/src/backup.rs`)

- Manual per-app snapshots via `VACUUM INTO` (consistent, the writer keeps running) into
  `<data>/backups/<app>/b<UTC timestamp>.db`; plain SQLite files, not yet `tar.zst`.
- `GET|POST /api/v1/apps/{app}/backups`, `GET|DELETE .../backups/{name}` (download streams in 256 KiB chunks),
  `POST .../backups:restore {backup, newApp}` restores into a **new** app. Admin UI: Backups page.
- Not yet: schedules and retention (B2), zstd, S3 targets, WAL shipping, encryption (B4), in-place restore (B3),
  `logs.db`/files in the archive, the `monolith backup` CLI. Backups are kept when the app is deleted.
