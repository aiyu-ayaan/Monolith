import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, type ApiError, type RequestLog } from "../api";
import { fmtUs } from "../charts";
import { Icon } from "../ui";
import ErrorBox from "./ErrorBox";
import { usePoll } from "./Analytics";

type Level = "all" | "errors" | "slow";

/** Recent requests of one app from the in-memory trace ring (doc 22): newest first. */
export default function Logs() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}`;
  const [logs, { refetch }] = createResource(base, (b) => api<{ items: RequestLog[] }>("GET", `${b}/logs?limit=500`));
  const [live, setLive] = usePoll(refetch);
  const [level, setLevel] = createSignal<Level>("all");
  const [q, setQ] = createSignal("");
  const [open, setOpen] = createSignal<string | null>(null);
  const rows = createMemo(() => {
    const t = q().trim().toLowerCase();
    return (ready(logs)?.items ?? []).filter(
      (l) =>
        (level() === "all" || (level() === "errors" ? l.status >= 400 : l.durationUs >= 100_000)) &&
        (!t || `${l.method} ${l.path} ${l.requestId}`.toLowerCase().includes(t)),
    );
  });

  return (
    <div class="page wide">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Logs</h1>
          <p class="muted">The last 2048 requests to this app. In memory only: cleared on restart.</p>
        </div>
        <div class="actions">
          <div class="seg" role="group" aria-label="Filter">
            <For each={[["all", "All"], ["errors", "Errors"], ["slow", "Slow (≥100 ms)"]] as [Level, string][]}>
              {([k, label]) => (
                <button class="seg-btn" aria-pressed={level() === k} onClick={() => setLevel(k)}>
                  {label}
                </button>
              )}
            </For>
          </div>
          <label class="check">
            <input type="checkbox" checked={live()} onChange={(e) => setLive(e.currentTarget.checked)} />
            Live (5 s)
          </label>
          <button class="icon-btn" aria-label="Refresh" title="Refresh" disabled={logs.loading} onClick={() => refetch()}>
            <Icon name="refresh" />
          </button>
        </div>
      </header>
      <div class="search wide-search">
        <Icon name="search" size={16} />
        <input type="search" aria-label="Filter logs" placeholder="Filter by method, path or request id" value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
      </div>
      <ErrorBox error={logs.error as ApiError | undefined} />
      <div class="table-wrap" aria-busy={logs.loading}>
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Time</th>
              <th scope="col">Request</th>
              <th scope="col" class="num">Status</th>
              <th scope="col" class="num">Duration</th>
              <th scope="col">Caller</th>
              <th scope="col">Request id</th>
            </tr>
          </thead>
          <tbody>
            <For each={rows()} fallback={<tr><td colspan="6" class="empty-row">{logs.loading ? "Loading…" : "No matching requests."}</td></tr>}>
              {(l) => (
                <>
                  <tr class="clickable" tabindex="0" onClick={() => setOpen(open() === l.requestId ? null : l.requestId)} onKeyDown={(e) => e.key === "Enter" && setOpen(open() === l.requestId ? null : l.requestId)}>
                    <td class="small nowrap">{new Date(l.timestamp).toLocaleTimeString()}</td>
                    <td class="mono small">
                      <strong>{l.method}</strong> {l.path}
                    </td>
                    <td class="num">
                      <span classList={{ "status-pill": true, bad: l.status >= 500, warn: l.status >= 400 && l.status < 500 }}>{l.status}</span>
                    </td>
                    <td class="num">{fmtUs(l.durationUs)}</td>
                    <td class="small">{l.superuser ? "superuser" : l.key ? `key ${l.key.slice(0, 8)}…` : "anonymous"}</td>
                    <td class="mono small">{l.requestId}</td>
                  </tr>
                  <Show when={open() === l.requestId}>
                    <tr>
                      <td colspan="6" class="log-detail">
                        <Show when={l.error} fallback={<span class="muted small">No error recorded. Use the request id to correlate with client logs (X-Request-Id).</span>}>
                          <span class="mono small">{l.error}</span>
                        </Show>
                      </td>
                    </tr>
                  </Show>
                </>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </div>
  );
}
