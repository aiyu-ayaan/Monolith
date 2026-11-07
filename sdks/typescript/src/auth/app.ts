import { MonolithError } from "../errors/index.js";
import type { HttpClient } from "../http/client.js";
import { type AuthStorage, memoryStorage } from "./session.js";

export interface AppUser {
  id: string;
  email: string | null;
  name: string;
  created?: string;
  disabled?: boolean;
  lastLogin?: string | null;
  /** `password` and linked provider ids. */
  providers: string[];
}

export interface PublicProvider {
  id: string;
  name: string;
  startPath: string;
}

export interface PublicAuth {
  app: string;
  emailPassword: { enabled: boolean; allowSignup: boolean; minPasswordLength: number };
  providers: PublicProvider[];
}

export type AuthState =
  | { status: "unknown"; user: null }
  | { status: "signedOut"; user: null }
  | { status: "signedIn"; user: AppUser };

export type AuthStateListener = (state: AuthState) => void;

export interface AuthRedirectError {
  /** `oauth_failed` from the server, or `session_failed` when the token could not be used. */
  code: string;
  description: string;
}

export type RedirectResult = { user: AppUser } | { error: AuthRedirectError } | { popup: true };

export interface ProviderSignInOptions {
  /** Where the server sends the browser back. Default: the init option, else the current page without its hash. */
  redirectUri?: string;
  /** `redirect` (default) navigates away; `popup` opens a window and resolves with the user. */
  mode?: "redirect" | "popup";
}

export interface AppAuthOptions {
  storage?: AuthStorage;
  redirectUri?: string;
  /** Refresh this long before the access token expires. Default 60 000 ms. */
  refreshSkewMs?: number;
  now?: () => number;
}

interface ServerSession {
  token: string;
  refreshToken?: string;
  expiresAt?: string | number;
  user: AppUser;
}

interface Stored {
  token: string;
  refreshToken?: string;
  /** ms since epoch */
  expiresAt?: number;
  user: AppUser | null;
}

const POPUP_NAME = "monolith-auth-popup";
const MESSAGE_TYPE = "monolith:auth";

function parseExpiry(v: string | number | null | undefined): number | undefined {
  if (v === null || v === undefined || v === "") return undefined;
  const n = typeof v === "number" ? v : /^\d+$/.test(v) ? Number(v) : Date.parse(v);
  return Number.isFinite(n) ? n : undefined;
}

export class AppAuth {
  private static instances = new WeakMap<HttpClient, AppAuth>();

  /** One auth manager per HTTP client. */
  static for(http: HttpClient, options?: AppAuthOptions): AppAuth {
    let a = AppAuth.instances.get(http);
    if (!a) {
      a = new AppAuth(http, options);
      AppAuth.instances.set(http, a);
    }
    return a;
  }

  private _state: AuthState = { status: "signedOut", user: null };
  private session: Stored | null = null;
  private refreshing: Promise<void> | null = null;
  private listeners = new Set<AuthStateListener>();
  private tokenListeners = new Set<(token: string | null) => void>();
  private storage: AuthStorage;
  private key: string;
  private started = false;
  private _ready: Promise<void> = Promise.resolve();
  private redirectError: AuthRedirectError | null = null;
  private skew: number;
  private now: () => number;

  constructor(
    private http: HttpClient,
    private options: AppAuthOptions = {},
  ) {
    this.storage = options.storage ?? memoryStorage();
    this.key = `monolith:session:${http.baseUrl}:${http.app}`;
    this.skew = options.refreshSkewMs ?? 60_000;
    this.now = options.now ?? Date.now;
    http.setAuthHandler({
      accessToken: () => this.accessToken(),
      onUnauthorized: (t) => this.onUnauthorized(t),
    });
  }

  // ---- state -------------------------------------------------------------

  get state(): AuthState {
    return this._state;
  }

  get user(): AppUser | null {
    return this._state.user;
  }

  /** Resolves when the stored session was restored and a redirect fragment was handled. */
  get ready(): Promise<void> {
    return this._ready;
  }

