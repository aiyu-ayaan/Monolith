---
title: Kotlin and Android
description: Kotlin Multiplatform core, Android storage and Jetpack Compose sign-in.
group: Platform guides
order: 210
---

Three modules: `monolith-core` (JVM, pure Kotlin on Ktor), `monolith-android` (encrypted session, Custom Tab, redirect activity) and `monolith-compose` (Material 3 sign-in).

## Set up

```kotlin
import dev.monolith.android.init
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")
```

The session lives in `EncryptedSharedPreferences` (private preferences if the Keystore fails). The default redirect is `monolith-shop://auth`. Declare `MonolithAuthRedirectActivity` with that scheme in your manifest and add the URI to the app's allowed redirect URLs.

## Compose

```kotlin
setContent { AuthGate(mono) { Home(mono) } }
```

`MonolithAuth`, `rememberAuthController`, `AuthUiOptions`, `UserChip`, `SignOutButton` are in `monolith-compose`.

## Auth, data, files and realtime

These work the same in every language and live on shared pages: [Authentication](/sdks/auth), [Read and query](/sdks/query), [Write data](/sdks/write), [Realtime](/sdks/realtime) and [Storage](/sdks/storage).

## Limits and status

:::status partial
`AuthController` has unit tests with a mock server. Compose has logic tests only, no render tests. `monolith-android` has a JVM unit test for `init`, not an instrumented test. Provider buttons draw a colored monogram, not the official logos.
:::

## Next steps

- [Sign-in screens](/build/auth/ui-kit)
- [Android quickstart](/get-started/quickstart-android)
