# 22 — Agent Skills & Debuggability

Goal: any AI agent that touches an app using Monolith can learn everything about it
from the repository itself and can debug failures with first-class server support,
without guessing.

## 1. Agent knowledge pack

Location: `agent-skills/` (see its README). One folder per skill, each a `SKILL.md` with
YAML frontmatter (`name`, `description`) plus optional `references/`. This format is read
natively by Claude Code and is easy to copy into other agents' instruction files.

| Skill | Purpose |
|-------|---------|
| `monolith-overview` | Concepts and how to pick an integration way |
| `monolith-rest` | REST contract and pagination |
| `monolith-realtime` | WebSocket/SSE protocol and resume |
| `monolith-sync` | Offline pull/push and conflicts |
| `monolith-rules` | Rules/filters language and pitfalls |
| `monolith-auth-keys` | Tokens, keys, scopes |
| `monolith-sdk-typescript`, `monolith-sdk-kotlin` | SDK usage |
| `monolith-debug` | Ordered debugging procedure and symptom map |
| `monolith-app-<app>` *(generated per app)* | That app's collections, fields, rules, scopes, example queries |

### Delivery options

| Option | How |
|--------|-----|
| CLI install | `monolith agent-init [--all] [--skills ...] [--target claude\|agents\|cursor\|codex] [--app A --url U]` writes skills and an `AGENTS.md`/`CLAUDE.md` snippet into the consuming project |
| Server download | `GET /agent/index.json`, `GET /agent/skills/{name}.zip`, `GET /api/v1/apps/{app}/agent/skills` (includes the app-specific skill) |
| `llms.txt` | `GET /llms.txt` (index with links) and `/llms-full.txt` (all docs concatenated) for agents that read the web |
| MCP | `resources/list` exposes the same skills as MCP resources, so an MCP-connected agent can read them in-band |
| Admin UI | "Connect an AI agent" page: pick app, pick agent type, copy command or download zip |

"All agents skills" means the install is not tied to one vendor: `--all` writes every
skill in every detected format, and `--target` selects one.

### Generation and freshness

- `tools/agent-pack` builds skills from `api/` (OpenAPI, AsyncAPI, JSON Schema) and
  `docs/api/`; hand-written guidance stays in `SKILL.md` files.
- The app-specific skill is generated from the live schema at request time, so rules,
  fields and scopes are never stale.
- CI check: every skill records the `api/` contract hash it was built from; the build
  fails when the contract changed and the skill did not.
- Skill descriptions state when to use them, so agents load only what is relevant.

## 2. Debug surface (server features)

Debugging support is a product feature, not an afterthought. Everything below is
available over REST and as MCP tools, gated by scopes (`debug:read`, plus `debug:write`
for the few mutating helpers), and is cheap enough to leave enabled in production.

| Capability | Endpoint | MCP tool | What it returns |
|------------|----------|----------|-----------------|
| Request id | `X-Request-Id` on every response (accepted from client if valid) | — | Correlates client, server logs, traces |
| Trace by id | `GET /apps/{app}/logs?request_id=` | `trace_request` | Route, principal, scopes, rule class, SQL shape and timings, writer batch id, errors |
| Rule explain | `POST /apps/{app}/rules:explain` | `explain_rule` | AST, rule class, compiled SQL, verdict for a sample row and `auth_as` principal |
| Query explain | `POST /apps/{app}/query:explain` | `explain_query` | Compiled SQL and `EXPLAIN QUERY PLAN`, index suggestions |
| Slow queries | `GET /apps/{app}/logs/slow` | `list_slow_queries` | Top queries by total time with plans |
| App & system stats | `GET /apps/{app}/stats`, `GET /system/stats` | — | Traffic (1h minute / 48h hour rings), record ops, store writer/reader/queue counters, DB/WAL sizes, per-collection counters |
| Realtime inspect | `GET /apps/{app}/realtime/inspect` | `inspect_realtime` | Connections, subscriptions, groups, last `seq`, queue depth, overflow counts |
| Sync status | `GET /apps/{app}/sync/status?device=` | `sync_status` | Cursor, retention floor, pending ids, recent conflict decisions |
| Health | `GET /system/health` | `check_health` | Writer queue, reader wait, app states, disk, backup age |
| Schema diff | `POST /apps/{app}/collections:diff` | `diff_schema` | Planned migration steps before applying |
| Dry-run write | `?dry_run=true` on write endpoints | `create_record` etc. | Runs rules and validation inside a rolled-back savepoint, returns what would happen |
| Live log tail | `GET /apps/{app}/logs/stream` (SSE) | `tail_logs` | Filterable by request id, level, collection |

Principles:
- **Explain, don't guess.** Rule and query problems are answered by explain endpoints,
  not by trial and error against production data.
- **Safe by default.** Debug endpoints never return secret fields; `auth_as`
  impersonation in `rules:explain` is evaluated read-only and audited.
- **Structured errors.** Every error carries `type`, `detail`, per-field `errors[]` and the
  request id; rule denials say which rule class failed without leaking hidden data.
- **Debug mode in SDKs.** `debug: true` logs request id, timing, retries and realtime
  frames in TypeScript and Kotlin.
- **Dev mode.** `monolith serve --dev` adds SQL logging, pretty errors, and the
  `/debug/pprof` profile endpoint.

## 3. Debug flow an agent follows

Defined in `agent-skills/monolith-debug/SKILL.md`: capture request id, check health,
trace, explain (rule or query), inspect realtime or sync, reproduce minimally, fix,
re-verify with a new request id. The skill also carries a symptom-to-first-check map.

## 4. Open decisions

| # | Question |
|---|----------|
| D1 | Which agent formats to target first beyond Claude Code `SKILL.md` and `AGENTS.md` (Cursor rules, Codex, Gemini, Copilot instructions) |
| D2 | Whether debug scopes are enabled on publishable keys (proposal: never) |
| D3 | Retention for request traces (proposal: 24 h in memory ring + 7 d sampled in `logs.db`) |
