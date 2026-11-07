---
name: monolith-realtime
description: Subscribe to live record changes from Monolith over Server-Sent Events. Use when a client needs live updates, or when diagnosing missing realtime events.
---

# Realtime (SSE)

```bash
curl -N -H "Authorization: Bearer $MONOLITH_KEY" \
  "$BASE/api/v1/apps/$APP/realtime?collections=posts,comments"
```

Events: `hello` (`{conn, hb}`), `record` (`{op: create|update|delete, collection, seq, record}`, SSE `id` = `seq`),
`resync` (refetch the data), and `: hb` comments every 25 s.

## Implemented today

- Whole-collection topics over SSE only. In a browser use `fetch` streaming (needs the `Authorization` header), not `EventSource`.
- No `since` replay: after a reconnect, refetch the collection. `resync` means the same.
- An event holds the row as it is when dispatched, so several quick writes to one row can arrive as one event.
- Superusers see all rows and hidden fields. Others need a non-relational view rule (`null` view rule = 403,
  relational rule = 400) and only receive rows it allows. They get `delete` events only when the view rule is public.
- A client that falls 256 frames behind is dropped: reconnect and refetch.
- Debug: `GET /api/v1/apps/{app}/stats` has `realtime.{connections,events,dropped}`; the admin UI Realtime page shows a live monitor.
