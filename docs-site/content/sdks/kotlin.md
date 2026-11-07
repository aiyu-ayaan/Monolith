---
title: Guide
description: Kotlin Multiplatform core, Android storage and Jetpack Compose sign-in.
group: Kotlin and Android
order: 10
---

Three modules: `monolith-core` (JVM, pure Kotlin on Ktor), `monolith-android` (encrypted session, Custom Tab, redirect activity) and `monolith-compose` (Material 3 sign-in).

## Set up

```kotlin
import dev.monolith.android.init
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")
```

The session lives in `EncryptedSharedPreferences` (private preferences if the Keystore fails). The default redirect is `monolith-shop://auth`. Declare `MonolithAuthRedirectActivity` with that scheme in your manifest and add the URI to the app's allowed redirect URLs.

## Auth

```kotlin
mono.auth.signIn("ada@example.com", "correct horse")
mono.auth.state.collect { s -> /* AuthState.Unknown | SignedOut | SignedIn(user) */ }
mono.auth.signInWithProvider("google")   // opens a Custom Tab
mono.auth.signOut()
```

## Data

```kotlin
val tasks = mono.collection<Task>("tasks")
val page = tasks.list(ListOptions(filter = "done = false", limit = 20))
tasks.onSnapshot().collect { snap -> render(snap.docs) }
```

## Compose

```kotlin
setContent { AuthGate(mono) { Home(mono) } }
```

`MonolithAuth`, `rememberAuthController`, `AuthUiOptions`, `UserChip`, `SignOutButton` are in `monolith-compose`.

## Limits and status

:::status partial
`AuthController` has unit tests with a mock server. Compose has logic tests only, no render tests. `monolith-android` has a JVM unit test for `init`, not an instrumented test. Provider buttons draw a colored monogram, not the official logos.
:::

## Next steps

- [Sign-in screens](/build/auth/ui-kit)
- [Android quickstart](/get-started/quickstart-android)
