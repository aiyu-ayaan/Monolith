---
title: Authentication
description: Sign users in and out, watch the session and call the API as them.
group: Use the SDKs
order: 20
---

The SDKs sign in the end users of one app. Which methods exist (email and password, providers) is set per app in the Admin UI: [Authentication overview](/build/auth/overview).

## Sign up and sign in

:::tabs
```ts title="TypeScript"
await mono.auth.signUp({ email: "ada@example.com", password: "correct horse", name: "Ada" });
await mono.auth.signIn({ email: "ada@example.com", password: "correct horse" });
```
```kotlin title="Kotlin"
val user = mono.auth.signUp("ada@example.com", "correct horse", name = "Ada")
val user2 = mono.auth.signIn("ada@example.com", "correct horse")
```
```dart title="Dart"
final user = await mono.auth().signUp('ada@example.com', 'correct horse', name: 'Ada');
final user2 = await mono.auth().signIn('ada@example.com', 'correct horse');
```
```python title="Python"
auth = mono.auth()
auth.sign_up("ada@example.com", "correct horse", name="Ada")
auth.sign_in("ada@example.com", "correct horse")
print(auth.user)   # {"id": ..., "email": ..., "name": ..., "providers": [...]}
```
```rust title="Rust"
let auth = mono.auth();
auth.sign_up("ada@example.com", "correct horse", Some("Ada")).await?;
let session = auth.sign_in("ada@example.com", "correct horse").await?;
println!("{:?}", auth.user());
```
:::

Both return the signed-in user: `id`, `email`, `name`, `created`, `disabled`, `lastLogin` and `providers`.

## Watch the signed-in state

:::tabs
```ts title="TypeScript"
const off = mono.auth.onChange((s) => {
  if (s.status === "signedIn") showHome(s.user);
  else if (s.status === "signedOut") showLogin();
  // "unknown": the stored session is still being restored
});
```
```kotlin title="Kotlin"
mono.auth.state.collect { s ->
    when (s) {
        AuthState.Unknown -> showSplash()          // the stored session is still being restored
        AuthState.SignedOut -> showLogin()
        is AuthState.SignedIn -> showHome(s.user)
    }
}
```
```dart title="Dart"
mono.auth().changes.listen((s) {
  switch (s) {
    case AuthUnknown(): showSplash();      // the stored session is still being restored
    case AuthSignedOut(): showLogin();
    case AuthSignedIn(:final user): showHome(user);
  }
});
```
```python title="Python"
auth.on_state_change(lambda st: print(st.status, st.user))   # AuthStatus.UNKNOWN / SIGNED_OUT / SIGNED_IN
```
```rust title="Rust"
let mut rx = auth.on_change();                 // tokio::sync::watch::Receiver<AuthState>
while rx.changed().await.is_ok() {
    match &*rx.borrow() {
        AuthState::Unknown => {}
        AuthState::SignedOut => println!("signed out"),
        AuthState::SignedIn(user) => println!("signed in: {user:?}"),
    }
}
```
:::

The state is `unknown` while the stored session is being restored, then `signedOut` or `signedIn`. Show a splash screen during `unknown`.

## Sign in with a provider

:::tabs
```ts title="TypeScript"
const provider = (await mono.auth.getProviders()).providers;   // [{ id: "google", name: "Google" }, ...]
await mono.auth.signInWithProvider("google");                  // redirects the page
const user = await mono.auth.signInWithProvider("google", { mode: "popup" }); // resolves with the user
```
```kotlin title="Kotlin"
val providers = mono.auth.providers().providers        // [AuthProviderInfo(id = "google", name = "Google"), ...]
mono.auth.signInWithProvider("google")                 // opens a Custom Tab on Android
```
```dart title="Dart"
final providers = (await mono.auth().providers()).providers;
await mono.auth().signInWithProvider('google');   // builds the URL and passes it to urlLauncher
```
```python title="Python"
print(auth.providers()["providers"])    # [{"id": "google", "name": "Google", ...}]
url = auth.provider_sign_in_url("google", "http://127.0.0.1:8765/cb")   # must be an allowed redirect URL
# open `url`; your /cb handler receives the browser
auth.handle_redirect("http://127.0.0.1:8765/cb#token=...&refreshToken=...&expiresAt=...")   # MonolithError on #error=
```
```rust title="Rust"
let providers = auth.providers().await?;
let url = auth.provider_sign_in_url("google", "http://127.0.0.1:8765/cb")?;   // must be an allowed redirect URL
// open `url`, catch the redirect on a loopback server, then:
auth.handle_redirect("http://127.0.0.1:8765/cb#token=...&refreshToken=...").await?;
```
:::

