import { createSignal, For, Show } from "solid-js";
import { NavItem, SIDEBAR_NAVIGATION } from "../data/navigation";

export default function Sidebar(props: {
  activePageId: string;
  activeSectionId?: string;
  onSelect: (pageId: string, sectionId?: string) => void;
}) {
  const [filter, setFilter] = createSignal("");
  const [openGroups, setOpenGroups] = createSignal<Record<string, boolean>>({
    "db-root": true,
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
          placeholder="Filter..."
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
                      const isActive = () =>
                        props.activePageId === item.pageId &&
                        (!item.sectionId || props.activeSectionId === item.sectionId);

                      return (
                        <div>
                          <div
                            class="sidebar-item"
                            classList={{ active: isActive() }}
                            onClick={() => {
                              if (hasChildren) {
                                toggleGroup(item.id);
                              }
                              if (item.pageId) {
                                props.onSelect(item.pageId, item.sectionId);
                              }
                            }}
                          >
                            <span>{item.title}</span>

                            <Show when={item.badge}>
                              <span
                                style={{
                                  "font-size": "10px",
                                  padding: "2px 6px",
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

                          <Show when={hasChildren && isOpen()}>
                            <div class="sidebar-sublist">
                              <For each={item.children}>
                                {(child) => {
                                  const isChildActive = () =>
                                    props.activePageId === child.pageId &&
                                    props.activeSectionId === child.sectionId;
                                  return (
                                    <div
                                      class="sidebar-item"
                                      classList={{ active: isChildActive() }}
                                      onClick={() => {
                                        if (child.pageId) {
                                          props.onSelect(child.pageId, child.sectionId);
                                        }
                                      }}
                                    >
                                      <span>{child.title}</span>
                                    </div>
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
