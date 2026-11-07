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

## Implemented today (Phase 1 server)

Check this list before relying on a feature; the rest of this page is the target design.

- Works: list (`filter`, `sort`, `limit`, `cursor`, `fields`, single/multi/back-relation `_via_` `expand`,
  `total=none|exact|estimate`), `GET .../records:count`, create, view (`fields`, `expand`, `ETag`,
  `If-None-Match` → 304), `PATCH` update (`If-Match` → 412), `PUT` upsert (201 when new, 200 when
  existing), delete (`If-Match`), `?dry_run=true` on all write endpoints, `POST /api/v1/apps/{app}/batch`,
  and multi-relation fields (`maxSelect > 1`, stored in `_rel_<coll>_<field>` junction tables).
- Debug & stats endpoints: `POST /api/v1/apps/{app}/collections:diff`, `POST /api/v1/apps/{app}/rules:explain`,
  `POST /api/v1/apps/{app}/query:explain`, `GET /api/v1/apps/{app}/logs?request_id=...`,
  `GET /api/v1/apps/{app}/logs/slow`, `GET /api/v1/system/stats`,
  `GET /api/v1/apps/{app}/stats?counts=estimate|exact`.
- Auth: only superuser tokens from `POST /api/v1/system/auth/login`. Without a token you are
  anonymous and the collection rules decide. A bad token is 401, never anonymous.
- First superuser: `GET /api/v1/system/install` → `{"required": true}` on a fresh server.
  Create it with `POST /api/v1/system/install` `{token, email, password}`; the token is the
  64-hex string in the setup link the server prints at start-up (ask the operator for it;
  restart for a new one). 401 = wrong/expired/spent token or a superuser already exists.
  Headless: the operator runs `monolith superuser create <email>` on the host/container.
- More superusers (superuser token): `GET /api/v1/system/superusers` lists `{id,email,created}`,
  `POST` `{email,password}` adds one, `DELETE .../superusers/{id}` removes one (409 for yourself or
  the last one; the removed admin's token still works until it expires, 1 h). `GET /api/v1/system/me`
  returns the caller (401 if that superuser was removed).
- Per-app sign-in config (superuser): `GET|PUT /api/v1/apps/{app}/auth/config` holds email/password
  settings and any number of OAuth providers (`google github apple microsoft facebook discord gitlab x
  linkedin twitch oidc`). `clientSecret` is write-only: omit it to keep the stored one, `""` clears it;
  responses only show `clientSecretSet`. Enabling a provider needs client id, secret and its required
  `extra` fields (Apple `teamId`+`keyId`, OIDC `issuer`). Config only: the sign-in flow is Phase 2.
- Backups (superuser): `POST /api/v1/apps/{app}/backups` takes a snapshot, `GET` lists, `GET .../backups/{name}` downloads the
  SQLite file, `DELETE` removes it, `POST .../backups:restore {backup,newApp}` creates a new app from it. No schedule yet.
- Not yet: user tokens, API keys, `openapi.json`, 429 rate limits.
- Status codes: create rule rejects → 403; update/delete rule rejects or record missing → 404;
  rule `null` → 403 for non-superusers; unique/FK violation → 409; stale `If-Match` → 412;
  body over 1 MiB → 413; writer queue full → 503 + `Retry-After`.
- Every response has `X-Request-Id`; error bodies repeat it as `requestId`. Send your own
  (`[A-Za-z0-9._-]{1,64}`) to correlate logs.
- Records: `id` (26-char base32 UUIDv7), `created`/`updated` (RFC 3339 µs UTC), `_v` (change seq).
  You may send `id` on create (UUIDv7 created within the last 30 days).
