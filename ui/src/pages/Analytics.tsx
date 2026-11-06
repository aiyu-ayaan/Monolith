import { createMemo, createResource, createSignal, For, onCleanup, Show, type JSX } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, type ApiError, type AppStats, type RequestLog, type SystemStats, type Traffic, type TrafficPoint, type TrafficSeries } from "../api";
import { fmtBytes, fmtClock, fmtHour, fmtNum, fmtUptime, fmtUs, LineChart } from "../charts";
import { fmtDate, Icon } from "../ui";
import ErrorBox from "./ErrorBox";

type Range = "1h" | "48h";
const POLL_MS = 5000;

/** Refetches every 5 s while on and the tab is visible. */
function usePoll(refetch: () => void) {
  const [on, setOn] = createSignal(true);
  const id = setInterval(() => on() && document.visibilityState === "visible" && refetch(), POLL_MS);
  onCleanup(() => clearInterval(id));
  return [on, setOn] as const;
}

/** Sums buckets; latency is request-weighted avg and worst max. */
function sumRange(pts: TrafficPoint[]): Traffic {
  const t: Traffic = { requests: 0, reads: 0, writes: 0, creates: 0, updates: 0, deletes: 0, errors4xx: 0, errors5xx: 0, bytesIn: 0, bytesOut: 0, avgUs: 0, maxUs: 0, p50Us: 0, p95Us: 0, p99Us: 0 };
  let dur = 0;
  for (const p of pts) {
    for (const k of ["requests", "reads", "writes", "creates", "updates", "deletes", "errors4xx", "errors5xx", "bytesIn", "bytesOut"] as const) t[k] += p[k];
    dur += p.avgUs * p.requests;
    t.maxUs = Math.max(t.maxUs, p.maxUs);
  }
  t.avgUs = t.requests ? dur / t.requests : 0;
  return t;
}

const pct = (a: number, b: number) => (b ? `${+((a / b) * 100).toFixed(2)}%` : "0%");

function Tile(p: { label: string; value: string; sub?: JSX.Element; tone?: "critical" }) {
  return (
    <div class="tile">
      <p class="tile-label">{p.label}</p>
      <p class="tile-value" classList={{ critical: p.tone === "critical" }}>
        <Show when={p.tone === "critical"}>
          <Icon name="alert" size={16} />
        </Show>
        {p.value}
      </p>
      <Show when={p.sub}>
        <p class="tile-sub">{p.sub}</p>
      </Show>
    </div>
  );
}

function Toolbar(p: { range: Range; setRange: (r: Range) => void; live: boolean; setLive: (b: boolean) => void; refetch: () => void; loading: boolean }) {
  return (
    <div class="actions">
      <div class="seg" role="group" aria-label="Time range">
        <For each={["1h", "48h"] as Range[]}>
          {(r) => (
            <button class="seg-btn" aria-pressed={p.range === r} onClick={() => p.setRange(r)}>
              {r === "1h" ? "Last hour" : "Last 48 hours"}
            </button>
          )}
        </For>
      </div>
      <label class="check">
        <input type="checkbox" checked={p.live} onChange={(e) => p.setLive(e.currentTarget.checked)} />
        Live (5 s)
      </label>
      <button class="icon-btn" aria-label="Refresh" title="Refresh" disabled={p.loading} onClick={() => p.refetch()}>
        <Icon name="refresh" />
      </button>
    </div>
  );
}

