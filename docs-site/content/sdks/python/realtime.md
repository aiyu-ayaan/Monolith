---
title: Realtime
description: Python: live queries, single-record listeners and change events.
group: Python
order: 60
---

Keep a screen current with live queries and change events.

Callbacks run on a background thread. Keep them short and thread-safe.

## Listen to a query

```python
def on_snap(snap):
    print(len(snap.docs), "docs")
    for c in snap.changes:
        print(c.type, c.doc["id"])      # added | modified | removed

stop = tasks.on_snapshot(on_snap, filter="done = false", sort="-created", limit=50,
                         on_error=lambda e: print("realtime error", e))
# later
stop()
```

## Listen to one record

```python
stop = tasks.on_doc_snapshot(record_id, lambda doc: print("gone" if doc is None else doc["title"]))
```

`None` means the record was deleted or is no longer visible to you.

## Raw change events

```python
stop = tasks.subscribe(lambda e: print(e.action, e.record["id"], e.seq), filter="done = false")
```

`action` is `create`, `update`, `delete` or `leave`. `leave` means the record stopped matching your filter or rule. It was not deleted.

## Behavior

- Without `filter` or `ids`, subscriptions share one connection; a filter opens its own.
- Rules apply: you only receive rows your view rule allows.
- Call `mono.close()` to stop every stream before the process exits.

Limits (connections, replay window, queue size): [Realtime](/build/database/realtime).

## Next steps

- [Storage](/sdks/python/storage)
- [SDKs overview](/sdks/overview)
