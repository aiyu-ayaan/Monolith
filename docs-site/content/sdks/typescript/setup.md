---
title: Set up
description: TypeScript: install, initialize and configure the client.
group: TypeScript
order: 20
---

Package `@monolith/client` for browsers and Node 20+. Calls return promises; listeners return an unsubscribe function.

## Initialize

```ts
import { Monolith } from "@monolith/client";
const mono = Monolith.init({ url: location.origin, app: "shop" });
```

Options: `url`, `app` (required), `storage`, `apiKey`, `redirectUri`, `fetch`, `headers`. The same `url` and `app` return the same client. `Monolith.reset()` forgets cached clients. `createClient(options)` builds a client without the cache and without restoring a session.

`mono` has `auth`, `collection(name)`, `files`, `realtime`, `analytics`, `batch(ops)`, `http`.

## Limits and status

:::status partial
`list()` returns the cursor as `next` on the wire while the type names it `nextCursor`; `count()` and `batch()` do not match the server. `sync` (offline) is a stub that records collection names only.
:::

## Next steps

- [Authentication](/sdks/typescript/auth)
- [SDKs overview](/sdks/overview)
