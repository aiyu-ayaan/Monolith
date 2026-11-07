---
title: Guide
description: Synchronous client built on httpx for scripts and servers.
group: Python
order: 13
---

Python 3.10+. The client is synchronous.

```python
import monolith
mono = monolith.init(url="https://api.example.com", app="shop", storage="session.json")
auth = mono.auth()
auth.sign_in("ada@example.com", "correct horse")
page = mono.collection("tasks").list(filter="done = false", limit=20)
```

`storage` is a file path or store; the default is memory. The access token refreshes 30 seconds before expiry and once after a `401`; concurrent calls share one refresh. A rejected refresh token signs the client out.

Auth calls: `providers()`, `sign_up`, `sign_in`, `sign_out`, `refresh`, `me`, `update_profile`, `change_password`, `access_token`, `provider_sign_in_url`, `handle_redirect`, `on_state_change`. Errors are `MonolithError` with `.status`, `.type`, `.detail`.

For provider sign-in the SDK never opens a browser: build the URL, open it yourself, catch the redirect on a loopback page that returns the URL fragment, then call `handle_redirect`.

## Limits and status

:::status partial
Not published to PyPI. Realtime is available through `collection(...).on_snapshot` and `subscribe`.
:::

## Next steps

- [Sessions and tokens](/build/auth/sessions-and-tokens)
