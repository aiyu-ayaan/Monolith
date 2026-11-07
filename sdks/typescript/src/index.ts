import { AnalyticsService } from "./analytics/index.js";
import { AuthService } from "./auth/index.js";
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
export { AuthService } from "./auth/index.js";
export { Collection } from "./collections/index.js";
export { RealtimeService } from "./realtime/index.js";
export { FilesService } from "./files/index.js";
export { AnalyticsService } from "./analytics/index.js";

export class MonolithClient {
  readonly http: HttpClient;
  readonly realtime: RealtimeService;
  readonly files: FilesService;
  readonly analytics: AnalyticsService;
  private authServices: Map<string, AuthService> = new Map();
  private collections: Map<string, Collection<any>> = new Map();

  constructor(options: ClientOptions) {
    this.http = new HttpClient(options);
    this.realtime = new RealtimeService(this.http);
    this.files = new FilesService(this.http);
    this.analytics = new AnalyticsService(this.http);
  }

  auth(collection: string = "users"): AuthService {
    let service = this.authServices.get(collection);
    if (!service) {
      service = new AuthService(this.http, collection);
      this.authServices.set(collection, service);
    }
    return service;
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

