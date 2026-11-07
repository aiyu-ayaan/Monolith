# Python SDK (PyPI: monolith-client)

Sync client built on `httpx`. Needs Python 3.10+.

## Quick start

```python
import monolith

mono = monolith.init(url="https://api.example.com", app="shop")   # client + session storage + auto refresh
print(mono.collection("tasks").list())                              # records
```

`monolith.init(...)` (same as `Monolith.init(...)`) returns a ready `MonolithClient`. Pass
`storage="session.json"` (or a `FileSessionStore`) to keep the session between runs; the default is memory.

## Auth for app users

```python
auth = mono.auth()                         # app users ("users"); mono.auth("system") is the superuser login

print(auth.providers())                    # {"emailPassword": {...}, "providers": [{"id": "google", ...}]}
auth.sign_up("ada@example.com", "s3cret-pass", name="Ada")   # or auth.sign_in(email, password)
print(auth.user)                           # current user dict, or None
auth.update_profile("Ada L.")
auth.change_password("s3cret-pass", "n3w-secret-pass")       # server ends all sessions: you are signed out, sign in again
token = auth.access_token()                # for your own calls; refreshes first if it is about to expire
auth.on_state_change(lambda st: print(st.status, st.user)) # AuthStatus.UNKNOWN / SIGNED_OUT / SIGNED_IN
auth.sign_out()                            # revokes the refresh token on the server
```

- The access token is refreshed 30 s before it expires and once after a `401`. Concurrent calls share one refresh.
  A rejected refresh token signs the client out.
- `auth.on_change(cb)` is the older listener that receives the access token string (or `None`).
- Errors are `MonolithError` (`.status`, `.type`, `.detail`, `.get_field_error(name)`): 401 bad credentials, 409 email taken, 403 sign-up closed.

### Provider (OAuth) sign-in for scripts and desktop apps

The SDK never opens a browser. Build the URL, open it yourself, catch the redirect on a loopback server, hand it back:

```python
url = auth.provider_sign_in_url("google", "http://127.0.0.1:8765/cb")   # redirect must be in the app's allowed redirects
# ... open `url`; your handler at /cb receives the browser, whose fragment carries the session
auth.handle_redirect("http://127.0.0.1:8765/cb#token=...&refreshToken=...&expiresAt=...")  # raises MonolithError on #error=
```

Browsers do not send the fragment to servers: serve a tiny page at `/cb` that posts `location.hash` back to your process.

### Session stores

`SessionStore` is any object with `load() -> dict | None`, `save(dict)`, `clear()`.
`MemorySessionStore` (default) and `FileSessionStore(path)` (atomic write, mode 0600) are included.
Superuser tokens (`mono.auth("system")`) are never stored.
