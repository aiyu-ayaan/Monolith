# 13 — Files

## 1. Storage backends

| Backend | Use |
|---------|-----|
| Local disk (`/data/apps/<app>/files`) | Default, single container |
| S3-compatible (AWS, R2, MinIO, B2, Wasabi) | Large data, CDN, cheap backups |

Implemented with the `object_store` crate (async, multipart, streaming) behind a
`FileStore` trait. Per-app backend selection.

## 2. Layout and metadata

- Content-addressed blobs: key = `blake3(content)` (dedupe within an app; incremental
  backups only upload new hashes).
- `_files` table: `id, collection, record, field, original_name, mime, size, hash,
  width, height, created`.
- Public filename in API: `<slug>_<shortid>.<ext>`; mapping resolved via `_files`.

## 3. Upload path

- Streaming multipart parser (`multer`), never buffers the whole file in memory.
- Hash and MIME sniff (`infer` / magic bytes) while streaming to a temp file.
- Validation: max size per field, allowed MIME list, max files per field.
- Record update with the new file reference goes through the normal writer path; on
  transaction failure the temp blob is deleted. Orphan GC job cleans unreferenced blobs.
- Direct-to-S3 uploads via presigned URLs for large files (client uploads, then confirms).

## 4. Download path

- Public files: served with `sendfile`-friendly streaming, `ETag` = hash,
  `Cache-Control: public, max-age=31536000, immutable` (content-addressed).
- Protected files: `?token=` short-lived file token (2 min) or `Authorization` header;
  rule = collection `viewRule`.
- S3 backend: redirect to presigned URL (default) or proxy (option).
- HTTP `Range` support for media.

## 5. Thumbnails (PocketBase OOM risk fixed)

- Sizes declared per field (`100x100`, `0x300`, `400x400f`), unknown sizes rejected
  (prevents thumbnail-generation abuse).
- Decode with dimension guard **before** full decode (read header; reject >
  `max_source_pixels`, default 50 MP).
- Resize with `fast_image_resize` (SIMD), encode WebP/AVIF/JPEG per `Accept`.
- Bounded worker pool (default `cores/2`) + per-app queue; generation is single-flight
  per `(hash,size)`.
- Generated once, stored next to the blob, served like any file afterwards.
- Optional: generate common sizes eagerly at upload, in the background.

## 6. Implemented today (Phase 2 slice: storage buckets, `crates/monolith-core/src/files.rs`)

Firebase-Storage style buckets, not yet the record-field files of §2–§5.

- Layout: `<data>/apps/<app>/files/files.db` (bucket config + usage counters, one row per object: key, blob hash,
  size, MIME; a `blobs` table with a reference count per hash) and `files/blobs/<h[0..2]>/<blake3 hex>` for the bytes.
  Bucket config is cached in memory.
- **Content addressed (§2):** the BLAKE3 of the content is the blob name and the ETag. Identical content in an app is
  stored once, across buckets and keys; the last reference deletes the file. Quota and usage count logical size.
- Endpoints (`api/openapi`): bucket CRUD (superuser), `GET .../objects?prefix&limit&cursor` (keyset),
  `PUT|GET|HEAD|DELETE .../objects/{key}`, `POST .../storage:gc`. Upload is the raw body (no multipart), streamed through
  a bounded channel to a blocking writer that hashes, enforces the bucket `maxSize` as bytes arrive, fsyncs and renames
  into place. The metadata commit (object row, blob reference counts, bucket counters) is one atomic write, serialized
  per app by a mutex that is held for a few statements, never during byte transfer.
- **MIME (§3):** the first 8 KiB are sniffed (`infer`). A detected type replaces the declared one, and the bucket's
  `allowedMime` is checked against the final type, so a renamed file is judged by what it is. Text formats that magic
  bytes cannot identify (`text/*`, JSON, SVG) keep the declared type.
- **Quotas:** `maxSize` per object (checked while streaming) and `maxBytes` per bucket (checked at commit, 413).
  Bucket usage is a counter read, not a scan.
- **Orphans:** `gc` deletes blob files no row references (older than 10 minutes) and reports referenced blobs whose file
  is missing. It runs once in the background when an app's storage is first opened and on `POST .../storage:gc`.
  `files/.tmp` is emptied at open.
