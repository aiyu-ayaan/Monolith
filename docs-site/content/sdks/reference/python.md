---
title: Python API
description: Every class and method of the monolith-client Python SDK: init, auth, database, realtime, storage, analytics.
group: API reference
order: 40
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

## Authentication

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

## Database

```python
tasks = mono.collection("tasks")
```

| Method | Returns | HTTP |
|---|---|---|
| `list(page=, per_page=, cursor=, limit=, filter=, sort=, expand=, fields=, total=)` or `list(ListOptions)` | `ListResult` | `GET .../records` |
| `get_one(id, expand=, fields=)` | record `dict` | `GET .../records/{id}` |
| `create(data)` | `dict` | `POST` |
| `update(id, data)` | `dict` | `PATCH` |
| `replace(id, data)` | `dict` | `PUT` |
| `delete(id)` | `bool` | `DELETE` |
| `count(filter=)` | `int` | `GET .../records:count` |

`ListResult` has `items`, `total`, `cursor`, `next_cursor`, `prev_cursor`, `raw`.

```python
page = tasks.list(filter="done = false", sort="-created", limit=20)
nxt = tasks.list(limit=20, cursor=page.next_cursor) if page.next_cursor else None
t = tasks.create({"title": "Buy milk"})
tasks.update(t["id"], {"done": True})
tasks.delete(t["id"])
```

Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch([{"method": "POST", "path": "...", "body": {...}}])` returns the response list.

## Realtime

| Method | Callback gets | Returns |
|---|---|---|
| `subscribe(listener, filter=, ids=)` | `RealtimeEvent(action, record, collection, seq)` | unsubscribe |
| `on_snapshot(callback, filter=, sort=, limit=, on_error=)` | `QuerySnapshot(docs, changes)` | unsubscribe |
| `on_doc_snapshot(id, callback, on_error=)` | record or `None` | unsubscribe |

`action` is `create`, `update`, `delete` or `leave`. `DocumentChange.type` is `"added"`, `"modified"` or `"removed"`. Call `mono.close()` to stop all streams.

```python
stop = tasks.on_snapshot(lambda snap: print(len(snap.docs)), filter="done = false")
```

## Storage

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, content_type=)` | `StorageObject` (`data`: `bytes`, `str` or file object) |
| `get_url(bucket, key, thumb=)` | `str` |
| `list(bucket, prefix=, cursor=, limit=)` | `StorageListResult` |
| `delete(bucket, key)` | `bool` |

## Analytics

```python
mono.analytics.track("job_done", {"rows": 120})
mono.analytics.flush()
```

## Errors

`MonolithError`: `.status`, `.type`, `.detail`, `.requestId`, `.get_field_error(name)`.

## Limits and status

:::status partial
Sync only; no `async` client. The SDK never opens a browser for provider sign-in.
:::

## Next steps

- [Python guide](/sdks/python)
- [Server quickstart](/get-started/quickstart-server)
