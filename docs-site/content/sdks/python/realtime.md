---
title: Realtime
description: Python: live queries and change events.
group: Python
order: 50
---

Live queries and change events.

| Method | Callback gets | Returns |
|---|---|---|
| `subscribe(listener, filter=, ids=)` | `RealtimeEvent(action, record, collection, seq)` | unsubscribe |
| `on_snapshot(callback, filter=, sort=, limit=, on_error=)` | `QuerySnapshot(docs, changes)` | unsubscribe |
| `on_doc_snapshot(id, callback, on_error=)` | record or `None` | unsubscribe |

`action` is `create`, `update`, `delete` or `leave`. `DocumentChange.type` is `"added"`, `"modified"` or `"removed"`. Call `mono.close()` to stop all streams.

```python
stop = tasks.on_snapshot(lambda snap: print(len(snap.docs)), filter="done = false")
```

## Next steps

- [Storage](/sdks/python/storage)
- [SDKs overview](/sdks/overview)
