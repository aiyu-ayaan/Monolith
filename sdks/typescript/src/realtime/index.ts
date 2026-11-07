import type { HttpClient } from "../http/client.js";
import type {
  DocumentChange,
  QuerySnapshot,
  RealtimeEvent,
  RealtimeListener,
  RealtimeSubscriptionOptions,
  RecordModel,
  SnapshotOptions,
} from "../http/types.js";

type Dispatch = (event: string, payload: any) => void;

/** One SSE connection with reconnect, backoff and resume (`since`, or a fresh snapshot). */
class Stream {
  private controller: AbortController | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private attempts = 0;
  private lastSeq = 0;
  private stopped = true;

  constructor(
    private http: HttpClient,
    private params: () => Record<string, string>,
    private dispatch: Dispatch,
    /** A snapshot stream re-snapshots on reconnect instead of replaying. */
    private snapshot: boolean,
  ) {}

  start(): void {
    this.stopped = false;
    void this.connect();
  }

  /** Reopens now (the collection list changed). Keeps `lastSeq` so nothing is missed. */
  restart(): void {
    this.controller?.abort();
    this.controller = null;
    if (!this.stopped) void this.connect();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.controller?.abort();
    this.controller = null;
    this.attempts = 0;
  }

  private async connect(): Promise<void> {
    const controller = new AbortController();
    this.controller = controller;
    const query: Record<string, unknown> = this.params();
    if (!this.snapshot && this.lastSeq > 0) query.since = this.lastSeq;
    const url = this.http.buildUrl(`/api/v1/apps/${encodeURIComponent(this.http.app)}/realtime`, query);
    const headers: Record<string, string> = { Accept: "text/event-stream", "Cache-Control": "no-cache" };
    const token = (await this.http.freshToken()) || this.http.getApiKey();
    if (token) headers.Authorization = `Bearer ${token}`;

    try {
      const res = await fetch(url, { headers, signal: controller.signal });
      if (!res.ok || !res.body) throw new Error(`SSE connection failed: ${res.status} ${res.statusText}`);
      this.attempts = 0;
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const blocks = buffer.split("\n\n");
        buffer = blocks.pop() ?? "";
        for (const block of blocks) this.parse(block);
      }
    } catch {
      // stream ended, network error or abort
    }
    if (!this.stopped && !controller.signal.aborted) this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    if (this.timer) return;
    const delay = Math.min(500 * 2 ** this.attempts + Math.random() * 200, 15000);
    this.attempts++;
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!this.stopped) void this.connect();
    }, delay);
  }

  private parse(block: string): void {
    let event = "message";
    let data = "";
    for (const raw of block.split("\n")) {
      const line = raw.trim();
      if (!line || line.startsWith(":")) continue;
      if (line.startsWith("event:")) event = line.slice(6).trim();
      else if (line.startsWith("data:")) data += (data ? "\n" : "") + line.slice(5).trim();
    }
    if (!data) return;
    let payload: any;
    try {
      payload = JSON.parse(data);
    } catch {
      return;
    }
    if (typeof payload.seq === "number" && payload.seq > this.lastSeq) this.lastSeq = payload.seq;
    if (event === "resync") {
      // Missed more than the server keeps: reconnect from scratch.
      this.lastSeq = 0;
      this.dispatch(event, payload);
      this.restart();
      return;
    }
    this.dispatch(event, payload);
  }
}

function toEvent<T>(p: any): RealtimeEvent<T> {
  return { action: p.op, record: p.record, collection: p.collection, seq: p.seq };
}

/** Client-side comparator for `-created,title` style sort strings. */
function comparator(sort: string | undefined): ((a: RecordModel, b: RecordModel) => number) | null {
  if (!sort) return null;
  const keys = sort
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => (s.startsWith("-") ? { f: s.slice(1), d: -1 } : { f: s.replace(/^\+/, ""), d: 1 }));
  return (a, b) => {
    for (const { f, d } of keys) {
      const x = a[f] as any;
      const y = b[f] as any;
      if (x === y) continue;
      if (x == null) return -d;
      if (y == null) return d;
      return (x < y ? -1 : 1) * d;
    }
    return 0;
  };
}

