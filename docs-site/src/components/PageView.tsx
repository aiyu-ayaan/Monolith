import { createEffect, createMemo, on, Show } from "solid-js";
import { A } from "@solidjs/router";
import { AREAS, readingOrder, type Page } from "../lib/content";
import { render } from "../lib/markdown";
import { lang, setLang } from "../lib/prefs";
import Toc from "./Toc";

/** Applies the remembered language to every tab group: its tab when the group has it, else the first. */
function applyLang(root: HTMLElement, key: string | null) {
  for (const g of root.querySelectorAll<HTMLElement>("[data-tabs]")) {
    const tabs = [...g.querySelectorAll<HTMLButtonElement>(".tab")];
    const pick = tabs.find((t) => t.dataset.key === key) ?? g.querySelector<HTMLButtonElement>(".tab.on") ?? tabs[0];
    for (const t of tabs) {
      const on = t === pick;
      t.classList.toggle("on", on);
      t.setAttribute("aria-selected", String(on));
    }
    for (const p of g.querySelectorAll<HTMLElement>(".tab-panel")) p.hidden = p.dataset.key !== pick?.dataset.key;
  }
}

export default function PageView(p: { page: Page }) {
  let el!: HTMLDivElement;
  const out = createMemo(() => render(p.page.body));
  const idx = createMemo(() => {
    const all = readingOrder();
    const i = all.findIndex((x) => x.path === p.page.path);
    return { prev: all[i - 1], next: all[i + 1] };
  });
  const area = () => AREAS.find((a) => a.id === p.page.area)!;

  createEffect(
    on(
      () => p.page.path,
      () => {
        el.innerHTML = out().html;
        applyLang(el, lang());
        const hash = new URLSearchParams(location.hash.split("?")[1] ?? "").get("s");
        if (hash) el.querySelector(`#${CSS.escape(hash)}`)?.scrollIntoView();
        else window.scrollTo(0, 0);
      },
    ),
  );
  createEffect(() => {
    const l = lang();
    if (el) applyLang(el, l);
  });

  const onClick = async (e: MouseEvent) => {
    const t = e.target as HTMLElement;
    const tab = t.closest<HTMLButtonElement>(".tab");
    if (tab?.dataset.key) {
      setLang(tab.dataset.key);
      return;
    }
    const copy = t.closest<HTMLButtonElement>(".copy");
    if (copy) {
      const code = copy.parentElement?.querySelector("code")?.textContent ?? "";
      try {
        await navigator.clipboard.writeText(code);
        copy.textContent = "Copied";
        setTimeout(() => (copy.textContent = "Copy"), 1500);
      } catch {
        copy.textContent = "Select and copy";
      }
      return;
    }
    const a = t.closest<HTMLAnchorElement>("a[data-anchor]");
    if (a) {
      e.preventDefault();
      const id = a.dataset.anchor!;
      history.replaceState(null, "", `#/${p.page.path}?s=${id}`);
      document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
    }
  };

  return (
    <div class="doc-layout">
      <article class="doc" aria-labelledby="doc-title">
        <nav class="crumbs" aria-label="Breadcrumb">
          <A href={`/${p.page.area}`}>{area().title}</A>
          <Show when={p.page.group}>
            <span aria-hidden="true">/</span>
            <span>{p.page.group}</span>
          </Show>
        </nav>
        <h1 id="doc-title">{p.page.title}</h1>
        <Show when={p.page.description}>
          <p class="lead">{p.page.description}</p>
        </Show>
        <div class="prose" ref={(n) => (el = n)} onClick={onClick} />
        <nav class="pager" aria-label="Pagination">
          <Show when={idx().prev} fallback={<span />}>
            {(x) => (
              <A class="pager-link prev" href={`/${x().path}`}>
                <small>Previous</small>
                {x().title}
              </A>
            )}
          </Show>
          <Show when={idx().next}>
            {(x) => (
              <A class="pager-link next" href={`/${x().path}`}>
                <small>Next</small>
                {x().title}
              </A>
            )}
          </Show>
        </nav>
      </article>
      <Toc headings={out().headings} />
    </div>
  );
}
