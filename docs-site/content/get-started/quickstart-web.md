---
title: Quickstart for web apps
description: Connect a browser app with the TypeScript SDK, sign a user in, write records and listen for live changes.
group: Quickstarts
order: 50
---

This quickstart connects a web app to Monolith with the TypeScript SDK. You will create a collection, sign a user in, add a record and watch the list update live.

## Before you start

- A running server and a superuser. See [Self-hosting](/get-started/self-host).
- Node.js 20 or newer.
- The TypeScript SDK, `@monolith/client`. It lives in the repository at `sdks/typescript`.

:::warning
The server does not send CORS headers today. A page served from another origin (for example Vite on `localhost:5173` calling `localhost:8090`) is blocked by the browser. Serve your app and the API from the same origin: use your bundler's dev proxy while developing, and a reverse proxy in production. The examples below call `window.location.origin` for that reason.
:::

## 1. Prepare the backend

Create an app and a collection where anyone can read and only signed-in users can write. Use the admin UI at `/_/`, or `curl` as in the [server quickstart](/get-started/quickstart-server):

```bash
export BASE=http://127.0.0.1:8090
TOKEN=$(curl -s $BASE/api/v1/system/auth/login -H 'content-type: application/json' \
  -d '{"email":"admin@example.com","password":"your-password"}' | jq -r .token)

curl -s $BASE/api/v1/system/apps -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"name":"shop"}'

curl -s $BASE/api/v1/apps/shop/collections -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{
    "name": "tasks",
    "fields": [
      {"name": "title", "type": "text", "required": true},
      {"name": "done",  "type": "bool"}
    ],
    "rules": {"list": "", "view": "", "create": "@request.auth.id != \"\"",
              "update": "@request.auth.id != \"\"", "delete": null}
  }'
```

The create rule `@request.auth.id != ""` means "any signed-in user".

By default an app lets people sign up with email and password. Check what an app offers with the public endpoint:

```bash
curl -s $BASE/api/v1/apps/shop/auth/providers
```

## 2. Install and create the client

```bash
pnpm add @monolith/client
```

```ts title="monolith.ts"
import { createClient } from "@monolith/client";

export const mono = createClient({
  url: window.location.origin,
  app: "shop",
});
export const tasks = mono.collection("tasks");
```

`createClient` takes the server `url`, the `app` name, and optionally a `key` or `token`. All requests go to `/api/v1/apps/<app>/...`.

## 3. Sign a user in

Create the user once, from a script or the admin UI (or let your sign-up form call `POST /api/v1/apps/shop/auth/register`):

```bash
curl -s $BASE/api/v1/apps/shop/auth/register -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","password":"correct horse","name":"Ada"}'
```

Then sign in from the browser:

```ts
await mono.auth().signInWithPassword("ada@example.com", "correct horse");
mono.auth().onChange((token) => console.log(token ? "signed in" : "signed out"));
```

The SDK keeps the access token in memory and sends it as `Authorization: Bearer ...` on every call. An access token lives 15 minutes and a refresh token 30 days.

:::status partial
Use `Monolith.init` from `@monolith/client` instead of `createClient` to get app-user auth with persistence: `Monolith.init({ url, app })`, then `mono.auth.signIn({ email, password })`. It stores the session in `localStorage` and refreshes the access token before it expires and once after a `401`. See [TypeScript](/sdks/typescript) and [Sign-in screens](/build/auth/ui-kit). The calls below use the older `mono.auth().signInWithPassword`, which keeps the token in memory only.
:::

## 4. Write and read records

```ts
const task = await tasks.create({ title: "Write the docs", done: false });
console.log(task.id, task._v);

await tasks.update(task.id, { done: true });

const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 20 });
console.log(page.items);
```

`update` sends `PATCH`, which changes only the fields you pass. `replace` sends `PUT`.

## 5. Listen for live changes

`onSnapshot` gives you the current result first, then a new snapshot after every change that affects it:

```ts
const stop = tasks.onSnapshot(
  (snap) => {
    render(snap.docs);
    for (const c of snap.changes) console.log(c.type, c.doc.id); // added | modified | removed
  },
  { filter: "done = false", sort: "-created", limit: 50 },
);

// later
stop();
```

If the connection drops, the SDK reconnects with exponential backoff and takes a fresh snapshot, so the list converges again. For raw events use `tasks.subscribe((event) => ...)`. See [Realtime](/build/database/realtime).

## Limits and status

- No CORS handling in the server (see the warning above).
- `tasks.count()` in the TypeScript SDK reads a `count` field, but the server answers with `total`. Until that is fixed, call `GET .../records:count` directly.
- `mono.batch()` in the SDK sends a different body than the server accepts. Use the REST call in [Batch operations](/build/database/batch).
- The list result's cursor is named `next` on the wire. The SDK type lists `nextCursor`; read `page.next`.
- No offline cache. See [Offline sync](/build/database/offline-sync).
- The package is not published to npm yet; use the workspace package in the repository.

## Next steps

- [Read and write records](/build/database/read-and-write)
- [Query records](/build/database/query)
- [Rules](/build/database/rules): who can do what.
