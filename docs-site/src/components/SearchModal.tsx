import { createSignal, createMemo, For, Show, onMount } from "solid-js";
import { ALL_SECTIONS, DocSection } from "../data/pages";

export default function SearchModal(props: {
  isOpen: boolean;
  onClose: () => void;
  onSelectResult: (sectionId: string) => void;
}) {
  const [query, setQuery] = createSignal("");
  let inputRef: HTMLInputElement | undefined;

  onMount(() => {
    if (props.isOpen && inputRef) {
      inputRef.focus();
    }
  });

  const results = createMemo((): DocSection[] => {
    const q = query().trim().toLowerCase();
    if (!q) return ALL_SECTIONS.slice(0, 8);
    return ALL_SECTIONS.filter(
      (sec) =>
        sec.title.toLowerCase().includes(q) ||
        sec.chapter.toLowerCase().includes(q) ||
        sec.content.toLowerCase().includes(q) ||
        (sec.lead && sec.lead.toLowerCase().includes(q)),
    );
  });

  const handleSelect = (sectionId: string) => {
    props.onSelectResult(sectionId);
    props.onClose();
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
          <div style={{ display: "flex", "align-items": "center", padding: "0 16px", "border-bottom": "1px solid var(--border-color)" }}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style={{ color: "var(--text-muted)" }}>
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              ref={inputRef}
              type="text"
              class="modal-search-input"
              placeholder="Search documentation, methods, rules, and guides..."
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") props.onClose();
                if (e.key === "Enter" && results().length > 0) {
                  handleSelect(results()[0].id);
                }
              }}
            />
            <button
              type="button"
              onClick={props.onClose}
              style={{
                background: "transparent",
                border: "none",
                color: "var(--text-muted)",
                cursor: "pointer",
                padding: "6px 10px",
                "font-size": "13px",
              }}
            >
              ESC
            </button>
          </div>

          <div class="modal-results">
            <Show
              when={results().length > 0}
              fallback={
                <div style={{ padding: "32px", "text-align": "center", color: "var(--text-muted)", "font-size": "14px" }}>
                  No matching documentation found for "{query()}"
                </div>
              }
            >
              <For each={results()}>
                {(sec) => (
                  <div class="modal-result-item" onClick={() => handleSelect(sec.id)}>
                    <div style={{ "font-size": "11px", color: "var(--text-muted)", "font-weight": "600", "text-transform": "uppercase", "margin-bottom": "2px" }}>
                      {sec.chapter}
                    </div>
                    <div style={{ "font-size": "14.5px", "font-weight": "600", color: "var(--text-primary)", "margin-bottom": "2px" }}>
                      {sec.title}
                    </div>
                    <div style={{ "font-size": "12.5px", color: "var(--text-secondary)", "white-space": "nowrap", "overflow": "hidden", "text-overflow": "ellipsis" }}>
                      {sec.lead ?? sec.content}
                    </div>
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
