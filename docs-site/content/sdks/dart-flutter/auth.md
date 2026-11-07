---
title: Authentication
description: Dart and Flutter: sign up, sign in, providers and session state.
group: Dart and Flutter
order: 30
---

Sign users in and out, watch the session and call the API as them.

## Sign up and sign in

```dart
final user = await mono.auth().signUp('ada@example.com', 'correct horse', name: 'Ada');
final user2 = await mono.auth().signIn('ada@example.com', 'correct horse');
```

Both return `AppUser` (`id`, `email`, `name`, `created`, `disabled`, `lastLogin`, `providers`).

## Watch the signed-in state

```dart
mono.auth().changes.listen((s) {
  switch (s) {
    case AuthUnknown(): showSplash();      // the stored session is still being restored
    case AuthSignedOut(): showLogin();
    case AuthSignedIn(:final user): showHome(user);
  }
});
```

With `monolith_flutter`, `AuthGate` and `AuthBuilder` do this for you.

## Sign in with a provider

```dart
final providers = (await mono.auth().providers()).providers;
await mono.auth().signInWithProvider('google');   // builds the URL and passes it to urlLauncher
```

Register the scheme `monolith-<app>` (or your own `redirectUri`) for deep links and add it to the app's allowed redirect URLs. Pass the incoming link to `mono.auth().handleRedirect(uri)`; `monolith_flutter` does this for you. Failures arrive on `redirectErrors`.

## Profile and password

```dart
await mono.auth().updateProfile(name: 'Ada L.');
await mono.auth().changePassword('correct horse', 'battery staple');   // the server ends all sessions
```

## Get the token for your own requests

```dart
final token = await mono.auth().accessToken();   // refreshes first if needed
```

## Sign out

```dart
await mono.auth().signOut();
```

## Where the session is kept

Memory in `monolith_client`. `MonolithFlutter.init` uses secure storage. Pass your own `SessionStorage` to change it.

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

- [Read and query](/sdks/dart-flutter/query)
- [SDKs overview](/sdks/overview)
