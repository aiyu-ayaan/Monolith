---
title: Write data
description: TypeScript: create, update, delete and batch records.
group: TypeScript
order: 50
---

Create, update, replace and delete records, run batches and handle write errors.

## Create

```ts
const task = await tasks.create({ title: "Write docs", done: false });
console.log(task.id, task._v);
```

Send your own `id` only if it is a UUIDv7 made in the last 30 days; otherwise leave it out. See [IDs and cursors](/reference/ids-and-cursors).

## Read one

```ts
const task = await tasks.getOne(id);
```

A record that does not exist and a record your rules hide both throw a `404`.

## Update

```ts
await tasks.update(id, { done: true });          // PATCH: only the fields you send
await tasks.replace(id, { title: "New", done: false }); // PUT: update, or create with this id
```

## Delete

```ts
await tasks.delete(id);   // resolves true
```

## Batch

```ts
const results = await mono.batch([
  { method: "POST", path: `/api/v1/apps/${app}/collections/tasks/records`, body: { title: "A" } },
  { method: "DELETE", path: `/api/v1/apps/${app}/collections/tasks/records/${oldId}` },
]);
```

:::note
`batch()` does not match the server's request shape yet. See [Batch operations](/build/database/batch) for the contract and use the REST call until the SDK is fixed.
:::

## Handle errors

```ts
import { MonolithError } from "@monolith/client";
try {
  await tasks.create({ title: "" });
} catch (e) {
  if (e instanceof MonolithError && e.status === 400) {
    showFieldError("title", e.getFieldError("title"));
  } else throw e;
}
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

`If-Match` (optimistic concurrency) and `?dry_run=true` are REST features that the collection methods do not expose yet. Call REST for them: [Read and write](/build/database/read-and-write).

## Next steps

- [Realtime](/sdks/typescript/realtime)
- [SDKs overview](/sdks/overview)
