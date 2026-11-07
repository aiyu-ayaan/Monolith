---
title: Storage
description: Rust: upload, download URLs, list and delete files.
group: Rust
order: 70
---

Upload files, build download URLs, list and delete objects.

## Upload

```rust
let obj = mono.files()
    .upload("avatars", &format!("users/{user_id}.png"), png_bytes, Some("image/png"))
    .await?;
println!("{} {}", obj.size, obj.etag);
```

`data` is anything that converts into `bytes::Bytes`. The server checks the bucket's size and type limits (`413`, `400`).

## Download

```rust
let bytes = mono.files().download("docs", "reports/2026/q3.pdf").await?;
let thumb = mono.files().download_with_thumb("avatars", "users/u1.png", Some("64x64")).await?;
```

Downloads send the client's credential, so they work for private buckets too.

## Get a URL

```rust
let url = mono.files().get_url("avatars", "users/u1.png");
let small = mono.files().get_url_with_thumb("avatars", "users/u1.png", Some("64x64"));
```

A URL carries no credential, so it works as it is only for a **public bucket**. `thumb` must be a size declared on the bucket: [Thumbnails](/build/storage/thumbnails).

## List objects

```rust
let mut cursor: Option<String> = None;
loop {
    let page = mono.files().list("docs", Some("reports/"), Some(100), cursor.as_deref()).await?;
    for o in &page.items { println!("{} {}", o.key, o.size); }
    cursor = page.next_cursor;
    if cursor.is_none() { break; }
}
```

## Delete

```rust
mono.files().delete("docs", "reports/2026/q3.pdf").await?;
```

## Keys and limits

Keys are 1 to 512 bytes and may contain `/`, but no empty, `.` or `..` segments. A leading `/` is removed. Buckets are made in the Admin UI or with REST: [Storage](/build/storage/overview), [Limits and quotas](/build/storage/limits-and-quotas).

## Next steps

- [Errors](/sdks/rust/errors)
- [SDKs overview](/sdks/overview)
