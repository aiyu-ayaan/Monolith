---
title: Write data
description: Kotlin and Android: create, update, delete and batch records.
group: Kotlin and Android
order: 50
---

Create, update, replace and delete records, run batches and handle write errors.

## Create

```kotlin
val task = tasks.create(Task(title = "Write docs"))        // typed
val task2 = tasks.create(buildJsonObject { put("title", "Write docs") })   // free-form
```

Leave `id` empty so the server makes one. A client-made `id` must be a UUIDv7 from the last 30 days: [IDs and cursors](/reference/ids-and-cursors).

## Read one

```kotlin
val task = tasks.getOne(id)
```

A missing record and a record your rules hide both throw a `404`.

## Update

```kotlin
tasks.update(id, buildJsonObject { put("done", true) })   // PATCH: only these fields
tasks.update(id, task.copy(done = true))                  // PATCH with a full model
tasks.replace(id, task.copy(title = "New"))               // PUT: update, or create with this id
```

## Delete

```kotlin
tasks.delete(id)   // true
```

## Batch

```kotlin
val results = mono.batch(listOf(
    BatchOperation(method = "POST", path = "/api/v1/apps/shop/collections/tasks/records", body = buildJsonObject { put("title", "A") }),
))
results.forEach { println(it.status) }
```

See [Batch operations](/build/database/batch) for the contract. `batch` has no test against a real server in the SDK yet.

## Handle errors

```kotlin
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

| Status | Meaning |
|---|---|
| 400 | Validation failed; read `e.errors` |
| 403 | A rule denies the write |
| 404 | Missing, or hidden by a rule |
| 409 | Unique or foreign-key conflict |
| 413 | Body over 1 MiB |
| 503 | Writer queue full; retry with jitter |

## Not in the SDK

`If-Match` and `?dry_run=true` are REST features that the collection methods do not expose yet: [Read and write](/build/database/read-and-write).

## Next steps

- [Realtime](/sdks/kotlin/realtime)
- [SDKs overview](/sdks/overview)
