# Monolith Client SDKs

Official client libraries for integrating applications with Monolith. Each SDK provides a consistent API structure across languages for data access, authentication, storage, realtime streams, and analytics.

## Core Features Provided by SDKs

| Feature | Capabilities |
|---|---|
| **Database & Collections** | Typed CRUD (`list`, `getOne`, `create`, `update`, `replace`, `delete`), record counts, keyset pagination, sorting, filtering expressions, field projection, relation expansion (`expand`), and atomic multi-operation `batch`. |
| **Authentication** | Email & password login, first-superuser installer bootstrap, token management, session change listeners (`onChange`), and scoped API key support. |
| **Storage & Files** | Bucket management, direct/streamed file uploads, download URL generation with on-demand thumbnail parameters (`?thumb=WxH`), and object deletion. |
| **Realtime** | Multiplexed Server-Sent Events (SSE) connection over `/api/v1/apps/{app}/realtime`, per-collection subscriptions, and automatic exponential backoff reconnection. |
| **Analytics** | Buffered in-memory event tracking (`track`), auto-flushing by batch size and time intervals, and retry resilience. |
| **Error Handling** | Native exception mapping for RFC 9457 `application/problem+json` error responses with HTTP status, problem type, request ID, and granular field-level validation errors. |
| **Offline Sync** | Local replica cache, change outbox, background synchronization, and conflict resolution policies *(planned for Phase 3)*. |

---

## SDK Catalog & Language Support

| SDK | Package / Registry | Platforms | Database & Auth | Storage & Realtime | Status |
|---|---|---|:---:|:---:|---|
| **TypeScript** | npm: `@monolith/client` | Web browsers, Node >= 20, Bun, Deno, React Native | Yes | Yes | **v0 implemented** |
| **Dart** | pub.dev: `monolith_client` | Flutter (Android, iOS, desktop, web) | Yes | Yes | **v0 implemented** |
| **Python** | PyPI: `monolith-client` | Python >= 3.10 (httpx), backend scripts, jobs | Yes | Yes | **v0 implemented** |
| **Kotlin / Android** | Maven: `dev.monolith:monolith-core`, `monolith-android` | Android (AAR), JVM, Kotlin Multiplatform | Yes | Yes | **v0 implemented** |
| **Rust** | crates.io: `monolith-client` | Rust CLI tools, background workers, microservices | Yes | Yes | **v0 implemented** |
| **Swift** | SwiftPM: `Monolith` | iOS, macOS, watchOS, visionOS | Planned | Planned | Phase 5 |

---

## Shared Architecture

All SDKs follow the uniform module layout outlined in `development/docs/21-repository-structure.md §5`:

```
sdks/<lang>/
├── client        # Root MonolithClient entry point & configuration
├── http          # Request pipeline, backoff retries, header handling
├── auth          # Password sign-in, token storage, listeners
├── collections   # Typed CRUD, query builders, count, batch
├── realtime      # Multiplexed SSE connection, subscriptions
├── files         # Bucket & object uploads, URLs, thumbnails
├── analytics     # Batched event queue & background flush
└── errors        # RFC 9457 problem+json error mapping
```

### Quick Usage Example (TypeScript)

```ts
import { createClient } from "@monolith/client";

const mono = createClient({
  url: "http://localhost:8090",
  app: "default",
});

// Authenticate
await mono.auth("users").signInWithPassword("alice@example.com", "secret123");

// Query collection
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
