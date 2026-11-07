# Monolith Client SDKs

Official native client libraries for integrating applications with Monolith. Each SDK follows a shared architecture and consistent module layout across platforms.

---

## SDK Feature Checklist & Support Matrix

Detailed breakdown of capabilities implemented across the official client SDKs:

| Category | Capability | TypeScript | Kotlin / Android | Dart / Flutter | Python | Rust | Swift |
|---|---|:---:|:---:|:---:|:---:|:---:|:---:|
| **Database** | Keyset list (`list()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Record lookup (`getOne()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Create record (`create()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Partial update (`update()` PATCH) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Full replace (`replace()` PUT) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Delete record (`delete()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Record count (`count()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Expression filters (`filter`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Sort order (`sort`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Field projection (`fields`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Relation joins (`expand`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Atomic batch (`batch()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| **Auth** | Password login (`signInWithPassword`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Installer link bootstrap (`install`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Session sign-out (`signOut()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Reactive listener (`onChange`) | [x] | [x] (Flow) | [x] (Stream) | [x] | [x] | [ ] |
| | Secure token persistence | Web Storage | EncryptedSharedPrefs | In-memory | In-memory | In-memory | [ ] |
| | Scoped API key auth | [x] | [x] | [x] | [x] | [x] | [ ] |
| **Storage** | Object upload (`upload()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Download URL (`getURL()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | On-demand thumbnails (`?thumb=WxH`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Object deletion (`delete()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Bucket object listing (`list()`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| **Realtime** | Multiplexed SSE connection | [x] | [x] | [x] | Planned | Planned | [ ] |
| | Per-collection subscription (`subscribe()`) | [x] | [x] | [x] | Planned | Planned | [ ] |
| | Reconnection with exponential backoff & jitter | [x] | [x] | [x] | Planned | Planned | [ ] |
| | Automatic connection lifecycle (ref-counted) | [x] | [x] | [x] | Planned | Planned | [ ] |
| | Filtered / per-record subscription (`filter`, `ids`) | [x] | Planned | [x] | Planned | Planned | [ ] |
| | Snapshot listener (`onSnapshot`, `onDocSnapshot`) | [x] | Planned | [x] (Stream) | Planned | Planned | [ ] |
| | Gap-free resume after reconnect (`since`) and `leave` events | [x] | Planned | [x] | Planned | Planned | [ ] |
| **Analytics** | Event tracking (`track()`) | [x] | [x] | [x] | [x] | Planned | [ ] |
| | In-memory queue buffering | [x] | [x] | [x] | [x] | Planned | [ ] |
| | Periodic interval flush | [x] | [x] | [x] | [x] | Planned | [ ] |
| | Size threshold flush | [x] | [x] | [x] | [x] | Planned | [ ] |
| **Errors** | RFC 9457 `problem+json` error mapping | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Field-level validation problems | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Request ID extraction (`X-Request-Id`) | [x] | [x] | [x] | [x] | [x] | [ ] |
| | Idempotent GET retry on network/503 | [x] | [x] | [x] | [x] | [x] | [ ] |
| **Offline** | Local replica caching & sync | Phase 3 | Phase 3 | Phase 3 | Phase 3 | Phase 3 | Phase 5 |

---

## Detailed Feature Specifications

### 1. Database & Collections
- **Typed CRUD Operations**: Full mapping to REST records endpoints (`/api/v1/apps/{app}/collections/{collection}/records`).
- **Keyset Cursor Pagination**: Transparent forward and backward paging using Crockford base32 cursor tokens without expensive `COUNT(*)` overhead.
- **Rule-Guarded Mutations**: Safe partial mutations via `PATCH`, upserts via `PUT`, and savepoint-backed dry runs via `?dry_run=true`.
- **Query Parameters**: Declarative sorting (`-created`), filtering syntax compatible with PocketBase rules (`status = 'active' && views >= 10`), relation expansion (`author,category`), and column projection (`fields=id,title`).
- **Atomic Batch Ingestion**: Group multiple record mutations (create, update, delete) in a single HTTP `POST .../batch` request inside an isolated database transaction.

### 2. Authentication & Credentials
- **Zero-DB Token Verification**: Uses HMAC-SHA256 access tokens validated on the server without database lookups.
- **Session Observers**: Subscriptions to token changes (`onChange`) enabling dynamic UI re-rendering and automatic token persistence.
- **Android Secure Storage**: Android library integrates with AndroidX `EncryptedSharedPreferences` and the hardware Android Keystore.
- **Runtime Scoped API Keys**: Pass API keys (`mbk_<env>_...`) seamlessly via `key` client option, attaching `Authorization: Bearer <key>` to all requests.

### 3. File Storage & Image Thumbnails
- **Content-Addressed Blobs**: Storage handles raw binary bytes, Blobs, byte arrays, and text payloads with BLAKE3 deduplication on the backend.
- **Direct & Presigned URLs**: Generate public or authenticated download URLs directly via `files.getURL(bucket, key)`.
- **Dynamic Image Resizing**: Request on-demand generated thumbnails by providing `{ thumb: "WxH" }` (e.g. `100x100`, `300x200`), cached on the server and bounded by a worker pool.

### 4. Realtime Streaming
- **Single Multiplexed Connection**: Shares a single persistent Server-Sent Events (SSE) connection per app rather than opening separate sockets per collection.
- **Demand-Driven Lifecycle**: Opens the SSE stream on the first collection subscription; closes the stream automatically when all subscribers unsubscribe.
- **Resilient Reconnection**: Exponential backoff with randomized jitter handles network drops and backend restarts. Plain subscriptions resume with `since=<last seq>`; snapshot listeners re-snapshot.
- **Snapshot Listeners (TypeScript)**: `collection.onSnapshot(cb, { filter, sort, limit })` calls back with the current result set, then after every change with `docs` and `changes` (`added`/`modified`/`removed`). `onDocSnapshot(id, cb)` watches one record.

### 5. Analytics & Event Queue
- **Buffered Event Pipeline**: Ingest analytics events via `mono.analytics.track("event_name", { properties })`.
- **Automated Flushing**: Events queue locally and automatically flush when reaching the batch size limit (e.g. 20 events) or on periodic timer intervals (e.g. every 5 seconds).

### 6. Error Handling & RFC 9457
- **Structured Error Model**: Non-2xx HTTP responses parse RFC 9457 `application/problem+json` into native language exceptions (`MonolithError`, `MonolithException`).
- **Validation Diagnostics**: Direct access to specific field validation errors (e.g. `err.getFieldError("email")`).
- **Tracing**: Automatically captures and surfaces `requestId` (`X-Request-Id`) on every failed request for instant log correlation.

---

## SDK Catalog & Package Coordinates

| Language | Registry & Package | Platforms | Implementation Status |
|---|---|---|---|
| **TypeScript** | npm: [`@monolith/client`](typescript) | Web browsers, Node >= 20, Bun, Deno, React Native | **v0 implemented** (ESM + CJS, bundled `.d.ts`) |
| **Kotlin / Android** | Maven: `dev.monolith:monolith-core`, `monolith-android` | Android (AAR), JVM, Kotlin Multiplatform | **v0 implemented** (Ktor CIO, EncryptedSharedPreferences) |
| **Dart** | pub.dev: [`monolith_client`](dart) | Flutter (Android, iOS, macOS, Windows, Linux, Web) | **v0 implemented** (http client, stream listeners) |
| **Python** | PyPI: [`monolith-client`](python) | Python >= 3.10 | **v0 implemented** (httpx client, pytest suite) |
| **Rust** | crates.io: [`monolith-client`](rust) | Rust stable 2024 (CLI, workers, microservices) | **v0 implemented** (reqwest, tokio, serde) |
| **Swift** | SwiftPM: [`Monolith`](swift) | iOS, macOS, watchOS, visionOS | Phase 5 planned |

---

## Shared Architecture

All SDKs follow the uniform module layout outlined in `development/docs/21-repository-structure.md §5`:

```
sdks/<lang>/
├── client        # Root MonolithClient entry point & configuration
├── http          # Request pipeline, backoff retries, header handling, types
├── auth          # Password sign-in, installer link bootstrap, token storage, listeners
├── collections   # Typed CRUD, query builders, count, batch
├── realtime      # Multiplexed SSE connection, subscriptions, reconnect backoff
├── files         # Bucket & object uploads, URLs, thumbnails, deletion
├── analytics     # Batched event queue & background flush
└── errors        # RFC 9457 problem+json error mapping & field problem parser
```

---

## Code Examples

### TypeScript
```ts
import { createClient } from "@monolith/client";

const mono = createClient({ url: "http://localhost:8090", app: "default" });

// Authenticate
await mono.auth("users").signInWithPassword("alice@example.com", "secret123");

// Database collection queries
const tasks = await mono.collection("tasks").list({
  filter: "status = 'active'",
  sort: "-created",
  limit: 20,
});

// Realtime subscriptions
const unsubscribe = mono.collection("tasks").subscribe((event) => {
  console.log("Task changed:", event.action, event.record);
});

// Storage uploads & thumbnails
await mono.files.upload("receipts", "task_1.png", fileBlob);
const thumbUrl = mono.files.getURL("receipts", "task_1.png", { thumb: "200x200" });
```

### Kotlin / Android
```kotlin
import dev.monolith.android.MonolithAndroid

// Android initialization with auto EncryptedSharedPreferences token storage
val mono = MonolithAndroid.builder(context)
    .url("http://10.0.2.2:8090")
    .app("default")
    .build()

// Authenticate
mono.auth("users").signInWithPassword("alice@example.com", "secret123")

// Database query
val tasks = mono.collection("tasks").list(filter = "status = 'active'", limit = 20)

// Realtime Flow subscription
mono.collection("tasks").subscribeAsFlow().collect { event ->
    println("Task ${event.action}: ${event.record}")
}

// Storage upload
mono.files.upload("receipts", "task_1.png", byteArray)
```

### Python
```python
from monolith import create_client

client = create_client(url="http://localhost:8090", app="default")

# Authenticate
client.auth("users").sign_in_with_password("alice@example.com", "secret123")

# Database query
tasks = client.collection("tasks").list(filter="status = 'active'", limit=20)

# Storage upload & URL
client.files.upload("receipts", "task_1.png", b"file bytes")
thumb_url = client.files.get_url("receipts", "task_1.png", thumb="200x200")
```

### Dart / Flutter
```dart
import 'package:monolith_client/monolith_client.dart';

final mono = createClient(url: 'http://localhost:8090', app: 'default');

// Authenticate
await mono.auth('users').signInWithPassword('alice@example.com', 'secret123');

// Database query
final tasks = await mono.collection('tasks').list(filter: "status = 'active'", limit: 20);

// Realtime SSE stream
final unsub = mono.collection('tasks').subscribe((event) {
  print('Task ${event.action}: ${event.record}');
});
```

### Rust
```rust
use monolith_client::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder("http://localhost:8090")
        .app("default")
        .build()?;

    // Authenticate
    client.auth("users").sign_in_with_password("alice@example.com", "secret123").await?;

    // Database query
    let tasks = client.collection("tasks").list().filter("status = 'active'").limit(20).send().await?;

    Ok(())
}
```
