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
