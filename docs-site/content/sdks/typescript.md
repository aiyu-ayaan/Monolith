---
title: TypeScript
description: Browser, Node and React usage of @monolith/client.
group: Guides
order: 20
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

## Auth

```ts
await mono.auth.signUp({ email, password, name });
await mono.auth.signIn({ email, password });
await mono.auth.signInWithProvider("google");          // { mode: "popup" } resolves with the user
const off = mono.auth.onChange((s) => console.log(s.status, s.user)); // unknown | signedOut | signedIn
const token = await mono.auth.accessToken();           // refreshes first if needed
await mono.auth.signOut();
```

Also `getProviders()`, `updateProfile({name})`, `changePassword(old, new)`, `handleRedirect()`. `Monolith.init` handles a provider redirect in the URL by itself and removes the fragment.

## Data

```ts
const tasks = mono.collection("tasks");
const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 20 });
await tasks.create({ title: "Hi" });
tasks.onSnapshot((snap) => render(snap.docs), { filter: "done = false" });
```

## React

```tsx
import { AuthGate, useAuth } from "@monolith/client/react";
const { user, signOut } = useAuth(mono);
<AuthGate mono={mono}><App /></AuthGate>
```

## Limits and status

:::status partial
`list()` returns the cursor as `next` on the wire, while the type names it `nextCursor`. `count()` and `batch()` do not match the server. The DOM rendering of `<monolith-auth>` and the React components has no render tests. See [Sign-in screens](/build/auth/ui-kit).
:::

## Next steps

- [Query records](/build/database/query)
- [Realtime](/build/database/realtime)
