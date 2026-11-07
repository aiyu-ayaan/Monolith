---
name: monolith-sdk-python
description: Use the Monolith Python SDK (monolith-client, import monolith): synchronous client, app-user auth, collections, on_snapshot live queries, files and analytics. Use when writing Python scripts or servers against a Monolith app.
---

# Python SDK

Not on PyPI yet: use `sdks/python`. Synchronous (`httpx`), Python 3.10+. Records are plain `dict`s.

```python
import monolith
mono = monolith.init(url=BASE, app="myapp", storage="session.json")   # storage: path or SessionStore; default memory
auth = mono.auth()                                                      # app users; mono.auth("system") = superuser login
auth.sign_in(email, password)                                           # sign_up(email, password, name=..), sign_out(), access_token()
auth.on_state_change(lambda st: print(st.status, st.user))              # AuthStatus.UNKNOWN / SIGNED_OUT / SIGNED_IN
tasks = mono.collection("tasks")
page = tasks.list(filter="done = false", sort="-created", limit=20, cursor=c)   # page.items, page.next_cursor
tasks.create({"title": "x"}); tasks.update(id, {"done": True}); tasks.delete(id)
stop = tasks.on_snapshot(lambda snap: print(len(snap.docs)), filter="done = false")   # callbacks run on a background thread
mono.files.upload("docs", "a.pdf", open("a.pdf", "rb"), content_type="application/pdf")
mono.analytics.track("job_done", {"rows": 3}); mono.analytics.flush()
mono.close()
```

- Provider sign-in: the SDK never opens a browser. `auth.provider_sign_in_url(provider, redirect_uri)`, open it, catch the redirect on a loopback page that posts `location.hash` back, then `auth.handle_redirect(url)`.
- Errors: `MonolithError` (`status`, `type`, `detail`, `request_id`, `errors`, `get_field_error(name)`).
- `key="mk_..."` for server-side use. `files.get_url` adds no credential: public buckets only.
