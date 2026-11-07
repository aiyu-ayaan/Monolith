export interface CodeSnippets {
  ts?: string;
  kotlin?: string;
  dart?: string;
  python?: string;
  rust?: string;
  rest?: string;
}

export interface DocSection {
  id: string;
  title: string;
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

export interface DocPage {
  id: string;
  title: string;
  category: string;
  subnav: string;
  breadcrumbs: string[];
  bannerAlert?: {
    text: string;
    linkText: string;
    linkUrl: string;
  };
  overview: string;
  keyCapabilities?: CapabilityCard[];
  sections: DocSection[];
}

export const DOC_PAGES: Record<string, DocPage> = {
  "firestore": {
    id: "firestore",
    title: "Cloud Database & Collections",
    category: "Cloud Database",
    subnav: "Build",
    breadcrumbs: ["Monolith", "Documentation", "Database", "Build"],
    bannerAlert: {
      text: "Learn how Monolith scales with SQLite group commit and per-app database isolation.",
      linkText: "Learn more in Architecture →",
      linkUrl: "#architecture-engine",
    },
    overview:
      "Monolith Cloud Database is a flexible, highly scalable multi-app relational database for mobile, web, and server development. It keeps your data in sync across client applications through multiplexed realtime listeners and provides strict schema validation, keyset cursor pagination, and ACID transactions.",
    keyCapabilities: [
      {
        title: "Relational Flexibility",
        desc: "Strict schemas with UUIDv7 primary keys, typed fields (text, email, number, bool, date, select, relation, json), and indexed junction tables for multi-relations.",
      },
      {
        title: "Expressive Querying",
        desc: "PocketBase-compatible expression filters with parameterized SQL compilation, multi-hop relation expand (up to 4 levels), and deterministic keyset pagination.",
      },
      {
        title: "Realtime Updates",
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
    ],
    sections: [
      {
        id: "quickstart-init",
        title: "Client Initialization",
        lead: "Initialize the Monolith client SDK with your backend URL and target application identifier.",
        content:
          "All Monolith SDKs provide a uniform client interface. When connecting to local development, use your server host port (default 8090). In Android emulators, route through `10.0.2.2:8090`.",
        snippets: {
          ts: `import { createClient } from "@monolith/client";\n\nconst mono = createClient({\n  url: "http://localhost:8090",\n  app: "default",\n  key: "mbk_live_..." // optional scoped API key\n});`,
          kotlin: `import dev.monolith.android.MonolithAndroid\n\n// Android client with EncryptedSharedPreferences token persistence\nval mono = MonolithAndroid.builder(context)\n    .url("http://10.0.2.2:8090")\n    .app("default")\n    .build()`,
          dart: `import 'package:monolith_client/monolith_client.dart';\n\nfinal mono = createClient(\n  url: 'http://localhost:8090',\n  app: 'default',\n);`,
          python: `from monolith import create_client\n\nclient = create_client(\n    url="http://localhost:8090",\n    app="default",\n)`,
          rust: `use monolith_client::Client;\n\n#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n    let client = Client::builder("http://localhost:8090")\n        .app("default")\n        .build()?;\n    Ok(())\n}`,
          rest: `# Ping the health check endpoint\ncurl -s http://127.0.0.1:8090/health`,
        },
      },
      {
        id: "crud-records",
        title: "CRUD Operations",
        lead: "Create, read, update, and delete collection records with automatic timestamping and schema checks.",
        content:
          "Records automatically generate a 16-byte UUIDv7 identifier, microsecond UTC `created` and `updated` timestamps, and a change sequence `_v` used as the HTTP ETag for optimistic concurrency.",
        snippets: {
          ts: `// Create a task record\nconst task = await mono.collection("tasks").create({\n  title: "Deploy release v1.0",\n  status: "active",\n  priority: 1\n});\n\n// Read by ID\nconst item = await mono.collection("tasks").getOne(task.id);\n\n// Partial update (PATCH)\nawait mono.collection("tasks").update(task.id, { status: "done" });\n\n// Delete record\nawait mono.collection("tasks").delete(task.id);`,
          kotlin: `// Create record\nval task = mono.collection("tasks").create(mapOf(\n    "title" to "Deploy release v1.0",\n    "status" to "active"\n))\n\n// Fetch by ID\nval item = mono.collection("tasks").getOne(task.id)\n\n// Partial update\nmono.collection("tasks").update(task.id, mapOf("status" to "done"))\n\n// Delete record\nmono.collection("tasks").delete(task.id)`,
          dart: `// Create record\nfinal task = await mono.collection('tasks').create({\n  'title': 'Deploy release v1.0',\n  'status': 'active',\n});\n\n// Read record\nfinal item = await mono.collection('tasks').getOne(task.id);\n\n// Update\nawait mono.collection('tasks').update(task.id, {'status': 'done'});\n\n// Delete\nawait mono.collection('tasks').delete(task.id);`,
          python: `# Create record\ntask = client.collection("tasks").create({\n    "title": "Deploy release v1.0",\n    "status": "active",\n})\n\n# Read record\nitem = client.collection("tasks").get_one(task["id"])\n\n# Update record\nclient.collection("tasks").update(task["id"], {"status": "done"})\n\n# Delete record\nclient.collection("tasks").delete(task["id"])`,
          rust: `// Create record\nlet task = client.collection("tasks").create(&serde_json::json!({\n    "title": "Deploy release v1.0",\n    "status": "active"\n})).await?;\n\n// Read record\nlet item = client.collection("tasks").get_one(&task.id).await?;\n\n// Delete record\nclient.collection("tasks").delete(&task.id).await?;`,
          rest: `# Create a record\ncurl -s -X POST http://127.0.0.1:8090/api/v1/apps/default/collections/tasks/records \\\n  -H "authorization: Bearer $TOKEN" \\\n  -H "content-type: application/json" \\\n  -d '{"title":"Deploy release v1.0","status":"active"}'\n\n# Read a record\ncurl -s http://127.0.0.1:8090/api/v1/apps/default/collections/tasks/records/018f3a2b4c5d6e7f8a9b0c1d2e \\\n  -H "authorization: Bearer $TOKEN"`,
        },
      },
      {
        id: "keyset-filtering",
        title: "Filtering & Keyset Pagination",
        lead: "Filter documents with expressive operators and paginate deterministically without COUNT overhead.",
        content:
          "Monolith compiles filter rules directly into parameterized SQL `WHERE` clauses with index pushdown. Keyset pagination uses monotonic cursors rather than slow offset skips.",
        snippets: {
          ts: `// Filter, sort and keyset page\nconst result = await mono.collection("tasks").list({\n  filter: "status = 'active' && priority >= 2",\n  sort: "-created",\n  limit: 25,\n  expand: "assignee,project"\n});\n\nconsole.log(result.items, result.nextCursor);`,
          kotlin: `val result = mono.collection("tasks").list(\n    filter = "status = 'active' && priority >= 2",\n    sort = "-created",\n    limit = 25,\n    expand = "assignee,project"\n)\nprintln("Found: \${result.items.size}")`,
          dart: `final result = await mono.collection('tasks').list(\n  filter: "status = 'active' && priority >= 2",\n  sort: "-created",\n  limit: 25,\n  expand: "assignee,project",\n);\nprint(result.items);`,
          python: `result = client.collection("tasks").list(\n    filter="status = 'active' && priority >= 2",\n    sort="-created",\n    limit=25,\n    expand="assignee,project",\n)\nprint(result.items)`,
          rust: `let page = client.collection("tasks")\n    .list()\n    .filter("status = 'active' && priority >= 2")\n    .sort("-created")\n    .limit(25)\n    .expand("assignee,project")\n    .send()\n    .await?;\nprintln!("Items: {}", page.items.len());`,
          rest: `curl -s "http://127.0.0.1:8090/api/v1/apps/default/collections/tasks/records?filter=status%3D%27active%27&sort=-created&limit=25&expand=assignee" \\\n  -H "authorization: Bearer $TOKEN"`,
        },
      },
      {
        id: "realtime-streaming",
        title: "Realtime Synchronization",
        lead: "Stream live record changes across all connected clients using multiplexed Server-Sent Events.",
        content:
          "Monolith shares a single persistent SSE connection per application. When subscribing to multiple collections, events are multiplexed over the existing stream. Connection lifecycle is automatically managed.",
        snippets: {
          ts: `// Subscribe to all changes in the tasks collection\nconst unsub = mono.collection("tasks").subscribe((event) => {\n  console.log("Action:", event.action); // 'create' | 'update' | 'delete'\n  console.log("Record:", event.record);\n});\n\n// Later unsubscribe\n// unsub();`,
          kotlin: `// Reactive Kotlin Flow streaming\nmono.collection("tasks").subscribeAsFlow().collect { event ->\n    println("Task \${event.action}: \${event.record}")\n}`,
          dart: `final unsub = mono.collection('tasks').subscribe((event) {\n  print('Action: \${event.action}, Task: \${event.record}');\n});`,
          python: `# Python websocket / SSE streaming (planned for Phase 4)`,
          rust: `// Rust realtime change bus stream (planned for Phase 4)`,
          rest: `curl -N -H "Accept: text/event-stream" \\\n  -H "authorization: Bearer $TOKEN" \\\n  "http://127.0.0.1:8090/api/v1/apps/default/realtime?collections=tasks,projects"`,
        },
      },
      {
        id: "batch-transactions",
        title: "Atomic Batch Operations",
        lead: "Execute multiple mutations in a single atomic database transaction.",
        content:
          "Batch operations execute atomically inside an isolated savepoint. If any mutation violates schema constraints or access rules, the entire batch rolls back with detailed RFC 9457 error explanations.",
        snippets: {
          ts: `const results = await mono.batch([\n  { method: "POST", path: "/collections/tasks/records", body: { title: "Step 1" } },\n  { method: "POST", path: "/collections/tasks/records", body: { title: "Step 2" } },\n  { method: "DELETE", path: "/collections/tasks/records/rec_old" }\n]);`,
          kotlin: `val results = mono.batch(listOf(\n    BatchOperation("POST", "/collections/tasks/records", mapOf("title" to "Step 1")),\n    BatchOperation("POST", "/collections/tasks/records", mapOf("title" to "Step 2"))\n))`,
          dart: `final results = await mono.batch([\n  BatchOperation(method: 'POST', path: '/collections/tasks/records', body: {'title': 'Step 1'}),\n  BatchOperation(method: 'POST', path: '/collections/tasks/records', body: {'title': 'Step 2'}),\n]);`,
          python: `results = client.batch([\n    {"method": "POST", "path": "/collections/tasks/records", "body": {"title": "Step 1"}},\n    {"method": "POST", "path": "/collections/tasks/records", "body": {"title": "Step 2"}},\n])`,
          rust: `let ops = vec![\n    BatchOp::post("/collections/tasks/records", serde_json::json!({"title": "Step 1"})),\n    BatchOp::post("/collections/tasks/records", serde_json::json!({"title": "Step 2"})),\n];\nlet res = client.batch(ops).await?;`,
          rest: `curl -s -X POST http://127.0.0.1:8090/api/v1/apps/default/batch \\\n  -H "authorization: Bearer $TOKEN" \\\n  -H "content-type: application/json" \\\n  -d '{"operations":[{"method":"POST","path":"/collections/tasks/records","body":{"title":"Step 1"}}]}'`,
        },
      },
    ],
  },
  "auth": {
    id: "auth",
    title: "Authentication & User Management",
    category: "Authentication",
    subnav: "Build",
    breadcrumbs: ["Monolith", "Documentation", "Authentication", "Build"],
    overview:
      "Monolith Authentication provides secure, scalable user identity management. It supports email and password authentication, OAuth2/OIDC federated providers, rotating refresh tokens with automatic reuse detection, and scoped API keys for server-to-server authorization.",
    keyCapabilities: [
      {
        title: "Zero-Lookup JWTs",
        desc: "Short-lived HMAC-SHA256 access tokens are verified on the hot path in memory with zero database reads.",
      },
      {
        title: "Rotating Refresh Tokens",
        desc: "High-security refresh token rotation (`mbr_...`) invalidates token families immediately upon reuse detection.",
      },
      {
        title: "First Superuser Link",
        desc: "One-time memory-held cryptographic installer link for headless and containerized bootstrapping.",
      },
      {
        title: "Scoped Runtime Keys",
        desc: "Granular API keys (`mbk_<env>_...`) scoped by collection, operations (read, write, admin), and rate limits.",
      },
    ],
    sections: [
      {
        id: "auth-signin",
        title: "Password Sign-in & Token Management",
        lead: "Sign in users with email and password and listen for reactive authentication state changes.",
        content:
          "Authentication state is managed by the client SDK and automatically attached as a Bearer token to all outgoing database, storage, and realtime requests.",
        snippets: {
          ts: `// Sign in with password\nawait mono.auth("users").signInWithPassword("alice@example.com", "secret123");\n\n// Reactive session listener\nconst unsub = mono.auth("users").onChange((token) => {\n  console.log("Auth state changed:", token ? "Logged in" : "Logged out");\n});\n\n// Sign out\nmono.auth("users").signOut();`,
          kotlin: `// Sign in with password\nval session = mono.auth("users").signInWithPassword("alice@example.com", "secret123")\n\n// Collect reactive StateFlow\nmono.auth("users").tokenFlow.collect { token ->\n    println("Session token: $token")\n}`,
          dart: `// Sign in\nawait mono.auth('users').signInWithPassword('alice@example.com', 'secret123');\n\n// Listen for auth changes\nmono.auth('users').onChange((token) {\n  print('Token: $token');\n});`,
          python: `# Sign in\nclient.auth("users").sign_in_with_password("alice@example.com", "secret123")\nprint("Current token:", client.http.token)`,
          rust: `client.auth("users").sign_in_with_password("alice@example.com", "secret123").await?;\nprintln!("Token stored in client");`,
          rest: `curl -s -X POST http://127.0.0.1:8090/api/v1/apps/default/auth/login \\\n  -H "content-type: application/json" \\\n  -d '{"email":"alice@example.com","password":"secret123"}'`,
        },
      },
    ],
  },
  "storage": {
    id: "storage",
    title: "Storage Buckets & Files",
    category: "Storage",
    subnav: "Build",
    breadcrumbs: ["Monolith", "Documentation", "Storage", "Build"],
    overview:
      "Store and serve user-generated content, media files, and binary assets with content-addressed BLAKE3 deduplication, quota tracking, on-demand thumbnail generation, and private or public bucket access policies.",
    keyCapabilities: [
      {
        title: "BLAKE3 Content Addressing",
        desc: "Files are hashed with BLAKE3 and deduplicated per app. Identical files uploaded multiple times share disk blocks.",
      },
      {
        title: "On-Demand Thumbnails",
        desc: "Request dynamic image resizing via `{ thumb: 'WxH' }` query parameter, processed through a bounded pool and cached.",
      },
      {
        title: "Streamed Uploads",
        desc: "Stream files directly into bucket storage with MIME type sniffing and Range header support for downloads.",
      },
    ],
    sections: [
      {
        id: "storage-upload-url",
        title: "Uploading Files & Generating URLs",
        lead: "Upload binary data or blobs and generate thumbnail-enabled download URLs.",
        content:
          "Bucket objects are identified by bucket name and key path. Uploads automatically calculate content hash and verify size quotas.",
        snippets: {
          ts: `// Upload image blob\nawait mono.files.upload("avatars", "user_123.jpg", fileBlob);\n\n// Generate download URL with 150x150 thumbnail\nconst thumbUrl = mono.files.getURL("avatars", "user_123.jpg", { thumb: "150x150" });\nconsole.log("Avatar URL:", thumbUrl);`,
          kotlin: `// Upload byte array\nmono.files.upload("avatars", "user_123.jpg", imageBytes)\n\n// Get thumbnail URL\nval thumbUrl = mono.files.getURL("avatars", "user_123.jpg", "150x150")`,
          dart: `await mono.files.upload('avatars', 'user_123.jpg', imageBytes);\nfinal url = mono.files.getURL('avatars', 'user_123.jpg', thumb: '150x150');`,
          python: `client.files.upload("avatars", "user_123.jpg", b"image bytes")\nurl = client.files.get_url("avatars", "user_123.jpg", thumb="150x150")`,
          rust: `client.files().upload("avatars", "user_123.jpg", bytes::Bytes::from("data")).await?;\nlet url = client.files().get_url_with_thumb("avatars", "user_123.jpg", "150x150");`,
          rest: `curl -s -X PUT http://127.0.0.1:8090/api/v1/apps/default/storage/buckets/avatars/objects/user_123.jpg \\\n  -H "authorization: Bearer $TOKEN" \\\n  --data-binary @photo.jpg`,
        },
      },
    ],
  },
  "sdks": {
    id: "sdks",
    title: "Client Libraries & SDK Reference",
    category: "Reference",
    subnav: "Reference",
    breadcrumbs: ["Monolith", "Documentation", "Reference", "SDKs"],
    overview:
      "Complete API reference for all officially supported Monolith client SDKs. Every SDK implements identical architectural layers: HTTP client with backoff retries, auth session store, typed collections, realtime streams, and RFC 9457 error decoding.",
    sections: [
      {
        id: "sdk-matrix",
        title: "SDK Compatibility & Features",
        content:
          "Below is the capability matrix for all client SDK libraries officially maintained by the Monolith project.",
        table: {
          headers: ["Language", "Registry", "Database CRUD", "Storage & Thumbs", "Realtime SSE", "Auth Observers"],
          rows: [
            ["TypeScript", "@monolith/client (npm)", "Yes", "Yes", "Yes", "Yes (onChange)"],
            ["Kotlin / Android", "dev.monolith (Maven)", "Yes", "Yes", "Yes", "Yes (Flow)"],
            ["Dart / Flutter", "monolith_client (pub.dev)", "Yes", "Yes", "Yes", "Yes (Stream)"],
            ["Python", "monolith-client (PyPI)", "Yes", "Yes", "Planned", "Yes"],
            ["Rust", "monolith-client (crates.io)", "Yes", "Yes", "Planned", "Yes"],
            ["Swift", "Monolith (SwiftPM)", "Phase 5", "Phase 5", "Phase 5", "Phase 5"],
          ],
        },
      },
    ],
  },
};