- **Thumbnails (§5):** `GET .../objects/{key}?thumb=100x100` for png, jpeg, gif and webp objects. Sizes are declared per
  bucket (`thumbSizes`, up to 16, sides up to 4096) and anything else is a 400, so a client cannot ask for arbitrary work.
  The source header is read first and images over 50 megapixels are refused (413) before any pixel is decoded; decoding has an
  allocation cap. Resizing uses `fast_image_resize` (Lanczos3, SIMD); `WxH` crops to fill, `Wx0`/`0xH` keep the aspect ratio,
  `WxHf` fits inside without enlarging. Output is JPEG q82, or PNG when the source has transparency. Jobs run on a pool of
  `cores/2` (one pool for the server), one job per thumbnail however many requests wait, and the result is cached at
  `files/thumbs/<hash>/<size>.jpg|png` (ETag `<hash>-<size>`), so later requests are plain file reads. They are deleted with the blob.
- Access: private bucket needs a superuser token or a key with `files:read[:bucket]`; public buckets are readable
  by key without credentials but never listable; writes need `files:write[:bucket]`. Keys cannot manage buckets.
- Traffic: object GET/HEAD count as reads and PUT/DELETE as writes in the per-app traffic metrics, with `Content-Length` as the
  byte count (a chunked upload has none, so its bytes in are not counted).
- Download: `ETag` + `If-None-Match`, single `Range`, `nosniff`, `Content-Security-Policy: sandbox`; only images
  (not SVG), audio, video, PDF and plain text are `inline`, everything else is `attachment`.
- Control-plane and file-metadata writes use `Mutation.coll = NO_CHANGE_LOG` (`monolith-store`): same commit, but no `_changes`
  row, no `seq`, no bus event, so `files.db` and `system.db` do not grow a change log.
- Data written by the first storage layout (`<bucket>/<id>` files, no counters) is migrated when the app's storage opens.
- **Deviations from §2–§5:** no presigned-URL redirects (S3 bytes are proxied); WebP/AVIF output is not produced (JPEG/PNG only); no eager generation at upload; thumbnails
  are not counted in quotas; a failed decode is not cached, so a broken image costs CPU on every request (bounded by the pool); no `_files`
  table (records have no file fields yet).
- **Backends (§1):** per app, `local` disk (default) or any S3-compatible bucket (`GET|PUT /api/v1/apps/{app}/storage/config`,
  Admin UI Storage page). Only blob bytes move: `files.db` (metadata, counters) and the thumbnail cache stay on local disk,
  so listing, quota and thumbnails never wait on the network. Blobs are `blobs/<h[0..2]>/<hash>` under the configured
  bucket and prefix, written once (a `HEAD` finds existing content, which is the dedupe), streamed in 8 MiB multipart parts
  above 8 MiB, read with range requests, deleted when the last reference goes. Uploads reach the store before the metadata
  commit and outside its lock; the commit re-checks and re-uploads if a concurrent delete reaped the blob in between.
  Saving an S3 config writes, reads and deletes a probe object first. Implemented with the `object_store` crate.
  - The secret access key is stored as plain JSON in `system.db` (like OAuth secrets; encryption at rest is Phase 3) and never returned.
  - Blobs are not migrated between backends, so the location (local / bucket / prefix / endpoint) is locked once the app holds files;
    credentials can change.
  - Remote GC lists `blobs/` once (a flat listing) and compares it with the table. Thumbnails from a remote source fetch the image
    into memory first (sources over 64 MiB are refused). A remote object read is `NotFound` if the store lost the blob.
  - Backups of an S3-backed app hold the metadata only; the bytes stay in the bucket (use bucket versioning/replication). Restoring
    into a new app gives metadata whose blobs exist only if that app is pointed at the same bucket and prefix.
  - Deleting an app does not delete its bucket contents.
  - Not verified against a real S3 service: the code path is tested with `object_store`'s in-memory store (`Registry::use_object_store`);
    the AWS builder and signing come from the library. Presigned-URL redirects are not implemented (bytes are proxied).
- Backups include storage (`docs/09`): `files.db` snapshot plus hard links to the blobs.
- Not yet: presigned URLs, per-user/rule-based access (needs end-user auth), file fields on records,
  per-app total quota, benchmarks (upload/download throughput and memory are unmeasured).
- More gaps: `development/TODO.md` "Known limitations".
