---
title: Analytics
description: Send custom events from TypeScript, Kotlin or Python.
group: Use the SDKs
order: 70
---

Track events from your app and read them in the Admin UI under **Analytics**.

## Track an event

:::tabs
```ts title="TypeScript"
mono.analytics.track("checkout_started", { items: 3 });
await mono.analytics.flush();
```
```kotlin title="Kotlin"
mono.analytics.track("checkout_started", buildJsonObject { put("items", 3) })
mono.analytics.flush()
```
```python title="Python"
mono.analytics.track("checkout_started", {"items": 3})
mono.analytics.flush()
```
:::

Events are queued and sent in batches. TypeScript flushes every 5 seconds or at 20 events and re-queues a failed send, up to 500 events. Call `flush()` before a short-lived process exits.

## Limits and status

:::status partial
Dart and Rust have no analytics service yet. Use REST: [REST API](/reference/rest-api).
:::

## Next steps

- [Errors](/sdks/errors)
