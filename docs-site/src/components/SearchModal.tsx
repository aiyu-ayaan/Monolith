import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import { DOC_PAGES } from "../data/pages";

export default function SearchModal(props: {
  isOpen: boolean;
  onClose: () => void;
  onSelect: (pageId: string, sectionId?: string) => void;
}) {
  const [query, setQuery] = createSignal("");
  let inputRef: HTMLInputElement | undefined;

  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        if (props.isOpen) props.onClose();
        else {
          // Open search modal via parent
        }
      }
      if (e.key === "Escape" && props.isOpen) {
        props.onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    onCleanup(() => window.removeEventListener("keydown", handleKeyDown));
  });

  const searchResults = () => {
    const q = query().trim().toLowerCase();
    if (!q) return [];

    const results: { pageId: string; sectionId?: string; title: string; desc: string }[] = [];

    for (const [pageId, page] of Object.entries(DOC_PAGES)) {
      if (page.title.toLowerCase().includes(q) || page.overview.toLowerCase().includes(q)) {
        results.push({
          pageId,
          title: page.title,
          desc: page.overview.slice(0, 90) + "...",
        });
      }

      for (const sec of page.sections) {
        if (sec.title.toLowerCase().includes(q) || sec.content.toLowerCase().includes(q)) {
          results.push({
            pageId,
            sectionId: sec.id,
            title: `${page.title} › ${sec.title}`,
            desc: sec.content.slice(0, 90) + "...",
          });
        }
      }
    }

    return results;
  };

  return (
    <Show when={props.isOpen}>
      <div
        class="modal-overlay"
        onClick={(e) => {
          if (e.target === e.currentTarget) props.onClose();
        }}
      >
        <div class="modal-card">
          <input
            ref={inputRef}
            type="text"
            class="modal-search-input"
            placeholder="Type a command or search documentation..."
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            autofocus
          />

          <div class="modal-results">
            <Show
              when={searchResults().length > 0}
              fallback={
                <div style={{ padding: "20px", "text-align": "center", color: "var(--text-muted)", "font-size": "13px" }}>
                  {query() ? "No results found for '" + query() + "'" : "Search for database, auth, realtime, storage, SDKs..."}
                </div>
              }
            >
              <For each={searchResults()}>
                {(res) => (
                  <div
                    class="modal-result-item"
                    onClick={() => {
                      props.onSelect(res.pageId, res.sectionId);
                      props.onClose();
                    }}
                  >
                    <div class="modal-result-title">{res.title}</div>
                    <div class="modal-result-desc">{res.desc}</div>
                  </div>
                )}
              </For>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
}
