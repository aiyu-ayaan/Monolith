import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import Header from "./components/Header";
import SubNav from "./components/SubNav";
import Sidebar from "./components/Sidebar";
import RightSidebar from "./components/RightSidebar";
import CodeBlock, { SupportedLanguage } from "./components/CodeBlock";
import SearchModal from "./components/SearchModal";
import { ALL_SECTIONS, CAPABILITIES, DocSection } from "./data/pages";

export default function App() {
  const [activeSectionId, setActiveSectionId] = createSignal<string>("overview");
  const [selectedLang, setSelectedLang] = createSignal<SupportedLanguage>("ts");
  const [searchOpen, setSearchOpen] = createSignal<boolean>(false);
  const [copiedLink, setCopiedLink] = createSignal<string | null>(null);
  const [feedbackSent, setFeedbackSent] = createSignal<boolean>(false);

  // Group sections by chapter for visual dividers
  const chapters = () => {
    const map = new Map<string, DocSection[]>();
    for (const sec of ALL_SECTIONS) {
      const list = map.get(sec.chapter) ?? [];
      list.push(sec);
      map.set(sec.chapter, list);
    }
    return Array.from(map.entries());
  };

  const scrollToSection = (sectionId: string, lang?: SupportedLanguage) => {
    if (lang) {
      setSelectedLang(lang);
    }
    setActiveSectionId(sectionId);
    window.history.replaceState(null, "", `#${sectionId}`);

    const el = document.getElementById(sectionId);
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  };

  const handleCopyLink = (sectionId: string) => {
    const url = `${window.location.origin}${window.location.pathname}#${sectionId}`;
    navigator.clipboard.writeText(url);
    setCopiedLink(sectionId);
    setTimeout(() => setCopiedLink(null), 2000);
  };

  onMount(() => {
    // Check initial hash
    const initialHash = window.location.hash.replace(/^#\/?/, "");
    if (initialHash) {
      setTimeout(() => {
        scrollToSection(initialHash);
      }, 100);
    }

    // Keyboard shortcut for Cmd/Ctrl+K
    const handleKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setSearchOpen((prev) => !prev);
      }
    };
    window.addEventListener("keydown", handleKey);

    // Scrollspy to update active section in TOC and Sidebar
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setActiveSectionId(entry.target.id);
            break;
          }
        }
      },
      {
        rootMargin: "-100px 0px -60% 0px",
        threshold: 0,
      },
    );

    for (const sec of ALL_SECTIONS) {
      const el = document.getElementById(sec.id);
      if (el) observer.observe(el);
    }

    onCleanup(() => {
      window.removeEventListener("keydown", handleKey);
      observer.disconnect();
    });
  });

  return (
    <div class="docs-layout">
      {/* Top Header */}
      <Header
        onOpenSearch={() => setSearchOpen(true)}
        onNavigateToSection={scrollToSection}
      />

      {/* Secondary Sticky SubNav Bar */}
      <SubNav
        activeSectionId={activeSectionId()}
        onNavigateToSection={scrollToSection}
      />

      {/* Main Full-Bleed Documentation Layout */}
      <div class="docs-body">
        {/* Left Sidebar Navigation */}
        <Sidebar
          activeSectionId={activeSectionId()}
          onNavigateToSection={scrollToSection}
        />

        {/* Center Continuous Single-Page Document */}
        <main class="main-content">
          {/* Top Banner Alert (Image 2 match) */}
          <div class="callout-alert">
            <span>
              Learn how Monolith scales with SQLite group commit, sub-millisecond p99 reads, and per-app database isolation.{" "}
              <a
                href="#architecture"
                onClick={(e) => {
                  e.preventDefault();
                  scrollToSection("architecture");
                }}
              >
                Learn more in Architecture →
              </a>
            </span>
          </div>

          {/* Page Hero Header */}
          <div class="page-title-row">
            <div>
              <h1 class="page-title">Monolith Cloud Database</h1>
              <p style={{ "font-size": "15px", color: "var(--text-secondary)", "margin-top": "6px" }}>
                Use the flexible, ultra-fast self-hosted backend suite in Rust to store, sync, and authenticate data for client and server development.
              </p>
            </div>

            <div class="page-actions">
              <button
                type="button"
                class="btn-action"
                onClick={() => handleCopyLink("overview")}
                title="Copy link to page"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
                  <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
                </svg>
                <span>{copiedLink() === "overview" ? "Copied!" : "Share"}</span>
              </button>

              <button
                type="button"
                class="btn-action"
                onClick={() => window.print()}
                title="Print documentation"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <polyline points="6 9 6 2 18 2 18 9" />
                  <path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2" />
                  <rect x="6" y="14" width="12" height="8" />
                </svg>
                <span>Print</span>
              </button>
            </div>
          </div>

          {/* Language Platform Pill Badges (Image 2 match) */}
          <div class="platform-badges">
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "ts" }}
              onClick={() => setSelectedLang("ts")}
            >
              <span>TypeScript / Web</span>
            </button>
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "kotlin" }}
              onClick={() => setSelectedLang("kotlin")}
            >
              <span>Android (Kotlin)</span>
            </button>
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "dart" }}
              onClick={() => setSelectedLang("dart")}
            >
              <span>Flutter (Dart)</span>
            </button>
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "python" }}
              onClick={() => setSelectedLang("python")}
            >
              <span>Python</span>
            </button>
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "rust" }}
              onClick={() => setSelectedLang("rust")}
            >
              <span>Rust</span>
            </button>
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "swift" }}
              onClick={() => setSelectedLang("swift")}
            >
              <span>Swift (iOS)</span>
            </button>
            <button
              type="button"
              class="platform-badge"
              classList={{ active: selectedLang() === "rest" }}
              onClick={() => setSelectedLang("rest")}
            >
              <span>REST / cURL</span>
            </button>
          </div>

          {/* Hero Split Card (Graphic + Overview) */}
          <div class="hero-split">
            <div>
              <p style={{ "font-size": "15px", "line-height": "1.65", color: "var(--text-secondary)", "margin-bottom": "14px" }}>
                Monolith is designed with a <strong>performance-first prime directive</strong>. Every tenant app operates with its own dedicated SQLite database file, eliminating cross-tenant lock contention and noisy neighbors while allowing effortless backup and migration.
              </p>
              <p style={{ "font-size": "15px", "line-height": "1.65", color: "var(--text-secondary)" }}>
                Zero-lookup HMAC-SHA256 JWT auth, parameterized rule compilation, and group-commit write batching provide sub-millisecond p99 latencies under high concurrency.
              </p>
            </div>

            <div class="hero-card">
              <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", "margin-bottom": "12px" }}>
                <span style={{ "font-size": "12px", "font-weight": "700", "text-transform": "uppercase", color: "#38BDF8", "letter-spacing": "0.05em" }}>
                  Architecture Highlights
                </span>
                <span style={{ "font-size": "11px", background: "rgba(56, 189, 248, 0.15)", color: "#38BDF8", padding: "2px 8px", "border-radius": "10px" }}>
                  Rust Engine
                </span>
              </div>
              <div style={{ "font-size": "13px", "line-height": "1.6", color: "#CBD5E1" }}>
                • <strong>Writer:</strong> 1 dedicated OS thread per app, group commit up to 256 ops<br />
                • <strong>Readers:</strong> WAL pool, concurrent zero-lock reads<br />
                • <strong>Realtime:</strong> SSE change bus, multiplexed per app<br />
                • <strong>Rules:</strong> Compiled AST → SQL parameter pushdown<br />
                • <strong>Storage:</strong> BLAKE3 content-addressed blocks + thumbs
              </div>
            </div>
          </div>

          {/* AI Assistance Callout (Image 2 match) */}
          <div class="ai-agent-card">
            <span class="ai-agent-icon">💡</span>
            <div>
              <strong>Looking for AI assistance?</strong> Install{" "}
              <a href="#sdk-matrix" onClick={(e) => { e.preventDefault(); scrollToSection("sdk-matrix"); }} style={{ "font-weight": "600" }}>
                Monolith agent skills
              </a>{" "}
              via <code>monolith agent-init</code> for automated context and type-safe integration with coding agents like Gemini, Claude Code, Antigravity, Cursor, and Codex.
            </div>
          </div>

          {/* Key Capabilities Grid (Image 2 match) */}
          <div style={{ "margin-bottom": "36px" }}>
            <h2 class="doc-heading" id="key-capabilities">
              <span>Key capabilities</span>
              <a
                href="#key-capabilities"
                class="anchor-link"
                onClick={(e) => {
                  e.preventDefault();
                  handleCopyLink("key-capabilities");
                }}
                title="Copy link"
              >
                #
              </a>
            </h2>

            <div class="capabilities-grid">
              <For each={CAPABILITIES}>
                {(cap) => (
                  <div class="capability-row">
                    <div class="capability-title">{cap.title}</div>
                    <div class="capability-desc">{cap.desc}</div>
                  </div>
                )}
              </For>
            </div>
          </div>

          {/* Continuous Single-Page Document Chapters & Sections */}
          <For each={chapters()}>
            {([chapterTitle, sections]) => (
              <div>
                <div class="chapter-divider">
                  <span class="chapter-badge">Chapter</span>
                  <h2 style={{ "font-size": "20px", "font-weight": "600", color: "var(--text-primary)" }}>
                    {chapterTitle}
                  </h2>
                </div>

                <For each={sections}>
                  {(sec) => (
                    <section class="doc-section" id={sec.id}>
                      <h3 class="doc-heading">
                        <span>{sec.title}</span>
                        <a
                          href={`#${sec.id}`}
                          class="anchor-link"
                          onClick={(e) => {
                            e.preventDefault();
                            handleCopyLink(sec.id);
                          }}
                          title="Copy section link"
                        >
                          #
                        </a>
                      </h3>

                      <Show when={sec.lead}>
                        <p style={{ "font-size": "14.5px", "font-weight": "500", color: "var(--text-primary)", "margin-bottom": "10px" }}>
                          {sec.lead}
                        </p>
                      </Show>

                      <p style={{ "font-size": "14px", color: "var(--text-secondary)", "line-height": "1.65", "margin-bottom": "16px" }}>
                        {sec.content}
                      </p>

                      {/* Code Snippets Block if present */}
                      <Show when={sec.snippets}>
                        {(snips) => (
                          <CodeBlock
                            snippets={snips()}
                            selectedLang={selectedLang()}
                            onSelectLang={(lang) => setSelectedLang(lang)}
                          />
                        )}
                      </Show>

                      {/* Data / Reference Table if present */}
                      <Show when={sec.table}>
                        {(tbl) => (
                          <div class="doc-table-wrap">
                            <table class="doc-table">
                              <thead>
                                <tr>
                                  <For each={tbl().headers}>
                                    {(header) => <th>{header}</th>}
                                  </For>
                                </tr>
                              </thead>
                              <tbody>
                                <For each={tbl().rows}>
                                  {(row) => (
                                    <tr>
                                      <For each={row}>
                                        {(cell) => <td>{cell}</td>}
                                      </For>
                                    </tr>
                                  )}
                                </For>
                              </tbody>
                            </table>
                          </div>
                        )}
                      </Show>
                    </section>
                  )}
                </For>
              </div>
            )}
          </For>

          {/* Feedback Section (Image 2 match) */}
          <div
            style={{
              "margin-top": "56px",
              padding: "24px",
              "border-radius": "10px",
              border: "1px solid var(--border-color)",
              background: "var(--bg-surface)",
              display: "flex",
              "align-items": "center",
              "justify-content": "space-between",
            }}
          >
            <div>
              <div style={{ "font-size": "15px", "font-weight": "600", color: "var(--text-primary)" }}>
                Was this documentation helpful?
              </div>
              <div style={{ "font-size": "13px", color: "var(--text-muted)", "margin-top": "2px" }}>
                Your feedback helps us continuously improve the Monolith developer experience.
              </div>
            </div>

            <div style={{ display: "flex", gap: "8px" }}>
              <button
                type="button"
                class="btn-action"
                onClick={() => setFeedbackSent(true)}
              >
                <span>👍 Yes</span>
              </button>
              <button
                type="button"
                class="btn-action"
                onClick={() => setFeedbackSent(true)}
              >
                <span>👎 No</span>
              </button>
            </div>
          </div>

          <Show when={feedbackSent()}>
            <div style={{ "font-size": "13px", color: "#10B981", "margin-top": "8px", "text-align": "right" }}>
              ✓ Thank you for your feedback!
            </div>
          </Show>
        </main>

        {/* Right Sidebar TOC & AI Assistant */}
        <RightSidebar
          sections={ALL_SECTIONS}
          activeSectionId={activeSectionId()}
          onNavigateToSection={scrollToSection}
        />
      </div>

      {/* Global Search Modal (Cmd/Ctrl + K) */}
      <SearchModal
        isOpen={searchOpen()}
        onClose={() => setSearchOpen(false)}
        onSelectResult={scrollToSection}
      />
    </div>
  );
}
