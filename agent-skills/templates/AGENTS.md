# Monolith backend (for AI agents)

This project uses a Monolith server as its backend.
- Base URL: {{BASE_URL}}  App id: {{APP}}
- Contract: {{BASE_URL}}/api/v1/apps/{{APP}}/openapi.json  Realtime protocol: docs/api/websocket.md
- MCP endpoint: {{BASE_URL}}/api/v1/apps/{{APP}}/mcp (needs a key with `mcp` scope)
- Skills installed in: {{SKILLS_DIR}}. Read `monolith-overview` first, `monolith-debug` before debugging.
- Never hardcode secret API keys; use env `MONOLITH_KEY`. Publishable keys (`mk_pub_...`) may ship in clients.
- On any failed request, capture the `X-Request-Id` response header and follow `monolith-debug`.
