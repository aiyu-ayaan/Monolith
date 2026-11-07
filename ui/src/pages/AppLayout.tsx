import { createContext, createResource, createSignal, For, Show, useContext, type Resource } from "solid-js";
import { useLocation, useNavigate, useParams, type RouteSectionProps } from "@solidjs/router";
import { api, enc, ready, type ApiError, type Collection } from "../api";
import { Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";
import SchemaEditor from "./SchemaEditor";
import AppDashboard from "./AppDashboard";

interface AppCtx {
  collections: Resource<{ items: Collection[] }>;
  /** Opens the schema drawer; `null` starts a new collection. */
  editSchema: (c: Collection | null) => void;
}

const Ctx = createContext<AppCtx>();
export const useApp = () => useContext(Ctx)!;

/** One app: collections sidebar + routed content (PocketBase layout). */
export default function AppLayout(props: RouteSectionProps) {
  const params = useParams();
  const loc = useLocation();
  const navigate = useNavigate();
  const [cols, { refetch }] = createResource(
    () => params.app,
    (app) => api<{ items: Collection[] }>("GET", `/api/v1/apps/${enc(app)}/collections`),
  );
  const [q, setQ] = createSignal("");
  const [editing, setEditing] = createSignal<{ c: Collection | null } | null>(null);
  const shown = () => (ready(cols)?.items ?? []).filter((c) => c.name.includes(q().trim().toLowerCase()));
  const appPath = () => `/apps/${enc(params.app!)}`;
  const onAnalytics = () => loc.pathname.endsWith("/analytics");
  const onAuth = () => loc.pathname.endsWith("/auth");
  const onOverview = () => loc.pathname === appPath() || loc.pathname === `${appPath()}/`;

  return (
    <Ctx.Provider value={{ collections: cols, editSchema: (c) => setEditing({ c }) }}>
      <div class="with-sidebar">
        <aside class="sidebar" aria-label="Collections">
          <a href="#/" class="back-link">
            <Icon name="chevronLeft" size={16} />
            All apps
          </a>
          <h2 class="sidebar-title">
            <Icon name="database" />
            <span class="truncate">{params.app}</span>
          </h2>
          <div class="search">
            <Icon name="search" size={16} />
            <input type="search" aria-label="Search collections" placeholder="Search collections" value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
          </div>
          <ErrorBox error={cols.error as ApiError | undefined} />
          <nav class="side-nav">
            <ul>
              <li>
                <a href={`#${appPath()}`} classList={{ active: onOverview() }} aria-current={onOverview() ? "page" : undefined}>
                  <Icon name="home" size={16} />
                  <span>Overview</span>
                </a>
              </li>
              <li>
                <a href={`#${appPath()}/auth`} classList={{ active: onAuth() }} aria-current={onAuth() ? "page" : undefined}>
                  <Icon name="key" size={16} />
                  <span>Authentication</span>
                </a>
              </li>
              <li>
                <a href={`#${appPath()}/analytics`} classList={{ active: onAnalytics() }} aria-current={onAnalytics() ? "page" : undefined}>
                  <Icon name="chart" size={16} />
                  <span>Analytics</span>
                </a>
              </li>
            </ul>
            <p class="side-label">Collections</p>
            <ul>
              <For each={shown()} fallback={<li class="muted small side-empty">{cols.loading ? "Loading…" : "No collections"}</li>}>
                {(c) => (
                  <li>
                    <a
                      href={`#${appPath()}/c/${enc(c.name)}`}
                      classList={{ active: params.coll === c.name }}
                      aria-current={params.coll === c.name ? "page" : undefined}
                    >
                      <Icon name="folder" size={16} />
                      <span class="truncate">{c.name}</span>
                    </a>
                  </li>
                )}
              </For>
            </ul>
          </nav>
          <button class="btn secondary block" onClick={() => setEditing({ c: null })}>
            <Icon name="plus" />
            New collection
          </button>
        </aside>
        <section class="content">{props.children}</section>
      </div>
      <Show when={editing()}>
        {(e) => (
          <SchemaEditor
            app={params.app!}
            initial={e().c}
            collections={ready(cols)?.items ?? []}
            onClose={() => setEditing(null)}
            onSaved={async (c) => {
              await refetch();
              setEditing(null);
              toast(`Saved "${c.name}" (schema v${c.version})`);
              navigate(`${appPath()}/c/${enc(c.name)}`);
            }}
            onDeleted={async (name) => {
              await refetch();
              setEditing(null);
              toast(`Deleted "${name}"`);
              navigate(appPath());
            }}
          />
        )}
      </Show>
    </Ctx.Provider>
  );
}

export function AppHome() {
  const ctx = useApp();
  const params = useParams();
  return <AppDashboard app={params.app!} editSchema={() => ctx.editSchema(null)} />;
}
