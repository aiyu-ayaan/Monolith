---
title: Realtime
description: Kotlin and Android: live queries, single-record listeners and change events.
group: Kotlin and Android
order: 60
---

Keep a screen current with live queries and change events.

## Listen to a query

```kotlin
// As a Flow (recommended on Android)
tasks.onSnapshot(SnapshotOptions(filter = "done = false", sort = "-created", limit = 50))
    .collect { snap ->
        adapter.submitList(snap.docs)
        snap.changes.forEach { println("${it.type} ${it.doc.id}") }   // ADDED | MODIFIED | REMOVED
    }

// As a callback
val stop = tasks.onSnapshot({ snap -> render(snap.docs) }, SnapshotOptions(filter = "done = false"))
stop()
```

Collect the Flow with `repeatOnLifecycle(Lifecycle.State.STARTED)` so the stream closes when the screen is hidden. Server errors go to `SnapshotOptions.onError`.

## Listen to one record

```kotlin
tasks.onDocSnapshot(id).collect { task ->
    if (task == null) showDeleted() else render(task)   // null: deleted or no longer visible
}
```

## Raw change events

```kotlin
tasks.subscribe(filter = "done = false").collect { e ->
    println("${e.action} ${e.record.id} seq=${e.seq}")   // create | update | delete | leave
}
```

`leave` means the record stopped matching your filter or rule. It was not deleted.

## Behavior

- The SDK reconnects with backoff and resumes from the last sequence number.
- Rules apply: you only receive rows your view rule allows.
- `SnapshotOptions.limit` defaults to 100, server cap 1000.

Limits (connections, replay window, queue size): [Realtime](/build/database/realtime).

## Next steps

- [Storage](/sdks/kotlin/storage)
- [SDKs overview](/sdks/overview)
