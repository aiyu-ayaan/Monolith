---
title: Write data
description: Python: create, update, delete and batch records.
group: Python
order: 50
---

Create, update, replace and delete records, run batches and handle write errors.

## Create

```python
task = tasks.create({"title": "Write docs", "done": False})
print(task["id"])
```

Leave `id` out so the server makes one. A client-made `id` must be a UUIDv7 from the last 30 days: [IDs and cursors](/reference/ids-and-cursors).

## Read one

```python
task = tasks.get_one(record_id)
```

A missing record and a record your rules hide both raise a `404` `MonolithError`.

## Update

```python
tasks.update(record_id, {"done": True})                       # PATCH: only these fields
tasks.replace(record_id, {"title": "New", "done": False})     # PUT: update, or create with this id
```

## Delete

```python
tasks.delete(record_id)   # True
```

## Batch

```python
results = mono.batch([
    {"method": "POST", "path": "/api/v1/apps/shop/collections/tasks/records", "body": {"title": "A"}},
])
```

See [Batch operations](/build/database/batch) for the contract. `batch` has no test against a real server in the SDK yet.

## Handle errors

```python
from monolith import MonolithError

try:
    tasks.create({"title": ""})
except MonolithError as e:
    if e.status == 400:
        print(e.get_field_error("title"))
    else:
        print(f"request {e.request_id} failed: {e.detail}")
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

- [Realtime](/sdks/python/realtime)
- [SDKs overview](/sdks/overview)
