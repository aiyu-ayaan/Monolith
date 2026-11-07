export interface CodeSnippets {
  ts?: string;
  kotlin?: string;
  dart?: string;
  python?: string;
  rust?: string;
  swift?: string;
  rest?: string;
}

export interface DocSection {
  id: string;
  title: string;
  chapter: string;
  lead?: string;
  content: string;
  snippets?: CodeSnippets;
  table?: {
    headers: string[];
    rows: string[][];
  };
}

export interface CapabilityCard {
  title: string;
  desc: string;
}

export const CAPABILITIES: CapabilityCard[] = [
  {
    title: "Relational Flexibility",
    desc: "Strict schemas with UUIDv7 primary keys, typed columns (text, email, number, bool, date, select, relation, json), and indexed junction tables for multi-relations.",
  },
  {
    title: "Expressive Querying",
    desc: "PocketBase-compatible expression filters with parameterized SQL compilation, multi-hop relation expand (up to 4 levels), and deterministic keyset pagination.",
  },
  {
    title: "Realtime Synchronization",
    desc: "Multiplexed Server-Sent Events (SSE) data streams synchronized across client devices with automatic exponential backoff reconnection.",
  },
  {
    title: "Zero-Lookup Auth",
    desc: "Short-lived HMAC-SHA256 access tokens verified on the hot path without a database hit, rotating refresh tokens with reuse detection, and scoped API keys.",
  },
  {
    title: "Group Commit Engine",
    desc: "One dedicated OS writer thread per app batches concurrent writes up to 256 operations inside savepoints so a single failure never poisons the batch.",
  },
  {
    title: "BLAKE3 Storage Deduplication",
    desc: "Content-addressed storage with cryptographic deduplication, on-demand image thumbnail generation, and private/public bucket access rules.",
  },
];

