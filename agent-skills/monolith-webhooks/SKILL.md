---
name: monolith-webhooks
description: Send Monolith record changes to your own HTTP endpoint: create webhooks, verify the HMAC signature, handle retries and duplicates, debug missing deliveries. Use when integrating a backend job, payment or email service with data changes.
---

# Webhooks

Superuser API under `/api/v1/apps/{app}/webhooks`. Admin UI: Webhooks page.

```bash
curl -X POST "$BASE/api/v1/apps/$APP/webhooks" -H "Authorization: Bearer $SU_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"name":"orders to ERP","url":"https://erp.example.com/hook","events":["orders.create","orders.update"]}'
# 201 → {"id":"…","secret":"whsec_…"}   the secret is shown once
```

- `events`: `collection.op` patterns. Ops: `create`, `update`, `delete`. `posts.*`, `*.delete`, `*` work.
- Test: `POST …/webhooks/{id}/test` sends a `ping`. Log: `GET …/webhooks/{id}/deliveries?status=pending|ok|dead`.
  Redeliver: `POST …/deliveries/{did}/retry`.

Payload (JSON): `{id, app, event, collection, seq, createdAt, record}`. `record` has no hidden fields; for `delete` it is `{id}` only.
Headers: `X-Monolith-Event`, `X-Monolith-Delivery`, `X-Monolith-Signature: t=<unix s>,v1=<hex>`.

Verify before trusting (raw body bytes, not re-serialized JSON):

```ts
const [t, v1] = header.split(",").map((p) => p.split("=")[1]);
const expected = createHmac("sha256", secret).update(`${t}.${rawBody}`).digest("hex");
if (!timingSafeEqual(Buffer.from(v1), Buffer.from(expected)) || Math.abs(Date.now() / 1000 - +t) > 300) reject();
```

Rules of thumb:
- Answer 2xx fast (10 s timeout, redirects are not followed). Do slow work after answering.
- Delivery is at least once: de-duplicate on `X-Monolith-Delivery` (or `event` + `record.id` + `seq`). After retries order can change: use `seq`.
- Failed attempts retry after 5 s, 10 s, 20 s … (max 1 h), 10 attempts, then `dead`.
- Missing deliveries: check the hook is `enabled`, the pattern matches `collection.op`, and the log (`lastStatus`, `lastError`).
  Changes made before the first hook existed are not sent.
