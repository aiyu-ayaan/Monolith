---
title: Storage
description: Rust: upload, download and list files.
group: Rust
order: 60
---

Upload, download and list files.

`mono.files()`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, content_type: Option<&str>)` | `ObjectInfo` |
| `download(bucket, key)` / `download_with_thumb(.., thumb)` | `Bytes` |
| `get_url(bucket, key)` / `get_url_with_thumb(..)` | `String` |
| `list(bucket, prefix, limit, cursor)` | `ObjectPage` (`items`, `next_cursor`) |
| `delete(bucket, key)` | `()` |

## Next steps

- [Errors](/sdks/rust/errors)
- [SDKs overview](/sdks/overview)
