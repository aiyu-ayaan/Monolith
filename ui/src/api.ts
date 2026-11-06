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
  let res: Response;
  try {
    res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch (e) {
    throw new ApiError({ type: "network", title: "Network error", status: 0, detail: String(e) });
  }
  if (res.status === 204) return undefined as T;
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    if (res.status === 401 && t) setToken(null);
    throw new ApiError(
      data ?? { type: "http", title: res.statusText, status: res.status, requestId: res.headers.get("x-request-id") ?? undefined },
    );
  }
  return data as T;
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
