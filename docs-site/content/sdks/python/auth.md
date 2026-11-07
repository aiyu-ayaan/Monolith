---
title: Authentication
description: Python: sign up, sign in, providers and session state.
group: Python
order: 30
---

Sign up, sign in, providers and session state.

`auth = mono.auth()`.

| Method | Returns | Notes |
|---|---|---|
| `sign_up(email, password, name=None)` | `AuthSession` | Creates and signs in |
| `sign_in(email, password)` | `AuthSession` | |
| `sign_out()` | | Revokes the refresh token |
| `update_profile(name)` | `dict` | |
| `change_password(old, new)` | | You are signed out afterwards |
| `access_token()` | `str \| None` | Refreshes first if needed |
| `providers()` | `dict` | Enabled sign-in methods |
| `provider_sign_in_url(provider, redirect_uri)` | `str` | You open it yourself |
| `handle_redirect(url_or_fragment)` | `AuthSession` | Raises `MonolithError` on `#error=` |
| `me()`, `refresh()` | | |
| `user`, `state`, `token` | properties | `state.status` is `AuthStatus.UNKNOWN`, `SIGNED_OUT` or `SIGNED_IN` |
| `on_state_change(cb)`, `on_change(cb)` | unsubscribe | State, or the token string |

```python
auth.sign_up("ada@example.com", "correct horse", name="Ada")
auth.on_state_change(lambda st: print(st.status, st.user))
```

`mono.auth("system")` is the superuser login (`sign_in`, `install`). Superuser tokens are never stored. Session stores: `MemorySessionStore`, `FileSessionStore(path)`, or your own `load/save/clear`.

## Next steps

- [Database](/sdks/python/database)
- [SDKs overview](/sdks/overview)
