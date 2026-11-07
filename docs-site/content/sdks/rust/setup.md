---
title: Set up
description: Rust: install, initialize and configure the client.
group: Rust
order: 20
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

## Limits and status

:::status partial
No analytics service. The batch operation shape (`op`, `collection`) differs from the other SDKs (`method`, `path`); treat `batch` as unverified.
:::

## Next steps

- [Authentication](/sdks/rust/auth)
- [SDKs overview](/sdks/overview)
