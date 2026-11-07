---
title: Python
description: Synchronous client built on httpx for scripts and servers.
group: Platform guides
order: 230
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


For provider sign-in the SDK never opens a browser: build the URL, open it yourself, catch the redirect on a loopback page that returns the URL fragment, then call `handle_redirect`.

## Auth, data, files and realtime

These work the same in every language and live on shared pages: [Authentication](/sdks/auth), [Read and query](/sdks/query), [Write data](/sdks/write), [Realtime](/sdks/realtime) and [Storage](/sdks/storage).

## Limits and status

:::status partial
Not published to PyPI. Realtime is available through `collection(...).on_snapshot` and `subscribe`.
:::

## Next steps

- [Sessions and tokens](/build/auth/sessions-and-tokens)
