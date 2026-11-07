---
title: Dart and Flutter API
description: Every class and method of monolith_client: init, auth, database, realtime, storage.
group: API reference
order: 30
---

Package `monolith_client` (pure Dart). Calls return `Future`s; listeners return `Stream`s or an unsubscribe function. Flutter apps add `monolith_flutter` for secure storage, deep links and widgets.

## Initialize

```dart
final mono = await Monolith.init(url: 'https://api.example.com', app: 'shop');
// Flutter: await MonolithFlutter.init(url: ..., app: ...);
```

| Parameter | Default | Meaning |
|---|---|---|
| `url` | required | Server base URL |
| `app` | `'default'` | App id |
| `storage` | memory | `SessionStorage` for the session |
| `key` | `null` | An `mk_` API key |
| `redirectUri` | `monolith-<app>://auth` | Return URL for provider sign-in |
| `urlLauncher` | `null` | `Future<bool> Function(Uri)` that opens the provider page |
| `client`, `headers` | | Own `http.Client`, extra headers |

The same `url` and `app` return the same client. `Monolith.reset()` closes them. Members of `MonolithClient`: `auth([collection])`, `collection<T>(name, decoder)`, `files`, `realtime`, `batch(ops)`, `close()`.

## Authentication

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

## Database

```dart
final tasks = mono.collection<RecordModel>('tasks');
final typed = mono.collection<Task>('tasks', Task.fromJson);   // your decoder
```

| Method | Returns | HTTP |
|---|---|---|
| `list({page, perPage, cursor, limit, filter, sort, expand, fields, total})` | `ListResult<T>` | `GET .../records` |
| `getOne(id, {expand, fields})` | `T` | `GET .../records/{id}` |
| `create(Map)` | `T` | `POST` |
| `update(id, Map)` | `T` | `PATCH` |
| `replace(id, Map)` | `T` | `PUT` |
| `delete(id)` | `bool` | `DELETE` |
| `count({filter})` | `int` | `GET .../records:count` |

```dart
final page = await tasks.list(filter: 'done = false', sort: '-created', limit: 20);
final created = await tasks.create({'title': 'Buy milk'});
await tasks.update(created.id, {'done': true});
await tasks.delete(created.id);
```

`ListResult` has `items`, `total`, `cursor`, `nextCursor`, `prevCursor`. Filter syntax: [Rules and filter syntax](/reference/rules-syntax). Paging: [IDs and cursors](/reference/ids-and-cursors).

### Batch

`mono.batch([BatchOperation(...)])` returns `List<BatchResponseItem>`.

## Realtime

| Method | Returns |
|---|---|
| `subscribe(listener, [RealtimeSubscriptionOptions(filter, ids)])` | Unsubscribe `void Function()` |
| `onSnapshot([SnapshotOptions(filter, sort, limit)])` | `Stream<QuerySnapshot<T>>` |
| `onDocSnapshot(id)` | `Stream<T?>` (`null` once deleted or hidden) |

`RealtimeEvent` has `action` (`create`, `update`, `delete`, `leave`), `record`, `collection`, `seq`. `QuerySnapshot` has `docs` and `changes`. Reconnects send a fresh snapshot. Server errors arrive as stream errors.

```dart
tasks.onSnapshot(SnapshotOptions(filter: 'done = false', sort: '-created'))
    .listen((snap) => setState(() => items = snap.docs));
```

## Storage

`mono.files`:

| Method | Returns |
|---|---|
| `upload(bucket, key, data, {contentType})` | `Future<StorageObject>` |
| `getURL(bucket, key, {thumb})` | `String` |
| `list(bucket, {prefix, cursor, limit})` | `Future<StorageListResult>` (`items`, `nextCursor`) |
| `delete(bucket, key)` | `Future<bool>` |

`StorageObject`: `bucket`, `key`, `size`, `contentType`, `blake3`, `created`, `updated`.

## Errors

Failed calls throw `MonolithException` (alias `MonolithError`) with status, `type`, `detail`, `requestId` and field errors. See [Errors](/reference/errors).

## Flutter widgets

`MonolithAuthScreen`, `AuthGate`, `AuthBuilder` (headless), `UserChip`, `SignOutButton`. See [Sign-in screens](/build/auth/ui-kit).

## Limits and status

:::status partial
There is no analytics service in the Dart SDK. `monolith_flutter` was written without a Flutter toolchain and is untested.
:::

## Next steps

- [Dart and Flutter guide](/sdks/dart-flutter)
- [Flutter quickstart](/get-started/quickstart-flutter)
- [Realtime](/build/database/realtime)
