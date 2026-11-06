---
name: monolith-sdk-typescript
description: Use the @monolith/client npm package (web, Node, Bun, Deno, React Native): auth, typed collections, realtime, offline sync, analytics, files.
---

# TypeScript SDK

```ts
import { createClient } from "@monolith/client";
const mono = createClient({ url: BASE, app: "myapp", key: "mk_pub_..." });
await mono.auth("users").signInWithPassword(email, pw);
const page = await mono.collection<Task>("tasks").list({ filter: "status = 'open'", limit: 20 });
const off = mono.collection("tasks").subscribe({ filter: "owner = @request.auth.id" }, (ev) => {});
```

- Types: `monolith codegen --lang ts --out src/monolith.gen.ts`.
- Offline: `import { enableSync } from "@monolith/client/sync"`. React Native: `@monolith/client/react-native` adapters.
- Debug: `createClient({ debug: true })` logs request id, timing and realtime frames.
- Full reference: `docs/api/typescript.md`.