/** Tiles + charts for one traffic series, scoped to the selected range. */
function TrafficPanel(p: { traffic: TrafficSeries; range: Range }) {
  const pts = () => (p.range === "1h" ? p.traffic.minutes : p.traffic.hours);
  const r = createMemo(() => sumRange(pts()));
  const tf = () => (p.range === "1h" ? fmtClock : fmtHour);
  const errs = () => r().errors4xx + r().errors5xx;
  const total = () => p.traffic.total;
  return (
    <>
      <div class="tiles">
        <Tile label="Requests" value={fmtNum(r().requests)} sub={`${fmtNum(total().requests)} since start`} />
        <Tile label="Reads" value={fmtNum(r().reads)} sub="record GET requests" />
        <Tile label="Writes" value={fmtNum(r().writes)} sub="record POST/PATCH/PUT/DELETE + batch" />
        <Tile
          label="Record changes"
          value={fmtNum(r().creates + r().updates + r().deletes)}
          sub={`${fmtNum(r().creates)} created · ${fmtNum(r().updates)} updated · ${fmtNum(r().deletes)} deleted`}
        />
        <Tile
          label="Errors"
          value={fmtNum(errs())}
          tone={r().errors5xx > 0 ? "critical" : undefined}
          sub={`${pct(errs(), r().requests)} · ${fmtNum(r().errors4xx)} 4xx · ${fmtNum(r().errors5xx)} 5xx`}
        />
        <Tile label="Avg latency" value={fmtUs(r().avgUs)} sub={`max ${fmtUs(r().maxUs)}`} />
        <Tile
          label="Latency since start"
          value={`p95 ${fmtUs(total().p95Us)}`}
          sub={`p50 ${fmtUs(total().p50Us)} · p99 ${fmtUs(total().p99Us)} (approx.)`}
        />
        <Tile label="Data" value={`${fmtBytes(r().bytesOut)} out`} sub={`${fmtBytes(r().bytesIn)} in`} />
      </div>
      <div class="charts">
        <LineChart
          title="Requests"
          points={pts()}
          fmt={fmtNum}
          tfmt={tf()}
          series={[
            { label: "All", color: "var(--series-1)", value: (x) => x.requests },
            { label: "Reads", color: "var(--series-2)", value: (x) => x.reads },
            { label: "Writes", color: "var(--series-3)", value: (x) => x.writes },
          ]}
        />
        <LineChart
          title="Record changes"
          points={pts()}
          fmt={fmtNum}
          tfmt={tf()}
          series={[
            { label: "Created", color: "var(--series-1)", value: (x) => x.creates },
            { label: "Updated", color: "var(--series-2)", value: (x) => x.updates },
            { label: "Deleted", color: "var(--series-3)", value: (x) => x.deletes },
          ]}
        />
        <LineChart
          title="Latency"
          points={pts()}
          fmt={fmtUs}
          tfmt={tf()}
          series={[
            { label: "Avg", color: "var(--series-1)", value: (x) => x.avgUs },
            { label: "p95", color: "var(--series-2)", value: (x) => x.p95Us },
          ]}
        />
        <LineChart
          title="Errors"
          points={pts()}
          fmt={fmtNum}
          tfmt={tf()}
          series={[
            { label: "4xx", color: "var(--status-warning)", value: (x) => x.errors4xx },
            { label: "5xx", color: "var(--status-critical)", value: (x) => x.errors5xx },
          ]}
        />
      </div>
    </>
  );
}

// ---------- global ----------

export function SystemAnalytics() {
  const [stats, { refetch }] = createResource(() => api<SystemStats>("GET", "/api/v1/system/stats"));
  const [range, setRange] = createSignal<Range>("1h");
  const [live, setLive] = usePoll(refetch);
  const s = () => stats.latest;

  return (
    <div class="page wide">
      <header class="page-head">
        <div>
          <h1>Analytics</h1>
          <p class="muted">All apps on this server. In memory since the last restart{s() ? ` (${fmtDate(s()!.since)})` : ""}.</p>
        </div>
        <Toolbar range={range()} setRange={setRange} live={live()} setLive={setLive} refetch={refetch} loading={stats.loading} />
      </header>
      <ErrorBox error={stats.error as ApiError | undefined} />
      <Show when={s()}>
        {(s) => (
          <div class="analytics" aria-busy={stats.loading}>
            <div class="tiles">
              <Tile label="Uptime" value={fmtUptime(s().uptimeS)} />
              <Tile label="Memory (RSS)" value={s().memoryBytes === null ? "n/a" : fmtBytes(s().memoryBytes!)} />
              <Tile label="Apps" value={`${s().appsTotal}`} sub={`${s().appsLoaded} loaded`} />
              <Tile label="Data on disk" value={fmtBytes(s().apps.reduce((a, x) => a + x.dbBytes + x.walBytes, 0))} sub="app DB + WAL files" />
            </div>
            <TrafficPanel traffic={s().traffic} range={range()} />
            <section class="card">
              <h2 class="card-title">Apps</h2>
              <p class="muted small">Totals since start. An app loads on its first request.</p>
              <div class="table-wrap flat">
                <table class="grid">
                  <thead>
                    <tr>
                      <th scope="col">App</th>
                      <th scope="col" class="num">Requests</th>
                      <th scope="col" class="num">Reads</th>
                      <th scope="col" class="num">Writes</th>
                      <th scope="col" class="num">Created</th>
                      <th scope="col" class="num">Updated</th>
                      <th scope="col" class="num">Deleted</th>
                      <th scope="col" class="num">Errors</th>
                      <th scope="col" class="num">p95</th>
                      <th scope="col" class="num">DB</th>
                      <th scope="col" class="num">WAL</th>
                    </tr>
                  </thead>
                  <tbody>
                    <For each={s().apps} fallback={<tr><td colspan="11" class="empty-row">No apps yet.</td></tr>}>
                      {(a) => (
                        <tr>
                          <td>
                            <a class="mono link" href={`#/apps/${enc(a.name)}/analytics`}>
                              {a.name}
                            </a>
                            <Show when={!a.loaded}>
                              <span class="type-tag">idle</span>
                            </Show>
                          </td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.requests) : "–"}</td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.reads) : "–"}</td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.writes) : "–"}</td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.creates) : "–"}</td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.updates) : "–"}</td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.deletes) : "–"}</td>
                          <td class="num">{a.traffic ? fmtNum(a.traffic.errors4xx + a.traffic.errors5xx) : "–"}</td>
                          <td class="num">{a.traffic ? fmtUs(a.traffic.p95Us) : "–"}</td>
                          <td class="num">{fmtBytes(a.dbBytes)}</td>
                          <td class="num">{fmtBytes(a.walBytes)}</td>
                        </tr>
                      )}
                    </For>
                  </tbody>
                </table>
              </div>
            </section>
          </div>
        )}
      </Show>
    </div>
  );
}

