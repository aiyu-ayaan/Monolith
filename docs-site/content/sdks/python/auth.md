---
title: Authentication
description: Python: sign up, sign in, providers and session state.
group: Python
order: 30
---

Sign users in and out, watch the session and call the API as them.

## Sign up and sign in

```python
auth = mono.auth()
auth.sign_up("ada@example.com", "correct horse", name="Ada")
auth.sign_in("ada@example.com", "correct horse")
print(auth.user)   # {"id": ..., "email": ..., "name": ..., "providers": [...]}
```

Both return an `AuthSession` (`token`, `user`, and the refresh details).

## Watch the signed-in state

```python
auth.on_state_change(lambda st: print(st.status, st.user))   # AuthStatus.UNKNOWN / SIGNED_OUT / SIGNED_IN
```

`auth.state`, `auth.user` and `auth.token` hold the current values.

## Sign in with a provider

The SDK never opens a browser. Build the URL, open it, catch the redirect on a loopback server and hand the URL back:

```python
print(auth.providers()["providers"])    # [{"id": "google", "name": "Google", ...}]
url = auth.provider_sign_in_url("google", "http://127.0.0.1:8765/cb")   # must be an allowed redirect URL
# open `url`; your /cb handler receives the browser
auth.handle_redirect("http://127.0.0.1:8765/cb#token=...&refreshToken=...&expiresAt=...")   # MonolithError on #error=
```

Browsers do not send the URL fragment to servers. Serve a small page at `/cb` that posts `location.hash` back to your process.

## Profile and password

```python
auth.update_profile("Ada L.")
auth.change_password("correct horse", "battery staple")   # the server ends all sessions: sign in again
```

## Get the token for your own requests

```python
token = auth.access_token()   # refreshes first if needed
```

## Sign out

```python
auth.sign_out()   # revokes the refresh token on the server
```

## Where the session is kept

Memory by default. Pass `storage="session.json"` or a `FileSessionStore` to `monolith.init` to keep it between runs (atomic write, mode 0600). A custom store needs `load()`, `save(dict)` and `clear()`. Superuser tokens from `mono.auth("system")` are never stored.

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

- [Read and query](/sdks/python/query)
- [SDKs overview](/sdks/overview)
