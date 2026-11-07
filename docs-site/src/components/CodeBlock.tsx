import { createSignal, createMemo, For } from "solid-js";
import Prism from "prismjs";
import "prismjs/components/prism-clike.js";
import "prismjs/components/prism-javascript.js";
import "prismjs/components/prism-typescript.js";
import "prismjs/components/prism-kotlin.js";
import "prismjs/components/prism-dart.js";
import "prismjs/components/prism-python.js";
import "prismjs/components/prism-rust.js";
import "prismjs/components/prism-swift.js";
import "prismjs/components/prism-bash.js";
import "prismjs/components/prism-json.js";
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

const getPrismLang = (lang: SupportedLanguage) => {
  switch (lang) {
    case "ts":
      return { grammar: Prism.languages.typescript, name: "typescript" };
    case "kotlin":
      return { grammar: Prism.languages.kotlin, name: "kotlin" };
    case "dart":
      return { grammar: Prism.languages.dart, name: "dart" };
    case "python":
      return { grammar: Prism.languages.python, name: "python" };
    case "rust":
      return { grammar: Prism.languages.rust, name: "rust" };
    case "swift":
      return { grammar: Prism.languages.swift, name: "swift" };
    case "rest":
      return { grammar: Prism.languages.bash, name: "bash" };
    default:
      return { grammar: Prism.languages.clike, name: "clike" };
  }
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

  const effectiveLang = (): SupportedLanguage => {
    if (props.snippets[props.selectedLang]) {
      return props.selectedLang;
    }
    const available = availableLangs();
    return available[0] ?? "ts";
  };

  const activeSnippet = (): string => {
    const lang = effectiveLang();
    return props.snippets[lang] ?? "";
  };

  const highlightedHtml = createMemo(() => {
    const code = activeSnippet();
    const lang = effectiveLang();
    const { grammar, name } = getPrismLang(lang);
    if (!grammar) return code;
    try {
      return Prism.highlight(code, grammar, name);
    } catch {
      return code;
    }
  });

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
                classList={{ active: effectiveLang() === lang }}
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
        <code class={`language-${getPrismLang(effectiveLang()).name}`} innerHTML={highlightedHtml()} />
      </pre>
    </div>
  );
}
