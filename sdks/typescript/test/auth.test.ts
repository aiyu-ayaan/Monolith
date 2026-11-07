import { beforeEach, describe, expect, it } from "vitest";
import { Monolith, memoryStorage } from "../src/index.js";
import { createAuthController, mapAuthError, selectAuthView } from "../src/ui/controller.js";
import type { PublicAuth } from "../src/auth/app.js";

const APP = "/api/v1/apps/shop/auth";
const USER = { id: "u1", email: "a@b.co", name: "A", providers: ["password"] };

const json = (status: number, body: unknown): Response =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": status >= 400 ? "application/problem+json" : "application/json" },
  });
const problem = (status: number, title: string, extra: Record<string, unknown> = {}): Response =>
  json(status, { type: "about:blank", title, status, ...extra });

class FakeServer {
  calls: string[] = [];
  valid = new Set<string>();
  refreshTokens = new Set<string>();
  n = 0;
  refreshDelay = 0;
  recordHits = 0;
  providers: PublicAuth = {
    app: "shop",
    emailPassword: { enabled: true, allowSignup: true, minPasswordLength: 8 },
    providers: [{ id: "google", name: "Google", startPath: "x" }],
  };

  session(ttlMs = 3_600_000) {
    this.n++;
    const token = `at${this.n}`;
    const refreshToken = `rt${this.n}`;
    this.valid.add(token);
    this.refreshTokens.add(refreshToken);
    return { token, refreshToken, expiresAt: new Date(Date.now() + ttlMs).toISOString(), user: USER };
  }

  count(name: string): number {
    return this.calls.filter((c) => c.endsWith(`/${name}`)).length;
  }

  fetch = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = new URL(typeof input === "string" ? input : input.toString());
    const path = url.pathname;
    this.calls.push(`${init?.method ?? "GET"} ${path}`);
    const body = init?.body ? JSON.parse(init.body as string) : {};
    const headers = (init?.headers ?? {}) as Record<string, string>;
    const bearer = (headers.authorization ?? "").replace(/^Bearer /, "");
    if (path === `${APP}/providers`) return json(200, this.providers);
    if (path === `${APP}/register`) {
      return body.email === "taken@b.co" ? problem(409, "Email taken") : json(201, this.session());
    }
    if (path === `${APP}/login`) {
      return body.password === "secret123" ? json(200, this.session()) : problem(401, "Invalid credentials");
    }
    if (path === `${APP}/refresh`) {
      if (this.refreshDelay) await new Promise((r) => setTimeout(r, this.refreshDelay));
      if (!this.refreshTokens.has(body.refreshToken)) return problem(401, "Invalid refresh token");
      this.refreshTokens.delete(body.refreshToken);
      return json(200, this.session());
    }
    if (path === `${APP}/logout`) {
      this.refreshTokens.delete(body.refreshToken);
      return new Response(null, { status: 204 });
    }
    if (path === `${APP}/me`) {
      if (!this.valid.has(bearer)) return problem(401, "Unauthorized");
      if (init?.method === "PATCH") return json(200, { ...USER, name: body.name });
      return json(200, USER);
    }
    if (path === `${APP}/password`) {
      if (!this.valid.has(bearer)) return problem(401, "Unauthorized");
      if (body.newPassword.length < 8) {
        return problem(400, "Validation", { errors: [{ field: "password", code: "too_short" }] });
      }
      return new Response(null, { status: 204 });
    }
    if (path.endsWith("/collections/tasks/records")) {
      this.recordHits++;
      return this.valid.has(bearer) ? json(200, { items: [], bearer }) : problem(401, "Unauthorized");
    }
    return problem(404, "Not found");
  };

  init(extra: Record<string, unknown> = {}) {
    Monolith.reset();
    return Monolith.init({
      url: "https://api.test",
      app: "shop",
      storage: memoryStorage(),
      redirectUri: "https://app.test/",
      fetch: ((i: string, o?: RequestInit) => this.fetch(i, o)) as typeof fetch,
      ...extra,
    });
  }
}

beforeEach(() => Monolith.reset());

