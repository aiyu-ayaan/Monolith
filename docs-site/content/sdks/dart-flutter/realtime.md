---
title: Realtime
description: Dart and Flutter: live queries, single-record listeners and change events.
group: Dart and Flutter
order: 60
---

Keep a screen current with live queries and change events.

## Listen to a query

```dart
final sub = tasks
    .onSnapshot(SnapshotOptions(filter: 'done = false', sort: '-created', limit: 50))
    .listen(
      (snap) {
        setState(() => items = snap.docs);
        for (final c in snap.changes) {
          print('${c.type} ${c.doc}');   // added | modified | removed
        }
      },
      onError: (e) => print('realtime error: $e'),
    );
// later
await sub.cancel();
```

In Flutter, `StreamBuilder<QuerySnapshot<RecordModel>>(stream: tasks.onSnapshot(), ...)` works without managing the subscription yourself.

## Listen to one record

```dart
tasks.onDocSnapshot(id).listen((task) {
  if (task == null) showDeleted();   // deleted, or no longer visible
  else render(task);
});
```

## Raw change events

```dart
final stop = tasks.subscribe((e) {
  print('${e.action} ${e.record} seq=${e.seq}');   // create | update | delete | leave
}, RealtimeSubscriptionOptions(filter: 'done = false'));
stop();
```

`leave` means the record stopped matching your filter or rule. It was not deleted.

## Behavior

- A reconnect sends a fresh snapshot, and server errors arrive as stream errors.
- Rules apply: you only receive rows your view rule allows.
- `SnapshotOptions.limit` defaults to 100, server cap 1000.

Limits (connections, replay window, queue size): [Realtime](/build/database/realtime).

## Next steps

- [Storage](/sdks/dart-flutter/storage)
- [SDKs overview](/sdks/overview)
