---
title: Realtime
description: Rust: live queries and change events.
group: Rust
order: 50
---

Live queries and change events.

| Method | Item |
|---|---|
| `subscribe(SubscribeOptions::new().filter(..).ids(..))` | `Result<RealtimeEvent<T>>` (`action`, `record`, `collection`, `seq`) |
| `on_snapshot(SnapshotOptions::new().filter(..).sort(..).limit(..))` | `Result<QuerySnapshot>` (`docs`, `changes`) |
| `on_doc_snapshot(id)` | `Result<Option<Record>>` |

`on_snapshot` and `on_doc_snapshot` exist on `Collection<Record>`. The return value is a `Subscription`: call `recv().await`, or use it as a `Stream`. Dropping it closes the connection.

```rust
let mut sub = mono.records("tasks").on_snapshot(SnapshotOptions::new().filter("done = false"));
while let Some(snap) = sub.recv().await {
    println!("{} docs", snap?.docs.len());
}
```

## Next steps

- [Storage](/sdks/rust/storage)
- [SDKs overview](/sdks/overview)
