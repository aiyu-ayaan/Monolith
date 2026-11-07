# 08 — Authentication & Authorization

## 1. Principals

| Principal | Where stored | Authenticates with | Scope |
|-----------|--------------|--------------------|-------|
| Superuser | `system.db/superusers` | password + mandatory TOTP/passkey (configurable) | All apps, admin UI |
| App admin *(later)* | `system.db` + app membership | same as superuser | One or more apps |
| App user | auth collection in `apps/<id>/data.db` | password, OAuth2/OIDC, email OTP, magic link, passkey (later), anonymous | One app |
| API key | `_api_keys` (per app) + `api_keys_index` (system) | `Authorization: Bearer mk_...` | One app, scoped |
| Device *(sync)* | `_sync_clients` | user token + device id | One app user |

## 2. Token design (hot path without DB)

PocketBase loads the user row on every authenticated request (B6). Monolith:

| Token | Format | Lifetime | Storage | Verification cost |
|-------|--------|----------|---------|-------------------|
| Access token | Compact signed token (JWT-compatible, `EdDSA` default, `HS256` optional) | 15 min (configurable 1–60) | Client only | Signature check + in-memory revocation lookup; **no DB** |
| Refresh token | 256-bit random, opaque | 30 days sliding, rotated on every use | `_sessions` stores **SHA-256 hash** only | One indexed lookup, only on refresh |
| API key | `mk_<app-prefix>_<random 32B base32>` | Until expiry/revoke | `_api_keys` stores **hash** + scopes | In-memory cache (`moka`) of hash → key record, invalidated on change |
| File token | Short signed token | 2 min | — | Signature only |

Access-token claims: `sub` (user id), `app`, `coll` (auth collection), `sid` (session id),
`sv` (session version), `role` + up to N **cached auth fields** marked "claim" in the
schema (e.g. `role`, `org`), `iat`, `exp`.

**Rules that reference only claimed fields** (`@request.auth.id`, `@request.auth.role`)
evaluate without loading the user. Rules referencing other auth fields trigger a
load through an **auth record cache** (LRU, TTL 30 s, invalidated by the change bus on
user update), not a DB query per request.

