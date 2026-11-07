---
title: Authentication
description: TypeScript: sign up, sign in, providers and session state.
group: TypeScript
order: 30
---

Sign up, sign in, providers and session state.

`mono.auth` handles the app's end users.

| Method | Returns | Notes |
|---|---|---|
| `signUp({ email, password, name? })` | `Promise<AppUser>` | Creates and signs in |
| `signIn({ email, password })` | `Promise<AppUser>` | |
| `signInWithProvider(id, { redirectUri?, mode? })` | `Promise<AppUser \| null>` | `mode: "popup"` resolves with the user; `"redirect"` leaves the page |
| `handleRedirect(url?)` | `Promise<RedirectResult \| null>` | `init` calls it for you |
| `signOut()` | `Promise<void>` | Revokes the refresh token |
| `updateProfile({ name })` | `Promise<AppUser>` | |
| `changePassword(old, new)` | `Promise<void>` | The server ends all sessions |
| `accessToken()` | `Promise<string \| null>` | Refreshes first if needed |
| `getProviders()` | `Promise<PublicAuth>` | Enabled sign-in methods |
| `onChange(listener, { immediate? })` | unsubscribe | State is `unknown`, `signedOut` or `signedIn` |
| `onTokenChange(listener)` | unsubscribe | Token string or `null` |

```ts
const off = mono.auth.onChange((s) => {
  if (s.status === "signedIn") console.log(s.user.email);
});
await mono.auth.signIn({ email: "ada@example.com", password: "correct horse" });
```

`mono.auth("_superusers")` returns the system-login `AuthService`.

## Next steps

- [Database](/sdks/typescript/database)
- [SDKs overview](/sdks/overview)