export const ALL_SECTIONS: DocSection[] = [
  {
    id: "overview",
    chapter: "Overview & Fundamentals",
    title: "Introduction to Monolith",
    lead: "Monolith is a self-hosted, single-container backend suite in Rust: database with access rules, auth, realtime, offline sync, storage, analytics, and backups.",
    content:
      "Inspired by PocketBase and Firebase, Monolith delivers a high-performance, developer-friendly backend that runs anywhere as a single binary or container. Every app gets its own isolated SQLite database file and dedicated writer thread, providing physical tenant isolation without operational overhead.",
    snippets: {
      ts: `// Install Monolith client SDK\n// npm install @monolith/client\n\nimport { createClient } from "@monolith/client";\n\nconst mono = createClient({\n  url: "http://localhost:8090",\n  app: "default",\n});\n\n// Query records with keyset pagination\nconst tasks = await mono.collection("tasks").list({\n  filter: "status = 'active'",\n  sort: "-created",\n  limit: 20,\n});`,
      kotlin: `// Gradle: implementation("dev.monolith:monolith-android:0.1.0")\n\nimport dev.monolith.android.MonolithAndroid\n\nval mono = MonolithAndroid.builder(context)\n    .url("http://10.0.2.2:8090")\n    .app("default")\n    .build()\n\nval tasks = mono.collection("tasks").list(\n    filter = "status = 'active'",\n    limit = 20\n)`,
      dart: `// pubspec.yaml: monolith_client: ^0.1.0\n\nimport 'package:monolith_client/monolith_client.dart';\n\nfinal mono = createClient(\n  url: 'http://localhost:8090',\n  app: 'default',\n);\n\nfinal tasks = await mono.collection('tasks').list(\n  filter: "status = 'active'",\n  limit: 20,\n);`,
      python: `# pip install monolith-client\n\nfrom monolith import create_client\n\nclient = create_client(url="http://localhost:8090", app="default")\ntasks = client.collection("tasks").list(filter="status = 'active'", limit=20)`,
      rust: `// Cargo.toml: monolith-client = "0.1.0"\n\nuse monolith_client::Client;\n\n#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n    let client = Client::builder("http://localhost:8090")\n        .app("default")\n        .build()?;\n    let tasks = client.collection("tasks").list().filter("status = 'active'").limit(20).send().await?;\n    Ok(())\n}`,
      swift: `// Swift Package Manager: .package(url: "https://github.com/monolith/monolith-swift", from: "0.1.0")\n\nimport Monolith\n\nlet client = MonolithClient(url: URL(string: "http://localhost:8090")!, app: "default")\nlet tasks = try await client.collection("tasks").list(filter: "status = 'active'", limit: 20)`,
      rest: `# Fetch collections list via REST API\ncurl -s http://127.0.0.1:8090/api/v1/apps/default/collections \\\n  -H "authorization: Bearer $TOKEN"`,
    },
  },
  {
    id: "architecture",
    chapter: "Overview & Fundamentals",
    title: "Architecture & Group Commit",
    lead: "One Rust process (Tokio + axum). Control plane in /data/system.db. Each app gets its own /data/apps/<app>/data.db.",
    content:
      "Each app database runs one dedicated OS writer thread with group commit (up to 256 pending mutations flushed per transaction). Reader queries execute concurrently across a thread pool with zero lock contention on SQLite WAL. Every commit appends a monotonic change sequence `seq` to `_changes`, which immediately publishes to an in-process broadcast bus driving realtime SSE, offline sync, and webhooks.",
    table: {
      headers: ["Component", "Implementation", "Performance Characteristic"],
      rows: [
        ["Writer Loop", "Dedicated OS thread + crossbeam channel", "Group commit up to 256 ops/batch, ACID savepoints"],
        ["Reader Pool", "Read-only connections pool in WAL mode", "Zero lock contention, sub-millisecond p99 latency"],
        ["Realtime Bus", "In-process tokio broadcast bus", "Zero-alloc fanout, serialized once per audience"],
        ["Rules Engine", "Compiled AST -> SQL + in-memory predicates", "Parameterized pushdown, sub-microsecond evaluation"],
        ["Storage Engine", "BLAKE3 content-addressable storage", "Deduplicated blocks, bounded thread pool for resizing"],
      ],
    },
  },
  {
    id: "quickstart",
    chapter: "Overview & Fundamentals",
    title: "Quickstart & Initialization",
    lead: "Set up the Monolith backend and initialize the official SDK in your client environment.",
    content:
      "Run the Monolith backend locally using `pnpm dev:server` or docker. The server listens on port 8090. On first startup, Monolith prints a one-time cryptographic superuser setup URL to configure your admin account.",
    snippets: {
      ts: `import { createClient } from "@monolith/client";\n\nexport const mono = createClient({\n  url: "http://localhost:8090",\n  app: "default",\n  autoRefreshToken: true,\n});`,
      kotlin: `import dev.monolith.android.MonolithAndroid\n\n// Android initializer automatically injects Context and EncryptedSharedPreferences\nval mono = MonolithAndroid.builder(applicationContext)\n    .url("http://10.0.2.2:8090")\n    .app("default")\n    .build()`,
      dart: `import 'package:monolith_client/monolith_client.dart';\n\nfinal mono = createClient(\n  url: 'http://localhost:8090',\n  app: 'default',\n);`,
      python: `from monolith import create_client\n\nclient = create_client(\n    url="http://localhost:8090",\n    app="default",\n)`,
      rust: `use monolith_client::Client;\n\nlet client = Client::builder("http://localhost:8090")\n    .app("default")\n    .build()?;`,
      swift: `import Monolith\n\nlet client = MonolithClient(url: URL(string: "http://localhost:8090")!, app: "default")`,
      rest: `# Verify server health\ncurl -i http://127.0.0.1:8090/health\n# HTTP/1.1 200 OK\n# {"status":"ok","version":"0.1.0"}`,
    },
  },
  {
    id: "data-model",
    chapter: "Overview & Fundamentals",
    title: "Data Model, UUIDv7 & Schemas",
    lead: "Strict SQLite schemas with UUIDv7 primary keys, microsecond UTC timestamps, and Crockford base32 wire encoding.",
    content:
      "All primary keys are UUIDv7 stored as 16-byte BLOBs for fast B-Tree indexing and temporal ordering. On the wire, IDs are formatted as 26-character lowercase Crockford base32 strings. Every table includes `id`, `created` (INTEGER µs UTC), `updated` (INTEGER µs UTC), and `_v` (version counter).",
    table: {
      headers: ["Field Type", "SQLite Column", "Wire Type", "Validation"],
      rows: [
        ["id", "BLOB (16 bytes)", "String (26-char base32)", "UUIDv7 time-ordered"],
        ["text", "TEXT", "String", "min/max length, regex"],
        ["email", "TEXT", "String", "RFC 5322 email syntax"],
        ["number", "REAL / INTEGER", "Number", "min, max, step"],
        ["bool", "INTEGER (0/1)", "Boolean", "true or false"],
        ["date", "INTEGER", "Number (µs UTC)", "Temporal bounds"],
        ["relation", "BLOB", "String (ID)", "Foreign key cascade rules"],
        ["multi-relation", "Junction table", "Array of IDs", "Normalized index junction"],
        ["json", "TEXT", "Object / Array", "JSONSchema validation"],
      ],
    },
  },
  {
    id: "crud-records",
    chapter: "Cloud Database",
    title: "CRUD Operations & Concurrency",
    lead: "Perform type-safe record creation, updates, and deletes with optimistic concurrency checking via ETags.",
    content:
      "Updates send `PATCH` requests containing only changed fields. Monolith checks the `If-Match` header containing the record's version `_v` to prevent conflicting updates in high-concurrency environments.",
    snippets: {
      ts: `// Create record\nconst task = await mono.collection("tasks").create({\n  title: "Ship documentation portal",\n  status: "in_progress",\n  priority: 1,\n});\n\n// Fetch by ID\nconst item = await mono.collection("tasks").getOne(task.id);\n\n// Update with optimistic concurrency\nawait mono.collection("tasks").update(task.id, {\n  status: "completed",\n});\n\n// Delete record\nawait mono.collection("tasks").delete(task.id);`,
      kotlin: `// Create record\nval task = mono.collection("tasks").create(mapOf(\n    "title" to "Ship documentation portal",\n    "status" to "in_progress",\n    "priority" to 1\n))\n\n// Fetch by ID\nval item = mono.collection("tasks").getOne(task.id)\n\n// Update\nmono.collection("tasks").update(task.id, mapOf("status" to "completed"))\n\n// Delete\nmono.collection("tasks").delete(task.id)`,
      dart: `// Create record\nfinal task = await mono.collection('tasks').create({\n  'title': 'Ship documentation portal',\n  'status': 'in_progress',\n  'priority': 1,\n});\n\n// Fetch by ID\nfinal item = await mono.collection('tasks').getOne(task.id);\n\n// Update\nawait mono.collection('tasks').update(task.id, {'status': 'completed'});\n\n// Delete\nawait mono.collection('tasks').delete(task.id);`,
      python: `# Create record\ntask = client.collection("tasks").create({\n    "title": "Ship documentation portal",\n    "status": "in_progress",\n    "priority": 1,\n})\n\n# Read & Update\nitem = client.collection("tasks").get_one(task["id"])\nclient.collection("tasks").update(task["id"], {"status": "completed"})\n\n# Delete\nclient.collection("tasks").delete(task["id"])`,
      rust: `let task = client.collection("tasks").create(&serde_json::json!({\n    "title": "Ship documentation portal",\n    "status": "in_progress",\n    "priority": 1\n})).await?;\n\nlet item = client.collection("tasks").get_one(&task.id).await?;\nclient.collection("tasks").update(&task.id, &serde_json::json!({"status": "completed"})).await?;\nclient.collection("tasks").delete(&task.id).await?;`,
      swift: `let task = try await client.collection("tasks").create(["title": "Ship documentation portal", "status": "in_progress"])\nlet item = try await client.collection("tasks").getOne(task.id)\ntry await client.collection("tasks").update(task.id, ["status": "completed"])\ntry await client.collection("tasks").delete(task.id)`,
      rest: `# Create a record\ncurl -s -X POST http://127.0.0.1:8090/api/v1/apps/default/collections/tasks/records \\\n  -H "authorization: Bearer $TOKEN" \\\n  -H "content-type: application/json" \\\n  -d '{"title":"Ship documentation portal","status":"in_progress","priority":1}'`,
    },
  },
  {
    id: "keyset-filtering",
    chapter: "Cloud Database",
    title: "Filtering & Keyset Pagination",
    lead: "High-performance queries using parameterized SQL filter pushdown and monotonic keyset cursor pagination.",
    content:
      "Avoid expensive `OFFSET` scans! Monolith uses deterministic keyset pagination with opaque cursor strings. Query filters support comparison (`=, !=, >, <, >=, <=`), string matches (`~, !~`), logical operators (`&&, ||`), and relation traversal.",
    snippets: {
      ts: `// List records with filter, sorting, and keyset cursor pagination\nconst page = await mono.collection("tasks").list({\n  filter: "status = 'active' && priority >= 2",\n  sort: "-created",\n  limit: 25,\n  cursor: nextCursor, // omit for first page\n});\n\nconsole.log(page.items, page.nextCursor);`,
      kotlin: `val page = mono.collection("tasks").list(\n    filter = "status = 'active' && priority >= 2",\n    sort = "-created",\n    limit = 25,\n    cursor = nextCursor\n)\nprintln("Loaded \${page.items.size} tasks, next cursor: \${page.nextCursor}")`,
      dart: `final page = await mono.collection('tasks').list(\n  filter: "status = 'active' && priority >= 2",\n  sort: "-created",\n  limit: 25,\n  cursor: nextCursor,\n);\nprint('Records: \${page.items.length}');`,
      python: `page = client.collection("tasks").list(\n    filter="status = 'active' && priority >= 2",\n    sort="-created",\n    limit=25,\n    cursor=next_cursor,\n)\nprint(page.items)`,
      rust: `let page = client.collection("tasks")\n    .list()\n    .filter("status = 'active' && priority >= 2")\n    .sort("-created")\n    .limit(25)\n    .cursor(next_cursor)\n    .send()\n    .await?;`,
      swift: `let page = try await client.collection("tasks").list(filter: "status = 'active' && priority >= 2", sort: "-created", limit: 25, cursor: nextCursor)`,
      rest: `curl -s "http://127.0.0.1:8090/api/v1/apps/default/collections/tasks/records?filter=status%3D%27active%27&sort=-created&limit=25" \\\n  -H "authorization: Bearer $TOKEN"`,
    },
  },
  {
    id: "relations-expand",
    chapter: "Cloud Database",
    title: "Relations & Multi-Hop Expand",
    lead: "Define foreign relations and expand related records up to 4 hops in a single optimized query.",
    content:
      "Single relations store foreign UUIDv7 keys; multi-relations use automatically indexed junction tables. The `expand` parameter resolves nested relations without extra round-trips.",
    snippets: {
      ts: `// Expand author and author's organization\nconst post = await mono.collection("posts").getOne(id, {\n  expand: "author,author.organization,tags",\n});\n\nconsole.log(post.title, post.expand?.author?.name);`,
      kotlin: `val post = mono.collection("posts").getOne(id, expand = "author,author.organization,tags")\nprintln("Author: \${post.expand["author"]}")`,
      dart: `final post = await mono.collection('posts').getOne(id, expand: 'author,author.organization,tags');`,
      python: `post = client.collection("posts").get_one(id, expand="author,tags")`,
      rust: `let post = client.collection("posts").get_one_with_expand(&id, "author,tags").await?;`,
      swift: `let post = try await client.collection("posts").getOne(id, expand: "author,tags")`,
      rest: `curl -s "http://127.0.0.1:8090/api/v1/apps/default/collections/posts/records/\$ID?expand=author,tags" \\\n  -H "authorization: Bearer \$TOKEN"`,
    },
  },
  {
    id: "batch-transactions",
    chapter: "Cloud Database",
    title: "Atomic Batch Transactions",
    lead: "Execute multiple mutations across multiple collections in a single ACID transaction savepoint.",
    content:
      "Batch requests execute sequentially inside an isolated database savepoint on the writer thread. If any mutation fails access checks or validation, all changes roll back and an RFC 9457 error is returned.",
    snippets: {
      ts: `const results = await mono.batch([\n  { method: "POST", path: "/collections/projects/records", body: { name: "Apollo" } },\n  { method: "POST", path: "/collections/tasks/records", body: { title: "Phase 1", project: "@0" } },\n  { method: "DELETE", path: "/collections/drafts/records/018f3a..." },\n]);`,
      kotlin: `val results = mono.batch(listOf(\n    BatchOperation("POST", "/collections/projects/records", mapOf("name" to "Apollo")),\n    BatchOperation("POST", "/collections/tasks/records", mapOf("title" to "Phase 1"))\n))`,
      dart: `final results = await mono.batch([\n  BatchOperation(method: 'POST', path: '/collections/projects/records', body: {'name': 'Apollo'}),\n  BatchOperation(method: 'POST', path: '/collections/tasks/records', body: {'title': 'Phase 1'}),\n]);`,
      python: `results = client.batch([\n    {"method": "POST", "path": "/collections/projects/records", "body": {"name": "Apollo"}},\n    {"method": "POST", "path": "/collections/tasks/records", "body": {"title": "Phase 1"}},\n])`,
      rust: `let results = client.batch(vec![\n    BatchOp::post("/collections/projects/records", serde_json::json!({"name": "Apollo"})),\n    BatchOp::post("/collections/tasks/records", serde_json::json!({"title": "Phase 1"})),\n]).await?;`,
      swift: `let results = try await client.batch([BatchOp(method: "POST", path: "/collections/projects/records", body: ["name": "Apollo"])])`,
      rest: `curl -s -X POST http://127.0.0.1:8090/api/v1/apps/default/batch \\\n  -H "authorization: Bearer $TOKEN" \\\n  -H "content-type: application/json" \\\n  -d '{"operations":[{"method":"POST","path":"/collections/projects/records","body":{"name":"Apollo"}}]}'`,
    },
  },
  {
    id: "realtime-streaming",
    chapter: "Cloud Database",
    title: "Realtime Synchronization (SSE)",
    lead: "Stream live database mutations to client applications using multiplexed Server-Sent Events.",
    content:
      "All subscriptions within an app share a single persistent HTTP connection. When records are created, updated, or deleted, Monolith checks subscriber view rules in memory and streams delta events with automatic exponential backoff reconnect.",
    snippets: {
      ts: `// Subscribe to changes in tasks collection\nconst unsubscribe = mono.collection("tasks").subscribe((event) => {\n  console.log("Action:", event.action); // "create" | "update" | "delete"\n  console.log("Record:", event.record);\n});\n\n// Later clean up\n// unsubscribe();`,
      kotlin: `// Collect reactive Kotlin StateFlow / SharedFlow\nmono.collection("tasks").subscribeAsFlow().collect { event ->\n    when (event.action) {\n        "create" -> addTaskToUI(event.record)\n        "update" -> updateTaskInUI(event.record)\n        "delete" -> removeTaskFromUI(event.record)\n    }\n}`,
      dart: `// Flutter Stream subscription\nfinal sub = mono.collection('tasks').subscribe((event) {\n  print('\${event.action}: \${event.record['title']}');\n});\n// sub.cancel();`,
      python: `# Python realtime SSE streaming (Phase 4)\n# Listen to collection changes with async generator`,
      rust: `// Rust realtime SSE stream\n// Connects to /api/v1/apps/{app}/realtime change bus`,
      swift: `// Swift AsyncSequence for realtime changes\nfor await event in client.collection("tasks").events() {\n    print("Changed: \\(event.record)")\n}`,
      rest: `# Connect to realtime event stream\ncurl -N -H "Accept: text/event-stream" \\\n  -H "authorization: Bearer $TOKEN" \\\n  "http://127.0.0.1:8090/api/v1/apps/default/realtime?collections=tasks,projects"`,
    },
  },
  {
    id: "access-rules",
    chapter: "Authentication & Security",
    title: "Collection Access Rules",
    lead: "Granular access control defined directly on collections using expressive SQL-compiled predicates.",
    content:
      "Every collection specifies five access rule expressions: `listRule`, `viewRule`, `createRule`, `updateRule`, and `deleteRule`. Set to `null` to restrict to superusers only, `\"\"` for public access, or expressions referencing `@request.auth.id`, `@request.data.*`, or related records.",
    snippets: {
      ts: `// Rule examples configured in collection schema:\n// Allow users to view only their own records:\n// listRule: "@request.auth.id != '' && author = @request.auth.id"\n\n// Allow team members to update team documents:\n// updateRule: "@request.auth.id != '' && @collection.team_members.user = @request.auth.id && @collection.team_members.team = team"`,
      kotlin: `// Access rules are enforced server-side for all queries.\n// Client automatically receives 403 Forbidden with RFC 9457 details on rule violation.`,
      dart: `// Rules compile once per schema version into parameterized SQL WHERE clauses.`,
      python: `# Superusers bypass access rules for backend jobs and migrations.`,
      rust: `// The rule compiler ensures no dynamic SQL string concatenation on hot paths.`,
      swift: `// Rules are verified without querying the database for simple auth predicates.`,
      rest: `# Attempting unauthorized query returns RFC 9457 Problem Details\n# HTTP/1.1 403 Forbidden\n# Content-Type: application/problem+json\n# {"type":"urn:monolith:error:forbidden","title":"Forbidden","status":403,"detail":"Rule evaluation rejected request"}`,
    },
  },
  {
    id: "auth-signin",
    chapter: "Authentication & Security",
    title: "Password Sign-in & Token Management",
    lead: "Authenticate users with email/password and manage rotating refresh tokens with automatic reuse detection.",
    content:
      "Monolith issues short-lived HMAC-SHA256 access tokens verified in memory on the hot path without a database lookup. Refresh tokens are stored as Argon2/BLAKE3 hashes and rotated on each refresh with immediate revocation upon reuse.",
    snippets: {
      ts: `// Sign in with email and password\nconst session = await mono.auth("users").signInWithPassword("alice@example.com", "secret123");\n\n// Listen to auth state transitions\nconst unsub = mono.auth("users").onChange((token) => {\n  console.log("Logged in:", Boolean(token));\n});\n\n// Sign out\nmono.auth("users").signOut();`,
      kotlin: `val session = mono.auth("users").signInWithPassword("alice@example.com", "secret123")\n\n// Observe token StateFlow (persisted to EncryptedSharedPreferences on Android)\nmono.auth("users").tokenFlow.collect { token ->\n    println("Active token: $token")\n}`,
      dart: `await mono.auth('users').signInWithPassword('alice@example.com', 'secret123');\nmono.auth('users').onChange((token) {\n  print('Session: \${token != null ? "Active" : "Logged out"}');\n});`,
      python: `session = client.auth("users").sign_in_with_password("alice@example.com", "secret123")\nprint("Token:", client.http.token)`,
      rust: `client.auth("users").sign_in_with_password("alice@example.com", "secret123").await?;`,
      swift: `let session = try await client.auth("users").signIn(email: "alice@example.com", password: "secret123")`,
      rest: `curl -s -X POST http://127.0.0.1:8090/api/v1/apps/default/auth/login \\\n  -H "content-type: application/json" \\\n  -d '{"email":"alice@example.com","password":"secret123"}'`,
    },
  },
  {
    id: "auth-oauth",
    chapter: "Authentication & Security",
    title: "OAuth2 / OIDC & Auth Policy",
    lead: "Federated single sign-on with Google, GitHub, Apple, and custom OpenID Connect providers.",
    content:
      "Configure OAuth2 providers in your app's auth policy. The client initiates the provider authorization flow and exchanges the code for a Monolith user session with verified email linking.",
    snippets: {
      ts: `// OAuth sign-in flow\nconst authUrl = mono.auth("users").getOAuthURL("google", {\n  redirectUrl: "https://myapp.com/auth/callback",\n});\n// Redirect user to authUrl...`,
      kotlin: `// Android Chrome Custom Tab or Credential Manager flow\nval authUrl = mono.auth("users").getOAuthURL("google", redirectUri)`,
      dart: `final authUrl = mono.auth('users').getOAuthURL('google', redirectUrl: 'myapp://callback');`,
      python: `# Configure OAuth providers via app admin settings`,
      rust: `// Auth policy supports provider allowlists and domain filters`,
      swift: `let authUrl = client.auth("users").getOAuthURL(provider: "apple", redirectUrl: redirectUrl)`,
      rest: `curl -s "http://127.0.0.1:8090/api/v1/apps/default/auth/oauth2/google/url?redirect=https%3A%2F%2Fmyapp.com%2Fcallback"`,
    },
  },
  {
    id: "auth-keys",
    chapter: "Authentication & Security",
    title: "Scoped Runtime API Keys",
    lead: "Create granular server-to-server API keys (`mbk_<env>_...`) with collection scopes, operation masks, and rate limits.",
    content:
      "Runtime API keys are ideal for backend microservices, CI/CD jobs, and edge workers. Keys support CIDR IP allowlisting, per-minute rate limits with GCRA leaky-bucket algorithms, and read/write/admin collection scopes.",
    snippets: {
      ts: `// Initialize client with a scoped API key\nconst mono = createClient({\n  url: "http://localhost:8090",\n  app: "default",\n  key: "mbk_live_018f3a2b4c5d...",\n});`,
      kotlin: `val mono = MonolithAndroid.builder(context)\n    .url("http://10.0.2.2:8090")\n    .app("default")\n    .apiKey("mbk_live_018f3a2b4c5d...")\n    .build()`,
      dart: `final mono = createClient(\n  url: 'http://localhost:8090',\n  app: 'default',\n  apiKey: 'mbk_live_018f3a2b4c5d...',\n);`,
      python: `client = create_client(url="http://localhost:8090", app="default", key="mbk_live_...")`,
      rust: `let client = Client::builder("http://localhost:8090").app("default").key("mbk_live_...").build()?;`,
      swift: `let client = MonolithClient(url: url, app: "default", key: "mbk_live_...")`,
      rest: `curl -s http://127.0.0.1:8090/api/v1/apps/default/collections/tasks/records \\\n  -H "x-api-key: mbk_live_018f3a2b4c5d..."`,
    },
  },
  {
    id: "auth-install",
    chapter: "Authentication & Security",
    title: "First-Superuser Bootstrap",
    lead: "Secure, headless first-superuser installer via memory-held cryptographic setup URLs.",
    content:
      "On first start, Monolith generates a 128-bit cryptographic setup token held only in RAM and prints it to stderr. Once used or on server restart, the installer token is cleared forever to prevent unauthorized takeover.",
    snippets: {
      ts: `// Setup URL format:\n// http://localhost:8090/_/#/install/<token>\n\n// In development mode:\n// pnpm dev\n// Prints direct link to local Vite UI setup!`,
      kotlin: `// Superusers manage apps, backups, and storage buckets.`,
      dart: `// Normal app clients never connect to the control plane.`,
      python: `# Automate initial bootstrap in integration tests:\n# POST /api/v1/system/install {"token": token, "email": email, "password": password}`,
      rust: `// Handled internally in monolith-server::installer`,
      swift: `// Setup is web-based via the SolidJS Admin Console.`,
      rest: `curl -s -X POST http://127.0.0.1:8090/api/v1/system/install \\\n  -H "content-type: application/json" \\\n  -d '{"token":"...","email":"admin@example.com","password":"superpassword"}'`,
    },
  },
  {
    id: "storage-buckets",
    chapter: "Storage, Backups & Ops",
    title: "Buckets & BLAKE3 Deduplication",
    lead: "Store files with content-addressable BLAKE3 deduplication, quota enforcement, and public/private access rules.",
    content:
      "Identical files uploaded across different records share underlying disk blocks automatically. Storage buckets support max upload sizes, MIME type allowlists, and access rules identical to collection access control.",
    snippets: {
      ts: `// Upload binary file blob\nawait mono.files.upload("avatars", "profile_pic.png", fileBlob);\n\n// Generate public or signed download URL\nconst fileUrl = mono.files.getURL("avatars", "profile_pic.png");\nconsole.log("Download:", fileUrl);`,
      kotlin: `// Upload raw bytes\nmono.files.upload("avatars", "profile_pic.png", byteArray)\nval fileUrl = mono.files.getURL("avatars", "profile_pic.png")`,
      dart: `await mono.files.upload('avatars', 'profile_pic.png', bytes);\nfinal fileUrl = mono.files.getURL('avatars', 'profile_pic.png');`,
      python: `client.files.upload("avatars", "profile_pic.png", b"file bytes")\nurl = client.files.get_url("avatars", "profile_pic.png")`,
      rust: `client.files().upload("avatars", "profile_pic.png", bytes::Bytes::from("data")).await?;`,
      swift: `try await client.files.upload(bucket: "avatars", name: "profile_pic.png", data: data)`,
      rest: `curl -s -X PUT http://127.0.0.1:8090/api/v1/apps/default/storage/buckets/avatars/objects/profile_pic.png \\\n  -H "authorization: Bearer $TOKEN" \\\n  --data-binary @photo.png`,
    },
  },
  {
    id: "storage-thumbs",
    chapter: "Storage, Backups & Ops",
    title: "On-Demand Image Thumbnails",
    lead: "Dynamic image resizing with width and height queries, executed on a dedicated thread pool and cached.",
    content:
      "Request image thumbnails by appending the `thumb=WxH` query parameter. Monolith generates resized WebP, PNG, or JPEG thumbnails on demand and caches them in the app's thumb directory for instant subsequent delivery.",
    snippets: {
      ts: `// Request 200x200 thumbnail\nconst thumbUrl = mono.files.getURL("avatars", "profile_pic.png", {\n  thumb: "200x200",\n});\n\n// Use in <img> tag\nconsole.log(thumbUrl);`,
      kotlin: `val thumbUrl = mono.files.getURL("avatars", "profile_pic.png", "200x200")`,
      dart: `final thumbUrl = mono.files.getURL('avatars', 'profile_pic.png', thumb: '200x200');`,
      python: `thumb_url = client.files.get_url("avatars", "profile_pic.png", thumb="200x200")`,
      rust: `let thumb_url = client.files().get_url_with_thumb("avatars", "profile_pic.png", "200x200");`,
      swift: `let thumbUrl = client.files.getURL(bucket: "avatars", name: "profile_pic.png", thumb: "200x200")`,
      rest: `curl -s "http://127.0.0.1:8090/api/v1/apps/default/storage/buckets/avatars/objects/profile_pic.png?thumb=200x200" \\\n  -o thumb.jpg`,
    },
  },
  {
    id: "backups-ops",
    chapter: "Storage, Backups & Ops",
    title: "Online Backups, Restore & Metrics",
    lead: "Non-blocking online SQLite WAL backups, point-in-time recovery, and Prometheus metrics.",
    content:
      "Create zero-downtime snapshots of any app database using SQLite's online backup API. Backups include database state, migrations, and file storage bundles. The Prometheus `/metrics` endpoint exports request latencies, WAL checkpoint status, and writer queue depths.",
    table: {
      headers: ["Endpoint", "Method", "Description", "Auth Level"],
      rows: [
        ["/api/v1/apps/{app}/backups", "POST", "Trigger online backup archive creation", "Superuser"],
        ["/api/v1/apps/{app}/backups", "GET", "List existing snapshot archives", "Superuser"],
        ["/api/v1/apps/{app}/backups/{id}/restore", "POST", "Restore app from backup archive", "Superuser"],
        ["/metrics", "GET", "Prometheus exposition format metrics", "Public / Scoped"],
        ["/health", "GET", "Liveness and readiness health probe", "Public"],
      ],
    },
  },
  {
    id: "sdk-matrix",
    chapter: "API & SDK Reference",
    title: "SDK Compatibility Matrix",
    lead: "Comprehensive feature support across all official Monolith client SDKs.",
    content:
      "Every SDK is tested against an active Monolith server using automated conformance test suites. All SDKs handle URL construction, retry backoff, auth tokens, error parsing, and strict types.",
    table: {
      headers: ["Language / Platform", "Package Registry", "Database CRUD", "Storage & Thumbs", "Realtime SSE", "Auth Observers"],
      rows: [
        ["TypeScript (Web, Node, Bun, Deno, RN)", "@monolith/client (npm)", "Yes", "Yes", "Yes", "Yes (onChange)"],
        ["Kotlin (Android, KMP, JVM)", "dev.monolith:monolith-android (Maven)", "Yes", "Yes", "Yes", "Yes (Flow)"],
        ["Dart (Flutter, Web, Mobile, Desktop)", "monolith_client (pub.dev)", "Yes", "Yes", "Yes", "Yes (Stream)"],
        ["Python (CLI, Automation, Scripts)", "monolith-client (PyPI)", "Yes", "Yes", "Phase 4", "Yes"],
        ["Rust (Backend Microservices, Tools)", "monolith-client (crates.io)", "Yes", "Yes", "Phase 4", "Yes"],
        ["Swift (iOS, macOS, watchOS)", "Monolith (SwiftPM)", "Phase 5", "Phase 5", "Phase 5", "Phase 5"],
        ["REST API (cURL, HTTP Clients)", "Direct HTTP /api/v1", "Yes", "Yes", "Yes (SSE)", "Bearer Token"],
      ],
    },
  },
  {
    id: "sdk-ts",
    chapter: "API & SDK Reference",
    title: "TypeScript SDK (@monolith/client)",
    lead: "Official TypeScript client SDK for Web browsers, Node.js 20+, Bun, Deno, and React Native.",
    content:
      "Built with zero dependencies, native `fetch`, SSE streaming, and complete TypeScript types generated from OpenAPI schemas.",
    snippets: {
      ts: `import { createClient } from "@monolith/client";\n\nconst mono = createClient({\n  url: "http://localhost:8090",\n  app: "default",\n});\n\n// Full type-safe collection operations\nconst task = await mono.collection("tasks").create({ title: "Build app" });\nconst list = await mono.collection("tasks").list({ limit: 10 });`,
    },
  },
  {
    id: "sdk-kotlin",
    chapter: "API & SDK Reference",
    title: "Kotlin & Android SDK (dev.monolith)",
    lead: "Kotlin Multiplatform (JVM, Android, iOS) and Android AAR client library.",
    content:
      "Provides native Kotlin coroutines, reactive `StateFlow` auth observers, and automated `EncryptedSharedPreferences` token persistence.",
    snippets: {
      kotlin: `import dev.monolith.android.MonolithAndroid\n\nval client = MonolithAndroid.builder(context)\n    .url("http://10.0.2.2:8090")\n    .app("default")\n    .build()\n\n// Suspending coroutine calls\nval record = client.collection("tasks").create(mapOf("title" to "Android Task"))`,
    },
  },
  {
    id: "sdk-dart",
    chapter: "API & SDK Reference",
    title: "Dart & Flutter SDK (monolith_client)",
    lead: "Pure Dart and Flutter client SDK targeting iOS, Android, macOS, Windows, Linux, and Web.",
    content:
      "Features reactive Dart Streams for realtime subscriptions and authentication changes, zero external dependencies, and built-in SSE parsing.",
    snippets: {
      dart: `import 'package:monolith_client/monolith_client.dart';\n\nfinal client = createClient(url: 'http://localhost:8090', app: 'default');\nfinal record = await client.collection('tasks').create({'title': 'Flutter Task'});`,
    },
  },
  {
    id: "sdk-python",
    chapter: "API & SDK Reference",
    title: "Python SDK (monolith-client)",
    lead: "Python client library for scripts, automation, data science, and backup utilities.",
    content:
      "Synchronous and asynchronous API support with standard dict mapping, session management, and HTTP retries.",
    snippets: {
      python: `from monolith import create_client\n\nclient = create_client(url="http://localhost:8090", app="default")\nrecord = client.collection("tasks").create({"title": "Python Script"})`,
    },
  },
  {
    id: "sdk-rust",
    chapter: "API & SDK Reference",
    title: "Rust SDK (monolith-client)",
    lead: "High-performance asynchronous Rust client powered by Tokio and Reqwest.",
    content:
      "Zero-copy serialization with Serde, typed builders, connection pooling, and RFC 9457 Problem Details error decoding.",
    snippets: {
      rust: `use monolith_client::Client;\n\nlet client = Client::builder("http://localhost:8090").app("default").build()?;\nlet task = client.collection("tasks").create(&serde_json::json!({"title": "Rust Task"})).await?;`,
    },
  },
  {
    id: "sdk-swift",
    chapter: "API & SDK Reference",
    title: "Swift SDK (SwiftPM)",
    lead: "Native Swift SDK for iOS 17+, macOS 14+, and watchOS.",
    content:
      "Modern Swift concurrency (async/await, AsyncSequence), Codable model binding, and Keychain token storage.",
    snippets: {
      swift: `import Monolith\n\nlet client = MonolithClient(url: URL(string: "http://localhost:8090")!, app: "default")\nlet task = try await client.collection("tasks").create(["title": "Swift Task"])`,
    },
  },
  {
    id: "sdk-rest",
    chapter: "API & SDK Reference",
    title: "REST API & RFC 9457 Errors",
    lead: "Universal JSON REST API with RFC 9457 Problem Details for all errors.",
    content:
      "Every API error returns `application/problem+json` with standard `type`, `title`, `status`, and `detail` fields, plus an `x-request-id` header for end-to-end tracing.",
    snippets: {
      rest: `# Example RFC 9457 Error Response:\n# HTTP/1.1 400 Bad Request\n# Content-Type: application/problem+json\n# x-request-id: 018f3a2b-4c5d-7e8f-9a0b-1c2d3e4f5a6b\n#\n# {\n#   "type": "urn:monolith:error:validation",\n#   "title": "Validation Failed",\n#   "status": 400,\n#   "detail": "Field 'email' must be a valid email address",\n#   "invalid_fields": {"email": "invalid syntax"}\n# }`,
    },
  },
];
