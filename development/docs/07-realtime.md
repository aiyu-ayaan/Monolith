# 07 — Realtime (Live Data Flow)

## 1. Requirements

- Live record changes (create/update/delete) for collections, single records and
  filtered queries, respecting rules.
- Custom broadcast channels (server or client-published messages), e.g. a live location stream
  "live location" stream that does not need to be persisted.
- 50k+ mostly idle connections per node within memory budget ([16](16-performance-engineering.md)).
- Resumable: reconnect without missing events (bounded by change-log retention).
- Works through proxies/CDNs (SSE) and supports bidirectional low-latency (WebSocket).

## 2. Transports

| Transport | Endpoint | When |
|-----------|----------|------|
| SSE | `GET /api/v1/apps/{app}/realtime` | Default for browsers and simple clients; works over HTTP/2 multiplexing |
| WebSocket | `GET /api/v1/apps/{app}/realtime/ws` | SDKs (mobile), client-published channel messages, lower overhead per message |

Both share one protocol and one hub; only framing differs.

## 3. Protocol

Messages are JSON (MessagePack optional via `?enc=msgpack` for SDKs).

```jsonc
// server → client on connect
{"t":"hello","conn":"c_01h...","seq":918273,"hb":25}

// client → server (WS frame, or POST /realtime/{conn}/subscriptions for SSE)
{"t":"sub","id":"s1","topic":"records:tasks","filter":"status = 'active'","fields":"id,status,assignee","expand":"assignee","since":918000}
{"t":"sub","id":"s2","topic":"records:tasks/01j9...x"}
{"t":"sub","id":"s3","topic":"channel:task-01j9...:location"}
{"t":"unsub","id":"s1"}
{"t":"pub","topic":"channel:task-01j9...:location","data":{"lat":..,"lon":..}}   // WS only, channel rules apply

// server → client
{"t":"ev","sub":"s1","seq":918274,"op":"update","record":{...}}
{"t":"ev","sub":"s3","data":{...}}
{"t":"resync","sub":"s1","reason":"gap"}          // client must refetch (cursor too old or queue overflow)
{"t":"err","sub":"s1","code":"forbidden"}
```

- `since` = last `seq` the client saw. The hub replays from the change log (if
  retained) before switching to live, so reconnects lose nothing. SSE also accepts
  the standard `Last-Event-ID` header carrying the same `seq`.
- Heartbeat every `hb` seconds (comment line for SSE, ping frame for WS) keeps
  proxies open; no forced 30-minute reconnect (B5). Optional max lifetime stays
  configurable.
- Auth: token in header on connect; WS can refresh auth in-band
  (`{"t":"auth","token":...}`) without reconnecting. Auth record changes (role change,
  logout, delete) re-evaluate or drop that connection's subscriptions.

## 4. Hub data structures (per app)

```
SubscriptionIndex {
  by_record:     HashMap<(CollId, RecordId), SmallVec<SubRef>>   // "collection/id"
  by_collection: HashMap<CollId, CollectionSubs>                 // "collection" and filtered queries
  channels:      HashMap<ChannelName, SmallVec<SubRef>>
}
CollectionSubs {
  groups: HashMap<AudienceShape, Group>   // subscribers sharing (rule class, filter, fields, expand)
}
Group {
  compiled_filter: Arc<Predicate>          // from rules engine
  members: Slab<SubRef>                    // conn + sub id + auth ctx handle
}
```

- Looking up subscribers for a change touches only that collection's groups and that
  record's direct subscribers (fixes B2).
- Index lives behind a sharded lock (`papaya`/`dashmap`-style or per-collection
  `RwLock`); subscribe/unsubscribe are rare compared to events.

## 5. Fan-out algorithm (per committed change)

```
on ChangeEvent(ev):
  for group in index.by_collection[ev.coll].groups ∪ by_record[(ev.coll, ev.id)]:
      # 1) rule + filter
      match group.rule_class:
         Const/AuthOnly → allowed = members filtered by cached per-connection bool
         RowLocal       → allowed = members where predicate(ev.new_row, auth) == true      (in memory)
         Relational     → allowed = batched SQL check (one query for all distinct auth ids)  (06 §5)
      # 2) serialize once per audience
      for (visibility_key, members) in allowed.group_by(visibility_key):   # e.g. owner sees email, others don't
          bytes = encode(ev, group.fields, group.expand, visibility_key)    # ONE encode → Bytes (refcounted)
          for m in members: m.conn.queue.try_push(Frame{sub: m.sub_id, bytes: bytes.clone()})
```

