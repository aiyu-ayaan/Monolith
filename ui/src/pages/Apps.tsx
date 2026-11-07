import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { api, enc, ready, type ApiError, type AppInfo, type SystemStats } from "../api";
import { fmtBytes, fmtNum, fmtUptime } from "../charts";
import { fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";
import { Tile, Toolbar, TrafficPanel, usePoll, type Range } from "./Analytics";

/** Landing dashboard: server health, traffic charts and every app with its own numbers. */
export default function Apps() {
  const [apps, { refetch: refetchApps }] = createResource(() => api<{ items: AppInfo[] }>("GET", "/api/v1/system/apps"));
  const [stats, { refetch: refetchStats }] = createResource(() => api<SystemStats>("GET", "/api/v1/system/stats"));
  const refetch = () => {
    refetchApps();
    refetchStats();
  };
  const [live, setLive] = usePoll(refetchStats);
  const [range, setRange] = createSignal<Range>("1h");
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const s = () => ready(stats);
  const perApp = createMemo(() => new Map((s()?.apps ?? []).map((a) => [a.name, a])));

  const run = async (f: () => Promise<unknown>, done: string) => {
    setError(null);
    setBusy(true);
    try {
      await f();
      refetch();
      toast(done);
    } catch (e) {
      setError(e as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const create = (e: SubmitEvent) => {
    e.preventDefault();
    const n = name();
    run(async () => {
      await api("POST", "/api/v1/system/apps", { name: n });
      setName("");
    }, `App "${n}" created`);
  };

  const remove = (app: string) => {
    if (!confirm(`Delete app "${app}" and all its data? This cannot be undone.`)) return;
    run(() => api("DELETE", `/api/v1/system/apps/${enc(app)}`), `App "${app}" deleted`);
  };

  return (
    <div class="page wide">
      <header class="page-head">
        <div>
          <h1>Dashboard</h1>
          <p class="muted">Your Monolith server: every app, its traffic and its health{s() ? ` · since ${fmtDate(s()!.since)}` : ""}.</p>
        </div>
        <Toolbar range={range()} setRange={setRange} live={live()} setLive={setLive} refetch={refetch} loading={stats.loading || apps.loading} />
      </header>

      <ErrorBox error={error() ?? (apps.error as ApiError | undefined) ?? (stats.error as ApiError | undefined)} />

      <Show when={s()}>
        {(s) => (
          <div class="tiles">
            <Tile label="Apps" value={`${s().appsTotal}`} sub={`${s().appsLoaded} loaded`} />
            <Tile label="Uptime" value={fmtUptime(s().uptimeS)} />
            <Tile label="Memory (RSS)" value={s().memoryBytes === null ? "n/a" : fmtBytes(s().memoryBytes!)} />
            <Tile label="Data on disk" value={fmtBytes(s().apps.reduce((a, x) => a + x.dbBytes + x.walBytes, 0))} sub="app DB + WAL files" />
          </div>
        )}
      </Show>

      <section class="card">
        <div class="card-head">
          <div>
            <h2 class="card-title">Apps</h2>
            <p class="muted small">Each app has its own database, collections, access rules and sign-in methods.</p>
          </div>
          <form class="create-bar inline" onSubmit={create}>
            <label for="app-name" class="sr-only">
              New app name
            </label>
            <input
              id="app-name"
              class="mono"
              placeholder="new_app_name"
              pattern="[a-z][a-z0-9_]{0,62}"
              title="Lowercase letters, digits and _, starting with a letter"
              required
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
            />
            <button class="btn" type="submit" disabled={busy()}>
              <Icon name="plus" />
              Create app
            </button>
          </form>
        </div>
        <Show
          when={ready(apps)}
          fallback={
            <Show when={!apps.error}>
              <div class="skeleton" aria-busy="true" aria-label="Loading apps" />
            </Show>
          }
        >
          <Show
            when={ready(apps)!.items.length}
            fallback={
              <div class="empty">
                <Icon name="database" size={32} />
                <h2>No apps yet</h2>
                <p class="muted">Create one above, then add collections and sign-in methods to it.</p>
              </div>
            }
          >
            <ul class="app-grid">
              <For each={ready(apps)!.items}>
                {(a) => {
                  const st = () => perApp().get(a.name);
                  return (
                    <li class="app-card rich">
                      <a class="app-link" href={`#/apps/${enc(a.name)}`}>
                        <span class="app-icon">
                          <Icon name="database" />
                        </span>
                        <span class="app-meta">
                          <strong>{a.name}</strong>
                          <span class="muted small">Created {fmtDate(a.created)}</span>
                        </span>
                        <Icon name="chevronRight" />
                      </a>
                      <dl class="app-stats">
                        <div>
                          <dt>Requests</dt>
                          <dd>{st()?.traffic ? fmtNum(st()!.traffic!.requests) : "–"}</dd>
                        </div>
                        <div>
                          <dt>Writes</dt>
                          <dd>{st()?.traffic ? fmtNum(st()!.traffic!.writes) : "–"}</dd>
                        </div>
                        <div>
                          <dt>Errors</dt>
                          <dd>{st()?.traffic ? fmtNum(st()!.traffic!.errors4xx + st()!.traffic!.errors5xx) : "–"}</dd>
                        </div>
                        <div>
                          <dt>Storage</dt>
                          <dd>{st() ? fmtBytes(st()!.dbBytes + st()!.walBytes) : "–"}</dd>
                        </div>
                      </dl>
                      <div class="app-actions">
                        <a class="btn secondary sm" href={`#/apps/${enc(a.name)}/auth`}>
                          <Icon name="key" size={14} />
                          Authentication
                        </a>
                        <a class="btn secondary sm" href={`#/apps/${enc(a.name)}/analytics`}>
                          <Icon name="chart" size={14} />
                          Analytics
                        </a>
                        <span class="grow" />
                        <button class="icon-btn danger" aria-label={`Delete app ${a.name}`} title="Delete app" onClick={() => remove(a.name)}>
                          <Icon name="trash" />
                        </button>
                      </div>
                    </li>
                  );
                }}
              </For>
            </ul>
          </Show>
        </Show>
      </section>

      <Show when={s()}>{(s) => <TrafficPanel traffic={s().traffic} range={range()} />}</Show>
    </div>
  );
}
