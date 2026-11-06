rootProject.name = "monolith-kotlin"
include(":monolith-core")      // Kotlin Multiplatform: JVM + Android + iOS targets; REST, WS, auth, sync engine (store abstracted)
include(":monolith-android")   // Android AAR: SQLite/Room offline store, WorkManager sync, EncryptedSharedPreferences token store
