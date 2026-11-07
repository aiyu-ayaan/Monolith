---
title: Write data
description: Rust: create, update, delete and batch records.
group: Rust
order: 50
---

Create, update, replace and delete records, run batches and handle write errors.

## Create

```rust
let task = tasks.create(&serde_json::json!({ "title": "Write docs", "done": false })).await?;
println!("{}", task.id);
```

Pass any `Serialize` value as the body. Leave `id` out so the server makes one. A client-made `id` must be a UUIDv7 from the last 30 days: [IDs and cursors](/reference/ids-and-cursors).

## Read one

```rust
let task = tasks.get_one(&id).await?;
```

A missing record and a record your rules hide both return an `Error::Api` with status `404`.

## Update

```rust
tasks.update(&id, &serde_json::json!({ "done": true })).await?;                    // PATCH: only these fields
tasks.replace(&id, &serde_json::json!({ "title": "New", "done": false })).await?;  // PUT: update, or create with this id
```

## Delete

```rust
tasks.delete(&id).await?;
```

## Batch

```rust
use monolith_client::BatchOp;
let res = mono.batch(&[
    BatchOp::create("tasks", serde_json::json!({ "title": "A" })),
    BatchOp::delete("tasks", old_id),
]).await?;
```

:::note
The Rust batch operation shape (`op`, `collection`) differs from the other SDKs (`method`, `path`). Check it against [Batch operations](/build/database/batch) before relying on it.
:::

## Handle errors

```rust
use monolith_client::Error;

match tasks.create(&serde_json::json!({ "title": "" })).await {
    Ok(t) => println!("{}", t.id),
    Err(e @ Error::Api { .. }) if e.status() == Some(400) => {
        let msg = e.problem().and_then(|p| p.get_field_error("title"));
        eprintln!("title: {msg:?}");
    }
    Err(e) => return Err(e.into()),
}
```

| Status | Meaning |
|---|---|
| 400 | Validation failed; read `problem().errors` |
| 403 | A rule denies the write |
| 404 | Missing, or hidden by a rule |
| 409 | Unique or foreign-key conflict |
| 413 | Body over 1 MiB |
| 503 | Writer queue full; retry with jitter |

## Not in the SDK

`If-Match` and `?dry_run=true` are REST features that the collection methods do not expose yet: [Read and write](/build/database/read-and-write).

## Next steps

- [Realtime](/sdks/rust/realtime)
- [SDKs overview](/sdks/overview)
