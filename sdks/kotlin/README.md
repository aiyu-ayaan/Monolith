# Monolith Kotlin / Android SDK

Idiomatic Kotlin Multiplatform and Android SDK for the Monolith backend suite.

## Modules

- **`monolith-core`**: Kotlin Multiplatform client (JVM, Android, iOS targets). Powered by Ktor Client and `kotlinx.serialization`. Pure Kotlin without Android platform dependencies.
- **`monolith-android`**: Android AAR library providing Android context integration, automatic encrypted token storage via `EncryptedSharedPreferences`, and builder convenience. `Monolith.init(context, url, app)` adds encrypted session storage, a Custom Tab launcher for provider sign-in and `MonolithAuthRedirectActivity`.
- **`monolith-compose`**: Jetpack Compose (Material 3) auth UI: `MonolithAuth`, `AuthGate`, `rememberAuthController`, `AuthUiOptions`, `UserChip`, `SignOutButton`.

## Coordinates

- `dev.monolith:monolith-core:<version>`
- `dev.monolith:monolith-android:<version>`
- `dev.monolith:monolith-compose:<version>`

## Quickstart

### Android

```kotlin
// Build client with Android context and encrypted token storage
val mono = MonolithAndroid.create(
    context = applicationContext,
    url = "https://api.example.com",
    app = "myapp",
    key = "mk_pub_..."
)

// Authenticate
val session = mono.auth("users").signInWithPassword("alice@example.com", "secret123")

// Typed CRUD
@Serializable
data class Task(val id: String = "", val title: String = "", val status: String = "open")

val tasksCol = mono.collection<Task>("tasks")
val openTasks = tasksCol.list(ListOptions(filter = "status = 'open'", limit = 20))
val newTask = tasksCol.create(Task(title = "Ship Kotlin SDK"))

// Realtime subscriptions as Kotlin Flow
tasksCol.subscribe().collect { event ->
    println("${event.action} in ${event.collection}: ${event.record}")
}

// Storage / Files
mono.files.upload("receipts", "invoice.pdf", pdfBytes, "application/pdf")
val invoiceUrl = mono.files.getURL("receipts", "invoice.pdf")

// Analytics
mono.analytics.track("task_completed", buildJsonObject { put("taskId", newTask.id) })
mono.analytics.flush()
```

### Android: app users and the sign-in screen

```kotlin
import dev.monolith.android.init

val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")

// Compose: shows the sign-in screen until the user is signed in
setContent { AuthGate(mono) { Home(mono) } }
```

Provider sign-in returns to `monolith-<app>://auth` by default. Declare `MonolithAuthRedirectActivity` in the
app manifest with that scheme (see the class docs) and add the URL to the app's *Allowed redirect URLs* in the Admin UI.
The Compose module is covered by logic unit tests only; the widgets have no render tests yet.

### Multiplatform (JVM / Common)

```kotlin
val mono = MonolithClient.create(
    url = "https://api.example.com",
    app = "myapp",
    key = "mk_pub_..."
)

val auth = mono.auth()
auth.onChange { token ->
    println("Active token changed: $token")
}
```

## Testing & Building

```bash
cd sdks/kotlin
./gradlew test                   # Run all tests (multiplatform JVM + Android unit tests)
./gradlew :monolith-core:jvmTest  # Run monolith-core unit tests
./gradlew :monolith-android:assembleDebug  # Assemble Android AAR
```
