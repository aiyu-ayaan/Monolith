---
title: Read and query
description: Python: list, filter, sort, page and count records.
group: Python
order: 40
---

Read records from a collection with filters, sorting, cursor paging and counts.

```python
tasks = mono.collection("tasks")   # records are plain dicts
```

`collection()` is cached and makes no network request. The client is synchronous.

## List records

```python
page = tasks.list(limit=20)
for t in page.items:
    print(t["id"], t["title"])
```

Every record has `id`, `created`, `updated` and `_v` next to your own fields.

## Filter and sort

```python
open_tasks = tasks.list(filter="done = false && created > '2026-01-01'", sort="-created,title")
```

`filter` uses the [rules syntax](/reference/rules-syntax). Put user values inside quotes you control and strip quotes from the value.

## Paginate

```python
cursor = None
while True:
    page = tasks.list(filter="done = false", sort="-created", limit=100, cursor=cursor)
    handle(page.items)
    cursor = page.next_cursor
    if not cursor:
        break
```

A cursor is only valid with the same `filter` and `sort`. Do not build page numbers.

## Choose fields and expand relations

```python
rows = tasks.list(fields="id,title", expand="owner")
one = tasks.get_one(record_id, expand="owner,project.org", fields="id,title,owner")
```

## Count

```python
open_count = tasks.count(filter="done = false")
```

## Parameters of `list`

| Parameter | Default | Meaning |
|---|---|---|
| `filter` | `None` | Up to 4,096 characters; cannot use `hidden` fields |
| `sort` | `id` | Comma-separated, `-` for descending, up to 3 fields |
| `limit` | 50 | 1 to 1000 |
| `cursor` | `None` | `next_cursor` of the previous page |
| `page`, `per_page` | `None` | Page-number paging (prefer cursors) |
| `fields` | all | Top-level fields; `id` is always included |
| `expand` | `None` | Relation paths, up to 4 hops |
| `total` | `"none"` | `"exact"` or `"estimate"` fills `ListResult.total` |

Pass a `ListOptions` object as the first argument instead of keywords if you prefer.

## Next steps

- [Write data](/sdks/python/write)
- [SDKs overview](/sdks/overview)
