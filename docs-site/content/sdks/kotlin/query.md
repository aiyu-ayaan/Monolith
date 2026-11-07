---
title: Read and query
description: Kotlin and Android: list, filter, sort, page and count records.
group: Kotlin and Android
order: 40
---

Read records from a collection with filters, sorting, cursor paging and counts.

```kotlin
@Serializable
data class Task(val id: String = "", val title: String, val done: Boolean = false)

val tasks = mono.collection<Task>("tasks")   // typed with kotlinx.serialization
val raw = mono.collection("tasks")           // RecordModel: id, created, updated, _v, data
```

`collection()` is cached and makes no network request. All data calls are `suspend` functions: call them from a coroutine such as `viewModelScope.launch { ... }`.

## List records

```kotlin
val page = tasks.list(ListOptions(limit = 20))
page.items.forEach { println("${it.id} ${it.title}") }
```

## Filter and sort

```kotlin
val open = tasks.list(
    ListOptions(filter = "done = false && created > '2026-01-01'", sort = "-created,title")
)
```

`filter` uses the [rules syntax](/reference/rules-syntax). Put user values inside quotes you control and strip quotes from the value.

## Paginate

```kotlin
var cursor: String? = null
do {
    val page = tasks.list(ListOptions(filter = "done = false", sort = "-created", limit = 100, cursor = cursor))
    adapter.add(page.items)
    cursor = page.nextCursor
} while (cursor != null)
```

A cursor is only valid with the same `filter` and `sort`. Do not build page numbers.

## Choose fields and expand relations

```kotlin
val rows = tasks.list(ListOptions(fields = "id,title", expand = "owner"))
val one = tasks.getOne(id, expand = "owner,project.org", fields = "id,title,owner")
```

## Count

```kotlin
val open: Long = tasks.count(filter = "done = false")
```

## ListOptions

| Option | Default | Meaning |
|---|---|---|
| `filter` | `null` | Up to 4,096 characters; cannot use `hidden` fields |
| `sort` | `id` | Comma-separated, `-` for descending, up to 3 fields |
| `limit` | 50 | 1 to 1000 |
| `cursor` | `null` | `nextCursor` of the previous page |
| `page`, `perPage` | `null` | Page-number paging (prefer cursors) |
| `fields` | all | Top-level fields; `id` is always included |
| `expand` | `null` | Relation paths, up to 4 hops |
| `total` | `"none"` | `"exact"` or `"estimate"` fills `ListResult.total` |

GET requests that get a `503` are retried with backoff and `Retry-After`.

## Next steps

- [Write data](/sdks/kotlin/write)
- [SDKs overview](/sdks/overview)
