---
title: Storage
description: Python: upload, download URLs, list and delete files.
group: Python
order: 70
---

Upload files, build download URLs, list and delete objects.

## Upload

```python
obj = mono.files.upload("avatars", f"users/{user_id}.png", open("me.png", "rb"), content_type="image/png")
print(obj.size, obj.blake3)
```

`data` can be `bytes`, `str` or a file object. The SDK reads it fully into memory before sending. The server checks the bucket's size and type limits (`413`, `400`).

## Get a download URL

```python
url = mono.files.get_url("avatars", f"users/{user_id}.png", thumb="64x64")
```

`get_url` only builds the address and adds no credential. It works as it is for a **public bucket**. A private bucket needs an `Authorization: Bearer ...` header: use `mono.auth().access_token()`. `thumb` must be a size declared on the bucket: [Thumbnails](/build/storage/thumbnails).

## List objects

```python
cursor = None
while True:
    page = mono.files.list("docs", prefix="reports/", limit=100, cursor=cursor)
    for o in page.items:
        print(o.key, o.size)
    cursor = page.next_cursor
    if not cursor:
        break
```

## Delete

```python
mono.files.delete("docs", "reports/2026/q3.pdf")
```

## Keys and limits

Keys are 1 to 512 bytes and may contain `/`, but no empty, `.` or `..` segments. The SDK URL-encodes the key. Buckets are made in the Admin UI or with REST: [Storage](/build/storage/overview), [Limits and quotas](/build/storage/limits-and-quotas).

## Next steps

- [Analytics](/sdks/python/analytics)
- [SDKs overview](/sdks/overview)
