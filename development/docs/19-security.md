# 19 — Security

## 1. Threat model (summary)

| Asset | Threat | Control |
|-------|--------|---------|
| App data | Unauthorized read/write via API | Default-deny rules (`null` = superuser only on new collections), compiled rules, scope checks, differential tests SQL vs predicate |
| App data | Cross-app access | Separate DB file per app; `RequestCtx.app` bound at routing; tokens and keys carry `app` and are rejected elsewhere |
| User accounts | Credential stuffing, brute force | Argon2id, GCRA rate limits, account/IP lockout, MFA, breach-password check (optional k-anonymity API) |
| Tokens | Theft, replay | Short access tokens, rotating refresh with reuse detection (reuse → revoke whole session family), secure cookie option for web |
| API keys | Leakage | Shown once, stored hashed, prefix for secret scanning, IP allowlist, expiry, last-used tracking, publishable vs secret keys |
| Filters | Injection / DoS | Parsed grammar (no raw SQL), bind params, AST/length/depth limits, query timeout via progress handler, hidden-field reference ban |
| Files | Malicious uploads, image bombs | MIME sniffing, size limits, pixel limit before decode, no execution, `Content-Disposition` + `nosniff`, separate download origin option |
| Server | Resource exhaustion | Bounded queues, body limits, connection limits per IP/app, per-app quotas |
| Plugins | Malicious/buggy WASM | Sandbox, capabilities manifest, fuel/epoch limits, memory caps |
| Backups | Exposure | Encrypted archives, separate backup key, signed download URLs |
| Secrets at rest | DB file theft | Envelope encryption of signing keys, OAuth secrets, SMTP/S3 creds with master key |
| Superuser | Account takeover | Mandatory MFA option, IP allowlist for `/_/` and `/api/v1/system`, audit log |
| SQL console | Abuse | Superuser only, read-only by default (`query_only` connection), timeout, row cap |

## 2. Transport and headers

- TLS terminated by Monolith (`rustls`) or reverse proxy; HSTS when TLS enabled.
- CORS per app (allowed origins list, no wildcard with credentials).
- Security headers on admin UI: CSP (no inline scripts), `frame-ancestors 'none'`,
  `Referrer-Policy`, `Permissions-Policy`.

## 3. Tenant isolation checklist

- Every data-plane handler receives `AppRuntime` from the router, never from request
  body.
- Tokens: `app` claim must equal route app; signing keys differ per app.
- API key prefix routes to one app; hash checked in that app's table.
- Realtime/sync connections are per app.
- Logs, analytics, files, backups stored under the app directory.

## 4. Supply chain and build

- `cargo deny` (licenses, advisories, bans), `cargo audit` in CI.
- Pinned toolchain (`rust-toolchain.toml`), `--locked` builds, reproducible Docker build.
- SBOM (CycloneDX) and image signing (cosign) for releases.
- `pnpm install --frozen-lockfile`; `pnpm audit` in CI for JS/TS packages.

## 5. Privacy

- Per-app data export and erasure endpoints (user data across collections, files,
  analytics).
- Analytics: no raw IP storage by default; consent flag honoured.
- Logs: configurable redaction of headers/body fields; retention limits.
