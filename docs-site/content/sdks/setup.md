---
title: Set up
description: Create a client in TypeScript, Kotlin, Dart, Python or Rust.
group: Use the SDKs
order: 10
---

One call gives you a ready client. It restores the stored session, refreshes tokens by itself and talks to one app. Pick your language in any code block: the choice is remembered on every page.

## Create the client

:::tabs
```ts title="TypeScript"
import { Monolith } from "@monolith/client";
const mono = Monolith.init({ url: "https://api.example.com", app: "shop" });
```
```kotlin title="Kotlin"
// Android: import dev.monolith.android.init
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")
// JVM
val mono = Monolith.init(url = "https://api.example.com", app = "shop")
```
```dart title="Dart"
final mono = await Monolith.init(url: 'https://api.example.com', app: 'shop');
// Flutter: await MonolithFlutter.init(url: ..., app: ...);
```
```python title="Python"
import monolith
mono = monolith.init(url="https://api.example.com", app="shop", storage="session.json")
```
```rust title="Rust"
use monolith_client::Monolith;
let mono = Monolith::init("https://api.example.com", "shop")?;
```
:::

Calling `init` again with the same `url` and `app` returns the same client (TypeScript, Kotlin, Dart). Older constructors (`createClient`, `MonolithClient(...)`) still work but do not restore a session.

## What the client gives you

| Member | Use it for |
|---|---|
| `auth` | Sign up, sign in, session: [Authentication](/sdks/auth) |
| `collection(name)` (Rust: `records(name)`) | Records: [Read and query](/sdks/query), [Write data](/sdks/write), [Realtime](/sdks/realtime) |
| `files` | Files: [Storage](/sdks/storage) |
| `analytics` | Events: [Analytics](/sdks/analytics) (not in Dart or Rust) |
| `batch(ops)` | Several writes at once |

## Options

| Option | Meaning |
|---|---|
| `url`, `app` | Server and app id. Required. (Dart, Kotlin and Python default `app` to `"default"`.) |
| `key` (TypeScript: `apiKey`) | An `mk_` API key, for servers and scripts |
| `storage` | Where the session is kept: [Authentication](/sdks/auth) |
| `redirectUri` | Return URL for provider sign-in. Android and Flutter default to `monolith-<app>://auth`. |
| `headers` | Extra headers on every request |
| `timeout` | Python and Rust only. Default 30 s in Python. |

Rust sets these on a builder: `Monolith::builder(url, app).session_file("session.json").timeout(..).build()?`.

## Use an API key instead of a user

A key (`mk_...`) acts as the server, not as a user. Use it in backends and scripts, never in a shipped app. Create one in the Admin UI: [API keys](/build/auth/api-keys).

## Limits and status

:::status partial
No SDK is published to a package registry yet, so use the repository. Offline sync is not in any SDK. Kotlin and TypeScript are tested against a mock server. Flutter code has not been compiled.
:::

## Next steps

- [Authentication](/sdks/auth)
- [Read and query](/sdks/query)

