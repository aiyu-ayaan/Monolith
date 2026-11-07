---
title: IDs and cursors
description: Record id format, timestamps, versions and pagination cursors.
group: API
order: 30
---

| Thing | Format |
|---|---|
| Record `id` | UUIDv7 stored as 16 bytes, shown as 26-character lowercase Crockford base32 |
| Client-chosen `id` on create | Allowed if it is a UUIDv7 created within the last 30 days |
| `created`, `updated` | RFC 3339, UTC, microseconds. Stored as integer microseconds. |
| `_v` | Change sequence number of the last write; also the `ETag` |
| `seq` | Gap-free, per-app sequence number of every committed change |
| `cursor` / `next` | Opaque keyset cursor. Valid only with the same `filter` and `sort`. |
| App name | `^[a-z][a-z0-9_]{0,62}$` |

IDs sort by creation time, so inserts stay ordered in the index. Treat cursors as opaque strings; their content can change between versions.

## Next steps

- [Query records](/build/database/query)
