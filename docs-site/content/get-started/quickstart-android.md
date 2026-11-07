---
title: Quickstart for Android
description: Connect an Android app with the Kotlin SDK, sign in, write records and collect live snapshots as a Flow.
order: 60
---

This quickstart connects an Android app to Monolith with the Kotlin SDK. You will create a client with encrypted token storage, sign a user in, write a record and collect live changes as a Kotlin `Flow`.

## Before you start

- A running server reachable from the device. On the Android emulator, the host machine is `http://10.0.2.2:8090`. See [Self-hosting](/get-started/self-host).
- An app and a `tasks` collection. The [web quickstart](/get-started/quickstart-web#1-prepare-the-backend) shows the commands. Create the user with `POST /api/v1/apps/shop/auth/register`.
- Android Studio and Kotlin with `kotlinx.serialization`.
- The SDK modules from the repository: `sdks/kotlin/monolith-core` (multiplatform client) and `sdks/kotlin/monolith-android` (Android helpers).

:::status partial
The modules use the coordinates `dev.monolith:monolith-core` and `dev.monolith:monolith-android`, but they are not published to Maven yet. Include them as Gradle modules from the repository.
:::

Declare the internet permission. The examples use plain `http` for local testing, which Android blocks by default; allow it for debug builds only and use HTTPS in production.

```xml title="AndroidManifest.xml"
<uses-permission android:name="android.permission.INTERNET" />
<application android:usesCleartextTraffic="true" ... />
```

## 1. Create the client

`MonolithAndroid.create` builds a client and keeps the access token in `EncryptedSharedPreferences`. On the next launch it loads the stored token.

```kotlin title="App.kt"
import dev.monolith.android.MonolithAndroid
import kotlinx.serialization.Serializable

@Serializable
data class Task(
    val id: String = "",
    val title: String = "",
    val done: Boolean = false,
)

class App : Application() {
    lateinit var mono: dev.monolith.client.MonolithClient

    override fun onCreate() {
        super.onCreate()
        mono = MonolithAndroid.create(
            context = this,
            url = "http://10.0.2.2:8090",
            app = "shop",
        )
    }
}
```

A typed `Collection<Task>` comes from `mono.collection<Task>("tasks")`. Without a type, `mono.collection("tasks")` returns `RecordModel` with your fields under `data`.

## 2. Sign in

```kotlin
suspend fun signIn(mono: MonolithClient) {
    mono.auth().signInWithPassword("ada@example.com", "correct horse")
}
```

The token is stored and sent on later calls. `auth().onChange { token -> ... }` and `auth().tokenFlow` report token changes.

:::status partial
`Monolith.init(context, url, app)` (import `dev.monolith.android.init`) is available: it keeps the whole session in encrypted storage, refreshes tokens, and exposes `mono.auth.signIn`, `mono.auth.signUp` and `mono.auth.state` as a `StateFlow`. A Compose sign-in screen is in `monolith-compose`. The example below uses the older `MonolithAndroid.create`, which stores only the access token. See [Kotlin and Android](/sdks/kotlin).
:::

## 3. Write records

```kotlin
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

val tasks = mono.collection<Task>("tasks")

suspend fun add() {
    val created: Task = tasks.create(buildJsonObject { put("title", "Write the docs") })
    tasks.update(created.id, buildJsonObject { put("done", true) })
}
```

Pass a `JsonObject` when you want to send only some fields. The overloads that take a `Task` serialize the whole object.

## 4. Listen for live changes

`onSnapshot` returns a `Flow` of `QuerySnapshot<Task>`. Each snapshot has `docs` (the current list) and `changes`.

```kotlin
import dev.monolith.client.realtime.SnapshotOptions

class TasksViewModel(private val tasks: Collection<Task>) : ViewModel() {
    val items: StateFlow<List<Task>> = tasks
        .onSnapshot(SnapshotOptions(filter = "done = false", sort = "-created", limit = 50))
        .map { it.docs }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
}
```

The stream starts when the Flow is collected and closes when collection stops. After a network drop the SDK reconnects with backoff and sends a fresh snapshot. For single events use `tasks.subscribe(filter = ..., ids = ...)`, which also returns a `Flow<RealtimeEvent<Task>>`. See [Realtime](/build/database/realtime).

## Limits and status

- `tasks.count()` reads a `count` field but the server returns `total`; it fails to report a real number until the SDK is fixed. Use `GET .../records:count`.
- `mono.batch()` sends a body the server does not accept. See [Batch operations](/build/database/batch).
- `ListResult` exposes `nextCursor`, while the server's field is `next`.
- No offline cache or sync. See [Offline sync](/build/database/offline-sync).

## Next steps

- [Query records](/build/database/query)
- [Rules](/build/database/rules)
- [Realtime](/build/database/realtime)
