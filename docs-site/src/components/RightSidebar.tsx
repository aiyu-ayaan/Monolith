import { createSignal, For, Show } from "solid-js";
import { DocSection } from "../data/pages";

interface ChatMessage {
  role: "user" | "assistant";
  text: string;
}

export default function RightSidebar(props: {
  sections: DocSection[];
  activeSectionId: string;
  onNavigateToSection: (id: string) => void;
}) {
  const [messages, setMessages] = createSignal<ChatMessage[]>([
    {
      role: "assistant",
      text: "Hi! I'm your Monolith documentation assistant. Ask me questions about database schemas, access rules, SDK methods, or storage.",
    },
  ]);
  const [input, setInput] = createSignal("");
  const [loading, setLoading] = createSignal(false);

  const suggestions = [
    "How does keyset pagination work?",
    "Show me a Kotlin CRUD example",
    "How does realtime SSE reconnect?",
    "How do image thumbnails work?",
    "What are scoped API keys?",
  ];

  const handleSend = (query?: string) => {
    const q = (query ?? input()).trim();
    if (!q) return;

    setMessages((prev) => [...prev, { role: "user", text: q }]);
    setInput("");
    setLoading(true);

    setTimeout(() => {
      let answer = "";
      const lower = q.toLowerCase();

      if (lower.includes("filter") || lower.includes("keyset") || lower.includes("query") || lower.includes("pagination")) {
        answer =
          "Monolith compiles filter rules directly into parameterized SQL WHERE clauses with index pushdown. Instead of expensive OFFSET scans, it uses deterministic keyset pagination with opaque cursor strings via `.list({ filter, sort: '-created', limit: 25, cursor })`.";
      } else if (lower.includes("kotlin") || lower.includes("android")) {
        answer =
          "In Kotlin/Android, initialize via `MonolithAndroid.builder(context).url('http://10.0.2.2:8090').app('default').build()`. It provides automatic EncryptedSharedPreferences token persistence and reactive Flows via `collection('tasks').subscribeAsFlow()`.";
      } else if (lower.includes("sse") || lower.includes("realtime") || lower.includes("reconnect")) {
        answer =
          "Realtime uses a single multiplexed SSE connection at `GET /api/v1/apps/{app}/realtime`. It features automatic reference-counted lifecycle (opens on first subscriber, closes when empty) and exponential backoff with jitter on network disconnects.";
      } else if (lower.includes("thumbnail") || lower.includes("image") || lower.includes("storage") || lower.includes("blake3")) {
        answer =
          "Storage provides content-addressable BLAKE3 deduplication and on-demand thumbnail generation! Pass `{ thumb: 'WxH' }` to `files.getURL('bucket', 'photo.jpg', { thumb: '200x200' })`. Thumbnails are generated via a bounded pool and cached.";
      } else if (lower.includes("key") || lower.includes("api key") || lower.includes("token")) {
        answer =
          "Monolith supports zero-lookup HMAC-SHA256 access tokens, rotating refresh tokens with reuse detection, and scoped runtime API keys (`mbk_<env>_...`) with CIDR IP allowlists and GCRA leaky-bucket rate limits.";
      } else {
        answer = `Monolith provides unified SDKs across TypeScript, Kotlin, Dart, Python, and Rust with full database CRUD, zero-lookup HMAC auth, realtime SSE, and BLAKE3 storage. Check out the code examples on this page!`;
      }

      setMessages((prev) => [...prev, { role: "assistant", text: answer }]);
      setLoading(false);
    }, 450);
  };

  return (
    <aside class="right-sidebar">
      {/* On this page TOC */}
      <div class="toc-title">On this page</div>
      <ul class="toc-list">
        <For each={props.sections}>
          {(sec) => (
            <li class="toc-item">
              <a
                href={`#${sec.id}`}
                class="toc-link"
                classList={{ active: props.activeSectionId === sec.id }}
                onClick={(e) => {
                  e.preventDefault();
                  props.onNavigateToSection(sec.id);
                }}
              >
                {sec.title}
              </a>
            </li>
          )}
        </For>
      </ul>

      {/* Ask about this page AI Assistant Drawer - Image 2 match */}
      <div class="ai-chat-panel">
        <div class="ai-chat-header">
          <div style={{ display: "flex", "align-items": "center", gap: "6px" }}>
            <span style={{ color: "var(--primary-blue)" }}>✨</span>
            <span>Ask about this page</span>
          </div>
          <span style={{ "font-size": "10.5px", color: "var(--text-muted)", "font-weight": "500" }}>Beta</span>
        </div>

        <div class="ai-chat-body">
          <For each={messages()}>
            {(msg) => (
              <div
                style={{
                  "margin-bottom": "8px",
                  padding: "8px 10px",
                  "border-radius": "8px",
                  background: msg.role === "user" ? "var(--primary-blue-light)" : "var(--bg-sidebar)",
                  color: msg.role === "user" ? "var(--primary-blue)" : "var(--text-primary)",
                  "line-height": "1.45",
                }}
              >
                <div style={{ "font-size": "10px", "font-weight": "700", "margin-bottom": "3px", opacity: 0.8 }}>
                  {msg.role === "user" ? "YOU" : "ASSISTANT"}
                </div>
                <div>{msg.text}</div>
              </div>
            )}
          </For>

          <Show when={loading()}>
            <div style={{ "font-size": "11.5px", color: "var(--text-muted)", "font-style": "italic", padding: "4px" }}>
              Thinking...
            </div>
          </Show>

          {/* Prompt Suggestions */}
          <Show when={messages().length <= 2}>
            <div style={{ "margin-top": "10px" }}>
              <div style={{ "font-size": "11px", color: "var(--text-muted)", "margin-bottom": "6px", "font-weight": "600" }}>
                Suggested questions:
              </div>
              <For each={suggestions}>
                {(s) => (
                  <button type="button" class="ai-prompt-suggestion" onClick={() => handleSend(s)}>
                    {s}
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>

        <div class="ai-chat-input-wrapper">
          <input
            type="text"
            class="ai-chat-input"
            placeholder="Ask a question..."
            value={input()}
            onInput={(e) => setInput(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSend();
            }}
          />
          <button type="button" class="ai-chat-send" onClick={() => handleSend()} title="Send">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <line x1="22" y1="2" x2="11" y2="13" />
              <polygon points="22 2 15 22 11 13 2 9 22 2" />
            </svg>
          </button>
        </div>
      </div>
    </aside>
  );
}
