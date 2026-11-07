import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import Header from "./components/Header";
import SubNav from "./components/SubNav";
import Sidebar from "./components/Sidebar";
import RightSidebar from "./components/RightSidebar";
import CodeBlock, { SupportedLanguage } from "./components/CodeBlock";
import SearchModal from "./components/SearchModal";
import { DOC_PAGES, DocPage } from "./data/pages";

export default function App() {
  const [currentPageId, setCurrentPageId] = createSignal<string>("firestore");
  const [currentSectionId, setCurrentSectionId] = createSignal<string | undefined>("quickstart-init");
  const [activeTab, setActiveTab] = createSignal<string>("Build");
  const [selectedLang, setSelectedLang] = createSignal<SupportedLanguage>("ts");
  const [searchOpen, setSearchOpen] = createSignal<boolean>(false);
  const [copiedLink, setCopiedLink] = createSignal<string | null>(null);

  // Sync hash routing on mount
  onMount(() => {
    const parseHash = () => {
      const hash = window.location.hash.replace(/^#\/?/, "");
      if (!hash) return;
      const parts = hash.split("#");
      const page = parts[0] || "firestore";
      const section = parts[1];
      if (DOC_PAGES[page]) {
        setCurrentPageId(page);
        setCurrentSectionId(section);
        if (section) {
          setTimeout(() => {
            const el = document.getElementById(section);
            if (el) el.scrollIntoView({ behavior: "smooth" });
          }, 100);
        }
      }
    };

    parseHash();
    window.addEventListener("hashchange", parseHash);

    const handleKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setSearchOpen((prev) => !prev);
      }
    };
    window.addEventListener("keydown", handleKey);

    onCleanup(() => {
      window.removeEventListener("hashchange", parseHash);
      window.removeEventListener("keydown", handleKey);
    });
  });

  const currentPage = (): DocPage => {
    return DOC_PAGES[currentPageId()] ?? DOC_PAGES.firestore;
  };

  const handleSelectPage = (pageId: string, sectionId?: string) => {
    setCurrentPageId(pageId);
    setCurrentSectionId(sectionId);
    window.location.hash = sectionId ? `#/${pageId}#${sectionId}` : `#/${pageId}`;
    if (sectionId) {
      setTimeout(() => {
        const el = document.getElementById(sectionId);
        if (el) el.scrollIntoView({ behavior: "smooth" });
      }, 50);
    } else {
      window.scrollTo({ top: 0, behavior: "smooth" });
    }
  };

  const handleCopyLink = (sectionId?: string) => {
    const url = sectionId
      ? `${window.location.origin}${window.location.pathname}#/${currentPageId()}#${sectionId}`
      : `${window.location.origin}${window.location.pathname}#/${currentPageId()}`;
    navigator.clipboard.writeText(url);
    setCopiedLink(sectionId ?? "page");
    setTimeout(() => setCopiedLink(null), 2000);
  };

  return (
    <div class="docs-layout">
      {/* Top Header with Brand, New-in Banner Pill, Search trigger, Console Link */}
      <Header
        onOpenSearch={() => setSearchOpen(true)}
        onSelectPage={handleSelectPage}
      />

      {/* Secondary SubNav with Breadcrumbs and Sub-Tabs (Image 2 match) */}
      <SubNav
        activeTab={activeTab()}
        breadcrumbs={currentPage().breadcrumbs}
        onSelectTab={(tab) => setActiveTab(tab)}
      />

      {/* Main Body Grid */}
      <div class="docs-body">
        {/* Left Navigation Sidebar with Filter (Image 2 & 3 match) */}
        <Sidebar
          activePageId={currentPageId()}
          activeSectionId={currentSectionId()}
          onSelect={handleSelectPage}
        />

        {/* Center Main Document Page */}
        <main class="main-content">
          {/* Top Banner Alert (Image 2 match) */}
          <Show when={currentPage().bannerAlert}>
            {(alert) => (
              <div class="callout-alert">
                <span>
                  {alert().text}{" "}
                  <a href={alert().linkUrl} onClick={(e) => {
                    e.preventDefault();
                    handleSelectPage("firestore", "quickstart-init");
                  }}>
                    {alert().linkText}
                  </a>
                </span>
              </div>
            )}
          </Show>

          {/* Page Title & Action Bar */}
          <div class="page-title-row">
            <h1 class="page-title">{currentPage().title}</h1>

            <div class="page-actions">
              <button
                class="btn-action"
                onClick={() => handleCopyLink()}
                title="Copy link to page"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/>
                  <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>
                </svg>
                <span>{copiedLink() === "page" ? "Copied!" : "Copy link"}</span>
              </button>

              <button class="btn-action" onClick={() => window.print()} title="Print page">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <polyline points="6 9 6 2 18 2 18 9"/>
                  <path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"/>
                  <rect x="6" y="14" width="12" height="8"/>
                </svg>
                <span>Print</span>
              </button>
            </div>
          </div>

          {/* Language Platform Pill Badges (Image 2 match) */}
          <div class="platform-badges">
            <button
              class="platform-badge"
              classList={{ active: selectedLang() === "ts" }}
              onClick={() => setSelectedLang("ts")}
            >
              <span>TypeScript / Web</span>
            </button>
            <button
              class="platform-badge"
              classList={{ active: selectedLang() === "kotlin" }}
              onClick={() => setSelectedLang("kotlin")}
            >
              <span>Android (Kotlin)</span>
            </button>
            <button
              class="platform-badge"
              classList={{ active: selectedLang() === "dart" }}
              onClick={() => setSelectedLang("dart")}
            >
              <span>Flutter (Dart)</span>
            </button>
            <button
              class="platform-badge"
              classList={{ active: selectedLang() === "python" }}
              onClick={() => setSelectedLang("python")}
            >
              <span>Python</span>
            </button>
            <button
              class="platform-badge"
              classList={{ active: selectedLang() === "rust" }}
              onClick={() => setSelectedLang("rust")}
            >
              <span>Rust</span>
            </button>
            <button
              class="platform-badge"
              classList={{ active: selectedLang() === "rest" }}
              onClick={() => setSelectedLang("rest")}
            >
              <span>REST / cURL</span>
            </button>
          </div>

          {/* Overview Prose */}
          <p style={{ "font-size": "15px", "line-height": "1.65", color: "var(--text-secondary)", "margin-bottom": "20px" }}>
            {currentPage().overview}
          </p>

          {/* AI Assistance Agent Skills Callout (Image 2 match) */}
          <div class="ai-agent-card">
            <span class="ai-agent-icon">💡</span>
            <div>
              <strong>Looking for AI assistance?</strong> Install{" "}
              <a href="#/sdks" style={{ "font-weight": "600" }}>
                Monolith agent skills
              </a>{" "}
              for direct automated integration with AI coding assistants like Gemini, Claude Code, Antigravity, Cursor, and Codex.
            </div>
          </div>

          {/* Key Capabilities Grid (Image 2 match) */}
          <Show when={currentPage().keyCapabilities}>
            {(caps) => (
              <>
                <h2 class="doc-heading" id="key-capabilities">
                  <span>Key capabilities</span>
                  <a
                    href="#key-capabilities"
                    class="anchor-link"
                    onClick={() => handleCopyLink("key-capabilities")}
                    title="Copy section link"
                  >
                    #
                  </a>
                </h2>

                <div class="capabilities-grid">
                  <For each={caps()}>
                    {(cap) => (
                      <div class="capability-row">
                        <div class="capability-title">{cap.title}</div>
                        <div class="capability-desc">{cap.desc}</div>
                      </div>
                    )}
                  </For>
                </div>
              </>
            )}
          </Show>

          {/* Detailed Document Sections */}
          <For each={currentPage().sections}>
            {(section) => (
              <section id={section.id} style={{ "margin-top": "36px" }}>
                <h2 class="doc-heading">
                  <span>{section.title}</span>
                  <a
                    href={`#${section.id}`}
                    class="anchor-link"
                    onClick={() => handleCopyLink(section.id)}
                    title="Copy section link"
                  >
                    #
                  </a>
                </h2>

                <Show when={section.lead}>
                  <p style={{ "font-size": "14.5px", "font-weight": "500", color: "var(--text-primary)", "margin-bottom": "8px" }}>
                    {section.lead}
                  </p>
                </Show>

                <p style={{ "font-size": "14px", color: "var(--text-secondary)", "margin-bottom": "14px" }}>
                  {section.content}
                </p>

                {/* Table if present */}
                <Show when={section.table}>
                  {(tbl) => (
                    <div style={{ "overflow-x": "auto", margin: "16px 0 24px" }}>
                      <table style={{ width: "100%", "border-collapse": "collapse", "font-size": "13px" }}>
                        <thead>
                          <tr style={{ background: "var(--bg-sidebar)", "border-bottom": "2px solid var(--border-color)" }}>
                            <For each={tbl().headers}>
                              {(th) => (
                                <th style={{ padding: "10px 14px", "text-align": "left", "font-weight": "600" }}>{th}</th>
                              )}
                            </For>
                          </tr>
                        </thead>
                        <tbody>
                          <For each={tbl().rows}>
                            {(row) => (
                              <tr style={{ "border-bottom": "1px solid var(--border-color)" }}>
                                <For each={row}>
                                  {(cell) => <td style={{ padding: "10px 14px" }}>{cell}</td>}
                                </For>
                              </tr>
                            )}
                          </For>
                        </tbody>
                      </table>
                    </div>
                  )}
                </Show>

                {/* Multi-language code snippet */}
                <Show when={section.snippets}>
                  {(snips) => (
                    <CodeBlock
                      snippets={snips()}
                      selectedLang={selectedLang()}
                      onSelectLang={(lang) => setSelectedLang(lang)}
                    />
                  )}
                </Show>
              </section>
            )}
          </For>

          {/* Feedback footer */}
          <div
            style={{
              "margin-top": "64px",
              padding: "24px",
              "border-radius": "8px",
              background: "var(--bg-sidebar)",
              border: "1px solid var(--border-color)",
              display: "flex",
              "align-items": "center",
              "justify-content": "space-between",
            }}
          >
            <span style={{ "font-size": "14px", color: "var(--text-secondary)" }}>
              Was this page helpful?
            </span>
            <div style={{ display: "flex", gap: "8px" }}>
              <button class="btn-action">👍 Yes</button>
              <button class="btn-action">👎 No</button>
            </div>
          </div>
        </main>

        {/* Right Sidebar: TOC & AI Chat Assistant (Image 2 match) */}
        <RightSidebar
          sections={currentPage().sections}
          activeSectionId={currentSectionId()}
          onSelectSection={(secId) => handleSelectPage(currentPageId(), secId)}
        />
      </div>

      {/* Global Search Dialog Modal (⌘K) */}
      <SearchModal
        isOpen={searchOpen()}
        onClose={() => setSearchOpen(false)}
        onSelect={handleSelectPage}
      />
    </div>
  );
}
