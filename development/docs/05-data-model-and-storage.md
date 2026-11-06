# 05 — Data Model & Storage

Applies to the SQLite engine (lead candidate, [04](04-database-engine-evaluation.md)).
Postgres equivalents are noted where they differ.

## 1. Files on disk

```
/data/
  monolith.toml
  system.db                      # control plane
  apps/<app_id>/
    data.db  (+ -wal, -shm)      # collections, users, sessions, change log
    logs.db                      # request/app logs (separate writer)
    analytics/                   # DuckDB file or SQLite rollup DB
    files/                       # local file storage (if not S3)
    backups/                     # local snapshots (if enabled)
  plugins/                       # WASM plugins
```

## 2. Identifiers

**Decision: UUIDv7, stored as 16-byte `BLOB`, encoded on the wire as 26-char lowercase
Crockford base32.**

| Option | Why not / why |
|--------|---------------|
| PocketBase 15-char random TEXT | Random insert positions, 15+ bytes, second B-tree (B13) |
| Server-assigned 64-bit int (Snowflake) | Smallest and fastest, but **offline clients cannot create records with final IDs**; temp-ID remapping across offline relations is error-prone |
| **UUIDv7 BLOB(16)** | Time-ordered → append-mostly inserts and good page locality; globally unique → clients generate IDs offline, relations created offline stay valid; also unique across apps (simplifies export/merge) |

Layout variant decided by bake-off: (a) `WITHOUT ROWID` table keyed by `id`, one
B-tree; (b) rowid table + `UNIQUE(id)`. SQLite favours `WITHOUT ROWID` for small rows;
large rows favour rowid tables. Junction and change-log tables are always optimal
as chosen below.

Server validates client-supplied IDs: correct version bits, timestamp within
`[now - 30 days, now + 5 min]` (configurable) to prevent index hot-spot abuse.

## 3. Collection types

| Type | Backing | Notes |
|------|---------|-------|
| `base` | Table | Generic records |
| `auth` | Table + system fields | Users that can log in ([08](08-authentication.md)) |
| `view` | SQL `VIEW` (read-only) | Defined by a SELECT; rules apply on top |
| `timeseries` *(new)* | Table, rowid, append-only, `INTEGER` time key | High-volume telemetry (e.g. GPS points); optional TTL purge; no updates |

## 4. System columns (every table)

| Column | Type | Purpose |
|--------|------|---------|
| `id` | BLOB(16) | UUIDv7 |
| `created` | INTEGER | µs since Unix epoch, UTC |
| `updated` | INTEGER | µs since Unix epoch, UTC |
| `_v` | INTEGER | Row version = `seq` of last change (optimistic concurrency + sync) |
| `_hlc` | INTEGER | Hybrid logical clock of last change (conflict resolution, [09](09-sync-and-backup.md)) |

All user tables are created `STRICT` so SQLite enforces column types.

## 5. Field types → storage

| Field type | SQLite column | Filter/sort support | Notes |
|-----------|---------------|---------------------|-------|
| `text` | `TEXT` | `= != ~ !~ < >`, `:lower` | max length validated; optional `COLLATE NOCASE` index |
| `email` | `TEXT` | same | lowercase-normalized on write |
| `url` | `TEXT` | same | |
| `editor` | `TEXT` | `~` only | HTML sanitized on write |
| `number` | `INTEGER` if `onlyInt`, else `REAL` | full | PocketBase uses `NUMERIC` |
| `bool` | `INTEGER` 0/1 | `= !=` | |
| `date` | `INTEGER` µs UTC | full | wire format RFC 3339 |
| `autodate` | `INTEGER` µs UTC | full | set on create/update |
| `select` (single) | `TEXT` | `= !=` | values validated against options |
| `select` (multi, ≤63 options) | `INTEGER` bitmask | `?=` → `(col & bit) != 0` | **new**: O(1) membership, indexable via expression index |
| `select` (multi, >63 options) | junction table | `?=` | |
| `relation` (single) | `BLOB(16)` FK + index | full, joins | `ON DELETE` cascade / set null / restrict |
| `relation` (multi) | **junction table** | `?=`, path traversal | replaces PocketBase JSON arrays (B10) |
| `file` (single) | `TEXT` (stored name) | `= !=` | metadata in `_files` |
| `file` (multi) | junction to `_files` | | |
| `json` | `BLOB` (SQLite JSONB) | `json path` filters | JSONB avoids re-parsing on every access |
| `password` | `TEXT` (Argon2id PHC) | none | never returned |
| `geoPoint` | `lat REAL`, `lon REAL` (two columns) | `geoDistance()`, bbox | optional `R*Tree` index per field |
| `vector` *(later)* | `BLOB` f32 | kNN | via `sqlite-vec` style extension; out of v1 |

### Junction table shape (multi-relation `post.tags → tags`)

