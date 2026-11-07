import { createSignal, For, Show } from "solid-js";
import { CodeSnippets } from "../data/pages";

export type SupportedLanguage = "ts" | "kotlin" | "dart" | "python" | "rust" | "rest";

export const LANGUAGE_LABELS: Record<SupportedLanguage, string> = {
  ts: "TypeScript",
  kotlin: "Android (Kotlin)",
  dart: "Flutter (Dart)",
  python: "Python",
  rust: "Rust",
  rest: "REST (cURL)",
};

export default function CodeBlock(props: {
  snippets: CodeSnippets;
  selectedLang: SupportedLanguage;
  onSelectLang: (lang: SupportedLanguage) => void;
}) {
  const [copied, setCopied] = createSignal(false);

  const availableLangs = (): SupportedLanguage[] => {
    const list: SupportedLanguage[] = ["ts", "kotlin", "dart", "python", "rust", "rest"];
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
                class="code-tab"
                classList={{ active: props.selectedLang === lang }}
                onClick={() => props.onSelectLang(lang)}
              >
                {LANGUAGE_LABELS[lang]}
              </button>
            )}
          </For>
        </div>

        <button class="code-copy-btn" onClick={handleCopy}>
          {copied() ? "Copied!" : "Copy"}
        </button>
      </div>

      <pre class="code-pre">
        <code>{activeSnippet()}</code>
      </pre>
    </div>
  );
}
