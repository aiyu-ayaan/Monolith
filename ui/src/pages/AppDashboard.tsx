import { createResource, createSignal, For, Show } from "solid-js";
import { api, enc, ready, type ApiError, type AppStats, type AuthConfig } from "../api";
import { fmtBytes, fmtNum } from "../charts";
import { fmtDate, Icon } from "../ui";
import ErrorBox from "./ErrorBox";
import { Tile, Toolbar, TrafficPanel, usePoll, type Range } from "./Analytics";

/** Project overview: numbers, traffic, collections and sign-in status in one screen. */
export default function AppDashboard(p: { app: string; editSchema: () => void }) {
  const base = () => `/api/v1/apps/${enc(p.app)}`;
  const [stats, { refetch }] = createResource(base, (b) => api<AppStats>("GET", `${b}/stats`));
  const [auth] = createResource(base, (b) => api<AuthConfig>("GET", `${b}/auth/config`));
  const [live, setLive] = usePoll(refetch);
  const [range, setRange] = createSignal<Range>("1h");
  const s = () => ready(stats);
  const a = () => ready(auth);
  const connected = () => (a()?.providers ?? []).filter((x) => x.enabled);
  const totalRecords = () => (s()?.collections ?? []).reduce((n, c) => n + (c.records ?? 0), 0);

  return (
    <div class="page wide">
      <header class="page-head">
        <div class="min0">
          <h1 class="truncate">{p.app}</h1>
          <p class="muted">Project overview{s() ? ` · loaded ${fmtDate(s()!.since)}` : ""}.</p>
        </div>
        <div class="actions">
          <button class="btn secondary" onClick={p.editSchema}>
            <Icon name="plus" />
            New collection
          </button>
          <Toolbar range={range()} setRange={setRange} live={live()} setLive={setLive} refetch={refetch} loading={stats.loading} />
        </div>
      </header>
      <ErrorBox error={stats.error as ApiError | undefined} />

      <Show when={s()}>
        {(s) => (
          <>
            <div class="tiles">
              <Tile label="Collections" value={fmtNum(s().collections.length)} sub={`schema v${s().schemaVersion}`} />
              <Tile label="Records" value={s().collections.some((c) => c.records !== null) ? `~${fmtNum(totalRecords())}` : "–"} sub="estimate from SQLite stats" />
              <Tile label="Storage" value={fmtBytes(s().dbBytes + s().walBytes)} sub={`${fmtBytes(s().dbBytes)} DB · ${fmtBytes(s().walBytes)} WAL`} />
              <Tile
                label="Sign-in methods"
                value={`${(a()?.emailPassword.enabled ? 1 : 0) + connected().length}`}
                sub={a() ? `${a()!.emailPassword.enabled ? "email/password" : "no email/password"} · ${connected().length} provider${connected().length === 1 ? "" : "s"}` : "…"}
              />
            </div>

            <div class="cards-2">
              <section class="card">
                <div class="card-head">
                  <div>
                    <h2 class="card-title">Collections</h2>
                    <p class="muted small">Tables with typed fields and access rules.</p>
                  </div>
                </div>
                <Show
                  when={s().collections.length}
                  fallback={
                    <div class="empty slim">
                      <Icon name="folder" size={28} />
                      <p class="muted">No collections yet.</p>
                      <button class="btn" onClick={p.editSchema}>
                        <Icon name="plus" />
                        New collection
                      </button>
                    </div>
                  }
                >
                  <ul class="rows">
                    <For each={s().collections}>
                      {(c) => (
                        <li>
                          <a class="row-link" href={`#/apps/${enc(p.app)}/c/${enc(c.name)}`}>
                            <Icon name="folder" size={16} />
                            <strong class="mono truncate">{c.name}</strong>
                            <span class="grow" />
                            <span class="muted small">{c.records === null ? "–" : `~${fmtNum(c.records)}`} records</span>
                            <span class="muted small">{fmtNum(c.reads)} reads</span>
                            <Icon name="chevronRight" size={16} />
                          </a>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </section>

              <section class="card">
                <div class="card-head">
                  <div>
                    <h2 class="card-title">Authentication</h2>
                    <p class="muted small">How users sign in to this app.</p>
                  </div>
                  <a class="btn secondary" href={`#/apps/${enc(p.app)}/auth`}>
                    <Icon name="key" size={16} />
                    Manage
                  </a>
                </div>
                <Show when={a()} fallback={<p class="muted small">{auth.loading ? "Loading…" : "Unavailable."}</p>}>
                  {(a) => (
                    <ul class="rows">
                      <li class="row-static">
                        <span class="avatar">@</span>
                        <strong>Email / password</strong>
                        <span class="grow" />
                        <span class="badge" classList={{ on: a().emailPassword.enabled }}>{a().emailPassword.enabled ? "Enabled" : "Disabled"}</span>
                      </li>
                      <For each={a().providers}>
                        {(pr) => (
                          <li class="row-static">
                            <span class="avatar">{pr.name[0]}</span>
                            <strong>{pr.name}</strong>
                            <span class="grow" />
                            <span class="badge" classList={{ on: pr.enabled }}>{pr.enabled ? "Enabled" : "Disabled"}</span>
                          </li>
                        )}
                      </For>
                      <Show when={!a().providers.length}>
                        <li class="muted small row-static">No social or OIDC providers connected yet.</li>
                      </Show>
                    </ul>
                  )}
                </Show>
              </section>
            </div>

            <TrafficPanel traffic={s().traffic} range={range()} />
          </>
        )}
      </Show>
    </div>
  );
}
