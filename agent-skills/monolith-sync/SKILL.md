---
name: monolith-sync
description: Implement or debug Monolith offline-first sync: cursor pull, idempotent push, conflict policies, bootstrap after 410. Use for any client that must work offline.
---

# Sync

- Pull: `GET /sync/pull?cursor=<seq>&collections=a,b&limit=1000` returns `changes[]`, `cursor`, `more`. Apply in order, store the new cursor atomically with the data.
- Push: `POST /sync/push` with `device` and `mutations[]` (each has unique `mid`). Retrying the same `mid` is safe.
- `410 Gone` on pull = cursor older than retention: run `GET /sync/bootstrap` then pull from its `seq`.
- Conflict policy is per collection: `lww-field` (default), `lww-row`, `server-wins`, `custom`, `append-only`. Check `GET /collections/{c}` for the active one.
- `op:"leave"` means delete the row locally (no longer visible to this user).
- Create IDs on the device (UUIDv7) so offline relations stay valid.
