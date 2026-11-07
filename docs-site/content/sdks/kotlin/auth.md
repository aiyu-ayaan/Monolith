---
title: Authentication
description: Kotlin and Android: sign up, sign in, providers and session state.
group: Kotlin and Android
order: 30
---

Sign users in and out, watch the session and call the API as them.

## Sign up and sign in

```kotlin
val user = mono.auth.signUp("ada@example.com", "correct horse", name = "Ada")
val user2 = mono.auth.signIn("ada@example.com", "correct horse")
```

Both are `suspend` and return `AppUser` (`id`, `email`, `name`, `created`, `disabled`, `lastLogin`, `providers`, and `displayName`).

## Watch the signed-in state

```kotlin
mono.auth.state.collect { s ->
    when (s) {
        AuthState.Unknown -> showSplash()          // the stored session is still being restored
        AuthState.SignedOut -> showLogin()
        is AuthState.SignedIn -> showHome(s.user)
    }
}
```

`state` is a `StateFlow`, so `collectAsState()` works in Compose. `mono.auth.user` is the current user or `null`.

## Sign in with a provider

```kotlin
val providers = mono.auth.providers().providers        // [AuthProviderInfo(id = "google", name = "Google"), ...]
mono.auth.signInWithProvider("google")                 // opens a Custom Tab on Android
```

Declare `MonolithAuthRedirectActivity` with the scheme `monolith-<app>` in the manifest, and add `monolith-<app>://auth` to the app's allowed redirect URLs. The activity hands the returned URI to `mono.auth.handleRedirect(uri)`. Failures arrive on `mono.auth.redirectFailures`, and `mono.auth.redirecting` is `true` while a sign-in is open.

## Profile and password

```kotlin
mono.auth.updateProfile("Ada L.")
mono.auth.changePassword("correct horse", "battery staple")   // the server ends all sessions
```

## Get the token for your own requests

```kotlin
val token = mono.auth.accessToken()   // refreshes first if needed
```

## Sign out

```kotlin
mono.auth.signOut()   // clears the local session, then revokes the refresh token
```

## Where the session is kept

`Monolith.init(context, ...)` uses `EncryptedSessionStorage` (private preferences if the Keystore fails). On the JVM it is memory unless you pass a `SessionStorage`.

## Errors

| Status | Meaning |
|---|---|
| 401 | Wrong email or password, or an expired session |
| 403 | Sign-up is closed for this app |
| 409 | The email is already registered |
| 400 | Validation failed, for example a password shorter than the app's minimum (default 8) |

## Sessions

An access token lasts 15 minutes and a refresh token 30 days. The SDK refreshes shortly before expiry and once after a `401`, one refresh at a time. A rejected refresh token signs the user out. Details: [Sessions and tokens](/build/auth/sessions-and-tokens).

Which sign-in methods exist is configured per app in the Admin UI under **Authentication**: [Authentication overview](/build/auth/overview), [Providers](/build/auth/providers).

## Next steps

- [Read and query](/sdks/kotlin/query)
- [SDKs overview](/sdks/overview)
