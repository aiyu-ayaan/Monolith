import { createSignal } from "solid-js";

export default function Header(props: {
  onOpenSearch: () => void;
  onSelectPage: (pageId: string, sectionId?: string) => void;
}) {
  const [dark, setDark] = createSignal(
    typeof window !== "undefined" &&
      (localStorage.getItem("monolith.docs.theme") === "dark" ||
        (!localStorage.getItem("monolith.docs.theme") && window.matchMedia("(prefers-color-scheme: dark)").matches)),
  );

  const toggleTheme = () => {
    const next = !dark();
    setDark(next);
    if (next) {
      document.documentElement.setAttribute("data-theme", "dark");
      localStorage.setItem("monolith.docs.theme", "dark");
    } else {
      document.documentElement.removeAttribute("data-theme");
      localStorage.setItem("monolith.docs.theme", "light");
    }
  };

  // Sync theme on initial load
  if (dark()) {
    document.documentElement.setAttribute("data-theme", "dark");
  }

  return (
    <header class="top-header">
      <div class="top-header-left">
        <a href="#/firestore" class="brand-logo" onClick={() => props.onSelectPage("firestore")}>
          <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
            <rect width="32" height="32" rx="6" fill="#1A73E8" />
            <path
              d="M8 24V8L16 16L24 8V24H19V14.5L16 17.5L13 14.5V24H8Z"
              fill="#FFFFFF"
            />
          </svg>
          <span>Monolith</span>
          <span class="brand-badge">Docs</span>
        </a>

        <nav class="primary-nav">
          <a href="#/firestore" class="nav-link active">Build</a>
          <a href="#/firestore" class="nav-link">Run</a>
          <a href="#/sdks" class="nav-link">Reference</a>
          <a href="#/storage" class="nav-link">Solutions</a>
          <a href="#/sdks" class="nav-link">Pricing</a>
        </nav>
      </div>

      <div class="top-header-right">
        {/* New in Cloud Database Pill Banner (Image 1 match) */}
        <button
          class="pill-banner"
          onClick={() => props.onSelectPage("firestore", "realtime-streaming")}
          title="See latest Cloud Database releases"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M11 5L6 9H2v6h4l5 4V5z"/>
            <path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"/>
          </svg>
          <span class="pill-dot"></span>
          <span>New in Cloud Database</span>
        </button>

        {/* Search trigger with keyboard shortcut */}
        <button class="search-trigger" onClick={props.onOpenSearch}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <span>Search docs...</span>
          <kbd class="search-kbd">⌘K</kbd>
        </button>

        {/* Theme Toggle Button */}
        <button class="icon-btn" onClick={toggleTheme} title="Toggle theme">
          {dark() ? (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="5" />
              <line x1="12" y1="1" x2="12" y2="3" />
              <line x1="12" y1="21" x2="12" y2="23" />
              <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
              <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
              <line x1="1" y1="12" x2="3" y2="12" />
              <line x1="21" y1="12" x2="23" y2="12" />
              <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
              <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
            </svg>
          ) : (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
            </svg>
          )}
        </button>

        {/* Go to Console button */}
        <a href="http://localhost:8090/_/" target="_blank" rel="noopener noreferrer" class="btn-console">
          <span>Go to console</span>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            <polyline points="15 3 21 3 21 9" />
            <line x1="10" y1="14" x2="21" y2="3" />
          </svg>
        </a>

        {/* User avatar */}
        <div
          style={{
            width: "32px",
            height: "32px",
            "border-radius": "50%",
            background: "#1A73E8",
            color: "#FFF",
            display: "flex",
            "align-items": "center",
            "justify-content": "center",
            "font-size": "13px",
            "font-weight": "600",
          }}
        >
          M
        </div>
      </div>
    </header>
  );
}
