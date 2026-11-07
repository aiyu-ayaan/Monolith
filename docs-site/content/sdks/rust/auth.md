---
title: Authentication
description: Rust: sign up, sign in, providers and session state.
group: Rust
order: 30
---

Sign up, sign in, providers and session state.

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

## Next steps

- [Database](/sdks/rust/database)
- [SDKs overview](/sdks/overview)
