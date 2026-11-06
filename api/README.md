# api/ — language-agnostic contract (source of truth)

Every SDK, doc page and conformance test derives from these files. Edit here first.

| Path | Content |
|------|---------|
| `openapi/monolith.v1.yaml` | REST: system + data plane (static part). Per-app dynamic spec is served at `/api/v1/apps/{app}/openapi.json` |
| `asyncapi/realtime.v1.yaml` | WebSocket and SSE protocol (hello, sub, ev, resync, pub, auth frames) |
| `jsonschema/` | Shared envelopes: `change.json`, `error.json` (RFC 9457), `sync-push.json`, `sync-pull.json` |
| `mcp/tools.json` | MCP tool definitions for AI harnesses |
| `conformance/vectors/` | JSON test vectors every SDK must pass (request/response pairs, filter strings, cursor and token cases) |

Versioning: `/api/v1` is additive-only. Breaking change means `v2` and a new directory.
