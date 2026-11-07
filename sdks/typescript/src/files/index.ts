import type { HttpClient } from "../http/client.js";
import type { StorageListOptions, StorageListResult, StorageObject } from "../http/types.js";

export class FilesService {
  constructor(private http: HttpClient) {}

  private bucketPath(bucket: string): string {
    return `/api/v1/apps/${encodeURIComponent(this.http.app)}/storage/buckets/${encodeURIComponent(bucket)}/objects`;
  }

  getURL(bucket: string, key: string, options: { thumb?: string } = {}): string {
    const path = `${this.bucketPath(bucket)}/${encodeURIComponent(key)}`;
    const query = options.thumb ? { thumb: options.thumb } : undefined;
    return this.http.buildUrl(path, query);
  }

  async upload(
    bucket: string,
    key: string,
    data: Blob | ArrayBuffer | Uint8Array | string,
    options: { contentType?: string } = {},
  ): Promise<StorageObject> {
    const path = `${this.bucketPath(bucket)}/${encodeURIComponent(key)}`;
    const headers: Record<string, string> = {};
    if (options.contentType) {
      headers["content-type"] = options.contentType;
    }

    return this.http.request<StorageObject>("PUT", path, {
      body: data,
      headers,
    });
  }

  async delete(bucket: string, key: string): Promise<boolean> {
    const path = `${this.bucketPath(bucket)}/${encodeURIComponent(key)}`;
    await this.http.request<void>("DELETE", path);
    return true;
  }

  async list(bucket: string, options: StorageListOptions = {}): Promise<StorageListResult> {
    const query: Record<string, unknown> = {};
    if (options.prefix !== undefined) query.prefix = options.prefix;
    if (options.cursor !== undefined) query.cursor = options.cursor;
    if (options.limit !== undefined) query.limit = options.limit;

    return this.http.request<StorageListResult>("GET", this.bucketPath(bucket), { query });
  }
}

