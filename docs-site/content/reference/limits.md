---
title: Limits
description: Hard limits of the server as implemented today.
order: 40
---

These are configured or coded limits, not benchmark results.

| Area | Limit |
|---|---|
| Request body (JSON) | 1 MiB (`413`) |
| List `limit` | default 50, max 1000 |
| Filter length | 4,096 characters |
| Sort fields | 3, plus `id` |
| Relation hops | 4 |
| Batch operations | 500 |
| Fields per collection | 61 |
| Password length | 8 to 72 (minimum settable per app) |
| Name | 100 characters |
| Email | 254 characters |
| Access token | 15 minutes |
| Refresh token | 30 days, single use |
| Superuser token | 1 hour |
| Allowed redirect URLs | 20 per app |
| Realtime connections | 10,000 per app, 32 collections each |
| Realtime replay | 20,000 changes |
| Realtime queue | 256 frames per connection |
| Webhook | 10 attempts, 10 s timeout, 32 parallel |
| Plugin | 8 MiB, 20 per app, 16 MiB memory, 50 M fuel default |

Storage limits are in [Limits and quotas](/build/storage/limits-and-quotas).
