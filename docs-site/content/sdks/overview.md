---
title: SDKs overview
description: One-line start, session handling and auth in the TypeScript, Kotlin, Dart, Flutter, Python and Rust SDKs.
order: 10
---

Every SDK follows the same idea: one call gives you a ready client that restores the stored session, refreshes tokens by itself and talks to one app. REST stays complete without an SDK, so you can use any language.

## Why use it

You do not write token refresh, retry-after-401 or provider redirect handling. The SDKs share one rule set: refresh shortly before expiry, refresh once after a `401` (single flight), then retry once.

## Start

:::tabs
```ts title="TypeScript"
import { Monolith } from "@monolith/client";
const mono = Monolith.init({ url: "https://api.example.com", app: "shop" });
```
```kotlin title="Kotlin"
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop") // Android
val mono = Monolith.init(url = "https://api.example.com", app = "shop")          // JVM
```
```dart title="Dart"
final mono = await Monolith.init(url: 'https://api.example.com', app: 'shop');
// Flutter: MonolithFlutter.init(...) adds secure storage and deep links
```
```python title="Python"
import monolith
mono = monolith.init(url="https://api.example.com", app="shop")
```
```rust title="Rust"
let mono = Monolith::init("https://api.example.com", "shop")?;
```
:::

`init` with the same `url` and `app` returns the same client. Older constructors (`createClient`, `MonolithClient(...)`) still work.

## Compare

| | TypeScript | Kotlin | Dart / Flutter | Python | Rust |
|---|---|---|---|---|---|
| Package | `@monolith/client` | `monolith-core`, `monolith-android`, `monolith-compose` | `monolith_client`, `monolith_flutter` | `monolith-client` | `monolith-client` |
| Default session storage | `localStorage` | Memory (Android: encrypted prefs) | Memory (Flutter: secure storage) | Memory | Memory |
| Auth state | `auth.onChange` | `auth.state` (`StateFlow`) | `auth().changes` (`Stream`) | `auth().on_state_change` | `auth().on_change()` |
| Provider sign-in | Redirect or popup | Custom Tab | System browser + deep link | Build URL, you open it | Build URL, you open it |
| Sign-in screen | `<monolith-auth>`, React | Compose | Flutter widgets | none | none |
| Realtime | `onSnapshot` | `onSnapshot` Flow | `onSnapshot` Stream | `on_snapshot` | `on_snapshot` |

## Limits and status

:::status partial
No SDK is published to a package registry yet; use the repository. The TypeScript `count()` and `batch()` helpers do not match the server's response and request shapes (see [Batch operations](/build/database/batch)). Offline sync is not in any SDK. Kotlin and TypeScript are tested against a mock server; Flutter has not been compiled.
:::

## Next steps

- [TypeScript](/sdks/typescript), [Kotlin](/sdks/kotlin), [Dart and Flutter](/sdks/dart-flutter), [Python](/sdks/python), [Rust](/sdks/rust)
- [Authentication overview](/build/auth/overview)
