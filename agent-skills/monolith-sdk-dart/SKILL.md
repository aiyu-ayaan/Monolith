---
name: monolith-sdk-dart
description: Use the Monolith Dart and Flutter SDK (monolith_client, monolith_flutter): Monolith.init, app-user auth streams, collections, onSnapshot streams, files and Flutter sign-in widgets. Use when writing Dart or Flutter code against a Monolith app.
---

# Dart / Flutter SDK

Not on pub.dev yet: use `sdks/dart` (`monolith_client`, pure Dart) and `sdks/flutter` (`monolith_flutter`). `monolith_flutter` was written without a Flutter toolchain and has not been compiled: expect fixes on first build.

```dart
final mono = await Monolith.init(url: BASE, app: 'myapp');          // Flutter: MonolithFlutter.init(...); memory storage unless you pass SessionStorage
await mono.auth().signIn(email, password);                           // signUp(email, password, name: ..), signOut(), accessToken()
mono.auth().changes.listen((s) {});                                  // AuthUnknown | AuthSignedOut | AuthSignedIn
await mono.auth().signInWithProvider('google');                      // builds the URL, passes it to urlLauncher; handleRedirect(uri) finishes it
final tasks = mono.collection<RecordModel>('tasks');                 // or collection<Task>('tasks', Task.fromJson)
final page = await tasks.list(filter: 'done = false', sort: '-created', limit: 20, cursor: c);  // page.nextCursor
await tasks.create({'title': 'x'}); await tasks.update(id, {'done': true}); await tasks.delete(id);
tasks.onSnapshot(SnapshotOptions(filter: 'done = false')).listen((snap) => render(snap.docs));  // Stream<QuerySnapshot<T>>
tasks.onDocSnapshot(id).listen((doc) {});                            // null when deleted or hidden
await mono.files.upload('avatars', 'u1.png', bytes, contentType: 'image/png');
```

- Default sign-in redirect is `monolith-<app>://auth`: register the scheme and add it to the app's allowed redirect URLs.
- Widgets (Flutter): `MonolithAuthScreen`, `AuthGate`, `AuthBuilder` (headless), `UserChip`, `SignOutButton`.
- Errors: `MonolithException` (alias `MonolithError`): `status`, `type`, `detail`, `requestId`, `errors`, `getFieldError`.
- No analytics service. `files.getURL` adds no credential: public buckets only.
