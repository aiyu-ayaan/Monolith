import type { HttpClient } from "../http/client.js";

export interface AuthSession {
  token: string;
  user?: Record<string, unknown>;
}

export type AuthChangeListener = (token: string | null) => void;

export class AuthService {
  private listeners: Set<AuthChangeListener> = new Set();
  private authCollection: string;

  constructor(
    private http: HttpClient,
    collection: string = "users",
  ) {
    this.authCollection = collection;
  }

  get token(): string | null {
    return this.http.getToken();
  }

  setToken(token: string | null): void {
    this.http.setToken(token);
    this.notify(token);
  }

  onChange(listener: AuthChangeListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private notify(token: string | null): void {
    for (const listener of this.listeners) {
      try {
        listener(token);
      } catch {
        // ignore listener errors
      }
    }
  }

  async signInWithPassword(email: string, password: string): Promise<AuthSession> {
    const isSystem = this.authCollection === "_superusers" || this.authCollection === "system";
    const path = isSystem
      ? "/api/v1/system/login"
      : `/api/v1/apps/${encodeURIComponent(this.http.app)}/auth/login`;

    const res = await this.http.request<{ token: string; user?: Record<string, unknown> }>("POST", path, {
      body: { email, password },
    });

    if (res.token) {
      this.setToken(res.token);
    }

    return res;
  }

  async install(token: string, email: string, password: string): Promise<AuthSession> {
    const res = await this.http.request<{ token: string }>("POST", "/api/v1/system/install", {
      body: { token, email, password },
    });

    if (res.token) {
      this.setToken(res.token);
    }

    return res;
  }

  signOut(): void {
    this.setToken(null);
  }
}

