---
name: monolith-sdk-kotlin
description: Use the Monolith Kotlin SDK (monolith-core, monolith-android, monolith-compose): Monolith.init, app-user auth with StateFlow, typed collections, onSnapshot Flows, files and Compose sign-in. Use when writing Kotlin or Android code against a Monolith app.
---

# Kotlin / Android SDK

Not on Maven yet: use `sdks/kotlin` modules `monolith-core` (JVM), `monolith-android`, `monolith-compose`. Docs: site pages `/sdks/auth`, `/sdks/query`, `/sdks/realtime`, `/sdks/kotlin`.

```kotlin
import dev.monolith.android.init
val mono = Monolith.init(context, url = BASE, app = "myapp")      // encrypted session, Custom Tab sign-in, redirect monolith-<app>://auth
// JVM: Monolith.init(url = BASE, app = "myapp")
mono.auth.signIn(email, password)                                  // signUp(email, password, name), signOut(), accessToken()
mono.auth.state.collect { s -> /* AuthState.Unknown | SignedOut | SignedIn(user) */ }
mono.auth.signInWithProvider("google")                             // opens a Custom Tab
val tasks = mono.collection<Task>("tasks")                         // @Serializable model; omit the type for RecordModel
val page = tasks.list(ListOptions(filter = "done = false", sort = "-created", limit = 20, cursor = c))  // page.nextCursor
tasks.create(Task(title = "x")); tasks.update(id, buildJsonObject { put("done", true) }); tasks.delete(id)
tasks.onSnapshot(SnapshotOptions(filter = "done = false")).collect { snap -> render(snap.docs) }  // Flow<QuerySnapshot<T>>
tasks.onDocSnapshot(id).collect { doc -> /* null when deleted or hidden */ }
mono.files.upload("avatars", "u1.png", bytes, contentType = "image/png")
```

- All data calls are `suspend`. Collect Flows with `repeatOnLifecycle(STARTED)`.
- Declare `MonolithAuthRedirectActivity` for the `monolith-<app>` scheme and add `monolith-<app>://auth` to the app's allowed redirect URLs.
- Compose: `AuthGate(mono) { Home(mono) }`, `rememberAuthController`, `UserChip`, `SignOutButton`.
- Errors are `MonolithException` (`status`, `type`, `detail`, `requestId`, `errors`, `getFieldError`).
- `mono.auth("_superusers")` is the system login (`signInWithPassword`). `key = "mk_..."` is for servers only.
- `files.getURL(...)` adds no credential: public buckets only. The SDK does not escape keys.

Known gaps: `batch` has no real-server test; `If-Match` and `dry_run` are REST-only; Compose has logic tests only, no render tests; no offline sync.
