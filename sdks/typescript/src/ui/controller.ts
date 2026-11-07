import type { AppUser, AuthRedirectError, AuthState, ProviderSignInOptions, PublicAuth, PublicProvider } from "../auth/app.js";
import { type AuthLabels, resolveLabels } from "./labels.js";

export type AuthUiErrorCode =
  | "invalid_credentials"
  | "email_taken"
  | "weak_password"
  | "invalid_email"
  | "signup_closed"
  | "network"
  | "redirect_not_allowed"
  | "provider_failed"
  | "unknown";

export interface AuthUiError {
  code: AuthUiErrorCode;
  message: string;
  /** Developer hint, e.g. which setting to change. */
  hint?: string;
}

export interface AuthUiState {
  /** Fetching providers or restoring the session. */
  loading: boolean;
  /** `null` while loading or when the server could not be reached. */
  methods: PublicAuth | null;
  mode: "signIn" | "signUp";
  /** `false`, `true` (email form), or the provider id being used. */
  submitting: boolean | string;
  error: AuthUiError | null;
  /** Set when signed in. */
  user: AppUser | null;
}

/** The part of the client the controller needs (`mono.auth`), so tests can pass a fake transport. */
export interface AuthApi {
  readonly state: AuthState;
  readonly ready?: Promise<void>;
  getProviders(): Promise<PublicAuth>;
  signIn(input: { email: string; password: string }): Promise<AppUser>;
  signUp(input: { email: string; password: string; name?: string }): Promise<AppUser>;
  signInWithProvider(id: string, opts?: ProviderSignInOptions): Promise<AppUser | null>;
  signOut(): Promise<void>;
  onChange(listener: (state: AuthState) => void): () => void;
  takeRedirectError?(): AuthRedirectError | null;
}

export interface AuthControllerOptions {
  /** `auto` follows the server, `hide` never shows the email form, `force` always shows it. */
  email?: "auto" | "hide" | "force";
  /** `auto` follows the server, `hide` never offers sign-up, `only` starts in sign-up. */
  signUp?: "auto" | "hide" | "only";
  /** `auto` (all enabled on the server) or an explicit order / allow list. */
  providers?: "auto" | string[];
  hideProviders?: string[];
  /** Redirect target for provider sign-in (default: the client's `redirectUri`, else the current page). */
  redirectUri?: string;
  providerMode?: "redirect" | "popup";
  labels?: Partial<AuthLabels>;
  onSignedIn?: (user: AppUser) => void;
  onError?: (error: AuthUiError) => void;
}

export interface AuthController {
  getState(): AuthUiState;
  /** Calls `listener` after every state change (not on subscribe). Returns unsubscribe. */
  subscribe(listener: (state: AuthUiState) => void): () => void;
  setMode(mode: "signIn" | "signUp"): void;
  signInWithEmail(email: string, password: string): Promise<void>;
  signUpWithEmail(email: string, password: string, name?: string): Promise<void>;
  signInWithProvider(id: string): Promise<void>;
  dismissError(): void;
  signOut(): Promise<void>;
  /** Fetches the provider list again, bypassing the 60 s cache. */
  refreshMethods(): Promise<void>;
  /** Resolves when the first load (session restore and providers) is done. */
  readonly ready: Promise<void>;
  destroy(): void;
}

export interface AuthView {
  showEmail: boolean;
  canSignUp: boolean;
  providers: PublicProvider[];
  /** Nothing to offer: no email form and no provider. */
  empty: boolean;
}

const CACHE_MS = 60_000;
const cache = new WeakMap<object, { at: number; value: PublicAuth }>();
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** What to draw, from the state and the options. Shared by every UI kit. */
export function selectAuthView(state: AuthUiState, options: AuthControllerOptions = {}): AuthView {
  const m = state.methods;
  const emailOpt = options.email ?? "auto";
  const showEmail = emailOpt === "hide" ? false : emailOpt === "force" ? true : Boolean(m?.emailPassword.enabled);
  const canSignUp = showEmail && options.signUp !== "hide" && Boolean(m?.emailPassword.allowSignup);
  const hidden = new Set(options.hideProviders ?? []);
  let providers = (m?.providers ?? []).filter((p) => !hidden.has(p.id));
  if (Array.isArray(options.providers)) {
    const byId = new Map(providers.map((p) => [p.id, p]));
    providers = options.providers.map((id) => byId.get(id)).filter((p): p is PublicProvider => Boolean(p));
  }
  return { showEmail, canSignUp, providers, empty: !showEmail && providers.length === 0 };
}

interface ErrLike {
  status?: number;
  type?: string;
  detail?: string;
  message?: string;
  errors?: { field: string; code?: string }[];
}

/** problem+json (or a network failure) to a UI error code. */
export function mapAuthError(err: unknown, ctx: "signIn" | "signUp" | "provider" | "load"): AuthUiErrorCode {
  const e = (err ?? {}) as ErrLike;
  if (e.type === "network_error" || err instanceof TypeError) return "network";
  const fields = e.errors ?? [];
  if (fields.some((f) => f.field === "password")) return "weak_password";
  if (fields.some((f) => f.field === "email")) return "invalid_email";
  const text = `${e.detail ?? ""} ${e.message ?? ""}`;
  if (e.status === 400 && (/redirectUri/i.test(text) || fields.some((f) => f.field === "redirectUri"))) {
    return "redirect_not_allowed";
  }
  if (ctx === "provider") return "provider_failed";
  if (e.status === 401) return "invalid_credentials";
  if (e.status === 409) return "email_taken";
  if (e.status === 403 && ctx === "signUp") return "signup_closed";
  return "unknown";
}

