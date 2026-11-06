---
name: monolith-rest
description: Call the Monolith REST API from any language: records CRUD, filters, cursor pagination, batch, fields/expand, error handling. Use when writing HTTP code against /api/v1/apps/{app}/collections.
---

# REST usage

Auth header: `Authorization: Bearer <user access token | mk_ API key>`.

```bash
curl -H "Authorization: Bearer $MONOLITH_KEY" \
  "$BASE/api/v1/apps/$APP/collections/tasks/records?filter=status%3D'open'&sort=-created&limit=50&fields=id,title"
```

- List params: `filter`, `sort` (`-` = desc), `limit` (max 1000), `cursor`, `fields`, `expand`, `total=none|estimate|exact`.
- Paging: pass the returned `next` cursor until it is null. Never use page numbers or assume a total.
- Update with `If-Match: <_v>` for optimistic concurrency; 412 means someone else changed the row.
- `POST /batch` runs up to 500 ops atomically.
- Errors: `application/problem+json` with `type`, `status`, `detail`, `errors[]` (per-field). 429/503 include `Retry-After`; retry with jitter.
- Spec: `GET /api/v1/apps/{app}/openapi.json`. Generate clients from it.
