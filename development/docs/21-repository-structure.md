# 21 — Repository Structure & Distribution

The repository is one monorepo. It ships the server, a language-agnostic API
contract, user-facing API docs, and installable client libraries (npm, Android/Maven,
and others). Any app can be wired in either by plain REST/WebSocket from any language,
or through an SDK.

## 1. Layout

```
Monolith/
├── CLAUDE.md                      contributor/agent guide
├── Cargo.toml, rust-toolchain.toml   (created in Phase 0)
├── package.json, pnpm-workspace.yaml, pnpm-lock.yaml  JS/TS workspace (ui, sdks/typescript, web-typescript)
├── crates/                        Rust server workspace (see 03 §8)
├── ui/                            SolidJS admin UI (see 15)
├── docker/                        Dockerfile, compose.yaml (see 18)
│
├── api/                           LANGUAGE-AGNOSTIC CONTRACT (source of truth)
│   ├── openapi/monolith.v1.yaml       REST (system + data plane, static part)
│   ├── asyncapi/realtime.v1.yaml      WebSocket / SSE frames
│   ├── jsonschema/                    change, error (RFC 9457), sync-push, sync-pull
│   ├── mcp/tools.json                 MCP tool definitions for AI harnesses
│   ├── conformance/vectors/           JSON vectors every SDK must pass
│   └── CHANGELOG.md
│
├── docs/                          USER-FACING API DOCS (ship with the repo and the site)
│   ├── api/
│   │   ├── overview.md                concepts, base URL, versioning, errors
│   │   ├── rest.md                    REST for any language, curl examples
│   │   ├── websocket.md               realtime + sync over WebSocket/SSE for any language
│   │   ├── kotlin.md                  Kotlin / Android SDK
│   │   ├── typescript.md              TypeScript / JavaScript SDK
│   │   ├── auth.md, rules.md, openapi-mcp.md
│   └── guides/                    quickstart, app-backup, offline-sync, docker
│
├── agent-skills/                  SKILLS FOR AI AGENTS (see 22): overview, rest, realtime, sync,
│                                  rules, auth-keys, sdk-typescript, sdk-kotlin, debug, templates/
│
├── sdks/                          INSTALLABLE LIBRARIES
│   ├── typescript/                npm  @monolith/client
│   ├── kotlin/                    Gradle multi-module
│   │   ├── monolith-core/             Kotlin Multiplatform (JVM, Android, iOS targets)
│   │   └── monolith-android/          Android library (AAR)
│   ├── dart/                      pub.dev  monolith_client
│   ├── swift/                     SwiftPM  Monolith
│   ├── python/                    PyPI  monolith-client
│   └── rust/                      crates.io  monolith-client
│
├── examples/                      curl, android-kotlin, web-typescript, python-script
├── tools/                         codegen templates, release scripts, conformance runner, agent-pack
├── bench/                         load harness, scenarios, results (see 17)
└── development/                   design docs (this folder) and TODO.md
```

Names such as `@monolith/client` and `dev.monolith` are placeholders until the project
name and registries are fixed.

## 2. The three ways to integrate

| Way | Needs an SDK? | Where it is documented |
|-----|---------------|------------------------|
| REST over HTTP | No | `docs/api/rest.md`, `api/openapi/` |
| WebSocket / SSE (realtime, sync, channels) | No | `docs/api/websocket.md`, `api/asyncapi/` |
| SDK (TypeScript, Kotlin/Android, Dart, ...) | Yes | `docs/api/typescript.md`, `docs/api/kotlin.md` |

REST and WebSocket are first-class and complete, so a language without an SDK loses no
feature. SDKs add convenience: session refresh, typed models, offline store, reconnect.

## 3. Contract-first flow

1. Change `api/` (OpenAPI, AsyncAPI, JSON Schemas, MCP tools) and add conformance vectors.
2. Server implements it; a CI check diffs the served static spec against `api/openapi/`.
3. `tools/codegen` and each SDK's `gen` script regenerate protocol types and clients.
4. Each SDK runs `api/conformance/` through `tools/conformance-runner` against a server
   container. An SDK that fails the vectors cannot be released.
5. Docs in `docs/api/` are updated in the same change.

## 4. Library packaging

### JS/TS Workspace (pnpm)

All JS/TS packages (`ui/`, `sdks/typescript/`, `examples/web-typescript/`) are unified
under a root `pnpm` monorepo workspace (`pnpm-workspace.yaml`).
- Root init: `pnpm install` initializes all projects at once.
- Inter-package dependencies use `workspace:*` (e.g. `@monolith/ui` and web examples
  consume `@monolith/client` directly from the tree).
- Common tasks: `pnpm dev` (backend + UI), `pnpm build`, `pnpm test`, `pnpm lint`; Rust under `pnpm rust:*`.

### TypeScript (npm: `@monolith/client`)

- ESM + CJS builds with bundled `.d.ts` (`tsup`), `sideEffects: false`, tree-shakeable.
- Entry points: `.` (core client), `./sync` (offline sync engine, optional),
  `./react-native` (storage and network adapters).
- Targets: browsers, Node >= 20, Bun, Deno, React Native.
- Protocol types generated from `api/openapi/monolith.v1.yaml` into
  `src/http/schema.gen.ts`. Hand-written code covers auth session, realtime and sync.
- Release: `npm publish --provenance` from CI on tag `ts-vX.Y.Z`.

### Kotlin / Android (Maven: `dev.monolith:monolith-core`, `dev.monolith:monolith-android`)

- `monolith-core`: Kotlin Multiplatform library with Ktor client and
  kotlinx.serialization. No Android dependencies. REST, WebSocket, auth, sync engine
  with a pluggable store interface.
- `monolith-android`: AAR that adds an SQLite/Room offline store, WorkManager background
  sync, and encrypted token storage. Depends on `monolith-core`.
- Consumers add one dependency: `implementation("dev.monolith:monolith-android:x.y.z")`.
  JVM-only services use `monolith-core`.
- minSdk 26 (to be confirmed against first consuming apps), consumer ProGuard rules
  bundled in the AAR.
- Release: Gradle `maven-publish` to Maven Central, tag `kt-vX.Y.Z`; an `android-kotlin`
  example module verifies the published coordinates.

### Others

| SDK | Package | Phase |
|-----|---------|-------|
| Dart / Flutter | pub.dev `monolith_client` | 2 |
| Python | PyPI `monolith-client` (httpx + websockets), aimed at scripts and backup jobs | 4 |
| Swift | SwiftPM `Monolith` | 5 |
| Rust | crates.io `monolith-client`, shares protocol types with the server | 5 |

## 5. SDK internal layout (same module names in every language)

| Module | Responsibility |
|--------|----------------|
| `http` | Request pipeline, retries with backoff, generated types |
| `auth` | Sign-in methods, token store, single-flight refresh |
| `collections` | Typed CRUD, query builder, batch |
| `realtime` | One multiplexed connection, subscriptions, resume by `seq` |
| `sync` | Local replica, outbox, pull/push, conflict policy |
| `analytics` | Batched, persisted event queue |
| `files` | Upload, download URLs, thumbnails |
| `errors` | Problem+json mapped to typed errors |

## 6. Versioning and release

- API versions (`v1`) are independent of SDK semver. Each SDK declares the API versions
  it supports and the minimum server version.
- Server, TypeScript SDK and Kotlin SDK release independently with prefixed tags
  (`server-v`, `ts-v`, `kt-v`, `dart-v`, ...).
- `api/CHANGELOG.md` records contract changes; each SDK's `CHANGELOG.md` links to it.