- Encoding produces the event body once; per-subscriber framing only prepends the
  subscription id (`{"t":"ev","sub":"s1",` + shared body). No per-subscriber
  `serde` work (fixes B3).
- `expand` in realtime is resolved once per group with a batched query.
- Updates that move a row **out of** a subscriber's filter emit `op:"leave"` (the
  hub evaluates the old row too), so live queries stay correct. PocketBase does not
  send these.
- Deletes use the pre-delete row (already in the change event, captured by the
  writer) — same correctness as PocketBase's dry cache, no extra query.

## 6. Connection handling

- Per connection: one Tokio task, one bounded queue (default 1024 frames), one write
  buffer. No task per message (fixes B4).
- Writer coalesces queued frames into one `write` syscall per wakeup.
- Overflow policy: when the queue is full, drop the queue, send `resync`, then close.
  A slow client never slows others.
- Memory target per idle connection: **≤ 8 KB** (task + queue header + small buffers;
  buffers allocated lazily) — verified in benchmark S7.
- Per-app limits: max connections, max subscriptions per connection (default 256),
  max filter length.

## 7. Channels (ephemeral pub/sub)

- Topic names `channel:<name>`; per-app channel rules defined like collection rules
  with `@request.auth`, `@request.channel.*` variables, e.g.
  `channel:task-*:location` → `subscribeRule: @request.auth.id = @collection.tasks.assignee || ...`.
- Messages are not persisted (fire-and-forget) unless the channel is configured with a
  short replay buffer (last N messages in memory).
- Server-side publish API: `POST /api/v1/apps/{app}/channels/{name}` (API key with
  `realtime:publish` scope) for backend jobs and webhooks.

## 8. Ordering and delivery guarantees

| Guarantee | Provided |
|-----------|----------|
| Per-subscription order | Events delivered in `seq` order |
| At-least-once on reconnect | With `since` and retained change log |
| Exactly-once | Client dedupes by `seq` (SDK does it) |
| Cross-node | Single node in v1; multi-node fan-out via change-log tailing in a later phase |

## 9. Implemented today (Phase 2 slice, `crates/monolith-core/src/realtime.rs`)

- Transport SSE only: `GET /api/v1/apps/{app}/realtime?collections=a,b` (events `hello`, `snapshot`, `record`, `resync`, `error`, `: hb`).
- Topics: a whole collection, narrowed per collection by `filter.<coll>=<expr>` (doc 06 syntax, non-relational) and/or
  `ids.<coll>=<id>,<id>`. A narrowed topic keeps a bounded `seen` set (50 000 ids) so it can send `op:"leave"`
  when a record stops matching and deliver deletes of records it showed. One topic per collection per connection;
  open another connection for another filter.
- `snapshot=<n>` (≤ 1000) sends the current result set first as `event: snapshot` (`{collection, seq, items, next}`, list rule,
  `sort.<coll>`), seeds `seen`, then live events. `since=<seq>` or `Last-Event-ID` replays missed changes from `_changes`
  (current row state, ≤ 20 000 changes, else `resync` with `reason:"since"`). A snapshot wins over `since`. Live events that
  arrive during priming wait behind a per-connection gate and are flushed in order, dropping seq ≤ the primed seq.
- Not done: channels, WebSocket, `fields`/`expand` on events.
- One hub task per app tails the change bus. Per batch and collection it fetches the changed rows in
  one statement (bucketed `IN` sizes of 1/8/64/512 so statements cache), encodes each event once per
  audience (superuser, everyone else) and pushes the shared `Arc<str>` into a bounded per-connection
  queue (256). A full queue drops that connection; bus lag broadcasts `resync`.
- **Deviation from §5:** the change event does not carry the row yet, so an event holds the row as it is
  at dispatch time (rapid writes coalesce) and a delete has no pre-delete row. Non-superusers therefore
  get `delete` only when the view rule is public, except narrowed topics, which deliver deletes of records they had shown.
  Fix: capture the old/new row in the writer (store crate).
- Non-superusers need a view rule that is not `Relational` (those need the batched SQL check of §5).
- Limits: 10 000 connections per app, 32 collections per connection. Not benchmarked: scenario S7 (doc 17) is still to run.
