---
name: monolith-overview
description: Orientation for Monolith, a self-hosted multi-app backend (database with rules, auth, realtime, sync, files, analytics, backup). Use before writing any code that talks to a Monolith server or when the user mentions Monolith, mk_ API keys, or /api/v1/apps.
---

# Monolith overview

- One server, many **apps**. Each app has isolated data, users, rules, keys, files, analytics.
- Base path: `/api/v1/apps/{app}/...`. Control plane: `/api/v1/system/...` (superuser only).
- Data lives in **collections** (base, auth, view, timeseries) made of typed fields. Access is controlled by **rules**.
- Choose how to integrate:
  1. No SDK: REST + WebSocket from any language (`monolith-rest`, `monolith-realtime`).
  2. SDK: TypeScript (`monolith-sdk-typescript`), Kotlin/Android (`monolith-sdk-kotlin`).
  3. AI agent: MCP endpoint `/api/v1/apps/{app}/mcp`.
- Get the exact schema first: `GET /api/v1/apps/{app}/openapi.json` (filtered by your credentials).
- IDs are UUIDv7 as 26-char lowercase base32. Clients may generate them.
- Lists are cursor-paginated; totals are opt-in. Errors are RFC 9457 problem+json.
- Every response has `X-Request-Id`. Always keep it when reporting or debugging a failure.
