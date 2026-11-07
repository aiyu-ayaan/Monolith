# Kotlin / Android SDK

The Monolith Kotlin SDK provides idiomatic access to Monolith backends for Kotlin Multiplatform and native Android applications.

## Packages

- **`dev.monolith:monolith-core`**: Pure Kotlin Multiplatform common client (JVM, Android, iOS). Uses Ktor client and `kotlinx.serialization`.
- **`dev.monolith:monolith-android`**: Android AAR library providing Android context integration, automatic encrypted token storage via `EncryptedSharedPreferences`, and builder convenience.

## Initialization

### Android

```kotlin
import dev.monolith.android.MonolithAndroid

val mono = MonolithAndroid.create(
    context = context,
    url = "https://api.example.com",
    app = "myapp",
    key = "mk_pub_..."
)
```

Or using the fluent builder:

```kotlin
val mono = MonolithAndroid.Builder(context)
    .url("https://api.example.com")
    .app("myapp")
    .key("mk_pub_...")
    .persistAuthTokens(true)
    .build()
```

### Multiplatform / JVM

```kotlin
import dev.monolith.client.ClientOptions
import dev.monolith.client.MonolithClient

val mono = MonolithClient(
    ClientOptions(
        url = "https://api.example.com",
        app = "myapp",
        key = "mk_pub_..."
    )
)
```

## Authentication

```kotlin
// Sign in with email and password
val session = mono.auth("users").signInWithPassword("alice@example.com", "secret123")
println("Logged in with token: ${session.token}")

// Observe token changes (e.g. login, refresh, logout)
val unsubscribe = mono.auth().onChange { token ->
    println("Active token changed: $token")
}

// Or reactively via Kotlin StateFlow
mono.auth().tokenFlow.collect { token ->
    // Update UI state
}

// System installation with initial superuser credentials
mono.auth("_superusers").install("setup_token", "admin@example.com", "pass123")

// Sign out
mono.auth().signOut()
```

## Collections & Typed CRUD

```kotlin
import dev.monolith.client.http.ListOptions
import kotlinx.serialization.Serializable

@Serializable
data class Task(
    val id: String = "",
    val title: String = "",
    val status: String = "open"
)

val tasks = mono.collection<Task>("tasks")

// List with pagination and filter
val page = tasks.list(ListOptions(filter = "status = 'open'", limit = 20, sort = "-created"))
println("Found ${page.items.size} tasks")

// Get one
val task = tasks.getOne("018f3a5e-...")

// Create
val created = tasks.create(Task(title = "Write tests", status = "open"))

// Update (partial)
val updated = tasks.update(created.id, created.copy(status = "done"))

// Replace (full)
tasks.replace(created.id, Task(title = "Write tests", status = "done"))

// Count
val openCount = tasks.count("status = 'open'")

// Delete
tasks.delete(created.id)
```

## Realtime Subscriptions

Realtime events are delivered over multiplexed Server-Sent Events (SSE) and surfaced as Kotlin coroutine `Flow`:

```kotlin
tasks.subscribe().collect { event ->
    when (event.action) {
        "create" -> println("New task: ${event.record}")
        "update" -> println("Updated task: ${event.record}")
        "delete" -> println("Deleted task: ${event.record}")
    }
}
```

Or with callback listeners:

```kotlin
val unsubscribe = tasks.subscribe({ event ->
    println("Event received: ${event.action} -> ${event.record}")
}, filter = "status = 'open'")

// Later: unsubscribe
unsubscribe()
```

## Storage & Files

```kotlin
val files = mono.files

// Upload file bytes
files.upload("receipts", "invoice.pdf", pdfByteArray, contentType = "application/pdf")

// Generate public/authenticated URL (with optional thumbnail resizing)
val fileUrl = files.getURL("receipts", "photo.jpg", thumb = "200x200")

// List bucket objects
val list = files.list("receipts", options = StorageListOptions(limit = 50))

// Delete object
files.delete("receipts", "invoice.pdf")
```

## Analytics

```kotlin
// Track event
mono.analytics.track("checkout_completed", buildJsonObject {
    put("amount", 99.95)
    put("currency", "USD")
})

// Flush pending batch immediately
mono.analytics.flush()
```

## Error Handling (RFC 9457)

Server errors are decoded into `MonolithException`:

```kotlin
import dev.monolith.client.http.MonolithException

try {
    tasks.create(Task(title = ""))
} catch (e: MonolithException) {
    println("HTTP ${e.status}: ${e.problem.title} (${e.type})")
    println("Request ID: ${e.requestId}")

    // Access field validation error
    val titleErr = e.getFieldError("title")
    if (titleErr != null) {
        println("Title error: $titleErr")
    }
}
```
