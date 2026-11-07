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

- SSE only. Query: `collections=a,b`, per collection `filter.<c>=<expr>` and `ids.<c>=<id>,<id>` (one topic per collection per
  connection), `sort.<c>`, `snapshot=<n>` (current result first), `since=<seq>` or `Last-Event-ID` (replay). Events: `hello`
  (`{conn, hb}`), `snapshot` (`{collection, seq, items, next}`), `record` (`{op: create|update|delete|leave, collection, seq, record}`,
  SSE `id` = `seq`), `resync`, `error`, and `: hb` comments every 25 s. Not yet: WebSocket, channels, `fields`/`expand`.
- `leave` and filtered deletes work only for records the connection was shown (snapshot or earlier event; 50 000 tracked per topic).
- In a browser use `fetch` streaming (needs the `Authorization` header), not `EventSource`.
- After a reconnect send `since=<last seq>`; on `resync` refetch (or reopen with `snapshot`). Replay is capped at 20 000 changes.
- An event holds the row as it is when dispatched, so quick writes to one row can arrive as one event.
- Superusers see all rows and hidden fields. Others need a non-relational view rule (`null` = 403, relational = 400) and only
  receive rows it allows; they get `delete` only when the view rule is public.
- A client 256 frames behind is dropped: reconnect and refetch.
- Debug: `GET /api/v1/apps/{app}/stats` has `realtime.{connections,events,dropped}`; the admin UI Realtime page is a live monitor.

## From an SDK

Prefer the SDK live queries over hand-written SSE: `collection.onSnapshot` (TypeScript, Kotlin Flow, Dart Stream, Python `on_snapshot`, Rust `on_snapshot`) gives the current result then a snapshot per change, `onDocSnapshot` follows one record, `subscribe` gives raw events. They reconnect with backoff and resume from the last `seq`. See the SDK skills.
