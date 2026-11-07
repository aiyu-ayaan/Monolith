---
title: Realtime
description: TypeScript: live queries and change events.
group: TypeScript
order: 50
---

Live queries and change events.

| Method | Callback gets | Returns |
|---|---|---|
| `subscribe(listener, { filter?, ids? })` | `RealtimeEvent { action, record, collection, seq }` | unsubscribe |
| `onSnapshot(callback, { filter?, sort?, limit?, onError? })` | `QuerySnapshot { docs, changes }` | unsubscribe |
| `onDocSnapshot(id, callback)` | record or `null` | unsubscribe |

`action` is `create`, `update`, `delete` or `leave` (the record stopped matching your filter). `changes` hold `{ type: "added" | "modified" | "removed", doc }`. The first snapshot reports every document as `added`. `limit` defaults to 100, server cap 1000.

```ts
const stop = tasks.onSnapshot((snap) => render(snap.docs), { filter: "done = false", sort: "-created" });
```

## Next steps

- [Storage](/sdks/typescript/storage)
- [SDKs overview](/sdks/overview)
