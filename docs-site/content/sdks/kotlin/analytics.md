---
title: Analytics
description: Kotlin and Android: send events from your app.
group: Kotlin and Android
order: 80
---

Send events from your app.

```kotlin
mono.analytics.track("checkout_started", buildJsonObject { put("items", 3) })
mono.analytics.flush()
```

Events are queued and sent in batches.

## Next steps

- [Errors](/sdks/kotlin/errors)
- [SDKs overview](/sdks/overview)
