---
name: monolith-sdk-kotlin
description: Use the Monolith Kotlin SDK (monolith-core multiplatform, monolith-android AAR): auth, collections, realtime flows, offline sync with WorkManager.
---

# Kotlin / Android SDK

```kotlin
// build.gradle.kts
implementation("dev.monolith:monolith-android:<version>")   // JVM-only: monolith-core

val mono = Monolith.create(context, url = BASE, app = "myapp", key = "mk_pub_...")
mono.auth("users").signInWithPassword(email, pw)
val tasks = mono.collection<Task>("tasks").list(filter = "status = 'open'", limit = 20)
mono.collection<Task>("tasks").subscribe().collect { ev -> /* Flow<RecordEvent<Task>> */ }
```

- Models: `@Serializable` data classes; generate with `monolith codegen --lang kotlin`.
- Offline: `mono.sync.enable("tasks")`; background work runs through WorkManager.
- Tokens are stored encrypted by the AAR. Debug: `Monolith.create(..., logLevel = LogLevel.DEBUG)`.
- Full reference: `docs/api/kotlin.md`.
