---
title: Realtime
description: Rust: live queries, single-record listeners and change events.
group: Rust
order: 60
---

Keep a screen current with live queries and change events.

Realtime methods return a `Subscription`, which is both a `Stream` and has `recv().await`. Dropping it closes the connection.

## Listen to a query

```rust
use monolith_client::SnapshotOptions;

let mut sub = mono.records("tasks")
    .on_snapshot(SnapshotOptions::new().filter("done = false").sort("-created").limit(50));
while let Some(snap) = sub.recv().await {
    let snap = snap?;
    println!("{} docs, {} changes", snap.docs.len(), snap.changes.len());
}
```

`snap.changes` hold `ChangeType::Added`, `Modified` or `Removed`. The first snapshot reports every document as `Added`.

## Listen to one record

```rust
let mut sub = mono.records("tasks").on_doc_snapshot(&id);
while let Some(doc) = sub.recv().await {
    match doc? { Some(r) => render(r), None => show_deleted() }   // None: deleted or no longer visible
}
```

## Raw change events

```rust
use monolith_client::SubscribeOptions;
let mut sub = mono.collection::<Task>("tasks").subscribe(SubscribeOptions::new().filter("done = false"));
while let Some(ev) = sub.recv().await {
    let ev = ev?;
    println!("{:?} seq={:?}", ev.action, ev.seq);   // Create | Update | Delete | Leave
}
```

`Leave` means the record stopped matching your filter or rule. It was not deleted.

## Behavior

- Reconnect backoff is set on the builder with `realtime_backoff(base, max)`.
- Rules apply: you only receive rows your view rule allows.
- `on_snapshot` and `on_doc_snapshot` exist on `Collection<Record>` only.

Limits (connections, replay window, queue size): [Realtime](/build/database/realtime).

## Next steps

- [Storage](/sdks/rust/storage)
- [SDKs overview](/sdks/overview)