// ---------- per app ----------

function KV(p: { rows: [string, string, string?][] }) {
  return (
    <dl class="kv">
      <For each={p.rows}>
        {([k, v, hint]) => (
          <>
            <dt title={hint}>{k}</dt>
            <dd>{v}</dd>
          </>
        )}
      </For>
    </dl>
  );
}

function LogTable(p: { items: RequestLog[] | undefined; empty: string }) {
  return (
    <div class="table-wrap flat">
      <table class="grid">
        <thead>
          <tr>
            <th scope="col">Time</th>
            <th scope="col">Request</th>
            <th scope="col" class="num">Status</th>
            <th scope="col" class="num">Duration</th>
            <th scope="col">Request id</th>
          </tr>
        </thead>
        <tbody>
          <For each={p.items} fallback={<tr><td colspan="5" class="empty-row">{p.empty}</td></tr>}>
            {(l) => (
              <tr>
                <td class="nowrap">{fmtDate(l.timestamp)}</td>
                <td class="mono cell-path" title={l.error ?? undefined}>
                  {l.method} {l.path}
                  <Show when={l.error}>
                    <span class="muted small block">{l.error}</span>
                  </Show>
                </td>
                <td class="num">
                  <span classList={{ "status-pill": true, bad: l.status >= 500, warn: l.status >= 400 && l.status < 500 }}>{l.status}</span>
                </td>
                <td class="num">{fmtUs(l.durationUs)}</td>
                <td class="mono small">{l.requestId}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
}

export function AppAnalytics() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}`;
  const [stats, { refetch }] = createResource(base, (b) => api<AppStats>("GET", `${b}/stats`));
  const [slow, { refetch: refetchSlow }] = createResource(base, (b) => api<{ items: RequestLog[] }>("GET", `${b}/logs/slow?limit=10`));
  const [logs, { refetch: refetchLogs }] = createResource(base, (b) => api<{ items: RequestLog[] }>("GET", `${b}/logs?limit=500`));
  const [exact, setExact] = createSignal<{ at: Date; counts: Map<string, number | null> } | null>(null);
  const [counting, setCounting] = createSignal(false);
  const [range, setRange] = createSignal<Range>("1h");
  const refreshAll = () => {
    refetch();
    refetchSlow();
    refetchLogs();
  };
  const [live, setLive] = usePoll(refreshAll);
  const s = () => stats.latest;
  const errors = () => (logs.latest?.items ?? []).filter((l) => l.status >= 400).slice(0, 10);

  // COUNT(*) per collection: on demand only, never on the 5 s poll.
  const countExact = async () => {
    setCounting(true);
    try {
      const r = await api<AppStats>("GET", `${base()}/stats?counts=exact`);
      setExact({ at: new Date(), counts: new Map(r.collections.map((c) => [c.name, c.records])) });
    } finally {
      setCounting(false);
    }
  };
  const records = (name: string, est: number | null) => {
    const e = exact()?.counts.get(name);
    if (e !== undefined && e !== null) return fmtNum(e);
    return est === null ? "–" : `~${fmtNum(est)}`;
  };

  return (
    <div class="page wide">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href="#/analytics">Analytics</a>
            <span aria-hidden="true"> / </span>
            {params.app}
          </p>
          <h1 class="truncate">App analytics</h1>
          <p class="muted">Since this app was loaded{s() ? ` (${fmtDate(s()!.since)})` : ""}; resets on restart.</p>
        </div>
        <Toolbar range={range()} setRange={setRange} live={live()} setLive={setLive} refetch={refreshAll} loading={stats.loading} />
      </header>
      <ErrorBox error={stats.error as ApiError | undefined} />
      <Show when={s()}>
        {(s) => {
          const st = () => s().store;
          return (
            <div class="analytics" aria-busy={stats.loading}>
              <TrafficPanel traffic={s().traffic} range={range()} />
              <div class="cards-2">
                <section class="card">
                  <h2 class="card-title">Storage engine</h2>
                  <p class="muted small">Writer thread with group commit, reader pool (since the app was opened).</p>
                  <KV
                    rows={[
                      ["Commits", fmtNum(st().commits), "Group-commit transactions"],
                      ["Write ops", `${fmtNum(st().ops)} (${fmtNum(st().opsFailed)} failed)`],
                      ["Rows changed", fmtNum(st().mutations), "Committed seq values in _changes"],
                      ["Avg batch", st().commits ? (st().ops / st().commits).toFixed(2) : "0", "Write ops per commit"],
                      ["Max batch", fmtNum(st().maxBatch)],
                      ["Commit latency", `${fmtUs(st().commitAvgUs)} avg · ${fmtUs(st().commitMaxUs)} max`],
                      ["Read jobs", fmtNum(st().reads)],
                      ["Read latency", `${fmtUs(st().readAvgUs)} avg`],
                      ["Queues", `writer ${st().writerQueue} · reader ${st().readerQueue}`, "Current depth"],
                      ["Readers", `${st().readers}`],
                      ["Busy (503)", fmtNum(st().busy), "Rejected because a queue was full"],
                    ]}
                  />
                </section>
                <section class="card">
                  <h2 class="card-title">Storage</h2>
                  <KV
                    rows={[
                      ["Database", fmtBytes(s().dbBytes)],
                      ["WAL", fmtBytes(s().walBytes)],
                      ["Schema version", `${s().schemaVersion}`],
                      ["Collections", `${s().collections.length}`],
                    ]}
                  />
                </section>
              </div>
              <section class="card">
                <div class="card-head">
                  <div>
                    <h2 class="card-title">Collections</h2>
                    <p class="muted small">
                      {exact() ? `Exact counts from ${exact()!.at.toLocaleTimeString()}.` : "~ = estimate from SQLite stats; – = not analyzed yet."}
                    </p>
                  </div>
                  <button class="btn secondary" disabled={counting()} onClick={countExact}>
                    {counting() ? "Counting…" : "Count records"}
                  </button>
                </div>
                <div class="table-wrap flat">
                  <table class="grid">
                    <thead>
                      <tr>
                        <th scope="col">Collection</th>
                        <th scope="col" class="num">Records</th>
                        <th scope="col" class="num">Reads</th>
                        <th scope="col" class="num">Created</th>
                        <th scope="col" class="num">Updated</th>
                        <th scope="col" class="num">Deleted</th>
                      </tr>
                    </thead>
                    <tbody>
                      <For each={s().collections} fallback={<tr><td colspan="6" class="empty-row">No collections yet.</td></tr>}>
                        {(c) => (
                          <tr>
                            <td>
                              <a class="mono link" href={`#/apps/${enc(params.app!)}/c/${enc(c.name)}`}>
                                {c.name}
                              </a>
                            </td>
                            <td class="num">{records(c.name, c.records)}</td>
                            <td class="num">{fmtNum(c.reads)}</td>
                            <td class="num">{fmtNum(c.creates)}</td>
                            <td class="num">{fmtNum(c.updates)}</td>
                            <td class="num">{fmtNum(c.deletes)}</td>
                          </tr>
                        )}
                      </For>
                    </tbody>
                  </table>
                </div>
              </section>
              <section class="card">
                <h2 class="card-title">Slowest requests</h2>
                <p class="muted small">From the last 2048 requests to this app.</p>
                <LogTable items={slow.latest?.items} empty="No requests yet." />
              </section>
              <section class="card">
                <h2 class="card-title">Recent errors</h2>
                <LogTable items={errors()} empty="No errors in the recent requests." />
              </section>
            </div>
          );
        }}
      </Show>
    </div>
  );
}
