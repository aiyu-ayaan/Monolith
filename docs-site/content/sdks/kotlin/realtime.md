---
title: Realtime
description: Kotlin and Android: live queries and change events.
group: Kotlin and Android
order: 50
---

Live queries and change events.

| Method | Returns |
|---|---|
| `subscribe(listener, filter, ids)` | Unsubscribe `() -> Unit` |
| `subscribe(filter, ids)` / `subscribeFlow(...)` | `Flow<RealtimeEvent<T>>` |
| `onSnapshot(callback, SnapshotOptions)` | Unsubscribe `() -> Unit` |
| `onSnapshot(SnapshotOptions)` | `Flow<QuerySnapshot<T>>` |
| `onDocSnapshot(id, callback, onError)` | Unsubscribe `() -> Unit` |
| `onDocSnapshot(id)` | `Flow<T?>` (`null` once deleted or hidden) |

`RealtimeEvent(action, record, collection, seq)`: `action` is `create`, `update`, `delete` or `leave` (the record stopped matching your filter). `QuerySnapshot(docs, changes)`: `changes` are `DocumentChange(type = ADDED | MODIFIED | REMOVED, doc)`. `SnapshotOptions(filter, sort, limit = 100, onError)`.

```kotlin
tasks.onSnapshot(SnapshotOptions(filter = "done = false", sort = "-created"))
    .collect { snap -> adapter.submit(snap.docs) }
```

## Next steps

- [Storage](/sdks/kotlin/storage)
- [SDKs overview](/sdks/overview)
