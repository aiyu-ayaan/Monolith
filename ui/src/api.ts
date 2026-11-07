// Thin fetch client for the Phase 1 API (api/openapi/monolith.v1.yaml).
// shortcut: hand-written; switch to the generated @monolith/client once the SDK lands.

import { createSignal } from "solid-js";

const TOKEN_KEY = "monolith.token";

function readToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

const [token, setTokenSignal] = createSignal<string | null>(readToken());
export { token };

export function setToken(t: string | null) {
  try {
    if (t) localStorage.setItem(TOKEN_KEY, t);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    // storage blocked: the session lasts until reload
  }
  setTokenSignal(t);
}

export interface FieldProblem {
  field: string;
  code: string;
  message: string;
}

export interface Problem {
  type: string;
  title: string;
  status: number;
  detail?: string;
  requestId?: string;
  errors?: FieldProblem[];
}

export class ApiError extends Error {
  constructor(public problem: Problem) {
    super(problem.detail ?? problem.title);
  }
}

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  const t = token();
  if (t) headers.authorization = `Bearer ${t}`;
  if (body !== undefined) headers["content-type"] = "application/json";
  for (let attempt = 0; ; attempt++) {
    let res: Response;
    try {
      res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    } catch (e) {
      if (method === "GET" && attempt < 6) {
        await new Promise((r) => setTimeout(r, 400));
        continue;
      }
      throw new ApiError({ type: "network", title: "Network error", status: 0, detail: String(e) });
    }
    if (res.status === 204) return undefined as T;
    const data = await res.json().catch(() => null);
    if (!res.ok) {
      // Vite's dev proxy returns 500/502 with a plain-text body while cargo is still compiling the server.
      if (method === "GET" && data === null && res.status >= 500 && attempt < 6) {
        await new Promise((r) => setTimeout(r, 400));
        continue;
      }
      if (res.status === 401 && t) setToken(null);
      throw new ApiError(
        data ?? {
          type: "http",
          title: res.statusText || `HTTP ${res.status}`,
          status: res.status,
          detail: data === null && res.status >= 500 ? "Backend server is not reachable (is `pnpm dev` running?)" : undefined,
          requestId: res.headers.get("x-request-id") ?? undefined,
        },
      );
    }
    return data as T;
  }
}

/** Reads a Solid resource without throwing when `r.error` is set. */
export function ready<T>(r: { error: unknown; latest?: T }): T | undefined {
  return r.error ? undefined : r.latest;
}

export const enc = encodeURIComponent;

export type FieldType = "text" | "email" | "url" | "number" | "bool" | "date" | "select" | "relation" | "json";
export const FIELD_TYPES: FieldType[] = ["text", "email", "url", "number", "bool", "date", "select", "relation", "json"];

export interface Field {
  name: string;
  type: FieldType;
  required?: boolean;
  hidden?: boolean;
  min?: number;
  max?: number;
  onlyInt?: boolean;
  values?: string[];
  collection?: string;
  onDelete?: "restrict" | "cascade" | "setNull";
  maxSelect?: number;
}

export type RuleKey = "list" | "view" | "create" | "update" | "delete";
export const RULE_KEYS: RuleKey[] = ["list", "view", "create", "update", "delete"];

export interface Collection {
  name: string;
  type?: "base";
  fields: Field[];
  rules: Partial<Record<RuleKey, string | null>>;
  indexes: { fields: string[]; unique?: boolean }[];
  version?: number;
}

export interface AppInfo {
  name: string;
  created: string;
}

export type RecordRow = { id: string; created: string; updated: string; _v: number } & Record<string, unknown>;

// ---------- stats (GET /api/v1/system/stats, GET /api/v1/apps/{app}/stats) ----------

export interface Traffic {
  requests: number;
  reads: number;
  writes: number;
  creates: number;
  updates: number;
  deletes: number;
  errors4xx: number;
  errors5xx: number;
  bytesIn: number;
  bytesOut: number;
  avgUs: number;
  maxUs: number;
  p50Us: number;
  p95Us: number;
  p99Us: number;
}

export type TrafficPoint = Traffic & { t: number };

export interface TrafficSeries {
  total: Traffic;
  lastHour: Traffic;
  minutes: TrafficPoint[];
  hours: TrafficPoint[];
}

export interface SystemStats {
  since: string;
  uptimeS: number;
  memoryBytes: number | null;
  appsTotal: number;
  appsLoaded: number;
  traffic: TrafficSeries;
  apps: { name: string; loaded: boolean; dbBytes: number; walBytes: number; traffic: Traffic | null }[];
}

export interface StoreStats {
  commits: number;
  ops: number;
  opsFailed: number;
  mutations: number;
  maxBatch: number;
  commitAvgUs: number;
  commitMaxUs: number;
  reads: number;
  readAvgUs: number;
  busy: number;
  writerQueue: number;
  readerQueue: number;
  readers: number;
}

export interface AppStats {
  app: string;
  since: string;
  schemaVersion: number;
  traffic: TrafficSeries;
  store: StoreStats;
  dbBytes: number;
  walBytes: number;
  collections: { name: string; records: number | null; reads: number; creates: number; updates: number; deletes: number }[];
}

export interface RequestLog {
  requestId: string;
  timestamp: string;
  method: string;
  path: string;
  status: number;
  durationUs: number;
  superuser: boolean;
  error?: string | null;
}

// ---------- per-app auth config (GET/PUT /api/v1/apps/{app}/auth/config) ----------

export interface EmailPasswordConfig {
  enabled: boolean;
  allowSignup: boolean;
  minPasswordLength: number;
}

export interface AuthProviderDef {
  id: string;
  name: string;
  fields: string[];
  required: string[];
}

export interface AuthProvider {
  id: string;
  name: string;
  enabled: boolean;
  clientId: string;
  clientSecretSet: boolean;
  extra: Record<string, string>;
  callbackPath: string;
}

export interface AuthConfig {
  emailPassword: EmailPasswordConfig;
  providers: AuthProvider[];
  catalog: AuthProviderDef[];
}

export interface AuthProviderInput {
  id: string;
  enabled: boolean;
  clientId: string;
  /** Omit to keep the stored secret. */
  clientSecret?: string;
  extra: Record<string, string>;
}
