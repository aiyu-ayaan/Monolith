---
title: Kotlin and Android API
description: Every class and method of the Kotlin SDK: init, auth, database, realtime, storage, analytics.
group: API reference
order: 20
---

Package `dev.monolith.client` (module `monolith-core`). Calls are `suspend` functions; listeners are `Flow`s or callbacks. Android adds `Monolith.init(context, ...)` from `monolith-android`.

## Initialize

```kotlin
// Android
import dev.monolith.android.init
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")

// JVM / any Kotlin
val mono = Monolith.init(url = "https://api.example.com", app = "shop")
```

| Parameter | Default | Meaning |
|---|---|---|
| `url` | required | Server base URL |
| `app` | `"default"` | App id |
| `key` | `null` | An `mk_` API key |
| `storage` | memory (Android: encrypted) | `SessionStorage` for the session |
| `redirectUri` | Android: `monolith-<app>://auth` | Return URL for provider sign-in |
| `browserLauncher` | Android: Custom Tab | Opens the provider page |
| `headers`, `httpClient`, `session` | | Extra headers, own Ktor client, `SessionOptions` |

The same `url` and `app` return the same `MonolithClient`. Members: `auth`, `auth(collection)`, `collection<T>(name)`, `files`, `analytics`, `realtime`, `batch(ops)`, `close()`.

## Authentication

`mono.auth` is an `AppAuth` for the app's end users.

| Member | Returns | Notes |
|---|---|---|
| `signUp(email, password, name = null)` | `AppUser` | Creates the account and signs in |
| `signIn(email, password)` | `AppUser` | |
| `signInWithProvider(id, redirectUri = null)` | `String` | Returns the URL to open, and launches it if a `browserLauncher` is set |
| `handleRedirect(uri)` / `handleRedirectAsync(uri)` | `RedirectResult` / `Job` | Call with the URI your redirect activity receives |
| `signOut()` | | Revokes the refresh token |
| `updateProfile(name)` | `AppUser` | |
| `changePassword(old, new)` | | The server ends all sessions |
| `accessToken()` | `String?` | Refreshes first if needed |
| `providers(forceRefresh = false)` | `PublicAuth` | Enabled sign-in methods |
| `state` | `StateFlow<AuthState>` | `Unknown`, `SignedOut`, `SignedIn(user)` |
| `user` | `AppUser?` | Current user |
| `redirecting`, `redirectFailures` | `StateFlow<Boolean>`, `SharedFlow` | Provider flow status and errors |

```kotlin
val user = mono.auth.signUp("ada@example.com", "correct horse", name = "Ada")
mono.auth.state.collect { s ->
    when (s) {
        AuthState.Unknown -> showSplash()
        AuthState.SignedOut -> showLogin()
        is AuthState.SignedIn -> showHome(s.user.displayName)
    }
}
```

`mono.auth("_superusers")` returns an `AuthService` for the system login (`signInWithPassword`, `token`, `setToken`, `signOut`).

## Database

```kotlin
@Serializable data class Task(val id: String = "", val title: String, val done: Boolean = false)

val tasks = mono.collection<Task>("tasks")   // typed
val raw = mono.collection("tasks")           // RecordModel
```

| Method | Returns | HTTP |
|---|---|---|
| `list(ListOptions)` | `ListResult<T>` | `GET .../records` |
| `getOne(id, expand, fields)` | `T` | `GET .../records/{id}` |
| `create(data: T)` or `create(JsonObject)` | `T` | `POST` |
| `update(id, data)` | `T` | `PATCH` (partial) |
| `replace(id, data)` | `T` | `PUT` |
| `delete(id)` | `Boolean` | `DELETE` |
| `count(filter)` | `Long` | `GET .../records:count` |

`ListOptions(page, perPage, cursor, limit, filter, sort, expand, fields, total)`; `total` is `"none"`, `"exact"` or `"estimate"`. `ListResult` has `items`, `total`, `cursor`, `nextCursor`, `prevCursor`.

```kotlin
val first = tasks.list(ListOptions(filter = "done = false", sort = "-created", limit = 20))
val next = first.nextCursor?.let { tasks.list(ListOptions(limit = 20, cursor = it)) }
val created = tasks.create(Task(title = "Buy milk"))
tasks.update(created.id, buildJsonObject { put("done", true) })
tasks.delete(created.id)
```

Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch(listOf(BatchOperation(method, path, body)))` returns `List<BatchResponseItem(status, body)>`.

## Realtime

| Method | Returns |
|---|---|
| `subscribe(listener, filter, ids)` | Unsubscribe `() -> Unit` |
| `subscribe(filter, ids)` / `subscribeFlow(...)` | `Flow<RealtimeEvent<T>>` |
| `onSnapshot(callback, SnapshotOptions)` | Unsubscribe `() -> Unit` |
| `onSnapshot(SnapshotOptions)` | `Flow<QuerySnapshot<T>>` |
| `onDocSnapshot(id, callback, onError)` | Unsubscribe `() -> Unit` |
| `onDocSnapshot(id)` | `Flow<T?>` (`null` once deleted or hidden) |

`RealtimeEvent(action, record, collection, seq)`: `action` is `create`, `update`, `delete` or `leave` (the record stopped matching your filter). `QuerySnapshot(docs, changes)`: `changes` are `DocumentChange(type = ADDED | MODIFIED | REMOVED, doc)`. `SnapshotOptions(filter, sort, limit = 100, onError)`.

```kotlin
tasks.onSnapshot(SnapshotOptions(filter = "done = false", sort = "-created"))
    .collect { snap -> adapter.submit(snap.docs) }
```

## Storage

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data: ByteArray, contentType)` | `StorageObject` |
| `upload(bucket, key, text: String, contentType = "text/plain")` | `StorageObject` |
| `getURL(bucket, key, thumb = null)` | `String` |
| `list(bucket, StorageListOptions(prefix, cursor, limit))` | `StorageListResult(items, nextCursor)` |
| `delete(bucket, key)` | `Boolean` |

`StorageObject(bucket, key, size, contentType, blake3, created, updated)`. Details: [Storage](/build/storage/overview).

## Analytics

```kotlin
mono.analytics.track("checkout_started", buildJsonObject { put("items", 3) })
mono.analytics.flush()
```

Events are queued and sent in batches.

## Errors

Failed calls throw `MonolithException` with the HTTP status, problem `type`, `detail`, `requestId` and field errors. Quote `requestId` when you debug: see [Errors](/reference/errors).

## Limits and status

:::status partial
No instrumented Android tests. Compose has logic tests only. `batch` is not covered by a test against a real server.
:::

## Next steps

- [Kotlin and Android guide](/sdks/kotlin)
- [Query records](/build/database/query)
- [Realtime](/build/database/realtime)
