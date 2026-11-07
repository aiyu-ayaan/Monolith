---
title: Authentication
description: Kotlin and Android: sign up, sign in, providers and session state.
group: Kotlin and Android
order: 30
---

Sign up, sign in, providers and session state.

`mono.auth` is an `AppAuth` for the app's end users.

| Member | Returns | Notes |
|---|---|---|
| `signUp(email, password, name = null)` | `AppUser` | Creates the account and signs in |
| `signIn(email, password)` | `AppUser` | |
| `signInWithProvider(id, redirectUri = null)` | `String` | Returns the URL to open, and launches it if a `browserLauncher` is set |
| `handleRedirect(uri)` / `handleRedirectAsync(uri)` | `RedirectResult` / `Job` | Call with the URI your redirect activity receives |
| `signOut()` | | Revokes the refresh token |
| `updateProfile(name)` | `AppUser` | |
| `changePassword(old, new)` | | The server ends all sessions |
| `accessToken()` | `String?` | Refreshes first if needed |
| `providers(forceRefresh = false)` | `PublicAuth` | Enabled sign-in methods |
| `state` | `StateFlow<AuthState>` | `Unknown`, `SignedOut`, `SignedIn(user)` |
| `user` | `AppUser?` | Current user |
| `redirecting`, `redirectFailures` | `StateFlow<Boolean>`, `SharedFlow` | Provider flow status and errors |

```kotlin
val user = mono.auth.signUp("ada@example.com", "correct horse", name = "Ada")
mono.auth.state.collect { s ->
    when (s) {
        AuthState.Unknown -> showSplash()
        AuthState.SignedOut -> showLogin()
        is AuthState.SignedIn -> showHome(s.user.displayName)
    }
}
```

`mono.auth("_superusers")` returns an `AuthService` for the system login (`signInWithPassword`, `token`, `setToken`, `signOut`).

## Next steps

- [Database](/sdks/kotlin/database)
- [SDKs overview](/sdks/overview)
