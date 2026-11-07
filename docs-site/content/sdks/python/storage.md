---
title: Storage
description: Python: upload, download and list files.
group: Python
order: 60
---

Upload, download and list files.

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, content_type=)` | `StorageObject` (`data`: `bytes`, `str` or file object) |
| `get_url(bucket, key, thumb=)` | `str` |
| `list(bucket, prefix=, cursor=, limit=)` | `StorageListResult` |
| `delete(bucket, key)` | `bool` |

## Next steps

- [Analytics](/sdks/python/analytics)
- [SDKs overview](/sdks/overview)
