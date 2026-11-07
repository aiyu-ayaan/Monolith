---
title: Read and query
description: TypeScript: list, filter, sort, page and count records.
group: TypeScript
order: 40
---

Read records from a collection with filters, sorting, cursor paging and counts.

```ts
type Task = { title: string; done: boolean };
const tasks = mono.collection<Task>("tasks");   // typed; omit <Task> for RecordModel
```

`collection()` is cheap and cached: call it wherever you need it. It makes no network request.

## List records

```ts
const page = await tasks.list({ limit: 20 });
for (const t of page.items) console.log(t.id, t.title);
```

Every record has `id`, `created`, `updated` and `_v` next to your own fields.

## Filter and sort

```ts
const open = await tasks.list({
  filter: "done = false && created > '2026-01-01'",
  sort: "-created,title",
});
```

`filter` uses the [rules syntax](/reference/rules-syntax). Build it from user input with care: put values inside quotes you control and strip quotes from the value, as `onDocSnapshot` does for an id.

## Paginate

```ts
let cursor: string | undefined;
do {
  const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 100, cursor });
  render(page.items);
  cursor = page.nextCursor ?? (page.next as string | undefined);
} while (cursor);
```

:::warning
The server sends the cursor as `next`, while the TypeScript type names it `nextCursor`. Read both until the SDK is fixed. A cursor is only valid with the same `filter` and `sort`.
:::

## Choose fields and expand relations

```ts
const rows = await tasks.list({ fields: "id,title", expand: "owner" });
const one = await tasks.getOne(id, { expand: "owner,project.org", fields: "id,title,owner" });
```

## Count

```ts
const n = await tasks.count({ filter: "done = false" });
```

:::note
`count()` does not match the server's response yet, so prefer `list({ total: "exact", limit: 1 })` and read `total`.
:::

## ListOptions

| Option | Default | Meaning |
|---|---|---|
| `filter` | none | Up to 4,096 characters; cannot use `hidden` fields |
| `sort` | `id` | Comma-separated, `-` for descending, up to 3 fields |
| `limit` | 50 | 1 to 1000 |
| `cursor` | none | The `next` value of the previous page |
| `page`, `perPage` | none | Page-number paging (prefer cursors) |
| `fields` | all | Top-level fields; `id` is always included |
| `expand` | none | Relation paths, up to 4 hops |
| `total` | `"none"` | `"exact"` or `"estimate"` adds `total` to the result |

## Next steps

- [Write data](/sdks/typescript/write)
- [SDKs overview](/sdks/overview)
