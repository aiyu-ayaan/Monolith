---
title: Storage
description: Dart and Flutter: upload, download URLs, list and delete files.
group: Dart and Flutter
order: 70
---

Upload files, build download URLs, list and delete objects.

## Upload

```dart
final obj = await mono.files.upload('avatars', 'users/$userId.png', bytes, contentType: 'image/png');
print(obj.size);
```

`data` can be a `Uint8List` or a `String`. The server checks the bucket's size and type limits (`413`, `400`).

## Get a download URL

```dart
final url = mono.files.getURL('avatars', 'users/$userId.png', thumb: '64x64');
```

`getURL` only builds the address and adds no credential. A **public bucket** URL works with `Image.network`. A private bucket needs an `Authorization: Bearer ${await mono.auth().accessToken()}` header. `thumb` must be a size declared on the bucket: [Thumbnails](/build/storage/thumbnails).

## List objects

```dart
String? cursor;
do {
  final page = await mono.files.list('docs', prefix: 'reports/', limit: 100, cursor: cursor);
  for (final o in page.items) { print('${o.key} ${o.size}'); }
  cursor = page.nextCursor;
} while (cursor != null);
```

## Delete

```dart
await mono.files.delete('docs', 'reports/2026/q3.pdf');
```

## Keys and limits

Keys are 1 to 512 bytes and may contain `/`, but no empty, `.` or `..` segments. The SDK URL-encodes the key; the server decodes `%2F` back to `/`. Buckets are made in the Admin UI or with REST: [Storage](/build/storage/overview), [Limits and quotas](/build/storage/limits-and-quotas).

## Next steps

- [UI components](/sdks/dart-flutter/ui)
- [SDKs overview](/sdks/overview)
