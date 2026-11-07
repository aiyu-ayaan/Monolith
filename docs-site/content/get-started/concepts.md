---
title: Concepts
description: The words Monolith uses: apps, collections, records, IDs, the change log, rules and the data directory.
order: 20
---

This page defines the terms you will meet in every other page. Read it once; the rest of the docs build on it.

## The model at a glance

```text
server (one process, one /data directory)
├── superusers                      operators who manage the server (system.db)
└── app "shop"                      an isolated backend for one product
    ├── collections                 tables with typed fields and rules
    │   └── records                 rows
    ├── users                       the app's end users
    ├── API keys, webhooks, plugins
    ├── storage buckets             files
    └── change log (seq 1, 2, 3 …)  every committed write, in order
```

## Superusers and the control plane

A **superuser** manages the server: creates apps, edits schemas, reads logs. Superusers are stored in the control-plane database `system.db`. The first superuser is created from a one-time setup link (see [Self-hosting](/get-started/self-host)). A superuser token bypasses all access rules, so keep it out of client code.

## Apps

An **app** is one isolated backend, for example `shop` or `notes`. Each app has its own data directory and its own SQLite database file at `/data/apps/<app>/data.db`. App names are lowercase identifiers matching `^[a-z][a-z0-9_]{0,62}$`.

Isolation matters in practice. Each app database has one dedicated writer thread and its own pool of reader threads, so a slow write in one app does not queue behind another app's writes. Every API path for app data starts with `/api/v1/apps/{app}/`.

## Collections

A **collection** is a table with a name, typed fields, access rules and optional indexes. Today there is one collection type, `base`. Fields are strictly typed: `text`, `email`, `url`, `number`, `bool`, `date`, `select`, `relation` and `json`. See [Collections and schema](/build/database/collections-and-schema).

Tables are created as SQLite `STRICT` tables, so the database itself rejects values of the wrong type.

## Records

A **record** is a row. Every record has four system fields in addition to your own:

| Field | Type | Meaning |
|---|---|---|
| `id` | string | 26-character record ID |
| `created` | string | RFC 3339 UTC timestamp with microseconds |
| `updated` | string | RFC 3339 UTC timestamp with microseconds |
| `_v` | integer | The `seq` of the last change to this record. It is the record's version and its `ETag`. |

## IDs

IDs are **UUIDv7**. The database stores them as 16 bytes. On the wire they are 26 lowercase characters in Crockford base32, for example `01jbaa1cy0fqqr28t5cy4tqkff`.

UUIDv7 starts with a timestamp, so new records land at the end of the index and sort roughly by creation time. Clients may generate an ID themselves and send it on create. This lets a client create related records before it has talked to the server. The server accepts only a real UUIDv7 whose timestamp is within the last 30 days. Details: [IDs and cursors](/reference/ids-and-cursors).

## The change log and `seq`

Every committed write is appended to a per-app log called `_changes`. Each entry gets a number, `seq`. The numbers are monotonic and have no gaps.

- The record's `_v` field is the `seq` of its last change.
- Realtime events carry the `seq` of the change. A client that reconnects sends the last `seq` it saw as `since`, and the server replays what it missed. See [Realtime](/build/database/realtime).
- The design for offline sync uses the same log. See [Offline sync](/build/database/offline-sync).

The log row is written in the same transaction as the data change, so the two never disagree.

## Rules

A **rule** is a short expression that decides who may do an operation. Each collection has five: `list`, `view`, `create`, `update`, `delete`. A rule is one of:

| Rule value | Meaning |
|---|---|
| `null` | Only superusers. This is the default. |
| `""` (empty string) | Anyone, including anonymous callers. |
| an expression | Allowed when it is true for the record and the caller. |

Example: `owner = @request.auth.id` lets only the owner through. The same language is used for the `filter` parameter on list requests. See [Rules](/build/database/rules) and the [syntax reference](/reference/rules-syntax).

## Who is calling

Every request carries one of these credentials, or none:

| Credential | How it is sent | What rules see |
|---|---|---|
| None | no header | Anonymous. `@request.auth.id` is empty. |
| App user token | `Authorization: Bearer <access token>` | The signed-in user. Works only on its own app. |
| API key | `Authorization: Bearer mk_...` | Anonymous to rules, narrowed by scopes. A key with the `records:admin` scope bypasses rules. |
| Superuser token | `Authorization: Bearer <token>` | Everything. |

A bad or expired token is a `401`. It is never treated as anonymous.

## One container, one volume

Everything the server stores lives under one directory, set by `MONOLITH_DATA` (default `./monolith_data`): the control-plane `system.db`, one folder per app, backups and file blobs. Back up that directory (or use the built-in backup endpoint) and you have backed up everything. See [Self-hosting](/get-started/self-host).

## Next steps

- Make requests against a real server: [Server quickstart](/get-started/quickstart-server).
- Design your tables: [Database overview](/build/database/overview).
- Protect your data: [Rules](/build/database/rules).