describe("Monolith.init", () => {
  it("returns the same instance for the same url and app", () => {
    const srv = new FakeServer();
    const a = srv.init();
    const b = Monolith.init({ url: "https://api.test/", app: "shop" });
    expect(b).toBe(a);
  });

  it("restores a stored session and signs out when storage is empty", async () => {
    const srv = new FakeServer();
    const storage = memoryStorage();
    const first = srv.init({ storage });
    await first.auth.ready;
    expect(first.auth.state.status).toBe("signedOut");
    await first.auth.signIn({ email: "a@b.co", password: "secret123" });
    expect(first.auth.state.status).toBe("signedIn");

    const second = srv.init({ storage });
    expect(second.auth.state.status).toBe("unknown");
    await second.auth.ready;
    expect(second.auth.user?.id).toBe("u1");
  });

  it("ignores corrupt stored data", async () => {
    const srv = new FakeServer();
    const storage = memoryStorage();
    storage.setItem("monolith:session:https://api.test:shop", "{not json");
    const mono = srv.init({ storage });
    await mono.auth.ready;
    expect(mono.auth.state.status).toBe("signedOut");
  });
});

describe("app user auth", () => {
  it("signs up, signs in, and emits state changes", async () => {
    const srv = new FakeServer();
    const auth = srv.init().auth;
    await auth.ready;
    const seen: string[] = [];
    auth.onChange((s) => seen.push(s.status));
    await auth.signUp({ email: "n@b.co", password: "secret123" });
    await auth.signOut();
    await auth.signIn({ email: "a@b.co", password: "secret123" });
    expect(seen).toEqual(["signedIn", "signedOut", "signedIn"]);
  });

  it("sends the access token on record calls", async () => {
    const srv = new FakeServer();
    const mono = srv.init();
    await mono.auth.signIn({ email: "a@b.co", password: "secret123" });
    const r = (await mono.collection("tasks").list()) as unknown as { bearer: string };
    expect(r.bearer).toBe("at1");
  });

  it("maps server errors and stays signed out", async () => {
    const srv = new FakeServer();
    const auth = srv.init().auth;
    await expect(auth.signIn({ email: "a@b.co", password: "wrong" })).rejects.toMatchObject({ status: 401 });
    await expect(auth.signUp({ email: "taken@b.co", password: "secret123" })).rejects.toMatchObject({ status: 409 });
    expect(auth.state.status).toBe("signedOut");
  });

  it("updates the profile and signs out after a password change", async () => {
    const srv = new FakeServer();
    const auth = srv.init().auth;
    await auth.signIn({ email: "a@b.co", password: "secret123" });
    expect((await auth.updateProfile({ name: "Zed" })).name).toBe("Zed");
    expect(auth.user?.name).toBe("Zed");
    await expect(auth.changePassword("secret123", "short")).rejects.toMatchObject({ status: 400 });
    expect(auth.state.status).toBe("signedIn");
    await auth.changePassword("secret123", "newsecret123");
    expect(auth.state.status).toBe("signedOut");
  });

  it("rotates tokens and refreshes before expiry", async () => {
    const srv = new FakeServer();
    const mono = srv.init({});
    const auth = mono.auth;
    await auth.signIn({ email: "a@b.co", password: "secret123" });
    const t1 = await auth.accessToken();
    await auth.refresh();
    const t2 = await auth.accessToken();
    expect(t2).not.toBe(t1);
    expect(srv.refreshTokens.has("rt1")).toBe(false);
  });

  it("refreshes an almost expired token before returning it", async () => {
    const srv = new FakeServer();
    const mono = srv.init({});
    await mono.auth.signIn({ email: "a@b.co", password: "secret123" });
    // Server hands out a token that expires in 5 s (inside the 60 s skew).
    const short = srv.session(5_000);
    const key = "monolith:session:https://api.test:shop";
    const storage = memoryStorage();
    storage.setItem(key, JSON.stringify({ ...short, expiresAt: Date.now() + 5_000 }));
    const m2 = srv.init({ storage });
    await m2.auth.ready;
    await m2.auth.accessToken();
    expect(srv.count("refresh")).toBeGreaterThanOrEqual(1);
  });

  it("shares one refresh between concurrent 401 retries", async () => {
    const srv = new FakeServer();
    srv.refreshDelay = 50;
    const mono = srv.init();
    await mono.auth.signIn({ email: "a@b.co", password: "secret123" });
    srv.valid.clear(); // server revoked the access token; the refresh token is still good
    const out = await Promise.all(Array.from({ length: 6 }, () => mono.collection("tasks").list()));
    expect(out).toHaveLength(6);
    expect(srv.count("refresh")).toBe(1);
  });

  it("signs out when the refresh token is dead", async () => {
    const srv = new FakeServer();
    const mono = srv.init();
    await mono.auth.signIn({ email: "a@b.co", password: "secret123" });
    srv.valid.clear();
    srv.refreshTokens.clear();
    await expect(mono.collection("tasks").list()).rejects.toMatchObject({ status: 401 });
    expect(mono.auth.state.status).toBe("signedOut");
  });

  it("retries a 401 only once", async () => {
    const srv = new FakeServer();
    const mono = srv.init();
    await mono.auth.signIn({ email: "a@b.co", password: "secret123" });
    // Refresh works but the records endpoint keeps answering 401.
    const orig = srv.fetch;
    srv.fetch = async (i, init) => {
      const u = new URL(typeof i === "string" ? i : i.toString());
      if (u.pathname.endsWith("/records")) {
        srv.recordHits++;
        return problem(401, "Unauthorized");
      }
      return orig(i, init);
    };
    await expect(mono.collection("tasks").list()).rejects.toMatchObject({ status: 401 });
    expect(srv.recordHits).toBe(2);
    expect(srv.count("refresh")).toBe(1);
  });
});

