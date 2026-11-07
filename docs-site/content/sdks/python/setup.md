---
title: Set up
description: Python: install, initialize and configure the client.
group: Python
order: 20
---

Package `monolith-client` (import `monolith`), a synchronous client on `httpx`. Python 3.10+. Realtime callbacks run on background threads.

## Initialize

```python
import monolith
mono = monolith.init(url="https://api.example.com", app="shop", storage="session.json")
```

| Parameter | Default | Meaning |
|---|---|---|
| `url`, `app` | required | Server and app id |
| `key` | `None` | An `mk_` API key |
| `storage` | memory | A `SessionStore` or a file path (`FileSessionStore`) |
| `headers`, `timeout`, `client` | `timeout=30.0` | Extra headers, request timeout, own `httpx.Client` |

`MonolithClient` members: `auth(collection="users")`, `collection(name)`, `files`, `analytics`, `realtime`, `batch(ops)`, `close()`. `monolith.create_client(...)` builds a client without a session store by default.

## Limits and status

:::status partial
Sync only; no `async` client. The SDK never opens a browser for provider sign-in.
:::

## Next steps

- [Authentication](/sdks/python/auth)
- [SDKs overview](/sdks/overview)
