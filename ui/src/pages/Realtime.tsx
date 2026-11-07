import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, token, type ApiError, type AppStats } from "../api";
import { fmtNum } from "../charts";
import { Icon } from "../ui";
import ErrorBox from "./ErrorBox";
import { Tile, usePoll } from "./Analytics";
import { useApp } from "./AppLayout";

interface Ev {
  n: number;
  at: Date;
  kind: string;
  op?: string;
  collection?: string;
  id?: string;
  raw: string;
}

const MAX_EVENTS = 200;

/** Live monitor: subscribes to the app's change stream (SSE over fetch) and lists what arrives. */
export default function Realtime() {
  const params = useParams();
  const ctx = useApp();
  const base = () => `/api/v1/apps/${enc(params.app!)}`;
  const [stats, { refetch }] = createResource(base, (b) => api<AppStats>("GET", `${b}/stats`));
  usePoll(refetch);
  const rt = () => ready(stats)?.realtime;
  const names = () => (ready(ctx.collections)?.items ?? []).map((c) => c.name);
  const [picked, setPicked] = createSignal<string[] | null>(null);
  const chosen = () => picked() ?? names();
  const [state, setState] = createSignal<"idle" | "connecting" | "live">("idle");
  const [error, setError] = createSignal<ApiError | null>(null);
  const [events, setEvents] = createSignal<Ev[]>([]);
  const [open, setOpen] = createSignal<number | null>(null);
  let ctl: AbortController | null = null;
  let seq = 0;

  const push = (e: Omit<Ev, "n" | "at">) => setEvents((l) => [{ ...e, n: ++seq, at: new Date() }, ...l].slice(0, MAX_EVENTS));

  const handle = (block: string) => {
    let kind = "message";
    const data: string[] = [];
    for (const line of block.split("\n")) {
      if (line.startsWith("event: ")) kind = line.slice(7);
      else if (line.startsWith("data: ")) data.push(line.slice(6));
    }
    if (!data.length) return; // heartbeat comment
    const raw = data.join("\n");
    if (kind === "hello") return setState("live");
    try {
      const j = JSON.parse(raw);
      push({ kind, raw: JSON.stringify(j, null, 2), op: j.op, collection: j.collection, id: j.record?.id });
    } catch {
      push({ kind, raw });
    }
  };

  const disconnect = () => {
    ctl?.abort();
    ctl = null;
    setState("idle");
  };

  const connect = async () => {
    disconnect();
    setError(null);
    setState("connecting");
    ctl = new AbortController();
    const mine = ctl;
    try {
      const res = await fetch(`${base()}/realtime?collections=${chosen().map(enc).join(",")}`, {
        headers: { authorization: `Bearer ${token()}` },
        signal: mine.signal,
      });
      if (!res.ok || !res.body) {
        const problem = await res.json().catch(() => null);
        throw { problem: problem ?? { type: "http", title: `HTTP ${res.status}`, status: res.status } };
      }
      const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
      let buf = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += value;
        let i: number;
        while ((i = buf.indexOf("\n\n")) >= 0) {
          handle(buf.slice(0, i));
          buf = buf.slice(i + 2);
        }
      }
      // The server ended the stream (shutdown, or this client was too slow).
      if (ctl === mine) push({ kind: "closed", raw: "The server closed the stream. Reconnect to resume; refetch your data first." });
    } catch (e) {
      if (!mine.signal.aborted) setError(e as ApiError);
    } finally {
      if (ctl === mine) disconnect();
      refetch();
    }
  };
  onCleanup(disconnect);

  const toggle = (n: string) => setPicked(chosen().includes(n) ? chosen().filter((x) => x !== n) : [...chosen(), n]);

  return (
    <div class="page wide">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Realtime</h1>
          <p class="muted">Live record changes over Server-Sent Events. Clients connect to <code>GET {base()}/realtime?collections=…</code>.</p>
        </div>
      </header>
      <ErrorBox error={error() ?? (stats.error as ApiError | undefined)} />
      <div class="tiles">
        <Tile label="Open connections" value={rt() ? fmtNum(rt()!.connections) : "–"} sub="across all clients" />
        <Tile label="Events sent" value={rt() ? fmtNum(rt()!.events) : "–"} sub="since this app was loaded" />
        <Tile label="Dropped (slow)" value={rt() ? fmtNum(rt()!.dropped) : "–"} tone={rt() && rt()!.dropped > 0 ? "critical" : undefined} sub="queue of 256 frames full" />
      </div>

      <section class="card">
        <div class="card-head">
          <div>
            <h2 class="card-title">Live monitor</h2>
            <p class="muted small">Listens as you (a superuser): you see every row and hidden fields.</p>
          </div>
          <Show
            when={state() === "idle"}
            fallback={
              <button class="btn secondary" onClick={disconnect}>
                <Icon name="x" />
                {state() === "connecting" ? "Connecting…" : "Disconnect"}
              </button>
            }
          >
            <button class="btn" disabled={!chosen().length} onClick={connect}>
              <Icon name="radio" />
              Connect
            </button>
          </Show>
        </div>
        <div class="chips" role="group" aria-label="Collections to listen to">
          <For each={names()} fallback={<span class="muted small">Create a collection first.</span>}>
            {(n) => (
              <label class="check chip">
                <input type="checkbox" disabled={state() !== "idle"} checked={chosen().includes(n)} onChange={() => toggle(n)} />
                <span class="mono">{n}</span>
              </label>
            )}
          </For>
        </div>
        <div class="table-wrap flat">
          <table class="grid">
            <thead>
              <tr>
                <th scope="col">Time</th>
                <th scope="col">Event</th>
                <th scope="col">Collection</th>
                <th scope="col">Record</th>
              </tr>
            </thead>
            <tbody>
              <For each={events()} fallback={<tr><td colspan="4" class="empty-row">{state() === "live" ? "Connected. Waiting for changes…" : "Connect, then create, edit or delete a record."}</td></tr>}>
                {(e) => (
                  <>
                    <tr class="clickable" tabindex="0" onClick={() => setOpen(open() === e.n ? null : e.n)} onKeyDown={(k) => k.key === "Enter" && setOpen(open() === e.n ? null : e.n)}>
                      <td class="small nowrap">{e.at.toLocaleTimeString()}</td>
                      <td>
                        <span class="badge" classList={{ on: e.op === "create", "badge-warn": e.op === "update", "badge-bad": e.op === "delete" || e.kind === "closed" }}>{e.op ?? e.kind}</span>
                      </td>
                      <td class="mono small">{e.collection ?? "–"}</td>
                      <td class="mono small">{e.id ?? "–"}</td>
                    </tr>
                    <Show when={open() === e.n}>
                      <tr>
                        <td colspan="4" class="log-detail">
                          <pre class="json">{e.raw}</pre>
                        </td>
                      </tr>
                    </Show>
                  </>
                )}
              </For>
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
