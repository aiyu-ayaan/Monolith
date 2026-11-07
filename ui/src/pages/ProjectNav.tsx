import { createSignal, For, Show } from "solid-js";
import { useLocation } from "@solidjs/router";
import { enc, type Collection } from "../api";
import { Icon, type IconName } from "../ui";
import UserMenu from "./UserMenu";

interface Item {
  id: string;
  label: string;
  icon: IconName;
  /** Path under the project; omitted for services that are not built yet. */
  to?: string;
}

/** Each product appears once, in one group; no duplicated shortcuts. */
const GROUPS: { label: string; items: Item[] }[] = [
  {
    label: "Build",
    items: [
      { id: "db", label: "Database", icon: "database" },
      { id: "storage", label: "Storage", icon: "storage", to: "/storage" },
      { id: "auth", label: "Authentication", icon: "key", to: "/auth" },
      { id: "rules", label: "Access rules", icon: "shield", to: "/rules" },
    ],
  },
  {
    label: "Realtime & sync",
    items: [
      { id: "realtime", label: "Realtime", icon: "radio", to: "/realtime" },
      { id: "sync", label: "Offline sync", icon: "refresh" },
    ],
  },
  {
    label: "Monitor",
    items: [
      { id: "analytics", label: "Analytics", icon: "chart", to: "/analytics" },
      { id: "logs", label: "Logs", icon: "list", to: "/logs" },
      { id: "backups", label: "Backups", icon: "archive", to: "/backups" },
    ],
  },
  {
    label: "Extend",
    items: [
      { id: "hooks", label: "Webhooks", icon: "plug", to: "/webhooks" },
      { id: "plugins", label: "Plugins", icon: "plug" },
    ],
  },
];

/** Project navigation: overview, grouped products, collections under Database, settings and user at the bottom. */
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
  const [dbOpen, setDbOpen] = createSignal(true);

  const at = (to: string) => loc.pathname === `${base()}${to}` || loc.pathname.startsWith(`${base()}${to}/`);
  const onOverview = () => loc.pathname === base() || loc.pathname === `${base()}/`;
  const term = () => q().trim().toLowerCase();
  const matches = (i: Item) => i.label.toLowerCase().includes(term());
  const colls = () => p.collections.filter((c) => c.name.toLowerCase().includes(term()));

  const Link = (r: { item: Item }) => (
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
      {(to) => (
        <a class="nav-item" classList={{ active: at(to()) }} href={`#${base()}${to()}`} aria-current={at(to()) ? "page" : undefined}>
          <Icon name={r.item.icon} size={16} />
          <span>{r.item.label}</span>
        </a>
      )}
    </Show>
  );

  const CollLink = (r: { c: Collection }) => (
    <a
      class="nav-item sub"
      classList={{ active: p.activeColl === r.c.name }}
      href={`#${base()}/c/${enc(r.c.name)}`}
      aria-current={p.activeColl === r.c.name ? "page" : undefined}
    >
      <Icon name="folder" size={14} />
      <span class="truncate">{r.c.name}</span>
    </a>
  );

  const Database = () => (
    <>
      <button class="nav-item" aria-expanded={dbOpen()} onClick={() => setDbOpen(!dbOpen())}>
        <Icon name="database" size={16} />
        <span>Database</span>
        <span class="grow" />
        <span class="count muted small">{p.collections.length || ""}</span>
        <span class="chev" classList={{ open: dbOpen() }}>
          <Icon name="chevronDown" size={14} />
        </span>
      </button>
      <Show when={dbOpen()}>
        <ul class="nav-sub tree">
          <For each={p.collections} fallback={<li class="muted small side-empty">{p.loading ? "Loading…" : "No collections yet"}</li>}>
            {(c) => (
              <li>
                <CollLink c={c} />
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
    </>
  );

  return (
    <aside class="sidebar pnav" aria-label="Project">
      <div class="pnav-top">
        <a href="#/" class="brand" aria-label="Monolith: all projects">
          <Icon name="logo" size={22} />
          <span>Monolith</span>
        </a>
        <a href="#/" class="project-switch" title="Switch project">
          <span class="app-icon sm">
            <Icon name="database" size={14} />
          </span>
          <span class="min0">
            <span class="muted small">Project</span>
            <span class="truncate mono">{p.app}</span>
          </span>
          <Icon name="chevronRight" size={14} />
        </a>
        <div class="search">
          <Icon name="search" size={16} />
          <input type="search" aria-label="Search products and collections" placeholder="Search" value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
        </div>
      </div>

      <nav class="side-nav" aria-label="Project navigation">
        <Show
          when={!term()}
          fallback={
            <ul class="nav-list">
              <For each={GROUPS.flatMap((g) => g.items).filter((i) => i.id !== "db" && matches(i))}>{(i) => <li><Link item={i} /></li>}</For>
              <For each={colls()}>{(c) => <li><CollLink c={c} /></li>}</For>
              <Show when={!GROUPS.some((g) => g.items.some((i) => i.id !== "db" && matches(i))) && !colls().length}>
                <li class="muted small side-empty">No match.</li>
              </Show>
            </ul>
          }
        >
          <ul class="nav-list">
            <li>
              <a class="nav-item" classList={{ active: onOverview() }} href={`#${base()}`} aria-current={onOverview() ? "page" : undefined}>
                <Icon name="home" size={16} />
                <span>Project overview</span>
              </a>
            </li>
          </ul>
          <For each={GROUPS}>
            {(g) => (
              <section class="nav-group" aria-label={g.label}>
                <p class="nav-heading">{g.label}</p>
                <ul class="nav-list">
                  <For each={g.items}>{(i) => <li>{i.id === "db" ? <Database /> : <Link item={i} />}</li>}</For>
                </ul>
              </section>
            )}
          </For>
        </Show>
      </nav>

      <footer class="side-foot">
        <a class="nav-item" href="http://localhost:5180" target="_blank" rel="noopener noreferrer" title="Documentation">
          <Icon name="archive" size={16} />
          <span>Documentation ↗</span>
        </a>
        <a class="nav-item" classList={{ active: at("/settings") }} href={`#${base()}/settings`} aria-current={at("/settings") ? "page" : undefined}>
          <Icon name="gear" size={16} />
          <span>Project settings</span>
        </a>
        <UserMenu settingsHref={`#${base()}/settings/users`} />
      </footer>
    </aside>
  );
}
