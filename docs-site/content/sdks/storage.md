---
title: Storage
description: Upload files, build download URLs, list and delete objects.
group: Use the SDKs
order: 60
---

Files live in buckets. An upload is one `PUT`; identical content is stored once. Create buckets in the Admin UI or with REST: [Storage](/build/storage/overview).

## Upload

:::tabs
```ts title="TypeScript"
const obj = await mono.files.upload("avatars", `users/${userId}.png`, file, { contentType: "image/png" });
console.log(obj.size, obj.blake3);
```
```kotlin title="Kotlin"
val obj = mono.files.upload("avatars", "users/$userId.png", bytes, contentType = "image/png")
mono.files.upload("notes", "hello.txt", "Hello")   // text overload, text/plain
```
```dart title="Dart"
final obj = await mono.files.upload('avatars', 'users/$userId.png', bytes, contentType: 'image/png');
print(obj.size);
```
```python title="Python"
obj = mono.files.upload("avatars", f"users/{user_id}.png", open("me.png", "rb"), content_type="image/png")
print(obj.size, obj.blake3)
```
```rust title="Rust"
let obj = mono.files()
    .upload("avatars", &format!("users/{user_id}.png"), png_bytes, Some("image/png"))
    .await?;
println!("{} {}", obj.size, obj.etag);
```
:::

The server checks the bucket's size and type limits and answers `413` or `400` when they fail. Kotlin and Python send the whole body from memory, so prefer small files there.

## Get a download URL

:::tabs
```ts title="TypeScript"
img.src = mono.files.getURL("avatars", `users/${userId}.png`);
img.src = mono.files.getURL("avatars", `users/${userId}.png`, { thumb: "64x64" });
```
```kotlin title="Kotlin"
val url = mono.files.getURL("avatars", "users/$userId.png", thumb = "64x64")
```
```dart title="Dart"
final url = mono.files.getURL('avatars', 'users/$userId.png', thumb: '64x64');
```
```python title="Python"
url = mono.files.get_url("avatars", f"users/{user_id}.png", thumb="64x64")
```
```rust title="Rust"
let url = mono.files().get_url("avatars", "users/u1.png");
let small = mono.files().get_url_with_thumb("avatars", "users/u1.png", Some("64x64"));
```
:::

A URL carries no credential. It works as it is for a **public bucket**, for example in an image view. For a private bucket send `Authorization: Bearer <access token>`, which you get from `auth.accessToken()` (`access_token()` in Python and Rust). `thumb` must be one of the sizes declared on the bucket: [Thumbnails](/build/storage/thumbnails).

Rust can also download the bytes directly, with the client's credential:

```rust title="Rust"
let bytes = mono.files().download("docs", "reports/2026/q3.pdf").await?;
```

## List objects

:::tabs
```ts title="TypeScript"
let cursor: string | undefined;
do {
  const page = await mono.files.list("docs", { prefix: "reports/2026/", limit: 100, cursor });
  page.items.forEach((o) => console.log(o.key, o.size));
  cursor = page.nextCursor;
} while (cursor);
```
```kotlin title="Kotlin"
var cursor: String? = null
do {
    val page = mono.files.list("docs", StorageListOptions(prefix = "reports/", limit = 100, cursor = cursor))
    page.items.forEach { println("${it.key} ${it.size}") }
    cursor = page.nextCursor
} while (cursor != null)
```
```dart title="Dart"
String? cursor;
do {
  final page = await mono.files.list('docs', prefix: 'reports/', limit: 100, cursor: cursor);
  for (final o in page.items) { print('${o.key} ${o.size}'); }
  cursor = page.nextCursor;
} while (cursor != null);
```
```python title="Python"
cursor = None
while True:
    page = mono.files.list("docs", prefix="reports/", limit=100, cursor=cursor)
    for o in page.items:
        print(o.key, o.size)
    cursor = page.next_cursor
    if not cursor:
        break
```
```rust title="Rust"
let mut cursor: Option<String> = None;
loop {
    let page = mono.files().list("docs", Some("reports/"), Some(100), cursor.as_deref()).await?;
    for o in &page.items { println!("{} {}", o.key, o.size); }
    cursor = page.next_cursor;
    if cursor.is_none() { break; }
}
```
:::

## Delete

:::tabs
```ts title="TypeScript"
await mono.files.delete("docs", "reports/2026/q3.pdf");
```
```kotlin title="Kotlin"
mono.files.delete("docs", "reports/2026/q3.pdf")
```
```dart title="Dart"
await mono.files.delete('docs', 'reports/2026/q3.pdf');
```
```python title="Python"
mono.files.delete("docs", "reports/2026/q3.pdf")
```
```rust title="Rust"
mono.files().delete("docs", "reports/2026/q3.pdf").await?;
```
:::

## Keys and limits

Keys are 1 to 512 bytes and may contain `/`. They may not contain empty, `.` or `..` segments, control characters or backslashes. Keys are case-sensitive. TypeScript, Dart and Python URL-encode the key for you; Kotlin does not, so encode special characters yourself. Quotas and size limits: [Limits and quotas](/build/storage/limits-and-quotas).

## Next steps

- [Thumbnails](/build/storage/thumbnails)
- [Authentication](/sdks/auth)
