import { afterEach, describe, expect, it, vi } from "vitest";
import { createClient } from "../src/index.js";

/** Fetch mock that serves each call one SSE body, then stays open until aborted. */
function sse(bodies: string[], end = false) {
  const urls: string[] = [];
  const fn = vi.fn(async (url: string, init: RequestInit) => {
    urls.push(url);
    const body = bodies.shift() ?? "";
    const enc = new TextEncoder();
    const stream = new ReadableStream({
      start(c) {
        if (body) c.enqueue(enc.encode(body));
        if (end && body) c.close();
        init.signal?.addEventListener("abort", () => c.error(new Error("abort")));
      },
    });
    return new Response(stream, { status: 200 });
  });
  vi.stubGlobal("fetch", fn);
  return urls;
}

const tick = () => new Promise((r) => setTimeout(r, 30));
const rec = (seq: number, op: string, record: object) =>
  `id: ${seq}\nevent: record\ndata: ${JSON.stringify({ op, collection: "tasks", seq, record })}\n\n`;

afterEach(() => vi.unstubAllGlobals());

describe("realtime", () => {
  it("maps op to action and sends filter/ids as dedicated topics", async () => {
    const urls = sse([rec(5, "leave", { id: "a" })]);
    const client = createClient({ url: "http://x", app: "app" });
    const got: any[] = [];
    const off = client.collection("tasks").subscribe((e) => got.push(e), { filter: "done = false" });
    await tick();
    off();
    expect(got).toEqual([{ action: "leave", record: { id: "a" }, collection: "tasks", seq: 5 }]);
    const u = new URL(urls[0]);
    expect(u.searchParams.get("collections")).toBe("tasks");
    expect(u.searchParams.get("filter.tasks")).toBe("done = false");
  });

  it("onSnapshot merges snapshot, updates, leave and delete", async () => {
    const snap = `event: snapshot\ndata: ${JSON.stringify({
      collection: "tasks",
      seq: 3,
      items: [
        { id: "a", n: 2 },
        { id: "b", n: 1 },
      ],
      next: null,
    })}\n\n`;
    const urls = sse([
      snap + rec(4, "create", { id: "c", n: 0 }) + rec(5, "leave", { id: "a" }) + rec(6, "delete", { id: "b" }),
    ]);
    const client = createClient({ url: "http://x", app: "app" });
    const snaps: any[] = [];
    const off = client.collection("tasks").onSnapshot((s) => snaps.push(s), { filter: "n >= 0", sort: "n", limit: 5 });
    await tick();
    off();
    expect(snaps.map((s) => s.docs.map((d: any) => d.id))).toEqual([["b", "a"], ["c", "b", "a"], ["c", "b"], ["c"]]);
    expect(snaps[1].changes).toEqual([{ type: "added", doc: { id: "c", n: 0 } }]);
    expect(snaps[2].changes[0].type).toBe("removed");
    const u = new URL(urls[0]);
    expect(u.searchParams.get("snapshot")).toBe("5");
    expect(u.searchParams.get("sort.tasks")).toBe("n");
  });

  it("resumes a plain subscription with since after the stream ends", async () => {
    vi.useFakeTimers();
    const urls = sse([rec(9, "create", { id: "z" }), ""], true);
    const client = createClient({ url: "http://x", app: "app" });
    // First body ends right away: the SDK must reconnect with since=9.
    const off = client.collection("tasks").subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(1500);
    off();
    vi.useRealTimers();
    expect(urls.length).toBeGreaterThan(1);
    expect(new URL(urls[1]).searchParams.get("since")).toBe("9");
  });
});
