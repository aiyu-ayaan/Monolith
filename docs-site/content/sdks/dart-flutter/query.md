---
title: Read and query
description: Dart and Flutter: list, filter, sort, page and count records.
group: Dart and Flutter
order: 40
---

Read records from a collection with filters, sorting, cursor paging and counts.

```dart
final tasks = mono.collection<RecordModel>('tasks');                 // RecordModel: id, created, updated, v, data
final typed = mono.collection<Task>('tasks', Task.fromJson);         // your own decoder
```

`collection()` is cached and makes no network request.

## List records

```dart
final page = await tasks.list(limit: 20);
for (final t in page.items) {
  print('${t.id} ${t['title']}');
}
```

## Filter and sort

```dart
final open = await tasks.list(
  filter: "done = false && created > '2026-01-01'",
  sort: '-created,title',
);
```

`filter` uses the [rules syntax](/reference/rules-syntax). Put user values inside quotes you control and strip quotes from the value.

## Paginate

```dart
String? cursor;
do {
  final page = await tasks.list(filter: 'done = false', sort: '-created', limit: 100, cursor: cursor);
  items.addAll(page.items);
  cursor = page.nextCursor;
} while (cursor != null);
```

A cursor is only valid with the same `filter` and `sort`. Do not build page numbers.

## Choose fields and expand relations

```dart
final rows = await tasks.list(fields: 'id,title', expand: 'owner');
final one = await tasks.getOne(id, expand: 'owner,project.org', fields: 'id,title,owner');
```

## Count

```dart
final open = await tasks.count(filter: 'done = false');
```

## Parameters of `list`

| Parameter | Default | Meaning |
|---|---|---|
| `filter` | `null` | Up to 4,096 characters; cannot use `hidden` fields |
| `sort` | `id` | Comma-separated, `-` for descending, up to 3 fields |
| `limit` | 50 | 1 to 1000 |
| `cursor` | `null` | `nextCursor` of the previous page |
| `page`, `perPage` | `null` | Page-number paging (prefer cursors) |
| `fields` | all | Top-level fields; `id` is always included |
| `expand` | `null` | Relation paths, up to 4 hops |
| `total` | `'none'` | `'exact'` or `'estimate'` fills `ListResult.total` |

You can also pass a `ListOptions` object with `options:`. GET requests that get a `503` are retried with backoff and `Retry-After`.

## Next steps

- [Write data](/sdks/dart-flutter/write)
- [SDKs overview](/sdks/overview)
