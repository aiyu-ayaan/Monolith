---
title: Authentication
description: Rust: sign up, sign in, providers and session state.
group: Rust
order: 30
---

Sign users in and out, watch the session and call the API as them.

## Sign up and sign in

```rust
let auth = mono.auth();
auth.sign_up("ada@example.com", "correct horse", Some("Ada")).await?;
let session = auth.sign_in("ada@example.com", "correct horse").await?;
println!("{:?}", auth.user());
```

Both return an `AuthSession`. `auth.user()` is the current user as JSON, `auth.state()` the current `AuthState`.

## Watch the signed-in state

```rust
let mut rx = auth.on_change();                 // tokio::sync::watch::Receiver<AuthState>
while rx.changed().await.is_ok() {
    match &*rx.borrow() {
        AuthState::Unknown => {}
        AuthState::SignedOut => println!("signed out"),
        AuthState::SignedIn(user) => println!("signed in: {user:?}"),
    }
}
```

## Sign in with a provider

The SDK never opens a browser:

```rust
let providers = auth.providers().await?;
let url = auth.provider_sign_in_url("google", "http://127.0.0.1:8765/cb")?;   // must be an allowed redirect URL
// open `url`, catch the redirect on a loopback server, then:
auth.handle_redirect("http://127.0.0.1:8765/cb#token=...&refreshToken=...").await?;
```

## Profile and password

```rust
auth.update_profile("Ada L.").await?;
auth.change_password("correct horse", "battery staple").await?;   // the server ends all sessions
```

## Get the token for your own requests

```rust
let token = auth.access_token().await?;   // Option<String>, refreshed if needed
```

## Sign out

```rust
auth.logout().await?;   // revokes the refresh token on the server
auth.sign_out();        // clears the local session only
```

## Where the session is kept

Memory by default. Use `.session_file("session.json")` or `.session_store(your_store)` on the builder to keep it between runs. `mono.auth_collection("_superusers")` is the system login (`sign_in_with_password`).

## Errors

| Status | Meaning |
|---|---|
| 401 | Wrong email or password, or an expired session |
| 403 | Sign-up is closed for this app |
| 409 | The email is already registered |
| 400 | Validation failed, for example a password shorter than the app's minimum (default 8) |

## Sessions

An access token lasts 15 minutes and a refresh token 30 days. The SDK refreshes shortly before expiry and once after a `401`, one refresh at a time. A rejected refresh token signs the user out. Details: [Sessions and tokens](/build/auth/sessions-and-tokens).

Which sign-in methods exist is configured per app in the Admin UI under **Authentication**: [Authentication overview](/build/auth/overview), [Providers](/build/auth/providers).

## Next steps

- [Read and query](/sdks/rust/query)
- [SDKs overview](/sdks/overview)
