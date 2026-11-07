---
title: Storage
description: Kotlin and Android: upload, download and list files.
group: Kotlin and Android
order: 60
---

Upload, download and list files.

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data: ByteArray, contentType)` | `StorageObject` |
| `upload(bucket, key, text: String, contentType = "text/plain")` | `StorageObject` |
| `getURL(bucket, key, thumb = null)` | `String` |
| `list(bucket, StorageListOptions(prefix, cursor, limit))` | `StorageListResult(items, nextCursor)` |
| `delete(bucket, key)` | `Boolean` |

`StorageObject(bucket, key, size, contentType, blake3, created, updated)`. Details: [Storage](/build/storage/overview).

## Next steps

- [Analytics](/sdks/kotlin/analytics)
- [SDKs overview](/sdks/overview)
