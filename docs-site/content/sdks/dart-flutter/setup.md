---
title: Set up
description: Dart and Flutter: install, initialize and configure the client.
group: Dart and Flutter
order: 20
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

## Limits and status

:::status partial
There is no analytics service in the Dart SDK. `monolith_flutter` was written without a Flutter toolchain and is untested.
:::

## Next steps

- [Authentication](/sdks/dart-flutter/auth)
- [SDKs overview](/sdks/overview)