export class RealtimeService {
  private shared: Stream | null = null;
  private listeners = new Map<string, Set<RealtimeListener<any>>>();

  constructor(private http: HttpClient) {}

  /**
   * Live events for a collection. Without options, all subscriptions share one connection;
   * a `filter` or `ids` opens a dedicated one. Returns the unsubscribe function.
   * After a reconnect the stream resumes from the last seen change (no gaps).
   */
  subscribe<T>(
    collection: string,
    listener: RealtimeListener<T>,
    options?: RealtimeSubscriptionOptions,
  ): () => void {
    if (options?.filter || options?.ids?.length) {
      const params = () => {
        const q: Record<string, string> = { collections: collection };
        if (options.filter) q[`filter.${collection}`] = options.filter;
        if (options.ids?.length) q[`ids.${collection}`] = options.ids.join(",");
        return q;
      };
      const stream = new Stream(
        this.http,
        params,
        (ev, p) => {
          if (ev === "record") safe(() => listener(toEvent(p)));
        },
        false,
      );
      stream.start();
      return () => stream.stop();
    }

    let set = this.listeners.get(collection);
    const fresh = !set;
    if (!set) this.listeners.set(collection, (set = new Set()));
    set.add(listener);
    if (!this.shared) {
      this.shared = new Stream(
        this.http,
        () => ({ collections: [...this.listeners.keys()].join(",") }),
        (ev, p) => {
          if (ev !== "record") return;
          for (const l of this.listeners.get(p.collection) ?? []) safe(() => l(toEvent(p)));
        },
        false,
      );
      this.shared.start();
    } else if (fresh) {
      this.shared.restart();
    }
    return () => {
      const cur = this.listeners.get(collection);
      cur?.delete(listener);
      if (cur && cur.size === 0) this.listeners.delete(collection);
      if (this.listeners.size === 0) {
        this.shared?.stop();
        this.shared = null;
      } else if (!cur || cur.size === 0) {
        this.shared?.restart();
      }
    };
  }

  /**
   * Firestore-style query listener: calls back with the current result set, then again after
   * every change that affects it. Reconnects re-snapshot, so the view always converges.
   */
  onSnapshot<T extends RecordModel = RecordModel>(
    collection: string,
    callback: (snapshot: QuerySnapshot<T>) => void,
    options: SnapshotOptions = {},
  ): () => void {
    const docs = new Map<string, T>();
    const cmp = comparator(options.sort);
    const ordered = (): T[] => {
      const all = [...docs.values()];
      return cmp ? all.sort(cmp as any) : all;
    };
    const params = () => {
      const q: Record<string, string> = {
        collections: collection,
        snapshot: String(options.limit ?? 100),
      };
      if (options.filter) q[`filter.${collection}`] = options.filter;
      if (options.sort) q[`sort.${collection}`] = options.sort;
      return q;
    };
    const stream = new Stream(
      this.http,
      params,
      (ev, p) => {
        const changes: DocumentChange<T>[] = [];
        if (ev === "snapshot") {
          const next = new Map<string, T>((p.items as T[]).map((d) => [d.id, d]));
          for (const [id, d] of next) {
            changes.push({ type: docs.has(id) ? "modified" : "added", doc: d });
          }
          for (const [id, d] of docs) if (!next.has(id)) changes.push({ type: "removed", doc: d });
          docs.clear();
          for (const [id, d] of next) docs.set(id, d);
        } else if (ev === "record") {
          const rec = p.record as T;
          if (p.op === "delete" || p.op === "leave") {
            const old = docs.get(rec.id);
            if (!old) return;
            docs.delete(rec.id);
            changes.push({ type: "removed", doc: old });
          } else {
            changes.push({ type: docs.has(rec.id) ? "modified" : "added", doc: rec });
            docs.set(rec.id, rec);
          }
        } else if (ev === "error") {
          options.onError?.(new Error(p.message ?? "realtime error"));
          return;
        } else {
          return;
        }
        safe(() => callback({ docs: ordered(), changes }));
      },
      true,
    );
    stream.start();
    return () => stream.stop();
  }
}

function safe(fn: () => void): void {
  try {
    fn();
  } catch {
    // a listener must not break the stream
  }
}
