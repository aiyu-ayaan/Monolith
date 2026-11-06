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
