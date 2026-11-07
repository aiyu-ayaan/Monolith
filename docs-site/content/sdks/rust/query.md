---
title: Read and query
description: Rust: list, filter, sort, page and count records.
group: Rust
order: 40
---

Read records from a collection with filters, sorting, cursor paging and counts.

```rust
use monolith_client::{ListOptions, Record};

let tasks = mono.records("tasks");                 // Collection<Record>: id, created, updated, version, fields
let typed = mono.collection::<Task>("tasks");      // your own serde type
```

`Task` needs `serde::Deserialize` (and `Serialize` for writes). Creating a handle makes no network request.

## List records

```rust
let page = tasks.list(None).await?;
for t in &page.items {
    println!("{} {:?}", t.id, t.fields.get("title"));
}
```

## Filter and sort

```rust
let opts = ListOptions::new()
    .filter("done = false && created > '2026-01-01'")
    .sort("-created,title");
let open = tasks.list(Some(&opts)).await?;
```

`filter` uses the [rules syntax](/reference/rules-syntax). Put user values inside quotes you control and strip quotes from the value.

## Paginate

```rust
let mut cursor: Option<String> = None;
loop {
    let mut opts = ListOptions::new().filter("done = false").sort("-created").limit(100);
    if let Some(c) = &cursor { opts = opts.cursor(c.clone()); }
    let page = tasks.list(Some(&opts)).await?;
    handle(&page.items);
    cursor = page.next;
    if cursor.is_none() { break; }
}
```

A cursor is only valid with the same `filter` and `sort`. Do not build page numbers.

## Choose fields and expand relations

```rust
let rows = tasks.list(Some(&ListOptions::new().fields("id,title").expand("owner"))).await?;
```

## Count

```rust
let open: u64 = tasks.count(Some("done = false")).await?;
```

## ListOptions

| Method | Default | Meaning |
|---|---|---|
| `filter(..)` | none | Up to 4,096 characters; cannot use `hidden` fields |
| `sort(..)` | `id` | Comma-separated, `-` for descending, up to 3 fields |
| `limit(..)` | 50 | 1 to 1000 |
| `cursor(..)` | none | `next` of the previous page |
| `page(..)`, `per_page(..)` | none | Page-number paging (prefer cursors) |
| `fields(..)` | all | Top-level fields; `id` is always included |
| `expand(..)` | none | Relation paths, up to 4 hops |
| `total(..)` | `"none"` | `"exact"` or `"estimate"` fills `ListResult.total` |

## Next steps

- [Write data](/sdks/rust/write)
- [SDKs overview](/sdks/overview)
