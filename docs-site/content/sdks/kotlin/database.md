---
title: Database
description: Kotlin and Android: read, write, query and page through records.
group: Kotlin and Android
order: 40
---

Read, write, query and page through records.

```kotlin
@Serializable data class Task(val id: String = "", val title: String, val done: Boolean = false)

val tasks = mono.collection<Task>("tasks")   // typed
val raw = mono.collection("tasks")           // RecordModel
```

| Method | Returns | HTTP |
|---|---|---|
| `list(ListOptions)` | `ListResult<T>` | `GET .../records` |
| `getOne(id, expand, fields)` | `T` | `GET .../records/{id}` |
| `create(data: T)` or `create(JsonObject)` | `T` | `POST` |
| `update(id, data)` | `T` | `PATCH` (partial) |
| `replace(id, data)` | `T` | `PUT` |
| `delete(id)` | `Boolean` | `DELETE` |
| `count(filter)` | `Long` | `GET .../records:count` |

`ListOptions(page, perPage, cursor, limit, filter, sort, expand, fields, total)`; `total` is `"none"`, `"exact"` or `"estimate"`. `ListResult` has `items`, `total`, `cursor`, `nextCursor`, `prevCursor`.

```kotlin
val first = tasks.list(ListOptions(filter = "done = false", sort = "-created", limit = 20))
val next = first.nextCursor?.let { tasks.list(ListOptions(limit = 20, cursor = it)) }
val created = tasks.create(Task(title = "Buy milk"))
tasks.update(created.id, buildJsonObject { put("done", true) })
tasks.delete(created.id)
```

Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch(listOf(BatchOperation(method, path, body)))` returns `List<BatchResponseItem(status, body)>`.

## Next steps

- [Realtime](/sdks/kotlin/realtime)
- [SDKs overview](/sdks/overview)
