# Writing docs for the Monolith site

Docs are Markdown files in `docs-site/content/`. The site builds navigation, search and the right-hand table of
contents from them. The model is Firebase's documentation: each page answers *why*, *when*, *how* and *what exactly*.

## Files and URLs

`content/<area>/<group>/<slug>.md` becomes `#/<area>/<group>/<slug>`. A page directly in an area (`content/get-started/intro.md`)
has no group. Areas are fixed: `get-started`, `build`, `sdks`, `reference`.

```
---
title: Query records            # page title (sentence case)
description: Filter, sort and page through records.   # one sentence, used in search and as the lead
group: Database                 # sidebar heading (same text on every page of the group); omit for area-level pages
order: 30                       # sort inside the group (10, 20, 30 ...)
---
```

## Page shape (follow it, skip a part only when it truly does not apply)

1. One-paragraph lead: what the feature is.
2. `## Why use it` / `## When to use it`: the problem it solves, when to choose something else.
3. `## How it works`: the model, in plain words, with one diagram-like list or table if it helps.
4. Task sections (`## Do the thing`): short steps, each with code in tabs for the SDKs that support it.
5. `## Reference`: exact parameters, options, defaults, limits, error codes.
6. `## Limits and status`: what is not implemented yet, stated plainly (see Honesty).
7. `## Next steps`: links to 2-3 related pages (`[Realtime](/build/database/realtime)`).

Use `##` and `###` only (`#` is the title). Headings get anchors automatically.

## Markdown extensions

Code tabs (the reader's language choice is remembered across pages). Only include languages the SDK really supports:

````
:::tabs
```ts title="TypeScript"
const mono = Monolith.init({ url, app });
```
```kotlin title="Kotlin"
val mono = Monolith.init(ctx, url, app)
```
```bash title="curl"
curl "$BASE/api/v1/apps/$APP/collections/tasks/records"
```
:::
````

Tab titles to use: `TypeScript`, `Kotlin`, `Dart`, `Python`, `Rust`, `curl`, `Response` (for JSON output). Other fenced blocks are
plain highlighted code; use ` ```json `, ` ```bash `, ` ```yaml `, ` ```sql `, ` ```text `. Add `title="file.ts"` for a file name.

Callouts:

```
:::note
Neutral extra information.
:::
:::tip
A better way to do it.
:::
:::warning
Something that surprises people or can lose data.
:::
:::status planned
This feature is on the roadmap and not in the server yet.   (also: `partial`, `implemented`)
:::
```

Tables, lists, inline code, bold and links are standard Markdown. Internal links start with `/` and use the URL form above.

## Honesty rules (mandatory)

- Verify every claim against the code or the contract: `api/openapi/monolith.v1.yaml`, `crates/*`, `sdks/*`, `development/docs/*`
  ("Implemented today" sections), `agent-skills/*`. Do not invent endpoints, options, defaults or numbers.
- Say what is not done in `## Limits and status` or a `:::status` callout. Never describe a planned feature as working.
- No performance numbers unless they come from a measured result in `development/docs/` or `bench/`; label targets as targets.
- Code samples must match the SDK's real API (read the SDK source). Keep samples short and runnable in spirit.
- Style: plain, direct, active voice, short sentences, no marketing language, no filler. Explain the *why* before the *how*.

## Reference pages

Reference pages are exact and dry: parameter tables (name, type, default, meaning), response shapes, error codes, limits.
Link each reference page from the guide that introduces the feature, and link back.

## Conventions added later

- **SDK pages are per topic, not per language.** `content/sdks/<topic>.md` (setup, auth, query, write, realtime, storage, analytics, errors)
  shows the same task in every language with `:::tabs`. Put prose once; put differences in a short note or table. Platform-only material
  (Android manifest, Flutter widgets) goes in a platform guide (`sdks/kotlin.md`, `sdks/dart-flutter.md`, ...), group "Platform guides".
- **Anchors:** `## Title {#custom-id}` sets the heading id by hand.
- **Drafts:** a file whose name starts with `_` is not published and not shown in the sidebar.
- **Sidebar:** every page in `get-started`, `build` and `reference` needs a `group`; groups collapse, the one with the current page is open.
- **Landing page:** `src/components/Home.tsx`. Screenshots live in `public/screens/` and show the admin console of a seeded demo app.
- **Honesty:** if an SDK method is broken or missing, say so on the page (`:::status partial`) and list the fix in `development/TODO.md`.
