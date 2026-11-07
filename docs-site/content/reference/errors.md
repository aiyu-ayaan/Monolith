---
title: Errors
description: The problem+json error shape, status codes and what to do for each.
order: 20
---

Errors use RFC 9457 `application/problem+json`.

```json
{ "type": "https://monolith.dev/errors/validation", "title": "Validation failed", "status": 400,
  "detail": "…", "requestId": "…", "errors": [ { "field": "title", "code": "required", "message": "…" } ] }
```

`requestId` equals the `X-Request-Id` response header. Look the request up with `GET /api/v1/apps/{app}/logs?request_id=…` (superuser).

## Types

`bad-request`, `validation`, `unauthorized`, `forbidden`, `not-found`, `conflict`, `precondition-failed`, `payload-too-large`, `busy`, `internal` (all under `https://monolith.dev/errors/`).

Field codes in `errors[]`: `required`, `type`, `too_long`, `too_short`, `out_of_range`, `invalid_format`, `invalid_option`, `unknown_field`, `read_only`.

## Status codes

| Status | Meaning | What to do |
|---|---|---|
| 400 | Bad input, bad filter, or a rule that does not type-check | Read `errors[]` and `detail` |
| 401 | Missing, bad or expired credential; wrong password | Refresh once; else sign in again |
| 403 | Valid credential, not allowed: rule `null`, missing scope, wrong app, IP | Check [rules](/build/database/rules) and scopes |
| 404 | Missing, or hidden by a rule | Same answer on purpose |
| 409 | Unique or foreign-key conflict, email taken | Change the data |
| 412 | `If-Match` does not match | Read again, retry |
| 413 | Body over 1 MiB | Send less |
| 429 | Key rate limit | Wait `Retry-After` |
| 503 | Writer queue full | Wait `Retry-After`, retry with jitter |

## Next steps

- [REST API](/reference/rest-api)
