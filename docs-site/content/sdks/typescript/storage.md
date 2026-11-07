---
title: Storage
description: TypeScript: upload, download URLs, list and delete files.
group: TypeScript
order: 70
---

Upload files, build download URLs, list and delete objects.

## Upload

```ts
const obj = await mono.files.upload("avatars", `users/${userId}.png`, file, { contentType: "image/png" });
console.log(obj.size, obj.blake3);
```

`file` can be a `Blob` (a browser `File` is one), `ArrayBuffer`, `Uint8Array` or `string`. The server checks the bucket's size and type limits and answers `413` or `400` when they fail.

## Get a download URL

```ts
img.src = mono.files.getURL("avatars", `users/${userId}.png`);
img.src = mono.files.getURL("avatars", `users/${userId}.png`, { thumb: "64x64" });
```

`getURL` only builds the address. It does not add a credential, so it works directly in `<img>` for a **public bucket**. For a private bucket fetch the URL with `Authorization: Bearer ${await mono.auth.accessToken()}`.

`thumb` must be one of the sizes declared on the bucket. See [Thumbnails](/build/storage/thumbnails).

## List objects

```ts
let cursor: string | undefined;
do {
  const page = await mono.files.list("docs", { prefix: "reports/2026/", limit: 100, cursor });
  page.items.forEach((o) => console.log(o.key, o.size));
  cursor = page.nextCursor;
} while (cursor);
```

## Delete

```ts
await mono.files.delete("docs", "reports/2026/q3.pdf");
```

## Keys and limits

Keys are 1 to 512 bytes, may contain `/`, and may not contain empty, `.` or `..` segments. The SDK URL-encodes the key, so a `/` becomes `%2F`; the server decodes it back. Buckets are created in the Admin UI or with REST: [Storage](/build/storage/overview), [Limits and quotas](/build/storage/limits-and-quotas).

## Next steps

- [Analytics](/sdks/typescript/analytics)
- [SDKs overview](/sdks/overview)
