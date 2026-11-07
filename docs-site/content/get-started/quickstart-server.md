---
title: Quickstart for servers and scripts
description: Create an app and a collection, then read and write records from curl, Python or Rust.
order: 40
---

This quickstart uses no UI. You create an app and a collection with `curl`, then read and write records from a script. It is also the fastest way to learn the REST API that every SDK wraps.

## What you will do

1. Run the server and sign in as a superuser.
2. Create an app and a collection.
3. Create, list and update records with `curl`, Python and Rust.

## Before you start

- A running server. See [Self-hosting](/get-started/self-host). The examples use `http://127.0.0.1:8090`.
- A superuser account (from the one-time setup link or `monolith superuser create`).
- `curl` and `jq`.

```bash
export BASE=http://127.0.0.1:8090
```

## 1. Sign in as a superuser

```bash
TOKEN=$(curl -s "$BASE/api/v1/system/auth/login" \
  -H 'content-type: application/json' \
  -d '{"email":"admin@example.com","password":"your-password"}' | jq -r .token)
```

The response is `{"token": "...", "expires": "..."}`. A superuser token lives for one hour. It bypasses every access rule, so use it for setup and server-side admin work only.

## 2. Create an app and a collection

```bash
curl -s "$BASE/api/v1/system/apps" \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"name":"shop"}'

curl -s "$BASE/api/v1/apps/shop/collections" \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{
    "name": "tasks",
    "fields": [
      {"name": "title", "type": "text", "required": true},
      {"name": "done",  "type": "bool"}
    ],
    "rules": {"list": "", "view": "", "create": "", "update": "", "delete": null}
  }'
```

The rules here mean: anyone may list, view, create and update; only superusers may delete (`null`). That is convenient for a tutorial and wrong for most real data. See [Rules](/build/database/rules).

## 3. Create and read records

This part needs no token, because the rules above are public.

:::tabs
```bash title="curl"
# create
curl -s "$BASE/api/v1/apps/shop/collections/tasks/records" \
  -H 'content-type: application/json' \
  -d '{"title":"Write the docs","done":false}'

# list, newest first
curl -s "$BASE/api/v1/apps/shop/collections/tasks/records?sort=-created&limit=10"
```
```python title="Python"
from monolith import MonolithClient

mono = MonolithClient(url="http://127.0.0.1:8090", app="shop")
tasks = mono.collection("tasks")

created = tasks.create({"title": "Write the docs", "done": False})
page = tasks.list(sort="-created", limit=10)
for task in page.items:
    print(task["id"], task["title"])
```
```rust title="Rust"
use monolith_client::{Client, ListOptions};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder("http://127.0.0.1:8090").app("shop").build()?;
    let tasks = client.records("tasks");

    tasks.create(&json!({"title": "Write the docs", "done": false})).await?;
    let page = tasks
        .list(Some(&ListOptions::new().sort("-created").limit(10)))
        .await?;
    for task in page.items {
        println!("{} {:?}", task.id, task.fields.get("title"));
    }
    Ok(())
}
```
```json title="Response"
{
  "items": [
    {
      "id": "01jbaa1cy0fqqr28t5cy4tqkff",
      "created": "2026-11-08T10:15:30.123456Z",
      "updated": "2026-11-08T10:15:30.123456Z",
      "_v": 1,
      "title": "Write the docs",
      "done": false
    }
  ],
  "next": null
}
```
:::

The list response has `items` and `next`. `next` is `null` on the last page; otherwise pass it back as `cursor`. See [Query records](/build/database/query).

## 4. Update safely

Every record response carries an `ETag` header equal to the record's `_v`. Send it back as `If-Match` and the server refuses the write if someone else changed the record first.

```bash
ID=01jbaa1cy0fqqr28t5cy4tqkff
curl -si -X PATCH "$BASE/api/v1/apps/shop/collections/tasks/records/$ID" \
  -H 'content-type: application/json' -H 'if-match: "1"' \
  -d '{"done":true}'
```

A stale `If-Match` returns `412` with problem type `precondition-failed`. See [Read and write records](/build/database/read-and-write).

## 5. Use an API key for server-to-server calls

A superuser token expires in an hour. For a long-running script, create a scoped API key instead:

```bash
curl -s "$BASE/api/v1/apps/shop/api-keys" \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"name":"worker","scopes":["records:read","records:write:tasks"]}'
```

The `key` field in the response (it starts with `mk_`) is shown once. Send it as `Authorization: Bearer mk_...`. A key is **anonymous to your rules**, narrowed by its scopes. Only a key with the `records:admin` scope bypasses rules. Keep keys on servers, never in a web page or an app.

## Limits and status

- The Python and Rust packages are not published to a registry yet; use them from the repository (`sdks/python`, `sdks/rust`).
- The Python SDK's `list` result exposes the next-page cursor under `raw["next"]`; its `next_cursor` attribute reads a differently named key and stays empty.
- Python `count()` and `batch()` do not match the server's responses yet. Use curl for those, or the Rust SDK.

:::status partial
SDK method names for sign-in are changing while the auth API is added. The record and realtime methods used above exist today.
:::

## Next steps

- [Collections and schema](/build/database/collections-and-schema): all field types.
- [Rules](/build/database/rules): who can read and write what.
- [REST API reference](/reference/rest-api): every endpoint.
