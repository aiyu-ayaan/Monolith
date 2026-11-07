---
title: Database
description: Rust: read, write, query and page through records.
group: Rust
order: 40
---

Read, write, query and page through records.

```rust
use monolith_client::ListOptions;
let tasks = mono.records("tasks");                      // Collection<Record>
let typed = mono.collection::<Task>("tasks");           // your serde type
```

| Method | Returns |
|---|---|
| `list(Option<&ListOptions>)` | `ListResult<T>` |
| `get_one(id)` / `get_one_with_options(id, ..)` | `T` |
| `create(&body)` | `T` |
| `update(id, &body)` | `T` (PATCH) |
| `replace(id, &body)` | `T` (PUT) |
| `delete(id)` | `()` |
| `count(Option<&str>)` | `u64` |

`ListOptions::new().page().per_page().limit().cursor().filter().sort().expand().fields().total()`. `ListResult` has `items`, `next`, `total`. `Record` has `id`, `created`, `updated`, `version`, and `fields` for everything else.

```rust
let page = tasks.list(Some(&ListOptions::new().filter("done = false").sort("-created").limit(20))).await?;
let created = tasks.create(&serde_json::json!({ "title": "Buy milk" })).await?;
tasks.update(&created.id, &serde_json::json!({ "done": true })).await?;
tasks.delete(&created.id).await?;
```

### Batch

`BatchOp::create(collection, body)`, `BatchOp::update(collection, id, body)`, `BatchOp::delete(collection, id)`, sent with `mono.batch(&ops)`.

## Next steps

- [Realtime](/sdks/rust/realtime)
- [SDKs overview](/sdks/overview)