**Revocation:** logout / password change / admin ban increments the session version or
deletes the session. The node keeps an in-memory `revoked_sessions` set (entries live
until the access token's max lifetime passes), loaded at startup from `_sessions`.
Worst-case revocation latency = 0 on a single node.

Key management: per-app signing key pair, stored encrypted in `system.db` (envelope
encryption with `MONOLITH_MASTER_KEY`); `kid` header supports rotation with an overlap
window. JWKS published at `/api/v1/apps/{app}/.well-known/jwks.json` so other services
can verify Monolith tokens.

## 3. Auth methods

| Method | Notes |
|--------|-------|
| Email/username + password | Argon2id (m=19 MiB, t=2, p=1 per OWASP baseline; tuned in bench), run on the bounded CPU pool; constant-time responses for unknown users |
| OAuth2 / OIDC | Google, Apple, GitHub, Microsoft, Discord, Facebook, GitLab + **generic OIDC** (covers most others). PKCE always. ID-token validation via cached JWKS. Link/unlink identities |
| Email OTP | 6–8 digit code, hashed, TTL 10 min, attempt limit |
| Magic link | Signed one-time link, TTL 15 min |
| Anonymous | Creates a user with `anonymous=true`; can be upgraded (link email/OAuth) keeping the same id — useful for guest users |
| MFA | TOTP and email OTP as second factor; per-collection policy (`off`, `optional`, `required`) |
| Passkeys (WebAuthn) | Phase 2 |
| Impersonation | Superuser obtains a short non-refreshable token for a user (audited) |

Flows: verification email, password reset, email change, new-device login alert
(like PocketBase `authAlert`), all with templated emails per app.

## 4. API keys (runtime-configurable, "ntfy style")

Goal: create a key in seconds from UI/CLI/API and use it with `curl`.

```bash
monolith keys create --app myapp --name backup-job --scopes "records:read,records:write:tasks" --expires 90d
# → mk_rdlk_7k3q...   (shown once)

curl -H "Authorization: Bearer mk_rdlk_7k3q..." \
     https://api.example.com/api/v1/apps/myapp/collections/tasks/records?limit=50
```

Scopes (grammar `resource:action[:target]`):

| Scope | Grants |
|-------|--------|
| `records:read[:coll]` / `records:write[:coll]` | CRUD, still filtered by collection rules evaluated with `@request.key.*` |
| `records:admin[:coll]` | Bypass collection rules (service role) |
| `users:manage` | Manage auth users |
| `realtime:subscribe` / `realtime:publish[:channel]` | Realtime |
| `analytics:ingest` / `analytics:read` | Analytics |
| `files:read` / `files:write` | Files |
| `schema:read` / `schema:write` | Collections/migrations |
| `backup:run` | Trigger/download backups |
| `mcp` | Use MCP endpoint (still limited by other scopes) |

Key options: expiry, IP/CIDR allowlist, per-key rate limit, `publishable` flag (safe
to embed in a client app — only scopes allowed by rules, like Firebase's web API key),
last-used timestamp (written asynchronously, batched).

## 5. Authorization flow per request

```
extract credential ─▶ access token? verify sig, exp, sid not revoked
                   └▶ API key?     prefix → app; hash lookup in cache; check expiry/IP
                   └▶ none          anonymous request context
build RequestCtx { app, principal, claims, scopes, ip, device }
route handler ─▶ scope check (fast, bitset) ─▶ collection rule (compiled, 06)
```

## 6. Rate limiting and abuse controls

- GCRA / token bucket per (app, principal or IP, route class), sharded in-memory map,
  O(1) per check. Defaults: auth endpoints 10/min/IP, OTP send 3/10 min/email, global
  per-key limits configurable.
- Exponential lockout on repeated failed passwords per account and per IP.
- Captcha hook point (Turnstile/hCaptcha) for sign-up endpoints, optional.

## 7. Implemented today (Phase 2 slice: API keys, `crates/monolith-core/src/apikeys.rs`)

- `POST|GET /api/v1/apps/{app}/api-keys`, `DELETE .../api-keys/{id}` (superuser). The secret
  `mk_<first 4 chars of app>_<52 base32 chars>` is returned once; `system.db/api_keys` keeps its SHA-256,
  scopes, expiry and a 12-character display prefix.
- Live keys are mirrored in memory by hash, so verifying a request is one SHA-256 and one map lookup
  with no DB read; revoke removes the entry before it returns.
- A key works only on routes of its own app (403 elsewhere) and never on admin routes (401).
- Scopes: `records:read|write[:collection]`, `records:admin`, `realtime:subscribe`, `files:read|write[:bucket]`.
  `records:admin` bypasses collection rules (like a superuser limited to the app). **Deviation from §4:**
  every other key is anonymous to the rules and only narrowed by its scopes, because `@request.key.*`
  does not exist yet. Realtime needs `realtime:subscribe` plus `records:read` for each collection.
- Per-key limits: `rateLimitPerMin` (GCRA with a burst of the same size, lock-free; 429 + `Retry-After`) and
  `allowedIps` (IPs/CIDRs, IPv4-mapped IPv6 counts as IPv4; 403 from other or unknown addresses). The client
  address is the socket peer; with `MONOLITH_TRUST_PROXY=1` it is the last `X-Forwarded-For` entry. The middleware
  sets it, a client-sent value is dropped. Order of checks: scope app, address, rate, then the use is recorded.
- `lastUsed` is an atomic store per request, saved by a 60 s background flush (and at shutdown). A crash can
  lose up to a minute of it.
- `PUT .../api-keys/{id}` replaces name, scopes, expiry (from now), limits and allowlist; the secret stays valid.
- Request logs carry the key id (`key`), shown in the Admin UI Logs page.
- Not yet: `users:manage`, `analytics:*`, `schema:*`, `backup:run`, `mcp` scopes; `publishable` keys, `monolith keys` CLI,
  per-collection `records:admin`, shared cache for several nodes.
- Gaps beyond this list (single-node cache, no rate limit for failed auth, no key rotation endpoint): `development/TODO.md` "Known limitations".

## Implemented today (`users.rs`, `oauth.rs`)

- Users live in system.db (`app_users`, `app_identities`, `app_sessions`), not yet as a collection with custom fields.
- Email + password: `POST /api/v1/apps/{app}/auth/register|login|refresh|logout`, `GET|PATCH /auth/me`, `POST /auth/password`.
  Argon2id on the blocking pool; unknown email, wrong password and disabled accounts answer the same 401 after the same work.
- Tokens: access token `u1.<app>.<user>.<exp>.<mac>` (15 min, HMAC with the server token key, bound to the app, verified with no DB read);
  refresh token 256-bit random, stored as SHA-256, 30 days, single use (rotation). A new password or a disabled account ends all sessions;
  an already issued access token lives until it expires.
- Rules see the caller as `@request.auth.id` with collection `users` (no cached claims yet). Realtime and records accept user tokens.
- OAuth 2.0 authorization code + PKCE for google, github, microsoft, facebook, discord, gitlab, linkedin, twitch, x and generic OIDC
  (`/auth/oauth/{provider}/start` → provider → `/callback` → app redirect with `#token=…&refreshToken=…&expiresAt=…`).
  The redirect must match the app's `allowedRedirects`; pending sign-ins are in memory (10 min, 2 000). Public origin from
  `MONOLITH_PUBLIC_URL` or the request. Link by verified email only; an unverified email that is taken is a 409. Apple is not implemented
  (needs a signed client secret) and is left out of the public provider list.
- Public `GET /auth/providers` lists what a sign-in screen can offer (email/password flags, working providers); nothing secret.
- Console: `GET|POST /users`, `PATCH|DELETE /users/{id}` (superuser).
- Not yet: email verification, password reset, OTP/magic link, MFA, anonymous users, id-token signature checks (the profile comes from the userinfo
  endpoint over TLS), rate limiting of failed logins, impersonation, revocation of issued access tokens, user collections with custom fields.
