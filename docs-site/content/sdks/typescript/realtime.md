---
title: Realtime
description: TypeScript: live queries, single-record listeners and change events.
group: TypeScript
order: 60
---

Keep a screen current with live queries and change events.

## Listen to a query

```ts
const stop = tasks.onSnapshot(
  (snap) => {
    render(snap.docs);                       // the full, ordered result
    for (const c of snap.changes) console.log(c.type, c.doc.id); // added | modified | removed
  },
  { filter: "done = false", sort: "-created", limit: 50, onError: (e) => console.error(e) },
);
// later
stop();
```

The first snapshot reports every document as `added`. After that you get a snapshot after each change that affects the result. `sort` is also applied to live changes on the client.

## Listen to one record

```ts
const stop = tasks.onDocSnapshot(id, (doc) => {
  if (doc === null) showDeleted();   // deleted, or no longer visible to you
  else render(doc);
});
```

## Raw change events

```ts
const stop = tasks.subscribe((e) => {
  console.log(e.action, e.record.id, e.seq);   // create | update | delete | leave
}, { filter: "done = false" });
```

`leave` means the record stopped matching your filter or rule. It was not deleted.

## Behavior

- The SDK reconnects with backoff and resumes from the last sequence number.
- Without `filter` or `ids`, subscriptions to one collection share a connection. A filter opens its own.
- Rules apply: you only receive rows your view rule allows.
- Use `fetch` streaming for raw HTTP; `EventSource` cannot send the `Authorization` header.

Limits (connections, replay window, queue size): [Realtime](/build/database/realtime).

## Next steps

- [Storage](/sdks/typescript/storage)
- [SDKs overview](/sdks/overview)
