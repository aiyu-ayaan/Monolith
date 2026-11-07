import { createSignal, For, Show } from "solid-js";
import { useLocation } from "@solidjs/router";
import { enc, setToken, type Collection } from "../api";
import { Icon, type IconName } from "../ui";

interface Item {
  id: string;
  label: string;
  icon: IconName;
  /** Path under the project; omitted for services that are not built yet. */
  to?: string;
}

const AUTH: Item = { id: "auth", label: "Authentication", icon: "key", to: "/auth" };
const DB: Item = { id: "db", label: "Database", icon: "database" };
const ANALYTICS: Item = { id: "analytics", label: "Analytics", icon: "chart", to: "/analytics" };
const STORAGE: Item = { id: "storage", label: "Storage", icon: "storage", to: "/storage" };
const REALTIME: Item = { id: "realtime", label: "Realtime", icon: "radio", to: "/realtime" };

const SHORTCUTS = [AUTH, DB, STORAGE, REALTIME, ANALYTICS];

const CATEGORIES: { id: string; label: string; items: Item[] }[] = [
  { id: "data", label: "Database & storage", items: [DB, STORAGE, { id: "backups", label: "Backups", icon: "archive", to: "/backups" }] },
  { id: "security", label: "Security", items: [AUTH, { id: "rules", label: "Access rules", icon: "shield", to: "/rules" }, { id: "keys", label: "API keys", icon: "lock", to: "/keys" }] },
  { id: "sync", label: "Realtime & sync", items: [REALTIME, { id: "sync", label: "Offline sync", icon: "refresh" }] },
  { id: "extend", label: "Extensibility", items: [{ id: "hooks", label: "Webhooks", icon: "plug" }, { id: "plugins", label: "Plugins", icon: "plug" }] },
  { id: "observe", label: "Monitoring", items: [ANALYTICS, { id: "logs", label: "Logs", icon: "list", to: "/logs" }] },
];