/** Options are read when used, so a caller may keep mutating the object it passed. */
export function createAuthController(mono: { auth: AuthApi }, options: AuthControllerOptions = {}): AuthController {
  const auth = mono.auth;
  const listeners = new Set<(s: AuthUiState) => void>();
  let methodsPending: Promise<void> | null = null;
  let notifiedUser: string | null = null;
  let destroyed = false;

  let state: AuthUiState = {
    loading: true,
    methods: null,
    mode: options.signUp === "only" ? "signUp" : "signIn",
    submitting: false,
    error: null,
    user: auth.state.user,
  };

  const set = (patch: Partial<AuthUiState>): void => {
    if (destroyed) return;
    state = { ...state, ...patch };
    for (const l of [...listeners]) l(state);
  };

  const makeError = (code: AuthUiErrorCode, hint?: string): AuthUiError => ({
    code,
    message: resolveLabels(options.labels)[`error_${code}` as keyof AuthLabels],
    ...(hint ? { hint } : {}),
  });

  const fail = (code: AuthUiErrorCode, hint?: string): void => {
    const error = makeError(code, hint);
    set({ error, submitting: false });
    options.onError?.(error);
  };

  const failWith = (err: unknown, ctx: "signIn" | "signUp" | "provider" | "load"): void => {
    const code = mapAuthError(err, ctx);
    const hint =
      code === "redirect_not_allowed"
        ? "Add this page's URL to Allowed redirect URLs in the Admin UI (Authentication), or pass a registered redirectUri."
        : undefined;
    fail(code, hint);
  };

  const notifySignedIn = (user: AppUser | null): void => {
    if (user && user.id !== notifiedUser) {
      notifiedUser = user.id;
      options.onSignedIn?.(user);
    } else if (!user) {
      notifiedUser = null;
    }
  };

  const fitMode = (m: PublicAuth | null, mode: AuthUiState["mode"]): AuthUiState["mode"] =>
    mode === "signUp" && m && !selectAuthView({ ...state, methods: m }, options).canSignUp ? "signIn" : mode;

  function loadMethods(force: boolean): Promise<void> {
    const hit = cache.get(auth);
    if (!force && hit && Date.now() - hit.at < CACHE_MS) {
      set({ methods: hit.value, mode: fitMode(hit.value, state.mode), loading: false });
      return Promise.resolve();
    }
    // One request at a time: the session restore and `ready` can both ask.
    if (methodsPending) return methodsPending;
    const p = (async () => {
      try {
        const value = await auth.getProviders();
        cache.set(auth, { at: Date.now(), value });
        set({ methods: value, mode: fitMode(value, state.mode), loading: false });
      } catch (err) {
        set({ loading: false });
        failWith(err, "load");
      } finally {
        methodsPending = null;
      }
    })();
    methodsPending = p;
    return p;
  }

  const unsubscribe = auth.onChange((s) => {
    set({ user: s.user, submitting: s.status === "signedIn" ? false : state.submitting });
    notifySignedIn(s.user);
    if (s.status === "signedOut" && !state.methods && !methodsPending) void loadMethods(false);
  });

  const ready = (async () => {
    if (auth.ready) await auth.ready.catch(() => {});
    set({ user: auth.state.user });
    notifySignedIn(auth.state.user);
    const redirectErr = auth.takeRedirectError?.();
    if (redirectErr) {
      const hint = redirectErr.description || undefined;
      fail("provider_failed", hint);
    }
    if (auth.state.status === "signedIn") set({ loading: false });
    else await loadMethods(false);
  })();

  const checkEmail = (email: string): boolean => {
    if (EMAIL_RE.test(email.trim())) return true;
    fail("invalid_email");
    return false;
  };

  return {
    getState: () => state,
    subscribe(l) {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    setMode(mode) {
      if (mode === "signUp" && state.methods && !selectAuthView(state, options).canSignUp) return;
      set({ mode, error: null });
    },
    async signInWithEmail(email, password) {
      if (state.submitting) return;
      if (!checkEmail(email)) return;
      if (!password) return fail("invalid_credentials");
      set({ submitting: true, error: null });
      try {
        await auth.signIn({ email: email.trim(), password });
        set({ submitting: false });
      } catch (err) {
        failWith(err, "signIn");
      }
    },
    async signUpWithEmail(email, password, name) {
      if (state.submitting) return;
      if (!checkEmail(email)) return;
      const min = state.methods?.emailPassword.minPasswordLength ?? 8;
      if (password.length < min || password.length > 72) return fail("weak_password");
      set({ submitting: true, error: null });
      try {
        const n = name?.trim();
        await auth.signUp({ email: email.trim(), password, ...(n ? { name: n } : {}) });
        set({ submitting: false });
      } catch (err) {
        failWith(err, "signUp");
      }
    },
    async signInWithProvider(id) {
      if (state.submitting) return;
      set({ submitting: id, error: null });
      try {
        const opts: ProviderSignInOptions = {};
        if (options.redirectUri) opts.redirectUri = options.redirectUri;
        if (options.providerMode) opts.mode = options.providerMode;
        const user = await auth.signInWithProvider(id, opts);
        // Redirect mode leaves the page; keep the busy state until it unloads.
        if (user) set({ submitting: false });
      } catch (err) {
        failWith(err, "provider");
      }
    },
    dismissError: () => set({ error: null }),
    async signOut() {
      try {
        await auth.signOut();
      } catch (err) {
        failWith(err, "load");
      }
    },
    refreshMethods: () => loadMethods(true),
    ready,
    destroy() {
      destroyed = true;
      unsubscribe();
      listeners.clear();
    },
  };
}
