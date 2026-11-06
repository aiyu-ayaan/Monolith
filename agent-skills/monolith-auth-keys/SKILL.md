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
