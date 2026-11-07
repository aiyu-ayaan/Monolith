---
title: Sign-in screens
description: Ready-made sign-in UI for web, React, Compose and Flutter.
group: Use the SDKs
order: 90
---

The screens read the app's authentication config, so every provider you enable in the Admin UI appears by itself. Full guide: [Sign-in screens](/build/auth/ui-kit).

## Gate your app behind sign-in

:::tabs
```tsx title="TypeScript"
import { AuthGate, useAuth } from "@monolith/client/react";
<AuthGate mono={mono}><App /></AuthGate>
const { user, signOut } = useAuth(mono);
```
```kotlin title="Kotlin"
setContent { AuthGate(mono) { Home(mono) } }
```
```dart title="Dart"
runApp(MaterialApp(home: AuthGate(client: mono, child: const Home())));
```
:::

Other pieces: a `<monolith-auth>` element for plain web pages (`@monolith/client/ui`), `UserChip` and `SignOutButton` in Compose and Flutter, and `AuthBuilder` (Flutter) for a headless build.

## Limits and status

:::status partial
Compose and Flutter have logic tests only, no render tests. Python and Rust have no UI.
:::

## Next steps

- [Authentication](/sdks/auth)