```sql
CREATE TABLE "_rel_posts_tags" (
  owner  BLOB NOT NULL,          -- posts.id
  target BLOB NOT NULL,          -- tags.id
  pos    INTEGER NOT NULL,       -- preserves client order
  PRIMARY KEY (owner, target)
) STRICT, WITHOUT ROWID;
CREATE INDEX "_rel_posts_tags_target" ON "_rel_posts_tags"(target, owner);
```

- Forward lookup (`post.tags`) and back-relation (`tags_via_tags`) both use an index.
- API keeps PocketBase's shape: `"tags": ["id1","id2"]` in JSON.
- Writes to the field diff old vs new set and insert/delete junction rows in the same
  transaction.

## 6. Per-app system tables (`data.db`)

| Table | Purpose |
|-------|---------|
| `_meta` | app id, schema version, engine settings |
| `_collections` | collection definitions (fields, rules, indexes, options) as JSON + version |
| `_migrations` | applied migration ids |
| `_sessions` | refresh-token sessions ([08](08-authentication.md)) |
| `_external_auths` | OAuth2 identities |
| `_mfa`, `_otp` | pending MFA / OTP state (short TTL) |
| `_api_keys` | per-app scoped keys (hash only) |
| `_changes` | change log ([§8](#8-change-log)) |
| `_sync_clients` | registered devices + cursors ([09](09-sync-and-backup.md)) |
| `_files` | file metadata (name, size, mime, hash, storage key, thumbs) |
| `_webhooks`, `_webhook_deliveries` | outbound webhooks + retry queue |
| `_settings` | per-app settings (SMTP override, OAuth providers, limits) |

Control plane `system.db`: `apps`, `superusers`, `superuser_sessions`, `api_keys_index`
(key prefix → app, for routing without opening every app DB), `settings`, `audit_log`,
`backups`.

## 7. Indexes

- Auto: PK, FK columns of single relations, junction indexes, unique fields,
  `(updated)` on sync-enabled collections, `_changes(seq)`.
- User-defined: any column list, `UNIQUE`, partial (`WHERE`), expression
  (`lower(email)`), validated through the same parser used for rules.
- **Index advisor** (admin UI): logs slow queries with their rule/filter shape and the
  SQLite `EXPLAIN QUERY PLAN` output; suggests indexes for frequent scans.
- `PRAGMA optimize` runs on a schedule and after schema changes to keep `sqlite_stat1`
  / `stat4` fresh for the planner.

## 8. Change log

```sql
CREATE TABLE _changes (
  seq     INTEGER PRIMARY KEY,   -- monotonic, gap-free per app (assigned by writer)
  ts      INTEGER NOT NULL,      -- µs commit time
  coll    INTEGER NOT NULL,      -- collection numeric id
  rid     BLOB NOT NULL,         -- record id
  op      INTEGER NOT NULL,      -- 1 insert, 2 update, 3 delete
  hlc     INTEGER NOT NULL,
  actor   BLOB,                  -- user/api-key/device id
  fields  INTEGER,               -- bitmap of changed field slots (≤64 fields), NULL = all
  patch   BLOB                   -- optional compact diff (sync-enabled collections only)
) STRICT;
```

- Written inside the same transaction as the data change → never out of sync.
- Rowid table with integer key → pure appends, cheapest possible insert.
- Retention: keep `changes_retention` (default 30 days or N million rows). Clients with
  a cursor older than the oldest retained `seq` get `410 Gone` → full resync.
- Collections can opt out of the change log (`sync: off`) if they never need sync or
  realtime resumption; realtime still works from the in-memory bus.

## 9. Schema changes and migrations

1. Admin UI / API / migration file submits a new collection definition.
2. Diff old vs new → `MigrationPlan` with typed steps:
   `AddColumn`, `DropColumn`, `RenameColumn`, `AddIndex`, `DropIndex`,
   `CreateJunction`, `RebuildTable{copy_expr}`, `ChangeRule` (no DDL).
3. Cheap steps (add/rename/drop column, index ops) run in one write transaction.
4. `RebuildTable` (type change, constraint change) runs **online**: create
   `new_table`, copy in chunks of N rows per writer turn (other writes interleave), use
   the change log to catch up rows modified during the copy, then a short final
   transaction swaps tables. Long DDL never holds the writer for seconds.
5. Schema version increments → `AppRuntime` swaps its `Arc<Schema>` atomically; rule
   compiler output and prepared statements are rebuilt for the new version.
6. Every applied plan is stored as a JSON migration file under the app, exportable for
   git (`monolith migrate export`), replayable on another instance.

## 10. Data limits (defaults, configurable per app)

| Limit | Default |
|-------|---------|
| Max request body (non-file) | 1 MiB |
| Max fields per collection | 64 (bitmap fits `u64`) |
| Max text field length | 64 KiB |
| Max JSON field size | 1 MiB |
| Max records per batch request | 500 |
| Max relation path depth in rules | 4 |
| Max expand depth | 4 |
