import { For } from "solid-js";

export interface SubNavTab {
  id: string;
  label: string;
  targetSectionId: string;
}

export default function SubNav(props: {
  activeSectionId: string;
  onNavigateToSection: (sectionId: string) => void;
}) {
  const tabs: SubNavTab[] = [
    { id: "overview", label: "Overview", targetSectionId: "overview" },
    { id: "fundamentals", label: "Fundamentals", targetSectionId: "architecture" },
    { id: "build", label: "Build & CRUD", targetSectionId: "crud-records" },
    { id: "realtime", label: "Realtime", targetSectionId: "realtime-streaming" },
    { id: "auth", label: "Auth & Rules", targetSectionId: "access-rules" },
    { id: "storage", label: "Storage", targetSectionId: "storage-buckets" },
    { id: "reference", label: "SDK Reference", targetSectionId: "sdk-matrix" },
  ];

  const isTabActive = (tab: SubNavTab) => {
    if (tab.id === "overview" && (props.activeSectionId === "overview" || props.activeSectionId === "quickstart")) return true;
    if (tab.id === "fundamentals" && (props.activeSectionId === "architecture" || props.activeSectionId === "data-model")) return true;
    if (tab.id === "build" && (props.activeSectionId === "crud-records" || props.activeSectionId === "keyset-filtering" || props.activeSectionId === "relations-expand" || props.activeSectionId === "batch-transactions")) return true;
    if (tab.id === "realtime" && props.activeSectionId === "realtime-streaming") return true;
    if (tab.id === "auth" && (props.activeSectionId.startsWith("auth") || props.activeSectionId === "access-rules")) return true;
    if (tab.id === "storage" && (props.activeSectionId.startsWith("storage") || props.activeSectionId === "backups-ops")) return true;
    if (tab.id === "reference" && props.activeSectionId.startsWith("sdk")) return true;
    return false;
  };

  return (
    <div class="sub-nav">
      <div class="breadcrumbs">
        <span class="crumb-product">Monolith</span>
        <span class="crumb-sep">›</span>
        <span>Documentation</span>
        <span class="crumb-sep">›</span>
        <span>Cloud Database</span>
        <span class="crumb-sep">›</span>
        <span class="crumb-active">Interactive Reference</span>
      </div>

      <nav class="subnav-tabs">
        <For each={tabs}>
          {(tab) => (
            <button
              type="button"
              class="subnav-tab"
              classList={{ active: isTabActive(tab) }}
              onClick={() => props.onNavigateToSection(tab.targetSectionId)}
            >
              {tab.label}
            </button>
          )}
        </For>
      </nav>
    </div>
  );
}
