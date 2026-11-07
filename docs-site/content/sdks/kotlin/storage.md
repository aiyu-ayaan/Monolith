---
title: Storage
description: Kotlin and Android: upload, download URLs, list and delete files.
group: Kotlin and Android
order: 70
---

Upload files, build download URLs, list and delete objects.

## Upload

```kotlin
val obj = mono.files.upload("avatars", "users/$userId.png", bytes, contentType = "image/png")
mono.files.upload("notes", "hello.txt", "Hello")   // text overload, text/plain
```

`bytes` is a `ByteArray`: the SDK sends the whole body at once, so prefer small files on a phone. The server checks the bucket's size and type limits (`413`, `400`).

## Get a download URL

```kotlin
val url = mono.files.getURL("avatars", "users/$userId.png", thumb = "64x64")
```

`getURL` only builds the address and adds no credential. A **public bucket** URL loads in Coil or Glide as it is. A private bucket needs `Authorization: Bearer ${mono.auth.accessToken()}`. `thumb` must be a size declared on the bucket: [Thumbnails](/build/storage/thumbnails).

## List objects

```kotlin
var cursor: String? = null
do {
    val page = mono.files.list("docs", StorageListOptions(prefix = "reports/", limit = 100, cursor = cursor))
    page.items.forEach { println("${it.key} ${it.size}") }
    cursor = page.nextCursor
} while (cursor != null)
```

## Delete

```kotlin
mono.files.delete("docs", "reports/2026/q3.pdf")
```

## Keys and limits

Keys are 1 to 512 bytes and may contain `/`, but no empty, `.` or `..` segments. The SDK does not escape the key, so encode special characters yourself. Buckets are made in the Admin UI or with REST: [Storage](/build/storage/overview), [Limits and quotas](/build/storage/limits-and-quotas).

## Next steps

- [Analytics](/sdks/kotlin/analytics)
- [SDKs overview](/sdks/overview)
