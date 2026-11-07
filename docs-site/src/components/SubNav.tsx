import { For } from "solid-js";

export default function SubNav(props: {
  activeTab: string;
  breadcrumbs: string[];
  onSelectTab: (tab: string) => void;
}) {
  const tabs = ["Overview", "Fundamentals", "AI & Agents", "Build", "Run", "Reference", "Samples"];

  return (
    <div class="sub-nav">
      <div class="breadcrumbs">
        <For each={props.breadcrumbs}>
          {(crumb, idx) => (
            <>
              {idx() > 0 && <span class="crumb-sep">›</span>}
              <span classList={{ "crumb-active": idx() === props.breadcrumbs.length - 1 }}>
                {crumb}
              </span>
            </>
          )}
        </For>
      </div>

      <nav class="subnav-tabs">
        <For each={tabs}>
          {(tab) => (
            <button
              class="subnav-tab"
              classList={{ active: props.activeTab.toLowerCase() === tab.toLowerCase() }}
              onClick={() => props.onSelectTab(tab)}
            >
              {tab}
            </button>
          )}
        </For>
      </nav>
    </div>
  );
}
