import { createSignal, For, Show } from "solid-js";
import { NavItem, SIDEBAR_NAVIGATION } from "../data/navigation";
import { SupportedLanguage } from "./CodeBlock";

export default function Sidebar(props: {
  activeSectionId: string;
  onNavigateToSection: (sectionId: string, lang?: SupportedLanguage) => void;
}) {
  const [filter, setFilter] = createSignal("");
  const [openGroups, setOpenGroups] = createSignal<Record<string, boolean>>({
    "nav-db-group": true,
  });

  const toggleGroup = (id: string) => {
    setOpenGroups((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const matchesFilter = (item: NavItem): boolean => {
    const q = filter().trim().toLowerCase();
    if (!q) return true;
    if (item.title.toLowerCase().includes(q)) return true;
    if (item.children?.some((c) => c.title.toLowerCase().includes(q))) return true;
    return false;
  };

  return (
    <aside class="left-sidebar">
      {/* Search Filter Box - Image 2 match */}
      <div class="sidebar-filter-wrapper">
        <input
          type="text"
          class="sidebar-filter-input"
          placeholder="Filter topics..."
          value={filter()}
          onInput={(e) => setFilter(e.currentTarget.value)}
        />
      </div>

      <nav>
        <For each={SIDEBAR_NAVIGATION}>
          {(section) => {
            const filteredItems = () => section.items.filter(matchesFilter);
            return (
              <Show when={filteredItems().length > 0}>
                <div class="sidebar-section-title">{section.title}</div>
                <div>
                  <For each={filteredItems()}>
                    {(item) => {
                      const hasChildren = item.children && item.children.length > 0;
                      const isOpen = () => openGroups()[item.id] || filter().length > 0;
                      const isActive = () => props.activeSectionId === item.sectionId;

                      return (
                        <div>
                          <button
                            type="button"
                            class="sidebar-item"
                            classList={{ active: isActive() }}
                            onClick={() => {
                              if (hasChildren) {
                                toggleGroup(item.id);
                              }
                              props.onNavigateToSection(item.sectionId, item.lang);
                            }}
                          >
                            <span style={{ "text-align": "left", "line-height": "1.3" }}>
                              {item.title}
                            </span>

                            <div style={{ display: "flex", "align-items": "center", gap: "6px" }}>
                              <Show when={item.badge}>
                                <span
                                  style={{
                                    "font-size": "10.5px",
                                    padding: "1px 6px",
                                    "border-radius": "10px",
                                    background: "var(--badge-bg)",
                                    color: "var(--badge-text)",
                                    "font-weight": "600",
                                  }}
                                >
                                  {item.badge}
                                </span>
                              </Show>

                              <Show when={hasChildren}>
                                <svg
                                  class="sidebar-item-chevron"
                                  classList={{ open: isOpen() }}
                                  width="14"
                                  height="14"
                                  viewBox="0 0 24 24"
                                  fill="none"
                                  stroke="currentColor"
                                  stroke-width="2"
                                >
                                  <polyline points="9 18 15 12 9 6" />
                                </svg>
                              </Show>
                            </div>
                          </button>

                          <Show when={hasChildren && isOpen()}>
                            <div class="sidebar-sublist">
                              <For each={item.children}>
                                {(child) => {
                                  const isChildActive = () => props.activeSectionId === child.sectionId;
                                  return (
                                    <button
                                      type="button"
                                      class="sidebar-item"
                                      classList={{ active: isChildActive() }}
                                      onClick={() => props.onNavigateToSection(child.sectionId, child.lang)}
                                    >
                                      <span style={{ "text-align": "left" }}>{child.title}</span>
                                    </button>
                                  );
                                }}
                              </For>
                            </div>
                          </Show>
                        </div>
                      );
                    }}
                  </For>
                </div>
              </Show>
            );
          }}
        </For>
      </nav>
    </aside>
  );
}
