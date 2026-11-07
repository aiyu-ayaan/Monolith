/** Loads `content/**.md`, reads frontmatter and builds the navigation tree and search index. */

const files = import.meta.glob("../../content/**/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

export const AREAS = [
  { id: "get-started", title: "Get started" },
  { id: "build", title: "Build" },
  { id: "sdks", title: "SDKs" },
  { id: "reference", title: "Reference" },
] as const;

export type AreaId = (typeof AREAS)[number]["id"];

export interface Page {
  /** URL path without the leading slash, e.g. `build/database/query`. */
  path: string;
  area: AreaId;
  group: string | null;
  title: string;
  description: string;
  order: number;
  body: string;
}

export interface NavGroup {
  title: string | null;
  pages: Page[];
}

function parse(raw: string): { meta: Record<string, string>; body: string } {
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(raw);
  if (!m) return { meta: {}, body: raw };
  const meta: Record<string, string> = {};
  for (const line of m[1].split(/\r?\n/)) {
    const i = line.indexOf(":");
    if (i < 1) continue;
    let v = line.slice(i + 1).trim();
    v = v.replace(/\s+#\s.*$/, "");
    if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) v = v.slice(1, -1);
    meta[line.slice(0, i).trim()] = v;
  }
  return { meta, body: raw.slice(m[0].length) };
}

export const PAGES: Page[] = Object.entries(files)
  .map(([file, raw]) => {
    const path = file.replace(/^.*\/content\//, "").replace(/\.md$/, "");
    const area = path.split("/")[0] as AreaId;
    const { meta, body } = parse(raw);
    return {
      path,
      area,
      group: meta.group || null,
      title: meta.title || path.split("/").pop()!.replace(/-/g, " "),
      description: meta.description || "",
      order: Number(meta.order ?? 1000),
      body,
    } satisfies Page;
  })
  .filter((p) => AREAS.some((a) => a.id === p.area))
  .sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));

const byPath = new Map(PAGES.map((p) => [p.path, p]));

export const pageFor = (path: string): Page | undefined => byPath.get(path.replace(/^\/+|\/+$/g, ""));

/** Sidebar of one area: ungrouped pages first, then groups in order of their first page. */
export function navFor(area: AreaId): NavGroup[] {
  const pages = PAGES.filter((p) => p.area === area);
  const groups: NavGroup[] = [];
  for (const p of pages) {
    let g = groups.find((x) => x.title === p.group);
    if (!g) groups.push((g = { title: p.group, pages: [] }));
    g.pages.push(p);
  }
  return groups.sort((a, b) => (a.title === null ? -1 : b.title === null ? 1 : 0));
}

/** All pages in reading order, for previous/next. */
export function readingOrder(): Page[] {
  return AREAS.flatMap((a) => navFor(a.id).flatMap((g) => g.pages));
}

export function firstPage(area: AreaId): Page | undefined {
  return navFor(area)[0]?.pages[0];
}

// ---------- search ----------

interface Doc {
  page: Page;
  title: string;
  head: string;
  text: string;
}

const strip = (md: string) =>
  md
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/^:::.*$/gm, " ")
    .replace(/[#*_`>|\[\]()-]/g, " ")
    .replace(/\s+/g, " ");

let index: Doc[] | null = null;

export interface Hit {
  page: Page;
  excerpt: string;
  score: number;
}

export function search(q: string, limit = 12): Hit[] {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return [];
  index ??= PAGES.map((page) => ({
    page,
    title: page.title.toLowerCase(),
    head: [...page.body.matchAll(/^#{2,3}\s+(.+)$/gm)].map((m) => m[1]).join(" ").toLowerCase(),
    text: strip(page.body),
  }));
  const hits: Hit[] = [];
  for (const d of index) {
    let score = 0;
    const lower = d.text.toLowerCase();
    for (const t of terms) {
      const inTitle = d.title.includes(t);
      const inHead = d.head.includes(t);
      const inText = lower.includes(t);
      if (!inTitle && !inHead && !inText && !d.page.description.toLowerCase().includes(t)) {
        score = 0;
        break;
      }
      score += (inTitle ? 10 : 0) + (inHead ? 4 : 0) + (inText ? 1 : 0) + (d.page.description.toLowerCase().includes(t) ? 2 : 0);
    }
    if (score > 0) {
      const at = lower.indexOf(terms[0]);
      const excerpt = at < 0 ? d.page.description : (at > 40 ? "…" : "") + d.text.slice(Math.max(0, at - 40), at + 120) + "…";
      hits.push({ page: d.page, excerpt, score });
    }
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, limit);
}
