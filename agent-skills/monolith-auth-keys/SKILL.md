---
name: monolith-auth-keys
description: Authenticate against Monolith: user sign-in, access/refresh tokens, API keys (mk_...) and scopes. Use when handling login, 401/403 errors, or choosing which key goes in a client vs a server.
---

# Auth and keys

- Access token lives ~15 min; refresh token rotates on every use. Reusing an old refresh token revokes the whole session.
- Refresh once, not per failed request (single-flight), then retry the original call.
- Key types: `mk_pub_...` publishable (safe in apps, limited by rules), `mk_...` secret (server only, scope-limited).
- Scopes look like `records:read:tasks`, `realtime:subscribe`, `analytics:ingest`, `schema:write`, `backup:run`, `mcp`.
- 401 = bad/expired credential. 403 = valid credential, missing scope or rule denies. Check `WWW-Authenticate` and the problem `detail`.
- Create keys: `monolith keys create --app <app> --name <n> --scopes <list> --expires 90d`.

## Implemented today

- Create: `POST /api/v1/apps/{app}/api-keys` `{"name","scopes":[...],"expiresInDays"}` with a superuser token. The secret is in the `key` field, once.
- A key works only on its own app (403 elsewhere) and never on admin routes (401). `DELETE .../api-keys/{id}` revokes at once.
- Working scopes: `records:read|write[:collection]`, `records:admin`, `realtime:subscribe`, `files:read|write[:bucket]`.
- Only `records:admin` bypasses collection rules (server-side only). Every other key is anonymous to the rules and just narrowed by scope: if a read returns 403 or empty, the rule needs `@request.auth` that a key cannot have; use `records:admin` server-side.
- Realtime with a key needs `realtime:subscribe` plus `records:read` for each collection.
- Optional per key: `rateLimitPerMin` (over it: 429 + `Retry-After`, wait that long, do not hammer), `allowedIps` (403 from other addresses; behind a proxy the server needs `MONOLITH_TRUST_PROXY=1`), `expiresInDays`.
- `PUT /api-keys/{id}` edits a key without changing its secret. `lastUsed` is approximate (saved about once a minute). Request logs show which key made a call (`key`).

## App users (end-user sign-in)

Public, per app: `GET /api/v1/apps/{app}/auth/providers` tells a client what to show (email/password flags and every working OAuth provider).

- Email: `POST /auth/register` and `/auth/login` with `{email, password}` → `{token, refreshToken, expiresAt, user}`. Send `Authorization: Bearer <token>`; it works on that app only.
- Refresh: `POST /auth/refresh {refreshToken}` returns a new pair; the old refresh token dies at once (single use). Refresh once with a single flight, then retry. `POST /auth/logout {refreshToken}` ends the session.
- Access token lives 15 minutes, refresh token 30 days. Rules see the user as `@request.auth.id`.
- Provider sign-in (google, github, microsoft, facebook, discord, gitlab, linkedin, twitch, x, generic oidc): open `/auth/oauth/{provider}/start?redirectUri=<your app URL>`; the browser comes back to `redirectUri#token=…&refreshToken=…&expiresAt=…` (or `#error=…`).
  The URL must be in the app's allowed redirects (Authentication page), or start answers 400. Register `<public origin>/api/v1/apps/{app}/auth/oauth/{provider}/callback` at the provider; set `MONOLITH_PUBLIC_URL` behind a proxy.
- 401 on login = wrong email or password (or disabled), never which. 409 on register = email taken. 403 = sign-up or the method is switched off.
- Console (superuser): `GET|POST /users`, `PATCH|DELETE /users/{id}`.
- Not yet: email verification, password reset, OTP, MFA, Apple.
