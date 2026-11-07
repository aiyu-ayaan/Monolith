import type { HttpClient } from "../http/client.js";
import type { RealtimeEvent, RealtimeListener, RealtimeSubscriptionOptions } from "../http/types.js";

interface SubscriptionEntry {
  listener: RealtimeListener<any>;
  filter?: string;
}

export class RealtimeService {
  private subscriptions: Map<string, Set<SubscriptionEntry>> = new Map();
  private abortController: AbortController | null = null;
  private isConnecting = false;
  private isConnected = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;

  constructor(private http: HttpClient) {}

  subscribe<T>(
    collection: string,
    listener: RealtimeListener<T>,
    options?: RealtimeSubscriptionOptions,
  ): () => void {
    let set = this.subscriptions.get(collection);
    if (!set) {
      set = new Set();
      this.subscriptions.set(collection, set);
    }

    const entry: SubscriptionEntry = {
      listener,
      filter: options?.filter,
    };
    set.add(entry);

    this.reconnect();

    return () => {
      const currentSet = this.subscriptions.get(collection);
      if (currentSet) {
        currentSet.delete(entry);
        if (currentSet.size === 0) {
          this.subscriptions.delete(collection);
        }
      }

      if (this.subscriptions.size === 0) {
        this.disconnect();
      } else {
        this.reconnect();
      }
    };
  }

  private disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }
    this.isConnected = false;
    this.isConnecting = false;
    this.reconnectAttempts = 0;
  }

  private reconnect(): void {
    if (this.subscriptions.size === 0) {
      this.disconnect();
      return;
    }

    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }

    this.connect();
  }

  private async connect(): Promise<void> {
    if (this.isConnecting || this.subscriptions.size === 0) return;
    this.isConnecting = true;

    const collections = Array.from(this.subscriptions.keys()).join(",");
    const path = `/api/v1/apps/${encodeURIComponent(this.http.app)}/realtime`;
    const url = this.http.buildUrl(path, collections ? { collections } : undefined);

    const headers: Record<string, string> = {
      Accept: "text/event-stream",
      "Cache-Control": "no-cache",
    };

    const token = this.http.getToken() || this.http.getApiKey();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }

    const controller = new AbortController();
    this.abortController = controller;

    try {
      const res = await fetch(url, {
        headers,
        signal: controller.signal,
      });

      if (!res.ok || !res.body) {
        throw new Error(`SSE connection failed: ${res.status} ${res.statusText}`);
      }

      this.isConnected = true;
      this.isConnecting = false;
      this.reconnectAttempts = 0;

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n\n");
        buffer = lines.pop() ?? "";

        for (const block of lines) {
          this.parseEventBlock(block);
        }
      }
    } catch {
      // Stream ended or aborted
    } finally {
      this.isConnected = false;
      this.isConnecting = false;

      if (this.subscriptions.size > 0 && !controller.signal.aborted) {
        this.scheduleReconnect();
      }
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    const delay = Math.min(500 * Math.pow(2, this.reconnectAttempts) + Math.random() * 200, 15000);
    this.reconnectAttempts++;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private parseEventBlock(block: string): void {
    let eventName = "message";
    let data = "";

    const lines = block.split("\n");
    for (const rawLine of lines) {
      const line = rawLine.trim();
      if (!line || line.startsWith(":")) continue; // comment/ping

      if (line.startsWith("event:")) {
        eventName = line.slice(6).trim();
      } else if (line.startsWith("data:")) {
        data += (data ? "\n" : "") + line.slice(5).trim();
      }
    }

    if (!data) return;

    try {
      const parsed = JSON.parse(data);
      this.dispatchEvent(eventName, parsed);
    } catch {
      // ignore unparseable payload
    }
  }

  private dispatchEvent(event: string, payload: any): void {
    // payload structure: { action: "create" | "update" | "delete", record: ..., collection: ... }
    const collection = payload.collection || event;
    const subscribers = this.subscriptions.get(collection);
    if (!subscribers) return;

    const rtEvent: RealtimeEvent = {
      action: payload.action ?? (event as any),
      record: payload.record ?? payload,
      collection,
    };

    for (const entry of subscribers) {
      try {
        entry.listener(rtEvent);
      } catch {
        // ignore subscriber errors
      }
    }
  }
}

