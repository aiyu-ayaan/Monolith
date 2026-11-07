import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import type { Heading } from "../lib/markdown";

/** "On this page" with scroll tracking. */
export default function Toc(p: { headings: Heading[] }) {
  const [active, setActive] = createSignal("");
  createEffect(() => {
    const hs = p.headings;
    setActive(hs[0]?.id ?? "");
    if (!hs.length) return;
    const obs = new IntersectionObserver(
      (entries) => {
        const vis = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
        if (vis) setActive(vis.target.id);
      },
      { rootMargin: "-72px 0px -70% 0px" },
    );
    queueMicrotask(() => {
      for (const h of hs) {
        const n = document.getElementById(h.id);
        if (n) obs.observe(n);
      }
    });
    onCleanup(() => obs.disconnect());
  });
  return (
    <Show when={p.headings.length > 1}>
      <aside class="toc" aria-label="On this page">
        <h2>On this page</h2>
        <ul>
          <For each={p.headings}>
            {(h) => (
              <li class={`l${h.level}`}>
                <a
                  href={`#${h.id}`}
                  classList={{ on: active() === h.id }}
                  onClick={(e) => {
                    e.preventDefault();
                    document.getElementById(h.id)?.scrollIntoView({ behavior: "smooth" });
                    setActive(h.id);
                  }}
                >
                  {h.text}
                </a>
              </li>
            )}
          </For>
        </ul>
      </aside>
    </Show>
  );
}
