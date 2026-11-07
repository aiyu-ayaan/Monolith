---
title: Authentication
description: Dart and Flutter: sign up, sign in, providers and session state.
group: Dart and Flutter
order: 30
---

Sign up, sign in, providers and session state.

`mono.auth()` is the app-user service.

| Method | Returns | Notes |
|---|---|---|
| `signUp(email, password, {name})` | `Future<AppUser>` | Creates and signs in |
| `signIn(email, password)` | `Future<AppUser>` | |
| `signInWithProvider(id, {redirectUri})` | `Future<Uri>` | The URL to open (also passed to `urlLauncher`) |
| `handleRedirect(uri)` | `Future<AppUser?>` | Call with the incoming deep link |
| `signOut()` | `Future<void>` | |
| `updateProfile({name})` | `Future<AppUser>` | |
| `changePassword(old, new)` | `Future<void>` | |
| `accessToken()` | `Future<String?>` | Refreshes first if needed |
| `providers({force})` | `Future<PublicAuth>` | |
| `changes` | `Stream<AuthState>` | `AuthUnknown`, `AuthSignedOut`, `AuthSignedIn` |
| `redirectErrors` | `Stream<MonolithException>` | |

```dart
mono.auth().changes.listen((s) {
  if (s is AuthSignedIn) print(s.user?.email);
});
await mono.auth().signIn('ada@example.com', 'correct horse');
```

`mono.auth('_superusers')` is the system login (`signInWithPassword`, `onChange`, `signOut`).

## Next steps

- [Database](/sdks/dart-flutter/database)
- [SDKs overview](/sdks/overview)
