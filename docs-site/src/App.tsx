import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { A, HashRouter, Route, useLocation, useNavigate } from "@solidjs/router";
import { AREAS, firstPage, navFor, pageFor, PAGES, type AreaId } from "./lib/content";
import { applyTheme, setTheme, theme } from "./lib/prefs";
import PageView from "./components/PageView";
import Search from "./components/Search";

function Header(p: { onSearch: () => void; onMenu: () => void }) {
  const loc = useLocation();
  const area = () => loc.pathname.split("/")[1];
  const next = () => (theme() === "dark" ? "light" : theme() === "light" ? "system" : "dark");
  return (
    <header class="top">
      <button class="icon menu" type="button" aria-label="Open navigation" onClick={p.onMenu}>
        ☰
      </button>
      <A href="/" class="brand" aria-label="Monolith documentation home">
        <img class="logo logo-light" src="/logo-light.svg" alt="" height="26" />
        <img class="logo logo-dark" src="/logo-dark.svg" alt="" height="26" />
        <span class="docs-tag">Docs</span>
      </A>
      <nav class="areas" aria-label="Sections">
        <For each={AREAS}>
          {(a) => (
            <A href={`/${firstPage(a.id)?.path ?? a.id}`} classList={{ on: area() === a.id }} aria-current={area() === a.id ? "page" : undefined}>
              {a.title}
            </A>
          )}
        </For>
      </nav>
      <div class="tools">
        <button class="search-btn" type="button" onClick={p.onSearch} aria-label="Search the docs">
          Search <kbd>/</kbd>
        </button>
        <button class="icon" type="button" onClick={() => setTheme(next())} aria-label={`Theme: ${theme()}. Switch to ${next()}`} title={`Theme: ${theme()}`}>
          {theme() === "dark" ? "☾" : theme() === "light" ? "☀" : "◐"}
        </button>
        <a class="console" href="/_/">Console</a>
      </div>
    </header>
  );
}

function SideNav(p: { area: AreaId; open: boolean; onClose: () => void }) {
  const loc = useLocation();
  const groups = createMemo(() => navFor(p.area));
  return (
    <>
      <Show when={p.open}>
        <div class="scrim" onClick={p.onClose} />
      </Show>
      <nav class="side" classList={{ open: p.open }} aria-label="Documentation">
        <For each={groups()}>
          {(g) => (
            <section>
              <Show when={g.title}>
                <h2>{g.title}</h2>
              </Show>
              <ul>
                <For each={g.pages}>
                  {(pg) => (
                    <li>
                      <A href={`/${pg.path}`} classList={{ on: loc.pathname === `/${pg.path}` }} aria-current={loc.pathname === `/${pg.path}` ? "page" : undefined} onClick={p.onClose}>
                        {pg.title}
                      </A>
                    </li>
                  )}
                </For>
              </ul>
            </section>
          )}
        </For>
      </nav>
    </>
  );
}

function Home() {
  return (
    <main class="home">
      <h1>Monolith documentation</h1>
      <p class="lead">Build your backend on one container: database with access rules, authentication, realtime, files, webhooks and plugins.</p>
      <div class="cards">
        <For each={AREAS}>
          {(a) => {
            const pages = () => PAGES.filter((p) => p.area === a.id);
            return (
              <A class="card" href={`/${firstPage(a.id)?.path ?? a.id}`}>
                <h2>{a.title}</h2>
                <p>{{ "get-started": "What Monolith is, the concepts, and your first app in a few minutes.", build: "Database, authentication, storage, extending the server and running it.", sdks: "Client libraries for web, Android, Flutter, Python and Rust.", reference: "Exact APIs, protocols, limits and configuration." }[a.id]}</p>
                <small>{pages().length} pages</small>
              </A>
            );
          }}
        </For>
      </div>
    </main>
  );
}

function Doc() {
  const loc = useLocation();
  const page = createMemo(() => pageFor(loc.pathname));
  const area = (): AreaId => (page()?.area ?? (loc.pathname.split("/")[1] as AreaId));
  const [open, setOpen] = createSignal(false);
  createEffect(() => {
    loc.pathname;
    setOpen(false);
  });
  return (
    <div class="shell">
      <SideNav area={area()} open={open()} onClose={() => setOpen(false)} />
      <Show
        when={page()}
        fallback={
          <main class="doc missing">
            <h1>Page not found</h1>
            <p>
              <A href="/">Go to the documentation home</A> or search with <kbd>/</kbd>.
            </p>
          </main>
        }
      >
        {(pg) => <PageView page={pg()} />}
      </Show>
    </div>
  );
}

function Layout(p: { children?: any }) {
  const [searching, setSearching] = createSignal(false);
  const [menu, setMenu] = createSignal(false);
  const nav = useNavigate();
  onMount(() => {
    applyTheme();
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      const typing = t.matches?.("input, textarea, select") || t.isContentEditable;
      if ((e.key === "/" && !typing) || ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k")) {
        e.preventDefault();
        setSearching(true);
      }
    };
    window.addEventListener("keydown", k);
    onCleanup(() => window.removeEventListener("keydown", k));
  });
  void nav;
  return (
    <>
      <a class="skip" href="#main">Skip to content</a>
      <Header onSearch={() => setSearching(true)} onMenu={() => setMenu(!menu())} />
      <div id="main">{p.children}</div>
      <Search open={searching()} onClose={() => setSearching(false)} />
    </>
  );
}

export default function App() {
  return (
    <HashRouter root={Layout}>
      <Route path="/" component={Home} />
      <Route path="/*" component={Doc} />
    </HashRouter>
  );
}
