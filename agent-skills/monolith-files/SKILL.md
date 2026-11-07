---
name: monolith-files
description: Store and serve files with Monolith storage buckets: create buckets, upload with PUT, download with Range/ETag, list by prefix, choose public vs private, and fix 401/403/413 errors. Use when handling uploads, images, attachments or /buckets routes.
---

# Storage buckets

Base: `/api/v1/apps/{app}/buckets`. Keys may contain `/` (`photos/2026/a.png`); no empty, `.` or `..` segments.

- Create a bucket (superuser): `POST /buckets {"name","public","maxSize","maxBytes","allowedMime":["image/*"],"thumbSizes":["100x100"]}`. `DELETE /buckets/{b}?force=true` also deletes its objects.
- Upload: `PUT /buckets/{b}/objects/{key}` with the **raw body** (not multipart) and `Content-Type`. Needs a superuser token or a key with `files:write[:bucket]`. Replaces an existing key.
- Download: `GET /buckets/{b}/objects/{key}`. Supports `ETag`/`If-None-Match` and one `Range`. Private bucket: superuser or key with `files:read[:bucket]`. Public bucket: no credential needed.
- Thumbnails: `GET /buckets/{b}/objects/{key}?thumb=100x100` for png/jpeg/gif/webp. Only sizes in the bucket's `thumbSizes` work (else 400): `WxH` crops to fill, `Wx0`/`0xH` keep aspect, `WxHf` fits inside. Output is JPEG (PNG if transparent); sources over 50 MP give 413.
- List: `GET /buckets/{b}/objects?prefix=&limit=&cursor=` (always needs `files:read`, even for public buckets); pass `nextCursor` as `cursor`.

Errors: 401 no/invalid credential or anonymous on a private bucket; 403 key lacks scope or belongs to another app; 400 bad key or MIME not in `allowedMime`; 413 over `maxSize` (default 100 MiB); 409 bucket exists or not empty.

Notes: identical files are stored once (the `etag` is the BLAKE3 of the content). The type is detected from the content when possible and replaces the declared `Content-Type`, so `allowedMime` judges what the file really is; a bucket `maxBytes` quota gives 413 too. `POST /storage:gc` (superuser) removes orphan blob files. Only images (not SVG), audio, video, PDF and text/plain display inline, the rest download. Keys cannot create buckets. Backups include the files (hard links); restoring a backup into a new app brings its buckets along.
