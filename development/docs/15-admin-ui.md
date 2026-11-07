# 15 — Admin UI

## 1. Requirements

- Embedded in the binary, served at `/_/`. No separate container or Node runtime.
- Fast first load on slow links; instant navigation afterwards.
- Heavy interactive screens: schema editor, rule editor with autocomplete and live
  validation, record grid with 100k+ rows (virtualized), realtime-updating grids,
  analytics charts, logs explorer, SQL console, multi-app switcher.
- Low memory in the browser; no jank while realtime events stream in.

## 2. Framework evaluation

| Option | Runtime perf | Bundle | Reactivity model | Ecosystem for our needs | Verdict |
|--------|-------------|--------|------------------|------------------------|---------|
| **SolidJS** | Among the fastest in js-framework-benchmark, close to vanilla JS | Small runtime | Fine-grained signals, no virtual DOM, components run once | TanStack Table/Virtual/Query have Solid adapters; CodeMirror 6 works directly | **Chosen** |
| Svelte 5 (runes) | Very fast | Small | Compiled signals | Good; fewer headless-table options than Solid/React | Strong runner-up |
| Preact + signals | Fast | Very small | VDOM + signals | React ecosystem via compat | Good, but VDOM diffing on large grids |
| React 19 | Slower on fine-grained updates (re-render trees); compiler helps | Larger | VDOM | Largest | Rejected (perf/bundle) |
| Leptos / Dioxus (Rust → WASM) | Good after load | Larger WASM payload, slower first paint | Signals | Small ecosystem for grids/editors/charts; JS interop needed anyway | Rejected for v1 (load time, ecosystem) |
| HTMX + server-rendered (askama/maud) | Minimal client JS | Tiny | Server round trips | Poor fit for rule editor, live grids, charts | Rejected |
| Vanilla + micro helper (PocketBase's Shablon approach) | Excellent | Tiny | Hand-written | We would build everything ourselves | Rejected (team velocity) |

**Why SolidJS:** fine-grained updates mean a realtime event touching one cell updates
one DOM text node, not a component tree — exactly the admin UI's hot path (live record
grids, log tail, analytics counters). JSX + TypeScript keeps the code familiar.

## 3. Stack

| Concern | Choice |
|---------|--------|
| Build | Vite, TypeScript strict, `vite-plugin-solid` |
| Routing | `@solidjs/router` (hash or path; path with server fallback to `index.html`) |
| Data fetching | `@tanstack/solid-query` + generated TS client from our OpenAPI spec |
| Tables | `@tanstack/solid-table` + `@tanstack/solid-virtual` (row and column virtualization) |
| Code / rule editor | CodeMirror 6 with a custom language mode for the rules grammar, autocomplete from schema, lint via server `POST /rules:validate` |
| Charts | uPlot (very fast canvas time-series) for analytics and metrics |
| Styling | Plain CSS with design tokens (CSS variables) + UnoCSS (build-time atomic CSS, no runtime) |
| Icons | Inline SVG sprite (tree-shaken) |
| Realtime | Same TS SDK used by apps (dogfooding via `workspace:*`) |

Managed as `@monolith/ui` in the root `pnpm` monorepo workspace. Dev: `pnpm dev` (backend + UI) or `pnpm dev:ui` alone;
build: `pnpm --filter @monolith/ui build` (or `pnpm build` across workspace).

## 4. Delivery

- `ui/dist` built in CI, embedded with `rust-embed` (or `include_bytes!` table generated
  by `build.rs`).
- Every asset stored **precompressed** (brotli level 11 + gzip) at build time; server
  picks by `Accept-Encoding`, zero runtime compression cost.
- Hashed filenames, `Cache-Control: immutable` for assets, `no-cache` for `index.html`.
- Route-level code splitting: rule editor, SQL console, analytics charts load on demand.
- Budgets (CI-enforced): initial JS ≤ 100 KB brotli, LCP ≤ 1.0 s on "Fast 3G" emulation
  for the login and app-list pages.

## 5. Screens (v1)

1. Login (+ MFA)
2. Apps list / create app / app settings (quotas, durability, backup policy)
3. Collections: list, schema editor (fields, indexes, rules with class badges and
   SQL preview), import/export
4. Records grid: filter bar (rules syntax), sort, virtual scroll, inline edit, live mode
5. Auth users: list, sessions, impersonate, revoke
6. API keys: create (scopes picker), revoke, last used
7. Realtime inspector: live connections, subscriptions per collection, event rate
8. Sync: devices, cursors, conflicts log
9. Analytics: dashboards, event explorer, funnels, retention
10. Files browser
11. Logs (request + app), slow query log with `EXPLAIN QUERY PLAN`, index advisor
12. Backups: list, run, restore (to new app or in place), policies
13. Webhooks, cron, plugins
14. System: metrics, superusers, global settings

### Navigation and settings (built)

- Project sidebar: brand, project switcher (back to all projects), search, then **Project overview**
  and four fixed groups, each product listed once: Build (Database with its collections as a tree,
  Storage, Authentication, Access rules), Realtime & sync, Monitor (Analytics, Logs, Backups),
  Extend. Unbuilt items show a "Soon" tag and do not link.
- Sidebar footer: **Project settings** and the signed-in user (avatar, email, role) with a menu:
  Users and permissions, Sign out. The server-level rail has the same user menu and a settings icon.
- **Project settings** (`#/apps/{app}/settings/{tab}`), Firebase-style tabs: General (project ID,
  created, data directory, REST and realtime URLs, delete project), API keys (moved out of the
  sidebar; `#/apps/{app}/keys` redirects here), Users and permissions.
- **Console settings** (`#/settings`): Users and permissions only, since superusers span all projects.
- Users and permissions: list, add (email + initial password), remove (not yourself, not the last
  one) via `/api/v1/system/superusers`; a roles table shows Superuser as the only active role.
  Project admin and Viewer roles are planned (doc 08 §1) and need role claims in the token.
