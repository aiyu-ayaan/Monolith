---
title: Write data
description: Create, update, replace and delete records, run batches and handle errors.
group: Use the SDKs
order: 40
---

Writes are checked against the collection's [rules](/build/database/rules) and then its schema. `update` changes only the fields you send; `replace` sets the whole record.

## Create

:::tabs
```ts title="TypeScript"
const task = await tasks.create({ title: "Write docs", done: false });
console.log(task.id, task._v);
```
```kotlin title="Kotlin"
val task = tasks.create(Task(title = "Write docs"))        // typed
val task2 = tasks.create(buildJsonObject { put("title", "Write docs") })   // free-form
```
```dart title="Dart"
final task = await tasks.create({'title': 'Write docs', 'done': false});
print(task.id);
```
```python title="Python"
task = tasks.create({"title": "Write docs", "done": False})
print(task["id"])
```
```rust title="Rust"
let task = tasks.create(&serde_json::json!({ "title": "Write docs", "done": false })).await?;
println!("{}", task.id);
```
:::

Leave `id` out so the server makes one. A client-made `id` must be a UUIDv7 created in the last 30 days: [IDs and cursors](/reference/ids-and-cursors).

## Read one

:::tabs
```ts title="TypeScript"
const task = await tasks.getOne(id);
```
```kotlin title="Kotlin"
val task = tasks.getOne(id)
```
```dart title="Dart"
final task = await tasks.getOne(id);
```
```python title="Python"
task = tasks.get_one(record_id)
```
```rust title="Rust"
let task = tasks.get_one(&id).await?;
```
:::

A record that does not exist and a record your rules hide both answer `404`, so existence does not leak.

## Update and replace

:::tabs
```ts title="TypeScript"
await tasks.update(id, { done: true });          // PATCH: only the fields you send
await tasks.replace(id, { title: "New", done: false }); // PUT: update, or create with this id
```
```kotlin title="Kotlin"
tasks.update(id, buildJsonObject { put("done", true) })   // PATCH: only these fields
tasks.update(id, task.copy(done = true))                  // PATCH with a full model
tasks.replace(id, task.copy(title = "New"))               // PUT: update, or create with this id
```
```dart title="Dart"
await tasks.update(id, {'done': true});                       // PATCH: only these fields
await tasks.replace(id, {'title': 'New', 'done': false});     // PUT: update, or create with this id
```
```python title="Python"
tasks.update(record_id, {"done": True})                       # PATCH: only these fields
tasks.replace(record_id, {"title": "New", "done": False})     # PUT: update, or create with this id
```
```rust title="Rust"
tasks.update(&id, &serde_json::json!({ "done": true })).await?;                    // PATCH: only these fields
tasks.replace(&id, &serde_json::json!({ "title": "New", "done": false })).await?;  // PUT: update, or create with this id
```
:::

| Call | HTTP | Effect |
|---|---|---|
| `update` | `PATCH` | Changes only the fields you send |
| `replace` | `PUT` | Updates the record, or creates it with this id |

## Delete

:::tabs
```ts title="TypeScript"
await tasks.delete(id);   // resolves true
```
```kotlin title="Kotlin"
tasks.delete(id)   // true
```
```dart title="Dart"
await tasks.delete(id);   // true
```
```python title="Python"
tasks.delete(record_id)   # True
```
```rust title="Rust"
tasks.delete(&id).await?;
```
:::

## Batch

:::tabs
```ts title="TypeScript"
const results = await mono.batch([
  { method: "POST", path: `/api/v1/apps/${app}/collections/tasks/records`, body: { title: "A" } },
  { method: "DELETE", path: `/api/v1/apps/${app}/collections/tasks/records/${oldId}` },
]);
```
```kotlin title="Kotlin"
val results = mono.batch(listOf(
    BatchOperation(method = "POST", path = "/api/v1/apps/shop/collections/tasks/records", body = buildJsonObject { put("title", "A") }),
))
results.forEach { println(it.status) }
```
```dart title="Dart"
final results = await mono.batch([
  BatchOperation(method: 'POST', path: '/api/v1/apps/shop/collections/tasks/records', body: {'title': 'A'}),
]);
```
```python title="Python"
results = mono.batch([
    {"method": "POST", "path": "/api/v1/apps/shop/collections/tasks/records", "body": {"title": "A"}},
])
```
```rust title="Rust"
use monolith_client::BatchOp;
let res = mono.batch(&[
    BatchOp::create("tasks", serde_json::json!({ "title": "A" })),
    BatchOp::delete("tasks", old_id),
]).await?;
```
:::

See [Batch operations](/build/database/batch) for the contract.

:::status partial
`batch` has no test against a real server in any SDK. TypeScript's `batch()` does not match the server's request shape, and Rust sends `{op, collection}` where the others send `{method, path}`. Use the REST call until this is fixed.
:::

## Handle errors

:::tabs
```ts title="TypeScript"
import { MonolithError } from "@monolith/client";
try {
  await tasks.create({ title: "" });
} catch (e) {
  if (e instanceof MonolithError && e.status === 400) {
    showFieldError("title", e.getFieldError("title"));
  } else throw e;
}
```
```kotlin title="Kotlin"
try {
    tasks.create(Task(title = ""))
} catch (e: MonolithException) {
    when (e.status) {
        400 -> showFieldError("title", e.getFieldError("title"))
        403 -> showDenied()
        else -> log("request ${e.requestId} failed: ${e.detail}")
    }
}
```
```dart title="Dart"
try {
  await tasks.create({'title': ''});
} on MonolithException catch (e) {
  if (e.status == 400) {
    showFieldError('title', e.getFieldError('title'));
  } else {
    log('request ${e.requestId} failed: ${e.detail}');
  }
}
```
```python title="Python"
from monolith import MonolithError

try:
    tasks.create({"title": ""})
except MonolithError as e:
    if e.status == 400:
        print(e.get_field_error("title"))
    else:
        print(f"request {e.request_id} failed: {e.detail}")
```
```rust title="Rust"
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
:::

| Status | Meaning |
|---|---|
| 400 | Validation failed. The error lists the fields. |
| 403 | A rule denies the write |
| 404 | Missing, or hidden by a rule |
| 409 | Unique or foreign-key conflict |
| 413 | Body over 1 MiB |
| 503 | Writer queue full. Wait for `Retry-After`, retry with jitter. |

More on the error type: [Errors](/sdks/errors).

## Limits and status

`If-Match` (optimistic concurrency) and `?dry_run=true` are REST features. The collection methods do not expose them yet, so call REST for them: [Read and write](/build/database/read-and-write).

## Next steps

- [Realtime](/sdks/realtime)
- [Errors](/sdks/errors)
- [Rules](/build/database/rules)