  onChange(listener: AuthStateListener, opts: { immediate?: boolean } = {}): () => void {
    this.listeners.add(listener);
    if (opts.immediate) listener(this._state);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /** Legacy: token string listener (used by `client.auth("users").onChange`). */
  onTokenChange(listener: (token: string | null) => void): () => void {
    this.tokenListeners.add(listener);
    return () => {
      this.tokenListeners.delete(listener);
    };
  }

  /** The error of a failed provider sign-in found by `handleRedirect`, once. */
  takeRedirectError(): AuthRedirectError | null {
    const e = this.redirectError;
    this.redirectError = null;
    return e;
  }

  private setState(next: AuthState): void {
    const cur = this._state;
    if (cur.status === next.status && cur.user === next.user) return;
    this._state = next;
    for (const l of [...this.listeners]) {
      try {
        l(next);
      } catch {
        // listener errors are the listener's problem
      }
    }
  }

  private notifyToken(token: string | null): void {
    for (const l of [...this.tokenListeners]) {
      try {
        l(token);
      } catch {
        // ignore
      }
    }
  }

  // ---- lifecycle ---------------------------------------------------------

  /** Restores the stored session and handles a provider redirect in the URL. Called by `Monolith.init`. */
  start(): Promise<void> {
    if (this.started) return this._ready;
    this.started = true;
    this.setState({ status: "unknown", user: null });
    this._ready = (async () => {
      let r: RedirectResult | null = null;
      try {
        r = await this.handleRedirect();
      } catch {
        // fall through to restore
      }
      if (r && "user" in r) return;
      if (r && "popup" in r) {
        this.setState({ status: "signedOut", user: null });
        return;
      }
      await this.restore();
    })();
    return this._ready;
  }

  private async readStored(): Promise<(Stored & { user: AppUser }) | null> {
    try {
      const raw = await this.storage.getItem(this.key);
      const s = raw ? (JSON.parse(raw) as Stored) : null;
      return s && typeof s.token === "string" && s.user ? (s as Stored & { user: AppUser }) : null;
    } catch {
      return null;
    }
  }

  private async restore(): Promise<void> {
    const s = await this.readStored();
    if (!s) {
      if (this._state.status === "unknown") this.setState({ status: "signedOut", user: null });
      return;
    }
    this.session = s;
    this.http.setToken(s.token);
    this.setState({ status: "signedIn", user: s.user });
    this.notifyToken(s.token);
    if (s.refreshToken && this.expiring(s)) void this.refresh().catch(() => {});
  }

  private persist(): void {
    const s = this.session;
    if (!s || !s.user) return;
    try {
      void Promise.resolve(this.storage.setItem(this.key, JSON.stringify(s))).catch(() => {});
    } catch {
      // storage unavailable
    }
  }

  private clearLocal(): void {
    this.session = null;
    this.http.setToken(null);
    try {
      void Promise.resolve(this.storage.removeItem(this.key)).catch(() => {});
    } catch {
      // storage unavailable
    }
    this.setState({ status: "signedOut", user: null });
    this.notifyToken(null);
  }

  private applySession(s: Stored, persist = true): void {
    this.session = s;
    this.http.setToken(s.token);
    if (persist) this.persist();
    const u = s.user;
    if (u) this.setState({ status: "signedIn", user: u });
    else if (this._state.status === "unknown") this.setState({ status: "signedOut", user: null });
    this.notifyToken(s.token);
  }

  private fromServer(r: ServerSession): AppUser {
    this.applySession({
      token: r.token,
      refreshToken: r.refreshToken,
      expiresAt: parseExpiry(r.expiresAt),
      user: r.user ?? null,
    });
    return r.user;
  }

  /** Legacy escape hatch: sets a bare token (no refresh, no stored session). */
  setToken(token: string | null): void {
    if (token === null) this.clearLocal();
    else this.applySession({ token, user: null }, false);
  }

  // ---- tokens ------------------------------------------------------------

  private expiring(s: Stored): boolean {
    return s.expiresAt !== undefined && s.expiresAt - this.now() < this.skew;
  }

  /** The access token, refreshed first when it is about to expire. */
  async accessToken(): Promise<string | null> {
    const s = this.session;
    if (!s) return this.http.getToken();
    if (s.refreshToken && this.expiring(s)) {
      try {
        await this.refresh();
      } catch {
        // 401 cleared the session; a network error keeps the old token for the caller to try
      }
    }
    return this.session?.token ?? null;
  }

  /** Single-flight refresh: concurrent callers share one request. */
  refresh(): Promise<void> {
    if (!this.refreshing) {
      this.refreshing = this.doRefresh().finally(() => {
        this.refreshing = null;
      });
    }
    return this.refreshing;
  }

  private async doRefresh(): Promise<void> {
    const cur = this.session;
    if (!cur?.refreshToken) {
      throw new MonolithError({ type: "no_refresh_token", title: "No refresh token", status: 401 });
    }
    try {
      const r = await this.http.request<ServerSession>("POST", this.path("refresh"), {
        body: { refreshToken: cur.refreshToken },
        skipAuth: true,
      });
      this.applySession({
        token: r.token,
        refreshToken: r.refreshToken ?? cur.refreshToken,
        expiresAt: parseExpiry(r.expiresAt),
        user: r.user ?? cur.user,
      });
    } catch (e) {
      if (e instanceof MonolithError && e.status === 401) {
        // Another tab may have rotated the refresh token already: adopt its session.
        const stored = await this.readStored();
        if (stored && stored.refreshToken && stored.refreshToken !== cur.refreshToken) {
          this.applySession(stored, false);
          return;
        }
        this.clearLocal();
      }
      throw e;
    }
  }

  private async onUnauthorized(used: string): Promise<boolean> {
    const s = this.session;
    if (!s) return false;
    if (s.token !== used) return true; // somebody refreshed meanwhile
    if (!s.refreshToken) return false;
    try {
      await this.refresh();
      return true;
    } catch {
      return false;
    }
  }

  // ---- API ---------------------------------------------------------------

  private path(rest: string): string {
    return `/api/v1/apps/${encodeURIComponent(this.http.app)}/auth/${rest}`;
  }

  getProviders(): Promise<PublicAuth> {
    return this.http.request<PublicAuth>("GET", this.path("providers"), { skipAuth: true });
  }

  async signUp(input: { email: string; password: string; name?: string }): Promise<AppUser> {
    const r = await this.http.request<ServerSession>("POST", this.path("register"), { body: input, skipAuth: true });
    return this.fromServer(r);
  }

  async signIn(input: { email: string; password: string }): Promise<AppUser> {
    const r = await this.http.request<ServerSession>("POST", this.path("login"), { body: input, skipAuth: true });
    return this.fromServer(r);
  }

  /** Redirect mode navigates away and resolves `null`; popup mode resolves with the user. */
  async signInWithProvider(id: string, opts: ProviderSignInOptions = {}): Promise<AppUser | null> {
    const loc = (globalThis as { location?: Location }).location;
    const redirectUri =
      opts.redirectUri ?? this.options.redirectUri ?? (loc ? `${loc.origin}${loc.pathname}${loc.search}` : undefined);
    if (!redirectUri) {
      throw new MonolithError({
        type: "redirect_required",
        title: "redirectUri is required outside a browser",
        status: 400,
      });
    }
    const url = this.http.buildUrl(this.path(`oauth/${encodeURIComponent(id)}/start`), { redirectUri });
    if (opts.mode === "popup") return this.popupSignIn(url);
    if (!loc) {
      throw new MonolithError({ type: "no_browser", title: "Provider sign-in needs a browser", status: 0 });
    }
    loc.assign(url);
    return null;
  }

  private popupSignIn(url: string): Promise<AppUser> {
    const win = globalThis as unknown as Window;
    return new Promise<AppUser>((resolve, reject) => {
      const w = win.open(url, POPUP_NAME, "width=480,height=680");
      if (!w) {
        reject(new MonolithError({ type: "popup_blocked", title: "The sign-in popup was blocked", status: 0 }));
        return;
      }
      const done = (): void => {
        win.removeEventListener("message", onMessage);
        clearInterval(timer);
      };
      const onMessage = (ev: MessageEvent): void => {
        const d = ev.data as Record<string, string> | null;
        if (ev.origin !== win.location.origin || !d || d.type !== MESSAGE_TYPE) return;
        done();
        if (d.error) {
          reject(
            new MonolithError({ type: d.error, title: "Sign-in failed", status: 0, detail: d.error_description }),
          );
          return;
        }
        this.finishFromFragment(d.token, d.refreshToken, d.expiresAt).then(
          (r) => {
            if ("user" in r) resolve(r.user);
            else if ("error" in r) {
              reject(new MonolithError({ type: r.error.code, title: "Sign-in failed", status: 0, detail: r.error.description }));
            }
          },
          reject,
        );
      };
      const timer = setInterval(() => {
        if (w.closed) {
          done();
          reject(new MonolithError({ type: "popup_closed", title: "The sign-in window was closed", status: 0 }));
        }
      }, 500);
      win.addEventListener("message", onMessage);
    });
  }

  /**
   * Finishes a provider sign-in from the URL fragment (`#token=&refreshToken=&expiresAt=` or `#error=`).
   * Without argument it reads `location.hash` and removes the fragment. Returns `null` when there is nothing to do.
   */
  async handleRedirect(input?: string): Promise<RedirectResult | null> {
    const fromLocation = input === undefined;
    const loc = (globalThis as { location?: Location }).location;
    const raw = fromLocation ? (loc?.hash ?? "") : input.includes("#") ? input.slice(input.indexOf("#")) : input;
    const p = new URLSearchParams(raw.replace(/^#/, ""));
    const token = p.get("token");
    const error = p.get("error");
    const isSession = Boolean(token && (p.get("refreshToken") || p.get("expiresAt")));
    const isError = Boolean(error && (error === "oauth_failed" || p.get("error_description")));
    if (!isSession && !isError) return null;
    if (fromLocation) this.stripFragment();

    const popup = this.isPopupChild();
    if (popup) {
      const w = globalThis as unknown as Window;
      const msg: Record<string, string> = { type: MESSAGE_TYPE };
      for (const k of ["token", "refreshToken", "expiresAt", "error", "error_description"]) {
        const v = p.get(k);
        if (v) msg[k] = v;
      }
      w.opener?.postMessage(msg, w.location.origin);
      w.close();
      return { popup: true };
    }
    if (isError) {
      const e = { code: error as string, description: p.get("error_description") ?? "" };
      this.redirectError = e;
      return { error: e };
    }
    return this.finishFromFragment(token as string, p.get("refreshToken") ?? undefined, p.get("expiresAt") ?? undefined);
  }

  private async finishFromFragment(token: string, refreshToken?: string, expiresAt?: string): Promise<RedirectResult> {
    try {
      const user = await this.http.request<AppUser>("GET", this.path("me"), {
        headers: { authorization: `Bearer ${token}` },
      });
      this.applySession({ token, refreshToken, expiresAt: parseExpiry(expiresAt), user });
      return { user };
    } catch (e) {
      const err = { code: "session_failed", description: e instanceof Error ? e.message : String(e) };
      this.redirectError = err;
      return { error: err };
    }
  }

  private isPopupChild(): boolean {
    const w = (globalThis as { window?: Window }).window;
    return Boolean(w && w.opener && w.name === POPUP_NAME);
  }

  private stripFragment(): void {
    const g = globalThis as { location?: Location; history?: History };
    try {
      if (g.location && g.history) {
        g.history.replaceState(g.history.state, "", g.location.pathname + g.location.search);
      }
    } catch {
      // sandboxed frame
    }
  }

  /** Clears the local session at once, then ends it on the server (logout with the refresh token). */
  async signOut(): Promise<void> {
    const refreshToken = this.session?.refreshToken;
    this.clearLocal();
    if (!refreshToken) return;
    try {
      await this.http.request<void>("POST", this.path("logout"), { body: { refreshToken }, skipAuth: true });
    } catch {
      // already signed out locally; the refresh token expires on its own
    }
  }

  async updateProfile(input: { name: string }): Promise<AppUser> {
    const user = await this.http.request<AppUser>("PATCH", this.path("me"), { body: input });
    if (this.session) {
      this.session = { ...this.session, user };
      this.persist();
      this.setState({ status: "signedIn", user });
    }
    return user;
  }

  /** The server ends every session of the user on a password change, so this also signs out locally. */
  async changePassword(oldPassword: string, newPassword: string): Promise<void> {
    await this.http.request<void>("POST", this.path("password"), { body: { oldPassword, newPassword } });
    this.clearLocal();
  }
}
