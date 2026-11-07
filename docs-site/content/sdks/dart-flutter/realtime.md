---
title: Realtime
description: Dart and Flutter: live queries and change events.
group: Dart and Flutter
order: 50
---

Live queries and change events.

| Method | Returns |
|---|---|
| `subscribe(listener, [RealtimeSubscriptionOptions(filter, ids)])` | Unsubscribe `void Function()` |
| `onSnapshot([SnapshotOptions(filter, sort, limit)])` | `Stream<QuerySnapshot<T>>` |
| `onDocSnapshot(id)` | `Stream<T?>` (`null` once deleted or hidden) |

`RealtimeEvent` has `action` (`create`, `update`, `delete`, `leave`), `record`, `collection`, `seq`. `QuerySnapshot` has `docs` and `changes`. Reconnects send a fresh snapshot. Server errors arrive as stream errors.

```dart
tasks.onSnapshot(SnapshotOptions(filter: 'done = false', sort: '-created'))
    .listen((snap) => setState(() => items = snap.docs));
```

## Next steps

- [Storage](/sdks/dart-flutter/storage)
- [SDKs overview](/sdks/overview)
