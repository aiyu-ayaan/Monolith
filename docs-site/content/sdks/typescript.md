---
title: TypeScript and web
description: Browser, Node and React usage of @monolith/client.
group: Platform guides
order: 200
---

`@monolith/client` works in browsers and Node 20+. Subpath `@monolith/client/ui` adds `<monolith-auth>` and `@monolith/client/react` adds hooks and components.

## Set up

```ts
import { Monolith } from "@monolith/client";
const mono = Monolith.init({ url: location.origin, app: "shop" });
```

| Option | Meaning |
|---|---|
| `url`, `app` | Required |
| `storage` | `getItem/setItem/removeItem`, sync or async. Default `localStorage`, else memory. |
| `apiKey` | An `mk_` key for server use |
| `redirectUri` | Default return URL for provider sign-in |
| `fetch`, `headers` | Custom transport |

## React

```tsx
import { AuthGate, useAuth } from "@monolith/client/react";
const { user, signOut } = useAuth(mono);
<AuthGate mono={mono}><App /></AuthGate>
```

## Auth, data, files and realtime

These work the same in every language and live on shared pages: [Authentication](/sdks/auth), [Read and query](/sdks/query), [Write data](/sdks/write), [Realtime](/sdks/realtime) and [Storage](/sdks/storage).

## Limits and status

:::status partial
`list()` returns the cursor as `next` on the wire, while the type names it `nextCursor`. `count()` and `batch()` do not match the server. The DOM rendering of `<monolith-auth>` and the React components has no render tests. See [Sign-in screens](/build/auth/ui-kit).
:::

## Next steps

- [Query records](/build/database/query)
- [Realtime](/build/database/realtime)
