import { marked, Renderer } from "marked";
import Prism from "prismjs";
import "prismjs/components/prism-clike.js";
import "prismjs/components/prism-javascript.js";
import "prismjs/components/prism-typescript.js";
import "prismjs/components/prism-jsx.js";
import "prismjs/components/prism-kotlin.js";
import "prismjs/components/prism-dart.js";
import "prismjs/components/prism-python.js";
import "prismjs/components/prism-rust.js";
import "prismjs/components/prism-java.js";
import "prismjs/components/prism-swift.js";
import "prismjs/components/prism-bash.js";
import "prismjs/components/prism-json.js";
import "prismjs/components/prism-yaml.js";
import "prismjs/components/prism-sql.js";
import "prismjs/components/prism-toml.js";
import "prismjs/components/prism-docker.js";
import "prismjs/components/prism-markup.js";
import "prismjs/components/prism-wasm.js";

export interface Heading {
  id: string;
  text: string;
  level: 2 | 3;
}

const ALIAS: Record<string, string> = {
  ts: "typescript",
  js: "javascript",
  sh: "bash",
  shell: "bash",
  curl: "bash",
  kt: "kotlin",
  py: "python",
  rs: "rust",
  yml: "yaml",
  html: "markup",
  xml: "markup",
  wat: "wasm",
  text: "",
  txt: "",
  response: "json",
};

export const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

function highlight(code: string, lang: string): string {
  const l = ALIAS[lang] ?? lang;
  const g = l ? Prism.languages[l] : undefined;
  return g ? Prism.highlight(code, g, l) : esc(code);
}

export const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .trim()
    .replace(/\s+/g, "-");

const LABELS: Record<string, string> = {
  ts: "TypeScript",
  typescript: "TypeScript",
  kotlin: "Kotlin",
  dart: "Dart",
  python: "Python",
  rust: "Rust",
  bash: "curl",
  curl: "curl",
  json: "Response",
};

/** Key a tab group remembers: the tab title in lower case (`typescript`, `kotlin`, `curl`...). */
const tabKey = (title: string) => title.toLowerCase().replace(/\s+/g, "-");

function codeBox(code: string, lang: string, title?: string): string {
  return (
    `<div class="code">${title ? `<div class="code-title">${esc(title)}</div>` : ""}` +
    `<pre class="language-${esc(lang || "text")}"><code>${highlight(code, lang)}</code></pre>` +
    `<button class="copy" type="button" aria-label="Copy code">Copy</button></div>`
  );
}

function tabs(inner: string): string {
  const re = /^```(\S*)(?:[ \t]+title="([^"]*)")?[ \t]*\r?\n([\s\S]*?)\r?\n```[ \t]*$/gm;
  const items: { key: string; title: string; lang: string; code: string }[] = [];
  for (const m of inner.matchAll(re)) {
    const lang = m[1] || "text";
    const title = m[2] || LABELS[lang] || lang;
    items.push({ key: tabKey(title), title, lang, code: m[3] });
  }
  if (!items.length) return "";
  const bar = items
    .map((t, i) => `<button type="button" role="tab" class="tab${i === 0 ? " on" : ""}" data-key="${esc(t.key)}" aria-selected="${i === 0}">${esc(t.title)}</button>`)
    .join("");
  const panels = items
    .map((t, i) => `<div role="tabpanel" class="tab-panel" data-key="${esc(t.key)}"${i === 0 ? "" : " hidden"}>${codeBox(t.code, t.lang)}</div>`)
    .join("");
  return `<div class="tabs" data-tabs><div class="tab-bar" role="tablist">${bar}</div>${panels}</div>`;
}

const STATUS: Record<string, string> = { implemented: "Available", partial: "Partly available", planned: "Planned" };

function block(kind: string, arg: string, inner: string, parse: (md: string) => string): string {
  if (kind === "tabs") return tabs(inner);
  const body = parse(inner);
  if (kind === "status") {
    const k = STATUS[arg] ? arg : "planned";
    return `<aside class="callout status ${k}"><strong>${STATUS[k]}</strong>${body}</aside>`;
  }
  const names: Record<string, string> = { note: "Note", tip: "Tip", warning: "Warning", danger: "Danger" };
  if (!names[kind]) return `:::${kind}\n${inner}\n:::`;
  return `<aside class="callout ${kind}"><strong>${names[kind]}</strong>${body}</aside>`;
}

/** Replaces `:::name` blocks by unique placeholders so marked leaves them alone. */
function extractBlocks(md: string, stash: string[], parse: (md: string) => string): string {
  return md.replace(/^:::(\w+)[ \t]*([^\n]*)\r?\n([\s\S]*?)\r?\n:::[ \t]*$/gm, (_all, kind: string, arg: string, inner: string) => {
    stash.push(block(kind, arg.trim(), inner, parse));
    return `\n\n<div data-block="${stash.length - 1}"></div>\n\n`;
  });
}

export function render(md: string): { html: string; headings: Heading[] } {
  const headings: Heading[] = [];
  const used = new Map<string, number>();
  const r = new Renderer();
  r.heading = function ({ tokens, depth }) {
    const text = this.parser.parseInline(tokens);
    const plain = text.replace(/<[^>]+>/g, "");
    let id = slug(plain) || "section";
    const n = used.get(id) ?? 0;
    used.set(id, n + 1);
    if (n) id += `-${n}`;
    if (depth === 2 || depth === 3) headings.push({ id, text: plain, level: depth });
    return `<h${depth} id="${id}">${text}<a class="anchor" href="#" data-anchor="${id}" aria-label="Link to this section">#</a></h${depth}>`;
  };
  r.code = ({ text, lang }) => {
    const m = /^(\S*)(?:\s+title="([^"]*)")?/.exec(lang ?? "");
    return codeBox(text, m?.[1] ?? "", m?.[2]);
  };
  r.link = function ({ href, title, tokens }) {
    const text = this.parser.parseInline(tokens);
    const t = title ? ` title="${esc(title)}"` : "";
    if (href.startsWith("/")) return `<a href="#${href}"${t}>${text}</a>`;
    if (/^https?:/.test(href)) return `<a href="${esc(href)}"${t} target="_blank" rel="noopener noreferrer">${text}</a>`;
    return `<a href="${esc(href)}"${t}>${text}</a>`;
  };
  r.table = function (token) {
    const head = token.header.map((c) => `<th>${this.parser.parseInline(c.tokens)}</th>`).join("");
    const rows = token.rows.map((row) => `<tr>${row.map((c) => `<td>${this.parser.parseInline(c.tokens)}</td>`).join("")}</tr>`).join("");
    return `<div class="table-wrap"><table><thead><tr>${head}</tr></thead><tbody>${rows}</tbody></table></div>`;
  };
  const parse = (s: string) => marked.parse(s, { async: false, renderer: r, gfm: true }) as string;
  const stash: string[] = [];
  const src = extractBlocks(md, stash, parse);
  let html = parse(src);
  html = html.replace(/<div data-block="(\d+)"><\/div>/g, (_m, i: string) => stash[Number(i)]);
  return { html, headings };
}
