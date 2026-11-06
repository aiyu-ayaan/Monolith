# agent-skills/ — knowledge pack for AI coding agents

Installable skills that teach any AI agent (Claude Code, Codex, Cursor, Gemini CLI, Copilot, ...)
how to integrate with, use and debug Monolith. Format: one folder per skill with a `SKILL.md`
(YAML frontmatter `name` + `description`, markdown body) and optional `references/`.

## Install into an app project

```bash
monolith agent-init                    # interactive: detects .claude/, .agents/, .cursor/, AGENTS.md
monolith agent-init --all              # every skill, every detected agent format
monolith agent-init --skills debug,rest,realtime --target claude
monolith agent-init --app myapp --url https://api.example.com   # bakes app-specific schema + base URL
```

Or fetch from a running server without the CLI:
`GET /agent/index.json` (list), `GET /agent/skills/{name}.zip`, `GET /llms.txt`, `GET /llms-full.txt`.
`GET /api/v1/apps/{app}/agent/skills` returns the same pack plus an app-specific skill
(`monolith-app-<app>`) generated from the live schema: collections, fields, rules, scopes, example filters.

## Skills

| Skill | Teaches |
|-------|---------|
| `monolith-overview` | What Monolith is, apps, concepts, base URLs, which integration way to pick |
| `monolith-rest` | REST contract, list/filter/cursor, batch, errors |
| `monolith-realtime` | WebSocket/SSE frames, subscriptions, resume, channels |
| `monolith-sync` | Offline-first pull/push, conflicts, bootstrap |
| `monolith-rules` | Rule/filter language, classes, how to write safe rules |
| `monolith-auth-keys` | Users, tokens, refresh, API keys and scopes |
| `monolith-sdk-typescript` | `@monolith/client` usage |
| `monolith-sdk-kotlin` | Kotlin core + Android usage |
| `monolith-debug` | Systematic debugging with request ids, explain endpoints, logs, MCP debug tools |

Skills are generated from `api/` and `docs/api/` by `tools/agent-pack` so they never drift;
hand-written parts live in each `SKILL.md`. CI fails if a skill is older than the contract.
