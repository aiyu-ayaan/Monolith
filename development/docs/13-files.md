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
- Access: private bucket needs a superuser token or a key with `files:read[:bucket]`; public buckets are readable
  by key without credentials but never listable; writes need `files:write[:bucket]`. Keys cannot manage buckets.
- Download: `ETag` + `If-None-Match`, single `Range`, `nosniff`, `Content-Security-Policy: sandbox`; only images
  (not SVG), audio, video, PDF and plain text are `inline`, everything else is `attachment`.
- Control-plane and file-metadata writes use `Mutation.coll = NO_CHANGE_LOG` (`monolith-store`): same commit, but no `_changes`
  row, no `seq`, no bus event, so `files.db` and `system.db` do not grow a change log.
- Data written by the first storage layout (`<bucket>/<id>` files, no counters) is migrated when the app's storage opens.
- **Deviations from §2–§4:** local disk only; no thumbnails yet; no `_files` table (records have no file fields yet).
- Not yet: S3 backend and presigned URLs, per-user/rule-based access (needs end-user auth), file fields on records,
  per-app total quota, files in backups, benchmarks (upload/download throughput and memory are unmeasured).
- More gaps: `development/TODO.md` "Known limitations".
