---
title: Storage
description: Dart and Flutter: upload, download and list files.
group: Dart and Flutter
order: 60
---

Upload, download and list files.

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, {contentType})` | `Future<StorageObject>` |
| `getURL(bucket, key, {thumb})` | `String` |
| `list(bucket, {prefix, cursor, limit})` | `Future<StorageListResult>` (`items`, `nextCursor`) |
| `delete(bucket, key)` | `Future<bool>` |

`StorageObject`: `bucket`, `key`, `size`, `contentType`, `blake3`, `created`, `updated`.

## Next steps

- [UI components](/sdks/dart-flutter/ui)
- [SDKs overview](/sdks/overview)
