---
title: Guide
description: The pure Dart client and the Flutter layer with secure storage and sign-in widgets.
group: Dart and Flutter
order: 10
---

`monolith_client` is pure Dart (server, CLI, Flutter). `monolith_flutter` adds secure session storage, deep-link handling and widgets.

## Set up

```dart
final mono = await MonolithFlutter.init(url: 'https://api.example.com', app: 'shop');
runApp(MaterialApp(home: AuthGate(client: mono, child: const Home())));
```

Pure Dart: `await Monolith.init(url: ..., app: ...)`, with memory storage unless you pass a `SessionStorage`. Provider sign-in redirects to `monolith-<app>://auth` by default; register that scheme and add it to the allowed redirect URLs.

## Auth and data

```dart
await mono.auth().signIn('ada@example.com', 'correct horse');
mono.auth().changes.listen((s) => print(s)); // AuthUnknown | AuthSignedOut | AuthSignedIn
final page = await mono.collection('tasks').list(filter: 'done = false', limit: 20);
mono.collection('tasks').onSnapshot().listen((snap) => render(snap.docs));
```

Widgets: `MonolithAuthScreen`, `AuthGate`, `AuthBuilder` (headless), `UserChip`, `SignOutButton`.

## Limits and status

:::status partial
`monolith_client` is analysed and tested (37 tests). `monolith_flutter` was written without a Flutter toolchain: it has not been analysed, compiled or tested, so expect fixes on first build.
:::

## Next steps

- [Flutter quickstart](/get-started/quickstart-flutter)
- [Sign-in screens](/build/auth/ui-kit)
