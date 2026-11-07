---
title: Authentication
description: TypeScript: sign up, sign in, providers and session state.
group: TypeScript
order: 30
---

Sign users in and out, watch the session and call the API as them.

## Sign up and sign in

```ts
await mono.auth.signUp({ email: "ada@example.com", password: "correct horse", name: "Ada" });
await mono.auth.signIn({ email: "ada@example.com", password: "correct horse" });
```

Both return the `AppUser` (`id`, `email`, `name`, `created`, `disabled`, `lastLogin`, `providers`).

## Watch the signed-in state

```ts
const off = mono.auth.onChange((s) => {
  if (s.status === "signedIn") showHome(s.user);
  else if (s.status === "signedOut") showLogin();
  // "unknown": the stored session is still being restored
});
```

`onChange` calls the listener at once with the current state. Show a splash screen while it is `unknown`.

## Sign in with a provider

```ts
const provider = (await mono.auth.getProviders()).providers;   // [{ id: "google", name: "Google" }, ...]
await mono.auth.signInWithProvider("google");                  // redirects the page
const user = await mono.auth.signInWithProvider("google", { mode: "popup" }); // resolves with the user
```

The redirect URL must be in the app's allowed redirect URLs. `Monolith.init` finishes a redirect by itself and clears the URL fragment. To handle it yourself call `mono.auth.handleRedirect()`, which returns `{ user }`, `{ error }` or `null`.

## Profile and password

```ts
await mono.auth.updateProfile({ name: "Ada L." });
await mono.auth.changePassword("correct horse", "battery staple");   // the server ends all sessions
```

## Get the token for your own requests

```ts
const token = await mono.auth.accessToken();   // refreshes first if needed
await fetch(url, { headers: { authorization: `Bearer ${token}` } });
```

## Sign out

```ts
await mono.auth.signOut();   // clears the local session, then revokes the refresh token
```

## Where the session is kept

`localStorage` by default, memory when it is blocked or on a server. Pass `storage` to `Monolith.init` with `getItem`, `setItem` and `removeItem` (sync or async).

## Errors

| Status | Meaning |
|---|---|
| 401 | Wrong email or password, or an expired session |
| 403 | Sign-up is closed for this app |
| 409 | The email is already registered |
| 400 | Validation failed, for example a password shorter than the app's minimum (default 8) |

## Sessions

An access token lasts 15 minutes and a refresh token 30 days. The SDK refreshes shortly before expiry and once after a `401`, one refresh at a time. A rejected refresh token signs the user out. Details: [Sessions and tokens](/build/auth/sessions-and-tokens).

Which sign-in methods exist is configured per app in the Admin UI under **Authentication**: [Authentication overview](/build/auth/overview), [Providers](/build/auth/providers).

## Next steps

- [Read and query](/sdks/typescript/query)
- [SDKs overview](/sdks/overview)