describe("handleRedirect", () => {
  it("finishes a provider sign-in from a fragment", async () => {
    const srv = new FakeServer();
    srv.valid.add("tok");
    const auth = srv.init().auth;
    const r = await auth.handleRedirect("https://app.test/#token=tok&refreshToken=rt&expiresAt=2099-01-01T00:00:00Z");
    expect(r).toMatchObject({ user: { id: "u1" } });
    expect(auth.state.status).toBe("signedIn");
  });

  it("reports provider errors and ignores plain URLs", async () => {
    const auth = new FakeServer().init().auth;
    expect(await auth.handleRedirect("https://app.test/")).toBeNull();
    const r = await auth.handleRedirect("https://app.test/#error=oauth_failed&error_description=email%20taken");
    expect(r).toEqual({ error: { code: "oauth_failed", description: "email taken" } });
    expect(auth.takeRedirectError()?.code).toBe("oauth_failed");
    expect(auth.takeRedirectError()).toBeNull();
  });

  it("returns session_failed when /me rejects the token", async () => {
    const auth = new FakeServer().init().auth;
    const r = await auth.handleRedirect("https://app.test/#token=bad&refreshToken=rt");
    expect(r).toMatchObject({ error: { code: "session_failed" } });
    expect(auth.state.status).not.toBe("signedIn");
  });

  it("requires a redirectUri outside a browser", async () => {
    const srv = new FakeServer();
    Monolith.reset();
    const mono = Monolith.init({ url: "https://api.test", app: "shop", fetch: srv.fetch as typeof fetch });
    await expect(mono.auth.signInWithProvider("google")).rejects.toMatchObject({ type: "redirect_required" });
  });
});

describe("mapAuthError", () => {
  it.each([
    [{ status: 401 }, "signIn", "invalid_credentials"],
    [{ status: 409 }, "signUp", "email_taken"],
    [{ status: 403 }, "signUp", "signup_closed"],
    [{ status: 400, errors: [{ field: "password" }] }, "signUp", "weak_password"],
    [{ status: 400, errors: [{ field: "email" }] }, "signUp", "invalid_email"],
    [{ status: 400, detail: "redirectUri not allowed" }, "provider", "redirect_not_allowed"],
    [{ status: 500 }, "provider", "provider_failed"],
    [{ type: "network_error", status: 0 }, "signIn", "network"],
    [new TypeError("fetch failed"), "load", "network"],
    [{ status: 500 }, "signIn", "unknown"],
  ] as const)("%j (%s) -> %s", (err, ctx, code) => {
    expect(mapAuthError(err, ctx)).toBe(code);
  });
});

describe("selectAuthView", () => {
  const methods = new FakeServer().providers;
  const base = { loading: false, methods, mode: "signIn", submitting: false, error: null, user: null } as const;

  it("follows the server by default", () => {
    const v = selectAuthView(base);
    expect(v).toMatchObject({ showEmail: true, canSignUp: true, empty: false });
    expect(v.providers.map((p) => p.id)).toEqual(["google"]);
  });

  it("applies email, signUp, providers and hideProviders options", () => {
    expect(selectAuthView(base, { email: "hide" })).toMatchObject({ showEmail: false, canSignUp: false });
    expect(selectAuthView(base, { signUp: "hide" }).canSignUp).toBe(false);
    expect(selectAuthView(base, { hideProviders: ["google"] }).providers).toEqual([]);
    expect(selectAuthView(base, { providers: ["github"] }).providers).toEqual([]);
    expect(selectAuthView(base, { email: "hide", hideProviders: ["google"] }).empty).toBe(true);
    expect(selectAuthView({ ...base, methods: null }, { email: "force" }).showEmail).toBe(true);
  });
});

