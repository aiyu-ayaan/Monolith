export interface ClientOptions {
  url: string;
  app?: string;
  key?: string;
  token?: string;
  /** Session storage for app users. Default: memory (use `Monolith.init` for `localStorage`). */
  storage?: import("../auth/session.js").AuthStorage;
  /** Default redirect target for provider sign-in (default: the current page without its hash). */
  redirectUri?: string;
  fetch?: typeof fetch;
  headers?: Record<string, string>;
}

export interface ListOptions {
  page?: number;
  perPage?: number;
  cursor?: string;
  limit?: number;
  filter?: string;
  sort?: string;
  expand?: string;
  fields?: string;
  total?: "none" | "exact" | "estimate";
}

export interface ListResult<T> {
  items: T[];
  total?: number;
  cursor?: string;
  nextCursor?: string;
  prevCursor?: string;
  [key: string]: unknown;
}

export interface CountOptions {
  filter?: string;
}

export interface CountResult {
  count: number;
}

export type RecordModel = {
  id: string;
  created: string;
  updated: string;
  _v: number;
} & Record<string, unknown>;

export interface BatchOperation {
  method: "POST" | "PATCH" | "PUT" | "DELETE";
  path: string;
  body?: unknown;
}

export interface BatchResponseItem {
  status: number;
  body?: unknown;
}

export interface RealtimeEvent<T = RecordModel> {
  /** `leave`: the record stopped matching your filter or rule (it was not deleted). */
  action: "create" | "update" | "delete" | "leave";
  record: T;
  collection: string;
  /** Change-log sequence number; resume point after a reconnect. */
  seq?: number;
}

export interface RealtimeListener<T = RecordModel> {
  (event: RealtimeEvent<T>): void;
}

export interface RealtimeSubscriptionOptions {
  /** Filter expression (same syntax as `list({ filter })`); only matching records arrive. */
  filter?: string;
  /** Only these record ids. */
  ids?: string[];
}

export interface DocumentChange<T = RecordModel> {
  type: "added" | "modified" | "removed";
  doc: T;
}

export interface QuerySnapshot<T = RecordModel> {
  /** The current result set, in order. */
  docs: T[];
  /** What changed since the previous snapshot (everything is `added` in the first one). */
  changes: DocumentChange<T>[];
}

export interface SnapshotOptions {
  filter?: string;
  /** Sort expression, e.g. `-created`. Also applied client-side to live changes. */
  sort?: string;
  /** Records in the initial result (server cap 1000). Default 100. */
  limit?: number;
  onError?: (error: Error) => void;
}

export interface StorageObject {
  bucket: string;
  key: string;
  size: number;
  contentType: string;
  blake3: string;
  created: string;
  updated: string;
}

export interface StorageListOptions {
  prefix?: string;
  cursor?: string;
  limit?: number;
}

export interface StorageListResult {
  items: StorageObject[];
  nextCursor?: string;
}

