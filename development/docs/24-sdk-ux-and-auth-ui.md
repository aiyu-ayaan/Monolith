# 24 — SDK ergonomics and the Auth UI kit

Goal: a developer gets a working app with two lines, the way Firebase feels, and a finished sign-in screen
that already shows the providers the server has enabled. Every part can be replaced or hidden.

## 1. Two-line start (all SDKs)

```ts
const mono = Monolith.init({ url: "https://api.example.com", app: "shop" });   // line 1: client, session storage, auto refresh
mono.collection("tasks").onSnapshot((snap) => render(snap.docs));               // line 2: live data
```

```kotlin
val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")      // Android: encrypted session storage
mono.collection<Task>("tasks").onSnapshot().collect { snap -> render(snap.docs) }       // Flow
```

```dart
final mono = await Monolith.init(url: 'https://api.example.com', app: 'shop');
mono.collection('tasks').onSnapshot().listen((snap) => render(snap.docs));
```

Rules for `init`:
- Returns a ready client. No second call is needed to restore a session: it loads the stored session and
  refreshes it in the background.
- Persistent storage by default (web: `localStorage`; Android: `EncryptedSharedPreferences`; Flutter: secure
  storage when available, else memory), overridable with a `storage` option; memory for servers and scripts.
- One shared HTTP stack: the access token is refreshed before it expires and once after a `401`
  (single-flight: concurrent calls wait for the same refresh, then retry once).
- `init` with the same `url` and `app` twice returns the same instance.
- Existing constructors (`createClient`, `MonolithClient(...)`) keep working.

## 2. Auth API (client side, all SDKs)

Naming follows each language, the capabilities are the same.

| Capability | TypeScript | Kotlin | Dart |
|-----------|------------|--------|------|
| What can I show? | `auth.getProviders()` → `PublicAuth` | `auth.providers()` | `auth.providers()` |
| Email sign-up | `auth.signUp({email,password,name?})` | `auth.signUp(email, password, name)` | `auth.signUp(...)` |
| Email sign-in | `auth.signIn({email,password})` | `auth.signIn(email, password)` | `auth.signIn(...)` |
| Provider sign-in | `auth.signInWithProvider(id)` | `auth.signInWithProvider(id)` | `auth.signInWithProvider(id)` |
| Finish provider sign-in | `auth.handleRedirect()` (called by `init` on web) | `auth.handleRedirect(uri)` | `auth.handleRedirect(uri)` |
| Sign out | `auth.signOut()` | `auth.signOut()` | `auth.signOut()` |
| Current user | `auth.user` | `auth.user` | `auth.user` |
| Watch auth state | `auth.onChange(cb)` → unsubscribe | `auth.state: StateFlow<AuthState>` | `auth.changes: Stream<AuthState>` |
| Profile | `auth.updateProfile({name})`, `auth.changePassword(old, new)` | same | same |
| Token for custom calls | `auth.accessToken()` (refreshes if needed) | `auth.accessToken()` | `auth.accessToken()` |

`AuthState` is one of `Unknown` (session being restored), `SignedOut`, `SignedIn(user)`.

Server endpoints used (doc 08 "Implemented today", `api/openapi`): `GET auth/providers`, `POST auth/register|login|refresh|logout`,
`GET|PATCH auth/me`, `POST auth/password`, `GET auth/oauth/{provider}/start?redirectUri=`.

### Provider sign-in per platform

The server redirects the browser back to `redirectUri` with `#token=…&refreshToken=…&expiresAt=…` or `#error=…`.
- Web: `signInWithProvider(id, { redirectUri? })` navigates to `start`; `redirectUri` defaults to the current page
  without hash. On load, `init` calls `handleRedirect()`, stores the session, and removes the fragment
  (`history.replaceState`). Popup mode (`{ mode: "popup" }`) opens a window and receives the session by `postMessage`
  from the same-origin redirect page.
- Android: opens a Custom Tab on `start` with `redirectUri = <scheme>://auth` (default `monolith-<app>://auth`,
  override in `init`); the app declares an intent filter for that scheme; `handleRedirect(intent.data)` completes it.
  The library ships a `MonolithAuthRedirectActivity` the app can reuse.
- Flutter: `url_launcher` (external or in-app browser) plus an app link or custom scheme; `handleRedirect(uri)`.
- The redirect must be in the app's *Allowed redirect URLs* (Admin UI → Authentication). `start` answers 400 otherwise,
  and the controller shows a developer hint.

## 3. Headless controller (state machine)

All UI kits sit on one controller per platform, so a custom UI reuses the logic and none of the widgets.

