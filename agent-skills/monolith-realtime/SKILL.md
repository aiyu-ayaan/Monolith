---
name: monolith-realtime
description: Use Monolith live data over WebSocket or SSE: subscribe to records, filtered queries and channels, resume after reconnect. Use when implementing live updates or debugging missing/duplicate events.
---

# Realtime protocol

Connect: `GET /api/v1/apps/{app}/realtime/ws` (WebSocket) or `/realtime` (SSE). Token in `Authorization` header or first `{"t":"auth","token":...}` frame.

Frames (JSON): server `hello{conn,seq,hb}`, `ev{sub,seq,op,record}`, `resync{sub,reason}`, `err{sub,code}`;
client `sub{id,topic,filter?,fields?,expand?,since?}`, `unsub{id}`, `pub{topic,data}`.
Topics: `records:<collection>`, `records:<collection>/<id>`, `channel:<name>`.

Rules of thumb:
- Remember the last `seq` per connection; send it as `since` (or `Last-Event-ID` on SSE) when reconnecting.
- `op` is `create|update|delete|leave`. `leave` = row no longer matches your filter or rule.
- On `resync` refetch the query, then resubscribe.
- Heartbeat every `hb` seconds; reconnect with exponential backoff + jitter.
- Missing events usually mean: rule denies the row, filter does not match, or `since` older than change-log retention.

## Implemented today (SSE slice)

Check this list before relying on a feature; the rest of this page is the target design.

```bash
curl -N -H "Authorization: Bearer $MONOLITH_KEY" \
  "$BASE/api/v1/apps/$APP/realtime?collections=posts,comments"
```

- SSE only, whole-collection topics only. Events: `hello` (`{conn, hb}`), `record` (`{op: create|update|delete, collection, seq, record}`,
  SSE `id` = `seq`), `resync`, and `: hb` comments every 25 s. Not yet: WebSocket, channels, filters, `fields`/`expand`, `leave`, `since`.
- In a browser use `fetch` streaming (needs the `Authorization` header), not `EventSource`.
- No `since` replay: after a reconnect or `resync`, refetch the collection.
- An event holds the row as it is when dispatched, so quick writes to one row can arrive as one event.
- Superusers see all rows and hidden fields. Others need a non-relational view rule (`null` = 403, relational = 400) and only
  receive rows it allows; they get `delete` only when the view rule is public.
- A client 256 frames behind is dropped: reconnect and refetch.
- Debug: `GET /api/v1/apps/{app}/stats` has `realtime.{connections,events,dropped}`; the admin UI Realtime page is a live monitor.
