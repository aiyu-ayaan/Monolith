import type { HttpClient } from "../http/client.js";
import type {
  CountOptions,
  CountResult,
  ListOptions,
  ListResult,
  RealtimeEvent,
  RealtimeListener,
  RealtimeSubscriptionOptions,
  RecordModel,
} from "../http/types.js";
import type { RealtimeService } from "../realtime/index.js";

export class Collection<T = RecordModel> {
  constructor(
    readonly name: string,
    private http: HttpClient,
    private realtime?: RealtimeService,
  ) {}

  private get basePath(): string {
    return `/api/v1/apps/${encodeURIComponent(this.http.app)}/collections/${encodeURIComponent(this.name)}/records`;
  }

  async list(options: ListOptions = {}): Promise<ListResult<T>> {
    const query: Record<string, unknown> = {};
    if (options.page !== undefined) query.page = options.page;
    if (options.perPage !== undefined) query.perPage = options.perPage;
    if (options.cursor !== undefined) query.cursor = options.cursor;
    if (options.limit !== undefined) query.limit = options.limit;
    if (options.filter !== undefined) query.filter = options.filter;
    if (options.sort !== undefined) query.sort = options.sort;
    if (options.expand !== undefined) query.expand = options.expand;
    if (options.fields !== undefined) query.fields = options.fields;
    if (options.total !== undefined) query.total = options.total;

    return this.http.request<ListResult<T>>("GET", this.basePath, { query });
  }

  async getOne(id: string, options: { expand?: string; fields?: string } = {}): Promise<T> {
    const query: Record<string, unknown> = {};
    if (options.expand !== undefined) query.expand = options.expand;
    if (options.fields !== undefined) query.fields = options.fields;

    return this.http.request<T>("GET", `${this.basePath}/${encodeURIComponent(id)}`, { query });
  }

  async create(data: Partial<T>): Promise<T> {
    return this.http.request<T>("POST", this.basePath, { body: data });
  }

  async update(id: string, data: Partial<T>): Promise<T> {
    return this.http.request<T>("PATCH", `${this.basePath}/${encodeURIComponent(id)}`, { body: data });
  }

  async replace(id: string, data: Partial<T>): Promise<T> {
    return this.http.request<T>("PUT", `${this.basePath}/${encodeURIComponent(id)}`, { body: data });
  }

  async delete(id: string): Promise<boolean> {
    await this.http.request<void>("DELETE", `${this.basePath}/${encodeURIComponent(id)}`);
    return true;
  }

  async count(options: CountOptions = {}): Promise<number> {
    const query: Record<string, unknown> = {};
    if (options.filter !== undefined) query.filter = options.filter;

    const res = await this.http.request<CountResult>("GET", `${this.basePath}:count`, { query });
    return res.count;
  }

  subscribe(listener: RealtimeListener<T>, options?: RealtimeSubscriptionOptions): () => void {
    if (!this.realtime) {
      throw new Error("Realtime service is not available on this client instance.");
    }
    return this.realtime.subscribe<T>(this.name, listener, options);
  }
}

