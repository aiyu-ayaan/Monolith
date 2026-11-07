---
title: Rust
description: Async client on tokio and reqwest.
group: Platform guides
order: 240
---

```rust
use monolith_client::Monolith;

let mono = Monolith::init("https://api.example.com", "shop")?;
mono.auth().sign_in("ada@example.com", "correct horse").await?;
let tasks = mono.records("tasks").list(None).await?;
```

For more control use `Client::builder(url).app("shop").session_file("session.json").build()?`; `session_store(...)` takes any `SessionStore`. The token refreshes before expiry and once after a `401` with a single flight.


## Auth, data, files and realtime

These work the same in every language and live on shared pages: [Authentication](/sdks/auth), [Read and query](/sdks/query), [Write data](/sdks/write), [Realtime](/sdks/realtime) and [Storage](/sdks/storage).

## Limits and status

:::status partial
Not published to crates.io. Session storage is memory or a JSON file.
:::

## Next steps

- [Sessions and tokens](/build/auth/sessions-and-tokens)
