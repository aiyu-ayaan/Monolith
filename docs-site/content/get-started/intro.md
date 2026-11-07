---
title: What is Monolith
description: A self-hosted backend for many apps in one Rust process and one data directory.
group: Basics
order: 10
---

Monolith is a backend you run yourself. One server process gives each of your apps a database with access rules, user sign-in, live updates, file storage, API keys, webhooks, plugins and backups. You talk to it over plain HTTP and JSON, or through SDKs for TypeScript, Kotlin, Dart, Python and Rust.

## Why it exists

Most apps need the same backend pieces: users, a database, rules about who may read and write, live updates, files and backups. Building them again for every app costs time. Hosted services remove that work, but they bill per use and keep your data on their servers. Running a separate PocketBase or Postgres stack per app multiplies the servers you maintain.

Monolith takes the opposite route: one process, one `/data` directory, many isolated apps.

- **One server, many apps.** Each app has its own database file, schema, rules, users, API keys and files. One app's schema change or heavy load does not touch another app's data.
- **Rules next to the data.** You write access rules per collection. The server compiles them once per schema version into SQL and in-memory checks.
- **Plain HTTP first.** The REST API is described by an OpenAPI 3.1 contract (`api/openapi/monolith.v1.yaml`). Every feature works with `curl`; SDKs are a convenience.
- **Easy to debug.** Every response has an `X-Request-Id`. There are endpoints that explain a rule, explain a query plan, and show the trace of a request.

## Who it is for

Monolith suits you if you:

- run several small or medium apps and want one backend for all of them,
- want your data on your own disk and your own backups,
- like the Firebase model (collections, rules, live listeners) but want to host it,
- are comfortable running a single Linux process or container.

It is not the right tool if you need a multi-node cluster, a managed service with a support contract, or features that are still on the roadmap (see [Limits and status](#limits-and-status)).

## How it compares

This table describes design and feature scope, not speed. Monolith has no published head-to-head benchmark against either product.

| | Monolith | Firebase | PocketBase |
|---|---|---|---|
| Hosting | Self-hosted | Google-hosted | Self-hosted |
| Apps per server | Many, isolated per app | Many projects, each hosted by Google | One app per instance |
| Data model | Relational (SQLite, strict tables) | Document stores | Relational (SQLite) |
| Access rules | Filter expressions, compiled to SQL and in-memory checks | Rules language | Filter expressions |
| Realtime | SSE today, per-collection and filtered | Built in | SSE |
| Offline sync | Designed, not built | Built in | No |
| Client SDKs | TypeScript, Kotlin, Dart, Python, Rust | Many platforms | JavaScript, Dart |
| Extensions | WASM plugins (after-commit handlers) and webhooks | Cloud Functions | Go and JavaScript hooks |

Monolith borrows PocketBase's collections and rules model on purpose. If you know PocketBase filter syntax, the [rules syntax](/reference/rules-syntax) will look familiar.

## What works today

The server is in active development. The table shows what you can use now. Each row links to the page that explains it.

| Area | State |
|---|---|
| Apps, collections, records, batch, query | Implemented. See [Database](/build/database/overview). |
| Access rules | Implemented. See [Rules](/build/database/rules). |
| Realtime over SSE | Implemented, not yet benchmarked. See [Realtime](/build/database/realtime). |
| End-user sign-in (email and password, OAuth) | Implemented. Email verification, password reset and MFA are not. |
| API keys, storage buckets, backups, webhooks, WASM plugins | Implemented in a first version. |
| Offline sync | Not implemented. See [Offline sync](/build/database/offline-sync). |
| WebSocket, channels, analytics ingestion, MCP | Not implemented. |

:::status partial
Phase 1 (the core data plane) is nearly finished and the first Phase 2 pieces have landed. Some SDK methods still lag the server. Where that matters, the pages say so.
:::

## Limits and status

- The primary storage engine is SQLite, one file per app. The final engine choice is still pending a benchmark bake-off, so treat the on-disk layout as subject to change before 1.0.
- A Docker image is designed but the repository does not ship a Dockerfile yet. See [Self-hosting](/get-started/self-host).
- Monolith runs on a single node. There is no clustering.
- The API is versioned as `v1` and marked draft.

## Next steps

- Learn the vocabulary in [Concepts](/get-started/concepts).
- Run the server and make your first request: [Server quickstart](/get-started/quickstart-server).
- Pick a client: [Web](/get-started/quickstart-web), [Android](/get-started/quickstart-android) or [Flutter](/get-started/quickstart-flutter).
