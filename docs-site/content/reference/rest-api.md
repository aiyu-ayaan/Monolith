---
title: REST API
description: Every endpoint of the v1 API, grouped, with the roles that may call it.
group: API
order: 10
---

Base path `/api/v1`. The machine-readable contract is `api/openapi/monolith.v1.yaml` in the repository. Send `Authorization: Bearer <token or mk_ key>`. Errors are `application/problem+json` (see [Errors](/reference/errors)). "SU" means superuser.

:::status partial
`GET /api/v1/apps/{app}/openapi.json` (a per-app spec) and per-app 429 limits other than API keys are not built.
:::

## System

| Method and path | Who | Purpose |
|---|---|---|
| `GET /system/health` | anyone | Liveness |
| `GET\|POST /system/install` | anyone with the setup token | First superuser |
| `POST /system/auth/login` | anyone | Superuser sign-in |
| `GET /system/me` | SU | Current superuser |
| `GET\|POST /system/superusers`, `DELETE /system/superusers/{id}` | SU | Manage superusers |
| `GET\|POST /system/apps`, `GET\|DELETE /system/apps/{app}` | SU | Manage apps |
| `GET /system/stats` | SU | Server stats |

## Collections {#collections}

| Method and path | Purpose |
|---|---|
| `GET\|POST /apps/{app}/collections` | List, create |
| `GET\|PUT\|DELETE /apps/{app}/collections/{c}` | Read, replace, delete |
| `POST /apps/{app}/collections:diff` | Plan a migration |

## Records {#records}

| Method and path | Purpose |
|---|---|
| `GET /apps/{app}/collections/{c}/records` | List ([parameters](/build/database/query)) |
| `POST .../records` | Create |
| `GET .../records:count` | Count |
| `GET\|PATCH\|PUT\|DELETE .../records/{id}` | Read, update, upsert, delete |
| `POST /apps/{app}/batch` | Up to 500 operations |

## Authentication {#authentication}

| Method and path | Who | Purpose |
|---|---|---|
| `GET /apps/{app}/auth/providers` | anyone | What to show on a sign-in screen |
| `POST .../auth/register`, `/login` | anyone | Returns a session |
| `POST .../auth/refresh`, `/logout` | refresh token | Rotate, end |
| `GET\|PATCH .../auth/me` | user token | Profile |
| `POST .../auth/password` | user token | Change password |
| `GET .../auth/oauth/{provider}/start?redirectUri=` | browser | Start provider sign-in |
| `GET .../auth/oauth/{provider}/callback` | provider | Return URL |
| `GET\|PUT .../auth/config` | SU | Methods, providers, allowed redirects |
| `GET\|POST .../users`, `PATCH\|DELETE .../users/{id}` | SU | User console |
| `GET\|POST .../api-keys`, `PUT\|DELETE .../api-keys/{id}` | SU | [API keys](/build/auth/api-keys) |

## Realtime, hooks, files, operations {#storage}

| Method and path | Purpose |
|---|---|
| `GET /apps/{app}/realtime` | [SSE stream](/build/database/realtime) |
| `GET\|POST .../webhooks`, `GET\|PUT\|DELETE .../webhooks/{id}`, `POST .../{id}/test`, `GET .../{id}/deliveries`, `POST .../deliveries/{did}/retry` | [Webhooks](/build/extend/webhooks) |
| `GET\|POST .../plugins`, `GET\|PUT\|DELETE .../plugins/{id}`, `GET .../plugins/{id}/logs` | [Plugins](/build/extend/plugins) |
| `GET\|PUT .../storage/config`, `POST .../storage:gc` | Storage settings, cleanup |
| `GET\|POST .../buckets`, `GET\|PUT\|DELETE .../buckets/{b}`, `GET .../buckets/{b}/objects`, `GET\|PUT\|DELETE .../buckets/{b}/objects/{key}` | [Storage](/build/storage/overview) |
| `GET\|POST .../backups`, `GET\|DELETE .../backups/{name}`, `POST .../backups:restore` | [Backups](/build/operate/backups) |
| `POST .../rules:explain`, `POST .../query:explain`, `GET .../logs`, `GET .../logs/slow`, `GET .../stats` | Debugging |

Paths under `/apps/{app}/` are prefixed with `/api/v1`. Check the OpenAPI file for exact methods and bodies.

## Next steps

- [Errors](/reference/errors)
- [Limits](/reference/limits)