describe("createAuthController", () => {
  it("loads providers, then signs in and calls onSignedIn once", async () => {
    const srv = new FakeServer();
    const mono = srv.init();
    const signedIn: string[] = [];
    const c = createAuthController(mono, { onSignedIn: (u) => signedIn.push(u.id) });
    expect(c.getState().loading).toBe(true);
    await c.ready;
    expect(c.getState().loading).toBe(false);
    expect(c.getState().methods?.providers[0]?.id).toBe("google");
    await c.signInWithEmail("a@b.co", "secret123");
    expect(c.getState().user?.id).toBe("u1");
    expect(signedIn).toEqual(["u1"]);
    c.destroy();
  });

  it("validates input before any request", async () => {
    const srv = new FakeServer();
    const c = createAuthController(srv.init());
    await c.ready;
    const before = srv.calls.length;
    await c.signInWithEmail("not-an-email", "secret123");
    expect(c.getState().error?.code).toBe("invalid_email");
    await c.signUpWithEmail("a@b.co", "short");
    expect(c.getState().error?.code).toBe("weak_password");
    expect(srv.calls.length).toBe(before);
  });

  it("maps server errors and clears them on dismiss and mode change", async () => {
    const srv = new FakeServer();
    const errors: string[] = [];
    const c = createAuthController(srv.init(), { onError: (e) => errors.push(e.code) });
    await c.ready;
    await c.signInWithEmail("a@b.co", "wrong-pass");
    expect(c.getState().error?.code).toBe("invalid_credentials");
    expect(c.getState().submitting).toBe(false);
    c.dismissError();
    expect(c.getState().error).toBeNull();
    await c.signUpWithEmail("taken@b.co", "secret123");
    expect(c.getState().error?.code).toBe("email_taken");
    c.setMode("signUp");
    expect(c.getState().error).toBeNull();
    expect(errors).toEqual(["invalid_credentials", "email_taken"]);
  });

  it("starts in sign-up mode with signUp:'only' and falls back when sign-up is closed", async () => {
    const srv = new FakeServer();
    srv.providers.emailPassword.allowSignup = false;
    const c = createAuthController(srv.init(), { signUp: "only" });
    expect(c.getState().mode).toBe("signUp");
    await c.ready;
    expect(c.getState().mode).toBe("signIn");
    c.setMode("signUp");
    expect(c.getState().mode).toBe("signIn");
  });

  it("shows a redirect error from a failed provider sign-in", async () => {
    const srv = new FakeServer();
    const mono = srv.init();
    await mono.auth.handleRedirect("https://app.test/#error=oauth_failed&error_description=nope");
    const c = createAuthController(mono);
    await c.ready;
    expect(c.getState().error).toMatchObject({ code: "provider_failed", hint: "nope" });
  });

  it("caches providers for 60 s and refreshMethods bypasses the cache", async () => {
    const srv = new FakeServer();
    const mono = srv.init();
    const a = createAuthController(mono);
    await a.ready;
    const b = createAuthController(mono);
    await b.ready;
    expect(srv.count("providers")).toBe(1);
    await b.refreshMethods();
    expect(srv.count("providers")).toBe(2);
  });

  it("overrides labels in error messages", async () => {
    const srv = new FakeServer();
    const c = createAuthController(srv.init(), { labels: { error_invalid_credentials: "Nope" } });
    await c.ready;
    await c.signInWithEmail("a@b.co", "wrong-pass");
    expect(c.getState().error?.message).toBe("Nope");
  });

  it("reports a network failure while loading providers", async () => {
    const srv = new FakeServer();
    srv.fetch = async () => {
      throw new TypeError("offline");
    };
    const c = createAuthController(srv.init());
    await c.ready;
    expect(c.getState().loading).toBe(false);
    expect(c.getState().error?.code).toBe("network");
  });
});
