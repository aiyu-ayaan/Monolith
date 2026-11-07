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

- Layout: `<data>/apps/<app>/files/files.db` (bucket config + one row per object: key, blob id, size, MIME,
  SHA-256 etag) and `files/<bucket>/<blob id>` for the bytes. Bucket config is cached in memory.
- Endpoints (`api/openapi`): bucket CRUD (superuser), `GET .../objects?prefix&limit&cursor` (keyset),
  `PUT|GET|HEAD|DELETE .../objects/{key}`. Upload is the raw body (no multipart), streamed through a bounded
  channel to a blocking writer that hashes, enforces the bucket `maxSize` as bytes arrive, fsyncs and renames
  into place; the metadata commit is serialized so an overwrite never orphans a blob.
- Access: private bucket needs a superuser token or a key with `files:read[:bucket]`; public buckets are readable
  by key without credentials but never listable; writes need `files:write[:bucket]`. Keys cannot manage buckets.
- Download: `ETag` + `If-None-Match`, single `Range`, `nosniff`, `Content-Security-Policy: sandbox`; only images
  (not SVG), audio, video, PDF and plain text are `inline`, everything else is `attachment`.
- **Deviations from §2–§4:** one blob per object version (no blake3 content addressing or dedupe, so no
  incremental-backup benefit yet); declared `Content-Type` only (no sniffing); local disk only.
- Not yet: S3 backend and presigned URLs, per-user/rule-based access (needs end-user auth), file fields on
  records, `_files` table, orphan GC beyond temp cleanup at open, thumbnails, quotas, delimiter listing,
  files in backups, benchmarks (upload/download throughput and memory are unmeasured).
- Gaps beyond this list (`_changes` growth in `files.db`, orphan blobs after a crash, serialized metadata commits, no quotas, storage bytes missing from metrics, single Range only): `development/TODO.md` "Known limitations".
