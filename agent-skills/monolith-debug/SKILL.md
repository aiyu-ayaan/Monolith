---
name: monolith-debug
description: Systematically debug anything involving a Monolith server: failed requests, 401/403/404, empty lists, slow queries, missing realtime events, sync not converging, write conflicts, startup or backup problems. Use before guessing at a fix.
---

# Debug Monolith

Work in this order. Do not change code before step 3.

1. **Capture facts.** Request id (`X-Request-Id`), HTTP status, problem+json `type`/`detail`, timestamp, app, collection, credential type.
2. **Check server health.** `GET /api/v1/system/health` (writer queue depth, reader wait, app state). 503 + `Retry-After` means backpressure, not a bug.
3. **Trace the request.** `GET /api/v1/apps/{app}/logs?request_id=<id>` shows route, principal, scopes, rule class, SQL shape, durations. MCP: `trace_request`.
4. **Explain, do not guess.**
   - Rule/filter: `POST /api/v1/apps/{app}/rules:explain {collection, op, filter?, auth_as?}` returns parsed AST, class (RowLocal/Relational), compiled SQL and a verdict for a sample row/principal. MCP: `explain_rule`.
   - Query plan: `POST /api/v1/apps/{app}/query:explain` returns `EXPLAIN QUERY PLAN`. A `SCAN` on a big table = missing index. MCP: `explain_query`.
   - Slow queries: `GET /api/v1/apps/{app}/logs/slow` with plans and index suggestions.
5. **Realtime:** `GET /api/v1/apps/{app}/realtime/inspect?conn=<id>` lists subscriptions, group, last `seq`, queue depth, overflows. Compare client `since` with `GET /sync/status`.
6. **Sync:** `GET /api/v1/apps/{app}/sync/status?device=<id>` shows cursor, retention floor, pending dedupe `mid`s, last conflict decisions.
7. **Reproduce minimally** with curl using the same credential; then fix; then re-run the failing request and confirm the new request id succeeds.

Symptom map (details in `references/symptoms.md`):

| Symptom | First check |
|---------|-------------|
| 401 | token expiry, wrong app, key revoked/expired, clock skew |
| 403 | missing scope, rule denies write (`rules:explain` with `auth_as`) |
| 404 on existing id | viewRule denies (existence hidden) |
| Empty list | listRule or filter excludes all; try `explain_rule` |
| Slow list | `query:explain`, missing index, sort not indexed |
| 412 | stale `If-Match` / `base_v` |
| 429/503 | rate limit or writer queue full, honour `Retry-After` |
| No realtime events | rule/filter, `since` older than retention, subscription not acked |
| Sync loops | cursor not stored with data, `mid` reused with different payload, clock skew on HLC |

Never disable rules, widen keys to `records:admin`, or paste secret keys into logs to make a problem go away.
