import { AnalyticsService } from "./analytics/index.js";
import { AppAuth, AuthService } from "./auth/index.js";
import { Collection } from "./collections/index.js";
import { HttpClient } from "./http/client.js";
import type {
  BatchOperation,
  BatchResponseItem,
  ClientOptions,
  RecordModel,
} from "./http/types.js";
import { FilesService } from "./files/index.js";
import { RealtimeService } from "./realtime/index.js";

export * from "./errors/index.js";
export * from "./http/types.js";
export { HttpClient } from "./http/client.js";
export * from "./auth/index.js";
export { Monolith, type InitOptions } from "./monolith.js";
export { Collection } from "./collections/index.js";
export { RealtimeService } from "./realtime/index.js";
export { FilesService } from "./files/index.js";
export { AnalyticsService } from "./analytics/index.js";

/** `client.auth("users")` (token service) and `client.auth.signIn(...)` (app users) on one property. */
export type AuthAccessor = ((collection?: string) => AuthService) & AppAuth;

export class MonolithClient {
  readonly http: HttpClient;
  readonly realtime: RealtimeService;
  readonly files: FilesService;
  readonly analytics: AnalyticsService;
  /** App-user auth: `getProviders`, `signUp`, `signIn`, `signInWithProvider`, `signOut`, `user`, `onChange`, ... Also callable: `auth("users")`. */
  readonly auth: AuthAccessor;
  private authServices: Map<string, AuthService> = new Map();
  private collections: Map<string, Collection<any>> = new Map();

  constructor(options: ClientOptions) {
    this.http = new HttpClient(options);
    this.realtime = new RealtimeService(this.http);
    this.files = new FilesService(this.http);
    this.analytics = new AnalyticsService(this.http);
    const appAuth = AppAuth.for(this.http, { storage: options.storage, redirectUri: options.redirectUri });
    const service = (collection: string = "users"): AuthService => {
      let svc = this.authServices.get(collection);
      if (!svc) {
        svc = new AuthService(this.http, collection, appAuth);
        this.authServices.set(collection, svc);
      }
      return svc;
    };
    this.auth = new Proxy(service, {
      get(_t, prop) {
        const v = (appAuth as unknown as Record<string | symbol, unknown>)[prop];
        return typeof v === "function" ? v.bind(appAuth) : v;
      },
    }) as unknown as AuthAccessor;
  }

  collection<T = RecordModel>(name: string): Collection<T> {
    let col = this.collections.get(name);
    if (!col) {
      col = new Collection<T>(name, this.http, this.realtime);
      this.collections.set(name, col);
    }
    return col as Collection<T>;
  }

  async batch(operations: BatchOperation[]): Promise<BatchResponseItem[]> {
    const path = `/api/v1/apps/${encodeURIComponent(this.http.app)}/batch`;
    return this.http.request<BatchResponseItem[]>("POST", path, {
      body: { operations },
    });
  }
}

export function createClient(options: ClientOptions): MonolithClient {
  return new MonolithClient(options);
}

export default createClient;

