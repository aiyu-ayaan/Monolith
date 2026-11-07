# monolith_flutter

Flutter layer for Monolith: `MonolithFlutter.init` (secure session storage, deep links for provider sign-in),
`MonolithAuthScreen`, `AuthGate`, headless `AuthBuilder`, `UserChip`, `SignOutButton`.

```dart
final mono = await MonolithFlutter.init(url: 'https://api.example.com', app: 'shop');
runApp(MaterialApp(home: AuthGate(client: mono, child: const Home())));
```

Provider sign-in returns to `monolith-<app>://auth` by default. Register that scheme in the app (Android intent filter,
iOS `CFBundleURLTypes`) and add it to the app's *Allowed redirect URLs* in the Admin UI.

Status: written without a Flutter toolchain. The pure Dart client (`sdks/dart`) is tested; this package has not been
analyzed, compiled or tested yet. Run `flutter pub get && flutter analyze && flutter test` before using it.
