---
title: Guide
description: Async client on tokio and reqwest.
group: Rust
order: 14
---

```rust
use monolith_client::Monolith;

let mono = Monolith::init("https://api.example.com", "shop")?;
mono.auth().sign_in("ada@example.com", "correct horse").await?;
let tasks = mono.records("tasks").list(None).await?;
```

For more control use `Client::builder(url).app("shop").session_file("session.json").build()?`; `session_store(...)` takes any `SessionStore`. The token refreshes before expiry and once after a `401` with a single flight.

Auth: `providers`, `sign_up`, `sign_in`, `refresh`, `me`, `update_profile`, `change_password`, `access_token`, `provider_sign_in_url`, `handle_redirect`, `on_change()` (a `watch::Receiver<AuthState>`), `sign_out`.

## Limits and status

:::status partial
Not published to crates.io. Session storage is memory or a JSON file.
:::

## Next steps

- [Sessions and tokens](/build/auth/sessions-and-tokens)
