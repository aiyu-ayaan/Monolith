import { MonolithError, Problem } from "../errors/index.js";
import type { ClientOptions } from "./types.js";

/** Lets the auth layer refresh tokens inside the HTTP stack. */
export interface AuthHandler {
  /** The access token to send; refreshes first when it is about to expire. */
  accessToken(): Promise<string | null>;
  /** A request sent with `usedToken` got 401. True when a single retry makes sense. */
  onUnauthorized(usedToken: string): Promise<boolean>;
}

export interface RequestOptions {
  /** Send no credentials and never refresh (public auth endpoints). */
  skipAuth?: boolean;
  query?: Record<string, unknown>;
  body?: unknown;
  headers?: Record<string, string>;
  signal?: AbortSignal;
}

export class HttpClient {
  readonly baseUrl: string;
  readonly app: string;
  private token: string | null = null;
  private apiKey: string | null = null;
  private customFetch: typeof fetch;
  private defaultHeaders: Record<string, string>;
  private authHandler: AuthHandler | null = null;

  constructor(options: ClientOptions) {
    this.baseUrl = options.url.replace(/\/+$/, "");
    this.app = options.app ?? "default";
    this.token = options.token ?? null;
    this.apiKey = options.key ?? null;
    this.customFetch = options.fetch ?? (globalThis.fetch ? globalThis.fetch.bind(globalThis) : (fetch as typeof globalThis.fetch));
    this.defaultHeaders = options.headers ?? {};
  }

  setToken(token: string | null): void {
    this.token = token;
  }

  getToken(): string | null {
    return this.token;
  }

  setAuthHandler(handler: AuthHandler | null): void {
    this.authHandler = handler;
  }

  /** The current access token, refreshed first when needed. */
  async freshToken(): Promise<string | null> {
    return this.authHandler ? this.authHandler.accessToken() : this.token;
  }

  setApiKey(key: string | null): void {
    this.apiKey = key;
  }

  getApiKey(): string | null {
    return this.apiKey;
  }

  buildUrl(path: string, query?: Record<string, unknown>): string {
    const cleanPath = path.startsWith("/") ? path : `/${path}`;
    const url = new URL(`${this.baseUrl}${cleanPath}`);

    if (query) {
      for (const [k, v] of Object.entries(query)) {
        if (v === undefined || v === null) continue;
        if (Array.isArray(v)) {
          for (const item of v) {
            url.searchParams.append(k, String(item));
          }
        } else {
          url.searchParams.set(k, String(v));
        }
      }
    }

    return url.toString();
  }

  async request<T>(method: string, path: string, options: RequestOptions = {}): Promise<T> {
    const url = this.buildUrl(path, options.query);
    const headers: Record<string, string> = {
      ...this.defaultHeaders,
      ...options.headers,
    };

    let bodyData: BodyInit | undefined = undefined;
    if (options.body !== undefined) {
      if (
        typeof options.body === "string" ||
        options.body instanceof Blob ||
        options.body instanceof ArrayBuffer ||
        options.body instanceof FormData ||
        options.body instanceof URLSearchParams
      ) {
        bodyData = options.body as BodyInit;
      } else {
        headers["content-type"] = "application/json";
        bodyData = JSON.stringify(options.body);
      }
    }

    const explicitAuth = Boolean(headers.authorization || headers.Authorization);
    const managed = !options.skipAuth && !explicitAuth && this.authHandler !== null;
    const sessionToken = async (): Promise<string | null> => {
      if (options.skipAuth || explicitAuth) return null;
      return managed ? this.authHandler!.accessToken() : this.token;
    };

    const used = await sessionToken();
    const firstAuth = options.skipAuth || explicitAuth ? null : used || this.apiKey;
    try {
      return await this.send<T>(method, url, headers, bodyData, options.signal, firstAuth);
    } catch (err) {
      if (managed && used && err instanceof MonolithError && err.status === 401) {
        // Refresh once (single-flight in the handler), then retry once.
        if (await this.authHandler!.onUnauthorized(used)) {
          const next = await sessionToken();
          return this.send<T>(method, url, headers, bodyData, options.signal, next || this.apiKey);
        }
      }
      throw err;
    }
  }

  private async send<T>(
    method: string,
    url: string,
    baseHeaders: Record<string, string>,
    bodyData: BodyInit | undefined,
    signal: AbortSignal | undefined,
    authToken: string | null,
  ): Promise<T> {
    const headers = { ...baseHeaders };
    if (authToken) headers.authorization = `Bearer ${authToken}`;
    const isGet = method.toUpperCase() === "GET";
    const maxAttempts = isGet ? 4 : 1;

    for (let attempt = 0; attempt < maxAttempts; attempt++) {
      let res: Response;
      try {
        res = await this.customFetch(url, {
          method,
          headers,
          body: bodyData,
          signal,
        });
      } catch (err: unknown) {
        if (attempt < maxAttempts - 1) {
          const backoff = Math.min(100 * Math.pow(2, attempt) + Math.random() * 50, 2000);
          await new Promise((r) => setTimeout(r, backoff));
          continue;
        }
        throw new MonolithError({
          type: "network_error",
          title: "Network Error",
          status: 0,
          detail: err instanceof Error ? err.message : String(err),
        });
      }

      if (res.status === 204) {
        return undefined as T;
      }

      const contentType = res.headers.get("content-type") ?? "";
      const isJson = contentType.includes("application/json") || contentType.includes("problem+json");

      if (!res.ok) {
        // Retry 503 Service Unavailable for idempotent GET
        if (isGet && res.status === 503 && attempt < maxAttempts - 1) {
          const retryAfter = res.headers.get("retry-after");
          const delay = retryAfter ? parseInt(retryAfter, 10) * 1000 : 100 * Math.pow(2, attempt);
          await new Promise((r) => setTimeout(r, delay));
          continue;
        }

        let problem: Problem;
        if (isJson) {
          try {
            problem = (await res.json()) as Problem;
          } catch {
            problem = {
              type: "http_error",
              title: res.statusText || `HTTP ${res.status}`,
              status: res.status,
            };
          }
        } else {
          const text = await res.text().catch(() => "");
          problem = {
            type: "http_error",
            title: res.statusText || `HTTP ${res.status}`,
            status: res.status,
            detail: text || undefined,
          };
        }

        if (!problem.requestId) {
          problem.requestId = res.headers.get("x-request-id") ?? undefined;
        }
        if (!problem.status) {
          problem.status = res.status;
        }

        throw new MonolithError(problem);
      }

      if (isJson) {
        return (await res.json()) as T;
      }

      return (await res.text()) as unknown as T;
    }

    throw new MonolithError({
      type: "request_failed",
      title: "Request failed after max attempts",
      status: 0,
    });
  }
}

