export interface NavItem {
  id: string;
  title: string;
  pageId?: string;
  sectionId?: string;
  badge?: string;
  children?: NavItem[];
}

export interface NavSection {
  title: string;
  items: NavItem[];
}

export const SIDEBAR_NAVIGATION: NavSection[] = [
  {
    title: "Overview",
    items: [
      { id: "intro", title: "Introduction & Quickstart", pageId: "firestore", sectionId: "quickstart-init" },
      { id: "arch", title: "Architecture & Group Commit", pageId: "firestore", sectionId: "quickstart-init" },
    ],
  },
  {
    title: "Cloud Database",
    items: [
      {
        id: "db-root",
        title: "Collections & Data",
        pageId: "firestore",
        children: [
          { id: "db-crud", title: "CRUD Operations", pageId: "firestore", sectionId: "crud-records" },
          { id: "db-filter", title: "Filtering & Keyset Pagination", pageId: "firestore", sectionId: "keyset-filtering" },
          { id: "db-batch", title: "Atomic Batch Operations", pageId: "firestore", sectionId: "batch-transactions" },
        ],
      },
      {
        id: "realtime-root",
        title: "Realtime Synchronization",
        pageId: "firestore",
        sectionId: "realtime-streaming",
      },
    ],
  },
  {
    title: "Authentication",
    items: [
      { id: "auth-signin", title: "Password Login & Tokens", pageId: "auth", sectionId: "auth-signin" },
      { id: "auth-keys", title: "Scoped Runtime API Keys", pageId: "auth", sectionId: "auth-signin" },
      { id: "auth-install", title: "First-Superuser Bootstrap", pageId: "auth", sectionId: "auth-signin" },
    ],
  },
  {
    title: "Storage & Files",
    items: [
      { id: "storage-files", title: "Buckets & File Uploads", pageId: "storage", sectionId: "storage-upload-url" },
      { id: "storage-thumbs", title: "On-Demand Thumbnails", pageId: "storage", sectionId: "storage-upload-url" },
    ],
  },
  {
    title: "API & SDK Reference",
    items: [
      { id: "ref-ts", title: "TypeScript / Web", pageId: "sdks", sectionId: "sdk-matrix", badge: "v0" },
      { id: "ref-kotlin", title: "Android (Kotlin)", pageId: "sdks", sectionId: "sdk-matrix", badge: "v0" },
      { id: "ref-dart", title: "Flutter (Dart)", pageId: "sdks", sectionId: "sdk-matrix", badge: "v0" },
      { id: "ref-python", title: "Python", pageId: "sdks", sectionId: "sdk-matrix", badge: "v0" },
      { id: "ref-rust", title: "Rust (Crates.io)", pageId: "sdks", sectionId: "sdk-matrix", badge: "v0" },
      { id: "ref-swift", title: "Swift", pageId: "sdks", sectionId: "sdk-matrix", badge: "Phase 5" },
      { id: "ref-rest", title: "REST & RFC 9457", pageId: "sdks", sectionId: "sdk-matrix" },
    ],
  },
];
