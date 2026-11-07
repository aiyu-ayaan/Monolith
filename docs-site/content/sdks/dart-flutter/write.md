---
title: Write data
description: Dart and Flutter: create, update, delete and batch records.
group: Dart and Flutter
order: 50
---

Create, update, replace and delete records, run batches and handle write errors.

## Create

```dart
final task = await tasks.create({'title': 'Write docs', 'done': false});
print(task.id);
```

Leave `id` out so the server makes one. A client-made `id` must be a UUIDv7 from the last 30 days: [IDs and cursors](/reference/ids-and-cursors).

## Read one

```dart
final task = await tasks.getOne(id);
```

A missing record and a record your rules hide both throw a `404`.

## Update

```dart
await tasks.update(id, {'done': true});                       // PATCH: only these fields
await tasks.replace(id, {'title': 'New', 'done': false});     // PUT: update, or create with this id
```

## Delete

```dart
await tasks.delete(id);   // true
```

## Batch

```dart
final results = await mono.batch([
  BatchOperation(method: 'POST', path: '/api/v1/apps/shop/collections/tasks/records', body: {'title': 'A'}),
]);
```

See [Batch operations](/build/database/batch) for the contract. Check the `BatchOperation` constructor in the SDK source before relying on it: it has no test against a real server yet.

## Handle errors

```dart
try {
  await tasks.create({'title': ''});
} on MonolithException catch (e) {
  if (e.status == 400) {
    showFieldError('title', e.getFieldError('title'));
  } else {
    log('request ${e.requestId} failed: ${e.detail}');
  }
}
```

| Status | Meaning |
|---|---|
| 400 | Validation failed; read `e.errors` |
| 403 | A rule denies the write |
| 404 | Missing, or hidden by a rule |
| 409 | Unique or foreign-key conflict |
| 413 | Body over 1 MiB |
| 503 | Writer queue full; retry with jitter |

## Not in the SDK

`If-Match` and `?dry_run=true` are REST features that the collection methods do not expose yet: [Read and write](/build/database/read-and-write).

## Next steps

- [Realtime](/sdks/dart-flutter/realtime)
- [SDKs overview](/sdks/overview)
