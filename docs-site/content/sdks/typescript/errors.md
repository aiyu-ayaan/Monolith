---
title: Errors
description: TypeScript: error types and what they carry.
group: TypeScript
order: 90
---

Error types and what they carry.

Failed calls reject with `MonolithError`: `status`, `type`, `detail`, `requestId`, `errors`, `getFieldError(field)`.

```ts
try { await tasks.create({}); }
catch (e) { if (e instanceof MonolithError) console.log(e.status, e.getFieldError("title"), e.requestId); }
```

## Next steps

- [SDKs overview](/sdks/overview)
