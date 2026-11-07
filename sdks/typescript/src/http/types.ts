export interface ClientOptions {
  url: string;
  app?: string;
  key?: string;
  token?: string;
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
  action: "create" | "update" | "delete";
  record: T;
  collection: string;
}

export interface RealtimeListener<T = RecordModel> {
  (event: RealtimeEvent<T>): void;
}

export interface RealtimeSubscriptionOptions {
  filter?: string;
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

