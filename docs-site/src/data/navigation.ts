import { SupportedLanguage } from "../components/CodeBlock";

export interface NavItem {
  id: string;
  title: string;
  sectionId: string;
  badge?: string;
  lang?: SupportedLanguage;
  children?: NavItem[];
}

export interface NavSection {
  title: string;
  category: string;
  items: NavItem[];
}

export const SIDEBAR_NAVIGATION: NavSection[] = [
  {
    title: "Overview & Fundamentals",
    category: "Overview",
    items: [
      { id: "nav-overview", title: "Introduction & Capabilities", sectionId: "overview" },
      { id: "nav-arch", title: "Architecture & Group Commit", sectionId: "architecture" },
      { id: "nav-quickstart", title: "Quickstart & Initialization", sectionId: "quickstart" },
      { id: "nav-datamodel", title: "Data Model, UUIDv7 & Schema", sectionId: "data-model" },
    ],
  },
  {
    title: "Cloud Database",
    category: "Database",
    items: [
      {
        id: "nav-db-group",
        title: "Collections & Records",
        sectionId: "crud-records",
        children: [
          { id: "nav-crud", title: "CRUD & Optimistic Concurrency", sectionId: "crud-records" },
          { id: "nav-filter", title: "Filtering & Keyset Pagination", sectionId: "keyset-filtering" },
          { id: "nav-relations", title: "Relations & Multi-Hop Expand", sectionId: "relations-expand" },
          { id: "nav-batch", title: "Atomic Batch Transactions", sectionId: "batch-transactions" },
        ],
      },
      {
        id: "nav-realtime",
        title: "Realtime Synchronization (SSE)",
        sectionId: "realtime-streaming",
        badge: "SSE",
      },
    ],
  },
  {
    title: "Authentication & Security",
    category: "Auth & Rules",
    items: [
      { id: "nav-rules", title: "Collection Access Rules", sectionId: "access-rules" },
      { id: "nav-signin", title: "Password Login & Token Rotation", sectionId: "auth-signin" },
      { id: "nav-oauth", title: "OAuth2 / OIDC & Auth Policy", sectionId: "auth-oauth" },
      { id: "nav-apikeys", title: "Scoped Runtime API Keys", sectionId: "auth-keys" },
      { id: "nav-install", title: "First-Superuser Bootstrap", sectionId: "auth-install" },
    ],
  },
  {
    title: "Storage, Backups & Ops",
    category: "Storage & Ops",
    items: [
      { id: "nav-storage", title: "Buckets & BLAKE3 Deduplication", sectionId: "storage-buckets" },
      { id: "nav-thumbs", title: "On-Demand Image Thumbnails", sectionId: "storage-thumbs" },
      { id: "nav-backups", title: "Online Backups, Restore & Metrics", sectionId: "backups-ops" },
    ],
  },
  {
    title: "API & SDK Reference",
    category: "SDK Reference",
    items: [
      { id: "nav-sdk-matrix", title: "SDK Compatibility Matrix", sectionId: "sdk-matrix" },
      { id: "nav-sdk-ts", title: "TypeScript / Web & Node.js", sectionId: "sdk-ts", badge: "v0", lang: "ts" },
      { id: "nav-sdk-kotlin", title: "Android (Kotlin & KMP)", sectionId: "sdk-kotlin", badge: "v0", lang: "kotlin" },
      { id: "nav-sdk-dart", title: "Flutter (Dart)", sectionId: "sdk-dart", badge: "v0", lang: "dart" },
      { id: "nav-sdk-python", title: "Python", sectionId: "sdk-python", badge: "v0", lang: "python" },
      { id: "nav-sdk-rust", title: "Rust (Crates.io)", sectionId: "sdk-rust", badge: "v0", lang: "rust" },
      { id: "nav-sdk-swift", title: "Swift (iOS & macOS)", sectionId: "sdk-swift", badge: "Phase 5", lang: "swift" },
      { id: "nav-sdk-rest", title: "REST API & RFC 9457 Errors", sectionId: "sdk-rest", badge: "v1", lang: "rest" },
    ],
  },
];
