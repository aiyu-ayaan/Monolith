---
title: Rust API
description: Every type and method of the monolith-client Rust crate: init, auth, database, realtime, storage.
group: API reference
order: 50
---

Crate `monolith-client` on Tokio and `reqwest`. Calls are `async` and return `monolith_client::Result<T>`.

## Initialize

```rust
use monolith_client::Monolith;
let mono = Monolith::init("https://api.example.com", "shop")?;   // memory session, auto refresh
```

For options use the builder:

```rust
let mono = Monolith::builder("https://api.example.com", "shop")
    .session_file("session.json")
    .build()?;
```

`ClientBuilder`: `base_url`, `app`, `token`, `api_key`, `header(k, v)?`, `timeout(Duration)`, `realtime_backoff(base, max)`, `session_store(impl SessionStore)`, `session_file(path)`, `build()`. `Client` also has `auth()`, `auth_collection(name)`, `collection::<T>(name)`, `records(name)`, `files()`, `batch(&[BatchOp])`, `token()`, `set_token()`, `api_key()`, `set_api_key()`.

## Authentication

`mono.auth()` returns an `AuthService` for app users.

| Method | Returns |
|---|---|
| `sign_up(email, password, name: Option<&str>)` | `AuthSession` |
| `sign_in(email, password)` | `AuthSession` |
| `sign_out()` / `logout().await` | local clear / revoke on the server |
| `update_profile(name)` / `me()` | user `serde_json::Value` |
| `change_password(old, new)` | `()` |
| `access_token()` | `Option<String>`, refreshed if needed |
| `providers()` | `PublicAuth` |
| `provider_sign_in_url(provider, redirect_uri)` | `String` |
| `handle_redirect(url_or_fragment)` | `AuthSession` |
| `user()`, `state()`, `token()` | current values |
| `on_change()` | `watch::Receiver<AuthState>` (`Unknown`, `SignedOut`, `SignedIn(user)`) |

`mono.auth_collection("_superusers")` is the system login (`sign_in_with_password`, `install_required`, `install`). Session stores: `MemorySessionStore`, `FileSessionStore::new(path)`, or your own `SessionStore`.

## Database

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

## Realtime

| Method | Item |
|---|---|
| `subscribe(SubscribeOptions::new().filter(..).ids(..))` | `Result<RealtimeEvent<T>>` (`action`, `record`, `collection`, `seq`) |
| `on_snapshot(SnapshotOptions::new().filter(..).sort(..).limit(..))` | `Result<QuerySnapshot>` (`docs`, `changes`) |
| `on_doc_snapshot(id)` | `Result<Option<Record>>` |

`on_snapshot` and `on_doc_snapshot` exist on `Collection<Record>`. The return value is a `Subscription`: call `recv().await`, or use it as a `Stream`. Dropping it closes the connection.

```rust
let mut sub = mono.records("tasks").on_snapshot(SnapshotOptions::new().filter("done = false"));
while let Some(snap) = sub.recv().await {
    println!("{} docs", snap?.docs.len());
}
```

## Storage

`mono.files()`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, content_type: Option<&str>)` | `ObjectInfo` |
| `download(bucket, key)` / `download_with_thumb(.., thumb)` | `Bytes` |
| `get_url(bucket, key)` / `get_url_with_thumb(..)` | `String` |
| `list(bucket, prefix, limit, cursor)` | `ObjectPage` (`items`, `next_cursor`) |
| `delete(bucket, key)` | `()` |

## Errors

`Error` variants: `Api { status, message, problem }`, `Network`, `Serialization`, `Url`, `Config`, `Realtime`. Use `err.status()` and `err.problem()` (with `get_field_error`). See [Errors](/reference/errors).

## Limits and status

:::status partial
No analytics service. The batch operation shape (`op`, `collection`) differs from the other SDKs (`method`, `path`); treat `batch` as unverified.
:::

## Next steps

- [Rust guide](/sdks/rust)
- [Server quickstart](/get-started/quickstart-server)