/** Firebase-style project navigation: overview, shortcuts, product categories. Project scope only. */
export default function ProjectNav(p: {
  app: string;
  collections: Collection[];
  loading: boolean;
  activeColl?: string;
  onNewCollection: () => void;
}) {
  const loc = useLocation();
  const base = () => `/apps/${enc(p.app)}`;
  const [q, setQ] = createSignal("");
  const [openCats, setOpenCats] = createSignal<Record<string, boolean>>({});
  const [dbOpen, setDbOpen] = createSignal<boolean | null>(null);

  const href = (i: Item) => `#${base()}${i.to ?? ""}`;
  const active = (i: Item) => (i.to ? loc.pathname === `${base()}${i.to}` : i.id === "db" && p.activeColl !== undefined);
  const onOverview = () => loc.pathname === base() || loc.pathname === `${base()}/`;
  const catOpen = (c: (typeof CATEGORIES)[number]) => openCats()[c.id] ?? c.items.some(active);
  const showDb = () => dbOpen() ?? p.activeColl !== undefined;
  const term = () => q().trim().toLowerCase();
  const found = () => {
    const seen = new Set<string>();
    // The Database entry is replaced by the matching collections below.
    return CATEGORIES.flatMap((c) => c.items).filter((i) => i.id !== "db" && i.label.toLowerCase().includes(term()) && !seen.has(i.id) && seen.add(i.id));
  };
  const colls = () => p.collections.filter((c) => c.name.includes(term()));

  const Row = (r: { item: Item }) => (
    <Show
      when={r.item.id === "db"}
      fallback={
        <Show
          when={r.item.to}
          fallback={
            <span class="nav-item soon" aria-disabled="true" title="Planned: not available yet">
              <Icon name={r.item.icon} size={16} />
              <span>{r.item.label}</span>
              <span class="soon-tag">Soon</span>
            </span>
          }
        >
          <a class="nav-item" classList={{ active: active(r.item) }} href={href(r.item)} aria-current={active(r.item) ? "page" : undefined}>
            <Icon name={r.item.icon} size={16} />
            <span>{r.item.label}</span>
          </a>
        </Show>
      }
    >
      <button class="nav-item" classList={{ active: active(r.item) }} aria-expanded={showDb()} onClick={() => setDbOpen(!showDb())}>
        <Icon name="database" size={16} />
        <span>Database</span>
        <span class="grow" />
        <span class="chev" classList={{ open: showDb() }}>
          <Icon name="chevronDown" size={14} />
        </span>
      </button>
      <Show when={showDb()}>
        <ul class="nav-sub">
          <For each={p.collections} fallback={<li class="muted small side-empty">{p.loading ? "Loading…" : "No collections yet"}</li>}>
            {(c) => (
              <li>
                <a class="nav-item sub" classList={{ active: p.activeColl === c.name }} href={`#${base()}/c/${enc(c.name)}`} aria-current={p.activeColl === c.name ? "page" : undefined}>
                  <Icon name="folder" size={14} />
                  <span class="truncate">{c.name}</span>
                </a>
              </li>
            )}
          </For>
          <li>
            <button class="nav-item sub add" onClick={p.onNewCollection}>
              <Icon name="plus" size={14} />
              <span>New collection</span>
            </button>
          </li>
        </ul>
      </Show>
    </Show>
  );

  return (
    <aside class="sidebar pnav" aria-label="Project">
      <a href="#/" class="brand" aria-label="Monolith: all projects">
        <Icon name="logo" size={22} />
        <span>Monolith</span>
      </a>
      <a href="#/" class="back-link">
        <Icon name="chevronLeft" size={16} />
        All projects
      </a>
      <h2 class="project-name" title={p.app}>
        <span class="app-icon sm">
          <Icon name="database" size={16} />
        </span>
        <span class="truncate mono">{p.app}</span>
      </h2>
      <div class="search">
        <Icon name="search" size={16} />
        <input type="search" aria-label="Search products and collections" placeholder="Search for products" value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
      </div>

      <nav class="side-nav" aria-label="Project navigation">
        <Show
          when={!term()}
          fallback={
            <ul class="nav-list">
              <For each={found()}>{(i) => <li><Row item={i} /></li>}</For>
              <For each={colls()}>
                {(c) => (
                  <li>
                    <a class="nav-item" href={`#${base()}/c/${enc(c.name)}`}>
                      <Icon name="folder" size={16} />
                      <span class="truncate">{c.name}</span>
                    </a>
                  </li>
                )}
              </For>
              <Show when={!found().length && !colls().length}>
                <li class="muted small side-empty">No match.</li>
              </Show>
            </ul>
          }
        >
          <ul class="nav-list">
            <li>
              <a class="nav-item strong" classList={{ active: onOverview() }} href={`#${base()}`} aria-current={onOverview() ? "page" : undefined}>
                <Icon name="home" size={16} />
                <span>Project overview</span>
              </a>
            </li>
          </ul>

          <p class="nav-heading">Project shortcuts</p>
          <ul class="nav-list">
            <For each={SHORTCUTS}>{(i) => <li><Row item={i} /></li>}</For>
          </ul>

          <p class="nav-heading">Product categories</p>
          <ul class="nav-list">
            <For each={CATEGORIES}>
              {(c) => (
                <li>
                  <button class="nav-item cat" aria-expanded={catOpen(c)} onClick={() => setOpenCats({ ...openCats(), [c.id]: !catOpen(c) })}>
                    <span>{c.label}</span>
                    <span class="grow" />
                    <span class="chev" classList={{ open: catOpen(c) }}>
                      <Icon name="chevronDown" size={14} />
                    </span>
                  </button>
                  <Show when={catOpen(c)}>
                    <ul class="nav-sub flat">
                      <For each={c.items}>
                        {(i) => (
                          <li>
                            <Show
                              when={i.to || i.id === "db"}
                              fallback={
                                <span class="nav-item sub soon" aria-disabled="true" title="Planned: not available yet">
                                  <Icon name={i.icon} size={14} />
                                  <span>{i.label}</span>
                                  <span class="soon-tag">Soon</span>
                                </span>
                              }
                            >
                              <Show
                                when={i.to}
                                fallback={
                                  <button class="nav-item sub" onClick={() => setDbOpen(true)}>
                                    <Icon name={i.icon} size={14} />
                                    <span>{i.label}</span>
                                  </button>
                                }
                              >
                                <a class="nav-item sub" classList={{ active: active(i) }} href={href(i)} aria-current={active(i) ? "page" : undefined}>
                                  <Icon name={i.icon} size={14} />
                                  <span>{i.label}</span>
                                </a>
                              </Show>
                            </Show>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </nav>

      <footer class="side-foot">
        <div>
          <strong>Self-hosted</strong>
          <span class="muted small">Single container</span>
        </div>
        <button class="icon-btn" aria-label="Sign out" title="Sign out" onClick={() => setToken(null)}>
          <Icon name="logout" />
        </button>
      </footer>
    </aside>
  );
}
