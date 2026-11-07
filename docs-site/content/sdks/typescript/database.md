---
title: Database
description: TypeScript: read, write, query and page through records.
group: TypeScript
order: 40
---

Read, write, query and page through records.

```ts
type Task = { title: string; done: boolean };
const tasks = mono.collection<Task>("tasks");
```

| Method | Returns | HTTP |
|---|---|---|
| `list(options)` | `ListResult<T>` | `GET .../records` |
| `getOne(id, { expand?, fields? })` | `T` | `GET .../records/{id}` |
| `create(data)` | `T` | `POST` |
| `update(id, data)` | `T` | `PATCH` (partial) |
| `replace(id, data)` | `T` | `PUT` |
| `delete(id)` | `boolean` | `DELETE` |
| `count({ filter? })` | `number` | `GET .../records:count` |

`ListOptions`: `page`, `perPage`, `cursor`, `limit`, `filter`, `sort`, `expand`, `fields`, `total` (`"none" | "exact" | "estimate"`). `ListResult`: `items`, `total`, `cursor`, `nextCursor`, `prevCursor`. Every record has `id`, `created`, `updated`, `_v`.

```ts
const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 20 });
const more = page.nextCursor ? await tasks.list({ limit: 20, cursor: page.nextCursor }) : null;
const t = await tasks.create({ title: "Buy milk", done: false });
await tasks.update(t.id, { done: true });
await tasks.delete(t.id);
```

Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch([{ method: "POST", path, body }])` returns `{ status, body }[]`.

## Next steps

- [Realtime](/sdks/typescript/realtime)
- [SDKs overview](/sdks/overview)
