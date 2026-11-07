---
title: Errors
description: The error type every SDK throws and what it carries.
group: Use the SDKs
order: 80
---

A failed call throws or returns one error type built from the server's `application/problem+json` body. Statuses and what to do for each: [Errors](/reference/errors).

## Catch an error

:::tabs
```ts title="TypeScript"
import { MonolithError } from "@monolith/client";
try { await tasks.create({}); }
catch (e) {
  if (e instanceof MonolithError) console.log(e.status, e.getFieldError("title"), e.requestId);
}
```
```kotlin title="Kotlin"
try { tasks.create(Task(title = "")) }
catch (e: MonolithException) { println("${e.status} ${e.getFieldError("title")} ${e.requestId}") }
```
```dart title="Dart"
try { await tasks.create({}); }
on MonolithException catch (e) { print('${e.status} ${e.getFieldError('title')} ${e.requestId}'); }
```
```python title="Python"
from monolith import MonolithError
try:
    tasks.create({})
except MonolithError as e:
    print(e.status, e.get_field_error("title"), e.request_id)
```
```rust title="Rust"
if let Err(e) = tasks.create(&serde_json::json!({})).await {
    println!("{:?} {:?}", e.status(), e.problem().and_then(|p| p.get_field_error("title")));
}
```
:::

## What it carries

| Field | Meaning |
|---|---|
| `status` | HTTP status. `0` in Kotlin when the network failed. |
| `type`, `title`, `detail` | The problem type and a readable message |
| `requestId` | Equals the `X-Request-Id` header. Quote it when you debug. |
| `errors` | Field problems: `field`, `code`, `message` |
| `getFieldError(field)` | The message for one field, or nothing |

Rust has one `Error` enum: `Api { status, message, problem }`, `Network`, `Serialization`, `Url`, `Config` and `Realtime`. Only `Api` has a status and a problem.

## Find the request

Look it up with `GET /api/v1/apps/{app}/logs?request_id=...` as a superuser, or in the Admin UI **Logs** page.

## Next steps

- [Errors reference](/reference/errors)
