# Monolith Kotlin / Android SDK

Idiomatic Kotlin Multiplatform and Android SDK for the Monolith backend suite.

## Modules

- **`monolith-core`**: Kotlin Multiplatform client (JVM, Android, iOS targets). Powered by Ktor Client and `kotlinx.serialization`. Pure Kotlin without Android platform dependencies.
- **`monolith-android`**: Android AAR library providing Android context integration, automatic encrypted token storage via `EncryptedSharedPreferences`, and builder convenience.

## Coordinates

- `dev.monolith:monolith-core:<version>`
- `dev.monolith:monolith-android:<version>`

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
