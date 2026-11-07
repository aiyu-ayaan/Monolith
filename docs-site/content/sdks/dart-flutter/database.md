---
title: Database
description: Dart and Flutter: read, write, query and page through records.
group: Dart and Flutter
order: 40
---

Read, write, query and page through records.

```dart
final tasks = mono.collection<RecordModel>('tasks');
final typed = mono.collection<Task>('tasks', Task.fromJson);   // your decoder
```

| Method | Returns | HTTP |
|---|---|---|
| `list({page, perPage, cursor, limit, filter, sort, expand, fields, total})` | `ListResult<T>` | `GET .../records` |
| `getOne(id, {expand, fields})` | `T` | `GET .../records/{id}` |
| `create(Map)` | `T` | `POST` |
| `update(id, Map)` | `T` | `PATCH` |
| `replace(id, Map)` | `T` | `PUT` |
| `delete(id)` | `bool` | `DELETE` |
| `count({filter})` | `int` | `GET .../records:count` |

```dart
final page = await tasks.list(filter: 'done = false', sort: '-created', limit: 20);
final created = await tasks.create({'title': 'Buy milk'});
await tasks.update(created.id, {'done': true});
await tasks.delete(created.id);
```

`ListResult` has `items`, `total`, `cursor`, `nextCursor`, `prevCursor`. Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch([BatchOperation(...)])` returns `List<BatchResponseItem>`.

## Next steps

- [Realtime](/sdks/dart-flutter/realtime)
- [SDKs overview](/sdks/overview)
