# 12 — SDKs & Integration Modes

## 1. Three modes

| Mode | For | Effort to integrate | Features |
|------|-----|---------------------|----------|
| **Plug and play** (HTTP + OpenAPI/MCP + API key) | Scripts, other backends, AI agents, any language | Minutes: create key, call API | CRUD, batch, analytics ingest, files, realtime (SSE), backup push |
| **Client SDK** | Mobile/web apps | Add dependency, init with app URL | All of the above + auth session handling, realtime listeners, offline sync, typed models |
| **Embedded crate** (`monolith-embed`) | Custom Rust servers | Rust project | Full server as a library + custom routes and in-process Rust hooks |

## 2. Client SDKs (priority order)

| SDK | Platforms | Offline store | Priority |
|-----|-----------|---------------|----------|
| TypeScript | Web, Node, Bun, Deno, React Native | IndexedDB (web), SQLite (RN/Node) | P0 |
| Dart | Flutter (Android/iOS/desktop/web) | `sqlite3` / `drift` | P0 (confirm client stack) |
| Kotlin | Android native | SQLite | P1 |
| Swift | iOS native | SQLite | P1 |
| Rust | CLI tools, services | SQLite | P2 (shares protocol types with server) |

### Common SDK surface

```ts
const mono = createClient({ url: "https://api.example.com", app: "myapp", key: "mk_pub_..." });

// auth
await mono.auth("users").signInWithPassword(email, password);
mono.auth("users").onChange((session) => ...);

// typed data (types generated from /openapi.json via `monolith codegen`)
const tasks = await mono.collection<Task>("tasks").list({ filter: "status = 'active'", limit: 20 });
await mono.collection("tasks").update(id, { status: "done" });

// realtime
const unsub = mono.collection("tasks").subscribe({ filter: "assignee = @request.auth.id" }, (ev) => ...);
mono.channel(`task-${id}:location`).publish({ lat, lon });

// offline sync
await mono.sync.enable(["tasks", "places"]);      // local replica + outbox
mono.sync.onStatus((s) => ...);                     // online/offline/pending count

// analytics
mono.analytics.track("task_started", { vehicle: "bike" });

// files
await mono.files.upload("tasks", id, "receipt", file);
```

### SDK engineering rules

- Generated **protocol core** (types, endpoints) from the OpenAPI spec + a hand-written
  thin layer per language for sessions, realtime and sync.
- Single shared realtime connection per client instance; multiplexed subscriptions.
- Exponential backoff with jitter; resume with `since` cursor.
- Token refresh is single-flight (one refresh even if many requests 401 at once).
- Analytics batched, persisted across restarts.
- Conformance test suite (same JSON test vectors for every SDK) runs in CI against a
  real server container.

## 3. Codegen CLI

```bash
monolith codegen --url https://api.example.com --app myapp --key mk_... --lang ts --out src/monolith.gen.ts
monolith codegen ... --lang dart --out lib/monolith.g.dart
```

Generates typed models, collection name constants, and filter helper builders from the
live schema (or from exported migration files, offline).

## 4. Embedded crate

```rust
use monolith_embed::{Monolith, hooks::*, Router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mono = Monolith::builder()
        .data_dir("./data")
        .on_record_create("tasks", |ctx, rec| async move {
            rec.set("fare", compute_fare(&rec)?);   // runs inside writer txn budget
            Ok(())
        })
        .after_commit("tasks", |ev| async move { notify_assignee(ev).await })
        .route("/api/custom/eta", Router::get(eta_handler))
        .build()
        .await?;
    mono.serve("0.0.0.0:8080").await
}
```

The standalone binary is itself built on `monolith-embed`, so both modes share code.

## 5. Migrating a PocketBase app

1. `monolith import pocketbase --pb-data ./pb_data --app myapp` reads `_collections`,
   converts field types (JSON multi-relations → junction tables, TEXT dates → INTEGER µs,
   15-char ids → mapped UUIDv7 with an `_pb_id` lookup column kept for a transition
   period), copies records and files, and translates rules (syntax is compatible;
   unsupported constructs reported).
2. Optional `pb-compat` route shim lets the PocketBase JS/Dart SDK keep working while
   clients migrate.