```
AuthController(client, options)
  state: AuthUiState {
    loading            // fetching providers or restoring session
    methods: PublicAuth // emailPassword flags + providers (null while loading)
    mode: SignIn | SignUp
    submitting: boolean | providerId being used
    error: AuthUiError? // code (invalid_credentials, email_taken, weak_password, invalid_email, signup_closed,
                        //   network, redirect_not_allowed, provider_failed, unknown) + message
    user: User?         // set when signed in
  }
  actions: setMode(), signInWithEmail(email, password), signUpWithEmail(email, password, name), signInWithProvider(id), dismissError(), signOut()
```
- Methods come from `auth.getProviders()` each time the controller starts (cached 60 s), so enabling Google in the
  Admin UI makes the button appear without an app update.
- Validation before the request: email shape, password length from `minPasswordLength`.
- Errors map from problem+json: 401 → `invalid_credentials`, 409 → `email_taken`, 403 → `signup_closed`, 400 with
  `redirectUri` → `redirect_not_allowed`, validation `password` → `weak_password`, network → `network`.

## 4. UI kits

All kits: automatic provider list, email/password form with a sign-in/sign-up switch, loading and error states,
accessible labels, dark/light, and every option below.

`AuthUiOptions` (names per language style):

| Option | Meaning | Default |
|--------|---------|---------|
| `email` | `Auto` (follow server), `Hide`, `Force` | `Auto` |
| `signUp` | `Auto`, `Hide` (never show sign-up even if open), `Only` (start in sign-up) | `Auto` |
| `providers` | `Auto` (all enabled on the server), or explicit order/allow list, | `Auto` |
| `hideProviders` | provider ids never shown | empty |
| `providerButton` | custom renderer per provider id (slot) | built-in branded buttons |
| `title`, `subtitle`, `logo` | header content (slots accepted) | app name |
| `termsUrl`, `privacyUrl` | footer links, shown when set | none |
| `layout` | `Card`, `Fullscreen`, `Plain` (no chrome) | `Card` |
| `theme` | colors, corner radius, font, button style; follows system dark mode | neutral |
| `labels` | every string overridable (for translation) | English |
| `onSignedIn(user)` | callback when a session exists | — |
| `onError(error)` | optional hook | — |

Three levels of customization, documented with one example each:
1. **Drop-in**: `MonolithAuth(mono)` / `<monolith-auth>` / `MonolithAuthScreen(...)`.
2. **Slots**: keep the screen, replace parts (header, provider button, email form, footer, error banner).
3. **Headless**: use the controller (and the same options) and draw everything yourself.

Also ship:
- `AuthGate`: shows the auth screen until signed in, then the app content (Compose `AuthGate { Home() }`, React
  `<AuthGate>`, Flutter `AuthGate(child: ...)`).
- Account helpers: sign-out button, user chip (avatar initial + name/email).

### Platform deliverables

| Platform | Package | Contents |
|----------|---------|----------|
| Web | `@monolith/client` subpath `@monolith/client/ui` (no dependencies: custom element `<monolith-auth>`, `mountAuth(el, mono, options)`, `createAuthController`) and `@monolith/client/react` (`useAuth`, `<MonolithAuth>`, `<AuthGate>`) | Vanilla, React |
| Android | `sdks/kotlin/monolith-compose`: `MonolithAuth`, `AuthGate`, `rememberAuthController`, `AuthUiOptions`, provider icons | Jetpack Compose (Material 3) |
| Flutter | `sdks/flutter` (`monolith_flutter`): `MonolithAuthScreen`, `AuthGate`, `AuthController` (ChangeNotifier + Stream), Flutter `Monolith.init` with secure storage | Flutter widgets |

Provider branding: brand name and color per provider id (google, github, apple, microsoft, facebook, discord,
gitlab, x, linkedin, twitch, oidc), a single-color or simple vector icon per provider, "Continue with {name}".
Unknown ids fall back to a neutral button with the server-provided `name`. Follow each brand's published button
rules where they exist (Google: neutral surface with the colored G; keep logos unmodified).

## 5. Testing expectations

- Controller: unit tests with a fake transport for every state and error code, and the provider list refresh.
- Facade: refresh single-flight, restore from storage, redirect fragment parsing, state flow.
- UI: at least render tests where the platform allows (jsdom, Compose JVM tests or screenshot-free semantics tests, Flutter widget tests).
  If a toolchain is missing, say so in the report; do not claim it was tested.

## 6. Out of scope for now

Email verification and password reset screens (the server has no mail transport yet), OTP, MFA, anonymous users, Apple sign-in
(server exchange not implemented, so Apple never appears in the list), account linking UI.
