---
title: Storage
description: TypeScript: upload, download and list files.
group: TypeScript
order: 60
---

Upload, download and list files.

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, { contentType? })` | `StorageObject` |
| `getURL(bucket, key, { thumb? })` | `string` |
| `list(bucket, { prefix?, cursor?, limit? })` | `{ items, nextCursor? }` |
| `delete(bucket, key)` | `boolean` |

`data` is `Blob`, `ArrayBuffer`, `Uint8Array` or `string`. `StorageObject`: `bucket`, `key`, `size`, `contentType`, `blake3`, `created`, `updated`.

## Next steps

- [Analytics](/sdks/typescript/analytics)
- [SDKs overview](/sdks/overview)
