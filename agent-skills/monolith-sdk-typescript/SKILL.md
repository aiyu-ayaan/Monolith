---
name: monolith-sdk-typescript
description: Use the @monolith/client package (browser, Node 20+, React): Monolith.init, app-user auth, typed collections, onSnapshot live queries, files and analytics. Use when writing TypeScript or React code against a Monolith app.
---

# TypeScript SDK (`@monolith/client`)

Not on npm yet: use `sdks/typescript` from the repository. Docs: site pages `/sdks/setup`, `/sdks/auth`, `/sdks/query`, `/sdks/write`, `/sdks/realtime`, `/sdks/storage`.

```ts
import { Monolith, MonolithError } from "@monolith/client";
const mono = Monolith.init({ url: BASE, app: "myapp" });          // restores the session, refreshes tokens, same url+app = same client
await mono.auth.signIn({ email, password });                       // signUp, signOut, updateProfile, changePassword, accessToken()
mono.auth.onChange((s) => s.status);                               // "unknown" | "signedOut" | "signedIn"
await mono.auth.signInWithProvider("google", { mode: "popup" });   // else redirects; Monolith.init finishes the redirect itself
const tasks = mono.collection<Task>("tasks");
const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 20, cursor });
await tasks.create({ title: "x" }); await tasks.update(id, { done: true }); await tasks.delete(id);
const stop = tasks.onSnapshot((snap) => render(snap.docs), { filter: "done = false", sort: "-created" });
await mono.files.upload("avatars", "u1.png", blob, { contentType: "image/png" });
```

- `mono.auth` is the end-user service. `mono.auth("_superusers")` is the system login (`signInWithPassword`).
- `apiKey: "mk_..."` in `init` is for servers and scripts, never for shipped apps.
- Realtime: `onSnapshot` (query), `onDocSnapshot(id, cb)` (one record, `null` when gone), `subscribe(cb, {filter, ids})` (raw events: create|update|delete|leave).
- Errors are `MonolithError` (`status`, `type`, `detail`, `requestId`, `errors`, `getFieldError(field)`). Keep `requestId` when reporting.
- Files: `files.getURL(bucket, key, { thumb })` adds no credential, so it only works for public buckets.
- UI: `@monolith/client/react` (`AuthGate`, `useAuth`, `MonolithAuth`), `@monolith/client/ui` (`<monolith-auth>`).

Known gaps (check before relying): `list()` cursor is `next` on the wire while the type says `nextCursor`; `count()` and `batch()` do not match the server (use `list({ total: "exact", limit: 1 })` and REST `/batch`); offline `sync` is a stub; `If-Match` and `dry_run` are REST-only.
