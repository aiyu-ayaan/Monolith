---
title: Analytics
description: TypeScript: send events from your app.
group: TypeScript
order: 80
---

Send events from your app.

```ts
mono.analytics.track("checkout_started", { items: 3 });
await mono.analytics.flush();
```

Events queue in memory and send every 5 s or at 20 events. A failed send is re-queued up to 500 events.

## Next steps

- [UI components](/sdks/typescript/ui)
- [SDKs overview](/sdks/overview)
