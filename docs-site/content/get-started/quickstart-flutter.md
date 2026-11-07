---
title: Quickstart for Flutter and Dart
description: Connect a Flutter app with the Dart SDK, sign in, write records and bind a live snapshot Stream to your UI.
group: Quickstarts
order: 70
---

This quickstart connects a Flutter app to Monolith with the Dart SDK. You will create a client, sign a user in, write a record and show a live list with `StreamBuilder`.

## Before you start

- A running server the device can reach. On the Android emulator the host is `10.0.2.2`. See [Self-hosting](/get-started/self-host).
- An app and a `tasks` collection. The [web quickstart](/get-started/quickstart-web#1-prepare-the-backend) has the commands.
- Flutter 3 / Dart 3 or newer.
- The package `monolith_client`, from `sdks/dart` in the repository.

:::status partial
The package is not published to pub.dev yet. Add it as a path dependency:

```yaml title="pubspec.yaml"
dependencies:
  monolith_client:
    path: ../Monolith/sdks/dart
```
:::

## 1. Create the client

```dart title="monolith.dart"
import 'package:monolith_client/monolith_client.dart';

final mono = MonolithClient(
  const ClientOptions(url: 'http://10.0.2.2:8090', app: 'shop'),
);

final tasks = mono.collection<RecordModel>('tasks');
```

`mono.collection<T>(name, decoder)` returns a handle for one collection. Without a decoder you get `RecordModel`, a record with `id`, `created`, `updated` and your fields. Pass a decoder function to get your own class.

## 2. Sign in

```dart
await mono.auth().signInWithPassword('ada@example.com', 'correct horse');
```

Create the user first with `POST /api/v1/apps/shop/auth/register` (see the [web quickstart](/get-started/quickstart-web#3-sign-a-user-in)). The SDK sends the token on later requests. `mono.auth().onAuthStateChange` is a `Stream<String?>` of token changes.

:::status partial
`MonolithFlutter.init` (package `monolith_flutter`) adds secure session storage, refresh handling, deep links and sign-in widgets. That package was written without a Flutter toolchain and is untested. The pure Dart client below keeps sessions in memory unless you pass a `SessionStorage`. See [Dart and Flutter](/sdks/dart-flutter).
:::

## 3. Write records

```dart
final created = await tasks.create({'title': 'Write the docs', 'done': false});
await tasks.update(created.id, {'done': true});

final page = await tasks.list(filter: 'done = false', sort: '-created', limit: 20);
for (final t in page.items) {
  print(t.id);
}
```

`update` sends `PATCH` and changes only the fields you pass. `replace` sends `PUT`.

## 4. Show live data

`onSnapshot` returns a `Stream<QuerySnapshot<T>>`. The first event is the current result. Later events follow each change.

```dart
StreamBuilder<QuerySnapshot<RecordModel>>(
  stream: tasks.onSnapshot(
    const SnapshotOptions(filter: 'done = false', sort: '-created', limit: 50),
  ),
  builder: (context, snap) {
    final docs = snap.data?.docs ?? const [];
    return ListView(
      children: [for (final d in docs) ListTile(title: Text(d.id))],
    );
  },
);
```

Each snapshot also has `changes` (a list of `DocumentChange` with `type` of added, modified or removed). The stream reconnects with exponential backoff and takes a fresh snapshot after a drop. Server errors arrive as stream errors. For a single event stream use `tasks.subscribe((event) { ... })`, which returns an unsubscribe function. See [Realtime](/build/database/realtime).

## Limits and status

- `tasks.count()` reads a `count` field but the server returns `total`. Use `GET .../records:count` until the SDK is fixed.
- `mono.batch()` sends a body the server does not accept. See [Batch operations](/build/database/batch).
- `ListResult.nextCursor` does not match the server's field `next`; read it from `page.raw['next']`.
- No offline cache or sync. See [Offline sync](/build/database/offline-sync).
- Flutter web needs the app and API on one origin, because the server sends no CORS headers.

## Next steps

- [Query records](/build/database/query)
- [Rules](/build/database/rules)
- [Realtime](/build/database/realtime)
