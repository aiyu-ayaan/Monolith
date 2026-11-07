import { MonolithClient } from "./index.js";
import { defaultStorage } from "./auth/session.js";
import type { AuthStorage } from "./auth/session.js";
import type { ClientOptions } from "./http/types.js";

export interface InitOptions {
  url: string;
  app: string;
  /** Where the session is kept. Default: `localStorage`, else memory. */
  storage?: AuthStorage;
  /** API key (`mk_...`) for server-side or anonymous app access. */
  apiKey?: string;
  /** Default redirect target for provider sign-in. Default: the current page without its hash. */
  redirectUri?: string;
  fetch?: typeof fetch;
  headers?: Record<string, string>;
}

const REGISTRY = Symbol.for("monolith.client.registry");

/** Shared across bundle copies (ESM and CJS) so `<monolith-auth>` can find the client. */
export function clientRegistry(): Map<string, MonolithClient> {
  const g = globalThis as unknown as Record<symbol, Map<string, MonolithClient> | undefined>;
  return (g[REGISTRY] ??= new Map());
}

export const Monolith = {
  /**
   * Returns a ready client: session restored from storage, token refreshed before expiry and once after a 401,
   * provider redirects handled. Calling it again with the same `url` and `app` returns the same instance.
   */
  init(options: InitOptions): MonolithClient {
    const key = `${options.url.replace(/\/+$/, "")}|${options.app}`;
    const reg = clientRegistry();
    const existing = reg.get(key);
    if (existing) return existing;
    const clientOptions: ClientOptions = {
      url: options.url,
      app: options.app,
      key: options.apiKey,
      storage: options.storage ?? defaultStorage(),
      redirectUri: options.redirectUri,
      fetch: options.fetch,
      headers: options.headers,
    };
    const client = new MonolithClient(clientOptions);
    reg.set(key, client);
    void client.auth.start();
    return client;
  },

  /** Forgets cached instances (tests, hot reload). */
  reset(): void {
    clientRegistry().clear();
  },
};
