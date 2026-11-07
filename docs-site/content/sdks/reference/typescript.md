---
title: TypeScript API
description: Every class and method of @monolith/client: init, auth, database, realtime, storage, analytics.
group: API reference
order: 10
---

Package `@monolith/client` for browsers and Node 20+. Calls return promises; listeners return an unsubscribe function.

## Initialize

```ts
import { Monolith } from "@monolith/client";
const mono = Monolith.init({ url: location.origin, app: "shop" });
```

Options: `url`, `app` (required), `storage`, `apiKey`, `redirectUri`, `fetch`, `headers`. The same `url` and `app` return the same client. `Monolith.reset()` forgets cached clients. `createClient(options)` builds a client without the cache and without restoring a session.

`mono` has `auth`, `collection(name)`, `files`, `realtime`, `analytics`, `batch(ops)`, `http`.

## Authentication

`mono.auth` handles the app's end users.

| Method | Returns | Notes |
|---|---|---|
| `signUp({ email, password, name? })` | `Promise<AppUser>` | Creates and signs in |
| `signIn({ email, password })` | `Promise<AppUser>` | |
| `signInWithProvider(id, { redirectUri?, mode? })` | `Promise<AppUser \| null>` | `mode: "popup"` resolves with the user; `"redirect"` leaves the page |
| `handleRedirect(url?)` | `Promise<RedirectResult \| null>` | `init` calls it for you |
| `signOut()` | `Promise<void>` | Revokes the refresh token |
| `updateProfile({ name })` | `Promise<AppUser>` | |
| `changePassword(old, new)` | `Promise<void>` | The server ends all sessions |
| `accessToken()` | `Promise<string \| null>` | Refreshes first if needed |
| `getProviders()` | `Promise<PublicAuth>` | Enabled sign-in methods |
| `onChange(listener, { immediate? })` | unsubscribe | State is `unknown`, `signedOut` or `signedIn` |
| `onTokenChange(listener)` | unsubscribe | Token string or `null` |

```ts
const off = mono.auth.onChange((s) => {
  if (s.status === "signedIn") console.log(s.user.email);
});
await mono.auth.signIn({ email: "ada@example.com", password: "correct horse" });
```

`mono.auth("_superusers")` returns the system-login `AuthService`.

## Database

```ts
type Task = { title: string; done: boolean };
const tasks = mono.collection<Task>("tasks");
```

| Method | Returns | HTTP |
|---|---|---|
| `list(options)` | `ListResult<T>` | `GET .../records` |
| `getOne(id, { expand?, fields? })` | `T` | `GET .../records/{id}` |
| `create(data)` | `T` | `POST` |
| `update(id, data)` | `T` | `PATCH` (partial) |
| `replace(id, data)` | `T` | `PUT` |
| `delete(id)` | `boolean` | `DELETE` |
| `count({ filter? })` | `number` | `GET .../records:count` |

`ListOptions`: `page`, `perPage`, `cursor`, `limit`, `filter`, `sort`, `expand`, `fields`, `total` (`"none" | "exact" | "estimate"`). `ListResult`: `items`, `total`, `cursor`, `nextCursor`, `prevCursor`. Every record has `id`, `created`, `updated`, `_v`.

```ts
const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 20 });
const more = page.nextCursor ? await tasks.list({ limit: 20, cursor: page.nextCursor }) : null;
const t = await tasks.create({ title: "Buy milk", done: false });
await tasks.update(t.id, { done: true });
await tasks.delete(t.id);
```

Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch([{ method: "POST", path, body }])` returns `{ status, body }[]`.

## Realtime

| Method | Callback gets | Returns |
|---|---|---|
| `subscribe(listener, { filter?, ids? })` | `RealtimeEvent { action, record, collection, seq }` | unsubscribe |
| `onSnapshot(callback, { filter?, sort?, limit?, onError? })` | `QuerySnapshot { docs, changes }` | unsubscribe |
| `onDocSnapshot(id, callback)` | record or `null` | unsubscribe |

`action` is `create`, `update`, `delete` or `leave` (the record stopped matching your filter). `changes` hold `{ type: "added" | "modified" | "removed", doc }`. The first snapshot reports every document as `added`. `limit` defaults to 100, server cap 1000.

```ts
const stop = tasks.onSnapshot((snap) => render(snap.docs), { filter: "done = false", sort: "-created" });
```

## Storage

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, { contentType? })` | `StorageObject` |
| `getURL(bucket, key, { thumb? })` | `string` |
| `list(bucket, { prefix?, cursor?, limit? })` | `{ items, nextCursor? }` |
| `delete(bucket, key)` | `boolean` |

`data` is `Blob`, `ArrayBuffer`, `Uint8Array` or `string`. `StorageObject`: `bucket`, `key`, `size`, `contentType`, `blake3`, `created`, `updated`.

## Analytics

```ts
mono.analytics.track("checkout_started", { items: 3 });
await mono.analytics.flush();
```

Events queue in memory and send every 5 s or at 20 events. A failed send is re-queued up to 500 events.

## Errors

Failed calls reject with `MonolithError`: `status`, `type`, `detail`, `requestId`, `errors`, `getFieldError(field)`.

```ts
try { await tasks.create({}); }
catch (e) { if (e instanceof MonolithError) console.log(e.status, e.getFieldError("title"), e.requestId); }
```

## React and UI

`@monolith/client/react`: `useAuth(mono)`, `<AuthGate mono>`, `<MonolithAuth mono>`. `@monolith/client/ui`: the `<monolith-auth>` element. See [Sign-in screens](/build/auth/ui-kit).

## Limits and status

:::status partial
`list()` returns the cursor as `next` on the wire while the type names it `nextCursor`; `count()` and `batch()` do not match the server. `sync` (offline) is a stub that records collection names only.
:::

## Next steps

- [TypeScript guide](/sdks/typescript)
- [Query records](/build/database/query)
- [Realtime](/build/database/realtime)
