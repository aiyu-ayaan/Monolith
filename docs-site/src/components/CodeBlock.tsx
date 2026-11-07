import { createSignal, For } from "solid-js";
import { CodeSnippets } from "../data/pages";

export type SupportedLanguage = "ts" | "kotlin" | "dart" | "python" | "rust" | "swift" | "rest";

export const LANGUAGE_LABELS: Record<SupportedLanguage, string> = {
  ts: "TypeScript / Web",
  kotlin: "Android (Kotlin)",
  dart: "Flutter (Dart)",
  python: "Python",
  rust: "Rust",
  swift: "Swift (iOS)",
  rest: "REST (cURL)",
};

export default function CodeBlock(props: {
  snippets: CodeSnippets;
  selectedLang: SupportedLanguage;
  onSelectLang: (lang: SupportedLanguage) => void;
}) {
  const [copied, setCopied] = createSignal(false);

  const availableLangs = (): SupportedLanguage[] => {
    const list: SupportedLanguage[] = ["ts", "kotlin", "dart", "python", "rust", "swift", "rest"];
    return list.filter((l) => Boolean(props.snippets[l]));
  };

  const activeSnippet = (): string => {
    return (
      props.snippets[props.selectedLang] ??
      props.snippets.ts ??
      props.snippets.kotlin ??
      props.snippets.dart ??
      props.snippets.python ??
      props.snippets.rust ??
      props.snippets.swift ??
      props.snippets.rest ??
      ""
    );
  };

  const handleCopy = () => {
    const text = activeSnippet();
    if (!text) return;
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div class="code-box">
      <div class="code-header">
        <div class="code-tabs">
          <For each={availableLangs()}>
            {(lang) => (
              <button
                type="button"
                class="code-tab"
                classList={{ active: props.selectedLang === lang || (!props.snippets[props.selectedLang] && availableLangs()[0] === lang) }}
                onClick={() => props.onSelectLang(lang)}
              >
                {LANGUAGE_LABELS[lang]}
              </button>
            )}
          </For>
        </div>

        <button type="button" class="code-copy-btn" onClick={handleCopy}>
          {copied() ? "✓ Copied" : "Copy"}
        </button>
      </div>

      <pre class="code-pre">
        <code>{activeSnippet()}</code>
      </pre>
    </div>
  );
}
