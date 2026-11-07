import type { HttpClient } from "../http/client.js";
import { AppAuth } from "./app.js";

export * from "./app.js";
export * from "./session.js";

export interface AuthSession {
  token: string;
  user?: Record<string, unknown>;
}

export type AuthChangeListener = (token: string | null) => void;

/**
 * Token-oriented auth service returned by `client.auth("users")` (kept for existing code and the superuser/admin flow).
 * App-user flows with state, refresh and storage live on `client.auth.*` (see `AppAuth`).
 */
export class AuthService {
  private authCollection: string;
  private appAuth: AppAuth;

  constructor(
    private http: HttpClient,
    collection: string = "users",
    appAuth?: AppAuth,
  ) {
    this.authCollection = collection;
    this.appAuth = appAuth ?? AppAuth.for(http);
  }

  private get isSystem(): boolean {
    return this.authCollection === "_superusers" || this.authCollection === "system";
  }

  get token(): string | null {
    return this.http.getToken();
  }

  setToken(token: string | null): void {
    this.appAuth.setToken(token);
  }

  /** Called with the access token (or `null`) on every change. For user objects use `client.auth.onChange`. */
  onChange(listener: AuthChangeListener): () => void {
    return this.appAuth.onTokenChange(listener);
  }

  async signInWithPassword(email: string, password: string): Promise<AuthSession> {
    if (!this.isSystem) {
      const user = await this.appAuth.signIn({ email, password });
      return { token: this.http.getToken() ?? "", user: user as unknown as Record<string, unknown> };
    }
    const res = await this.http.request<{ token: string; user?: Record<string, unknown> }>(
      "POST",
      "/api/v1/system/auth/login",
      { body: { email, password }, skipAuth: true },
    );
    if (res.token) this.setToken(res.token);
    return res;
  }

  async install(token: string, email: string, password: string): Promise<AuthSession> {
    const res = await this.http.request<{ token: string }>("POST", "/api/v1/system/install", {
      body: { token, email, password },
      skipAuth: true,
    });
    if (res.token) this.setToken(res.token);
    return res;
  }

  /** Clears the local session at once (synchronously), then logs out on the server when there is a refresh token. */
  signOut(): Promise<void> {
    return this.appAuth.signOut();
  }
}
