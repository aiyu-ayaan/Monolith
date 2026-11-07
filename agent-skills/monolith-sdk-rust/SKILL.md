---
name: monolith-sdk-rust
description: Use the Monolith Rust client (monolith-client crate): Monolith::init, builder, app-user auth, Collection<T>, on_snapshot streams and files. Use when writing Rust code against a Monolith app.
---

# Rust SDK

Not on crates.io yet: use `sdks/rust`. Tokio plus `reqwest`; every call is `async` and returns `monolith_client::Result<T>`.

```rust
use monolith_client::{Monolith, ListOptions, SnapshotOptions};
let mono = Monolith::init(BASE, "myapp")?;   // memory session, auto refresh
// options: Monolith::builder(BASE, "myapp").session_file("session.json").api_key("mk_...").timeout(d).build()?
mono.auth().sign_in(email, password).await?;          // sign_up(email, password, Some("name")), logout().await, access_token().await
let mut rx = mono.auth().on_change();                  // watch::Receiver<AuthState>
let tasks = mono.records("tasks");                     // Collection<Record>; mono.collection::<T>("tasks") for your serde type
let page = tasks.list(Some(&ListOptions::new().filter("done = false").sort("-created").limit(20))).await?;  // page.items, page.next
tasks.create(&json!({"title": "x"})).await?; tasks.update(&id, &json!({"done": true})).await?; tasks.delete(&id).await?;
let mut sub = tasks.on_snapshot(SnapshotOptions::new().filter("done = false"));   // Subscription: recv().await or Stream
mono.files().upload("docs", "a.pdf", bytes, Some("application/pdf")).await?;
```

- `on_snapshot` and `on_doc_snapshot` exist on `Collection<Record>` only; `subscribe(SubscribeOptions)` works for `Collection<T>`. Dropping a `Subscription` closes the stream.
- Provider sign-in only builds the URL: `provider_sign_in_url`, then `handle_redirect`.
- Errors: `Error::Api { status, message, problem }`, `Network`, `Serialization`, `Url`, `Config`, `Realtime`; use `e.status()`, `e.problem()`.
- No analytics service. `batch` uses `{op, collection}` where other SDKs use `{method, path}`: treat it as unverified.
