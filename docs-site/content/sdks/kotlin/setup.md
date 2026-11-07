---
title: Set up
description: Kotlin and Android: install, initialize and configure the client.
group: Kotlin and Android
order: 20
---

Package `dev.monolith.client` (module `monolith-core`). Calls are `suspend` functions; listeners are `Flow`s or callbacks. Android adds `Monolith.init(context, ...)` from `monolith-android`.

## Initialize

```kotlin
// Android
import dev.monolith.android.init
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")

// JVM / any Kotlin
val mono = Monolith.init(url = "https://api.example.com", app = "shop")
```

| Parameter | Default | Meaning |
|---|---|---|
| `url` | required | Server base URL |
| `app` | `"default"` | App id |
| `key` | `null` | An `mk_` API key |
| `storage` | memory (Android: encrypted) | `SessionStorage` for the session |
| `redirectUri` | Android: `monolith-<app>://auth` | Return URL for provider sign-in |
| `browserLauncher` | Android: Custom Tab | Opens the provider page |
| `headers`, `httpClient`, `session` | | Extra headers, own Ktor client, `SessionOptions` |

The same `url` and `app` return the same `MonolithClient`. Members: `auth`, `auth(collection)`, `collection<T>(name)`, `files`, `analytics`, `realtime`, `batch(ops)`, `close()`.

## Limits and status

:::status partial
No instrumented Android tests. Compose has logic tests only. `batch` is not covered by a test against a real server.
:::

## Next steps

- [Authentication](/sdks/kotlin/auth)
- [SDKs overview](/sdks/overview)
