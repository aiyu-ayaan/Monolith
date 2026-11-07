import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { AREAS, search } from "../lib/content";

export default function Search(p: { open: boolean; onClose: () => void }) {
  const [q, setQ] = createSignal("");
  const [sel, setSel] = createSignal(0);
  const nav = useNavigate();
  let input!: HTMLInputElement;
  const hits = createMemo(() => search(q()));
  createEffect(() => {
    hits();
    setSel(0);
  });
  createEffect(() => {
    if (p.open) queueMicrotask(() => input?.focus());
    else setQ("");
  });
  const go = (path: string) => {
    p.onClose();
    nav(`/${path}`);
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") (e.preventDefault(), setSel(Math.min(sel() + 1, hits().length - 1)));
    else if (e.key === "ArrowUp") (e.preventDefault(), setSel(Math.max(sel() - 1, 0)));
    else if (e.key === "Enter" && hits()[sel()]) go(hits()[sel()].page.path);
    else if (e.key === "Escape") p.onClose();
  };
  return (
    <Show when={p.open}>
      <div class="search-back" onClick={p.onClose}>
        <div class="search" role="dialog" aria-modal="true" aria-label="Search the docs" onClick={(e) => e.stopPropagation()} onKeyDown={key}>
          <input
            ref={input}
            type="search"
            placeholder="Search the docs"
            aria-label="Search"
            value={q()}
            onInput={(e) => setQ(e.currentTarget.value)}
          />
          <ul role="listbox">
            <For each={hits()} fallback={<li class="none">{q() ? "No results." : "Type to search titles, headings and text."}</li>}>
              {(h, i) => (
                <li role="option" aria-selected={sel() === i()} classList={{ on: sel() === i() }} onMouseEnter={() => setSel(i())} onClick={() => go(h.page.path)}>
                  <strong>{h.page.title}</strong>
                  <span class="where">{[AREAS.find((a) => a.id === h.page.area)?.title, h.page.group].filter(Boolean).join(" · ")}</span>
                  <span class="ex">{h.excerpt}</span>
                </li>
              )}
            </For>
          </ul>
        </div>
      </div>
    </Show>
  );
}