The return URL must be in the app's allowed redirect URLs. TypeScript, Kotlin and Dart open the browser and finish the redirect for you. Python and Rust only build the URL: you open it and hand the returned URL back. Browsers do not send the URL fragment to servers, so a loopback handler needs a small page that posts `location.hash` back. Android scheme and manifest: [Kotlin and Android guide](/sdks/kotlin).

## Profile and password

:::tabs
```ts title="TypeScript"
await mono.auth.updateProfile({ name: "Ada L." });
await mono.auth.changePassword("correct horse", "battery staple");   // the server ends all sessions
```
```kotlin title="Kotlin"
mono.auth.updateProfile("Ada L.")
mono.auth.changePassword("correct horse", "battery staple")   // the server ends all sessions
```
```dart title="Dart"
await mono.auth().updateProfile(name: 'Ada L.');
await mono.auth().changePassword('correct horse', 'battery staple');   // the server ends all sessions
```
```python title="Python"
auth.update_profile("Ada L.")
auth.change_password("correct horse", "battery staple")   # the server ends all sessions: sign in again
```
```rust title="Rust"
auth.update_profile("Ada L.").await?;
auth.change_password("correct horse", "battery staple").await?;   // the server ends all sessions
```
:::

Changing the password ends every session of the user.

## Get the token for your own requests

:::tabs
```ts title="TypeScript"
const token = await mono.auth.accessToken();   // refreshes first if needed
await fetch(url, { headers: { authorization: `Bearer ${token}` } });
```
```kotlin title="Kotlin"
val token = mono.auth.accessToken()   // refreshes first if needed
```
```dart title="Dart"
final token = await mono.auth().accessToken();   // refreshes first if needed
```
```python title="Python"
token = auth.access_token()   # refreshes first if needed
```
```rust title="Rust"
let token = auth.access_token().await?;   // Option<String>, refreshed if needed
```
:::

## Sign out

:::tabs
```ts title="TypeScript"
await mono.auth.signOut();   // clears the local session, then revokes the refresh token
```
```kotlin title="Kotlin"
mono.auth.signOut()   // clears the local session, then revokes the refresh token
```
```dart title="Dart"
await mono.auth().signOut();
```
```python title="Python"
auth.sign_out()   # revokes the refresh token on the server
```
```rust title="Rust"
auth.logout().await?;   // revokes the refresh token on the server
auth.sign_out();        // clears the local session only
```
:::

## Where the session is kept

| Language | Default | Change it |
|---|---|---|
| TypeScript | `localStorage`, else memory | `storage` option |
| Kotlin | Android: encrypted preferences. JVM: memory | `storage` parameter |
| Dart | Memory. Flutter: secure storage | `SessionStorage` |
| Python | Memory | `storage="session.json"` or a `SessionStore` |
| Rust | Memory | `session_file(..)` or `session_store(..)` on the builder |

## Errors

| Status | Meaning |
|---|---|
| 401 | Wrong email or password, or an expired session |
| 403 | Sign-up is closed for this app |
| 409 | The email is already registered |
| 400 | Validation failed, for example a password under the app's minimum (default 8) |

An access token lasts 15 minutes and a refresh token 30 days. The SDK refreshes shortly before expiry and once after a `401`, one refresh at a time, and signs the user out if the refresh token is rejected: [Sessions and tokens](/build/auth/sessions-and-tokens).

## Next steps

- [Read and query](/sdks/query)
- [Sign-in screens](/build/auth/ui-kit)
