---
title: Database
description: Python: read, write, query and page through records.
group: Python
order: 40
---

Read, write, query and page through records.

```python
tasks = mono.collection("tasks")
```

| Method | Returns | HTTP |
|---|---|---|
| `list(page=, per_page=, cursor=, limit=, filter=, sort=, expand=, fields=, total=)` or `list(ListOptions)` | `ListResult` | `GET .../records` |
| `get_one(id, expand=, fields=)` | record `dict` | `GET .../records/{id}` |
| `create(data)` | `dict` | `POST` |
| `update(id, data)` | `dict` | `PATCH` |
| `replace(id, data)` | `dict` | `PUT` |
| `delete(id)` | `bool` | `DELETE` |
| `count(filter=)` | `int` | `GET .../records:count` |

`ListResult` has `items`, `total`, `cursor`, `next_cursor`, `prev_cursor`, `raw`.

```python
page = tasks.list(filter="done = false", sort="-created", limit=20)
nxt = tasks.list(limit=20, cursor=page.next_cursor) if page.next_cursor else None
t = tasks.create({"title": "Buy milk"})
tasks.update(t["id"], {"done": True})
tasks.delete(t["id"])
```

Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch([{"method": "POST", "path": "...", "body": {...}}])` returns the response list.

## Next steps

- [Realtime](/sdks/python/realtime)
- [SDKs overview](/sdks/overview)
