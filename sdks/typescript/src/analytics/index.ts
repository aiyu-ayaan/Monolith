import type { HttpClient } from "../http/client.js";

export interface AnalyticsEvent {
  event: string;
  properties?: Record<string, unknown>;
  timestamp: string;
}

export class AnalyticsService {
  private queue: AnalyticsEvent[] = [];
  private flushTimer: ReturnType<typeof setTimeout> | null = null;
  private isFlushing = false;

  constructor(
    private http: HttpClient,
    private flushIntervalMs: number = 5000,
    private maxBatchSize: number = 20,
  ) {}

  track(event: string, properties?: Record<string, unknown>): void {
    this.queue.push({
      event,
      properties,
      timestamp: new Date().toISOString(),
    });

    if (this.queue.length >= this.maxBatchSize) {
      void this.flush();
    } else if (!this.flushTimer) {
      this.flushTimer = setTimeout(() => {
        this.flushTimer = null;
        void this.flush();
      }, this.flushIntervalMs);
    }
  }

  async flush(): Promise<void> {
    if (this.isFlushing || this.queue.length === 0) return;
    this.isFlushing = true;

    if (this.flushTimer) {
      clearTimeout(this.flushTimer);
      this.flushTimer = null;
    }

    const batch = this.queue.splice(0, this.maxBatchSize);
    const path = `/api/v1/apps/${encodeURIComponent(this.http.app)}/analytics/events`;

    try {
      await this.http.request<void>("POST", path, {
        body: { events: batch },
      });
    } catch {
      // Re-queue on failure if within bounds
      if (this.queue.length < 500) {
        this.queue.unshift(...batch);
      }
    } finally {
      this.isFlushing = false;
      if (this.queue.length > 0 && !this.flushTimer) {
        this.flushTimer = setTimeout(() => {
          this.flushTimer = null;
          void this.flush();
        }, this.flushIntervalMs);
      }
    }
  }
}

