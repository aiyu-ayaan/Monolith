# 23 — Brand & Logo Guidelines

Monolith's identity in one line: **one solid block, cut once, built for speed.**
The mark and the red say the same thing as the product: a single binary that is
fast, powerful and self-contained.

All logo files live in [`logo/`](../../logo). Do not redraw the mark; use these files.

## 1. The mark: Keystone M

A single slab with one V cut from the top. The slab is the monolith (one server,
one container); the cut turns it into an M. One shape, one colour, no gradients.

Construction (256 × 256 grid, `logo/monolith-symbol.svg`):

| Element | Value |
|---------|-------|
| Slab | x 50–206, y 24–232 (156 × 208, 3:4) |
| Corner radius | 8 (sharp on purpose: rounded corners read as a shirt) |
| Cut | from (98, 24) and (158, 24) to the apex (128, 136); edges at exactly 75° |
| Cut depth | 112 of 208 (54 % of the slab height) |

Why this mark (the concept sheet is `logo/concepts.png`): it reads as both a
monolith and an M, stays bold at 16 px, and avoids the backend-category clichés
(database cylinders, clouds, lightning bolts, flames, isometric cubes).

## 2. Colour

| Name | HEX | RGB | Use |
|------|-----|-----|-----|
| **Monolith Red** | `#E11D2E` | 225 29 46 | The logo, brand accents, theme colour |
| Red Strong | `#D90F23` | 217 15 35 | Light-theme primary buttons (white text 5.2:1) |
| Red Bright | `#FF4D55` | 255 77 85 | Dark-theme brand and primary (on `#0B0D12`: 6.0:1) |
| Ink | `#111827` | 17 24 39 | Wordmark on light backgrounds, body text |
| Night | `#0B0D12` | 11 13 18 | Dark backgrounds |
| Paper | `#FFFFFF` | 255 255 255 | Wordmark and reversed mark on dark/red backgrounds |

Approximate print matches: Pantone 1795 C / CMYK 0 95 85 0. Check a physical
swatch before printing.

Red signals speed, power and energy. Red also signals errors, so the admin UI
keeps danger distinct from the brand:

- `--danger` is crimson (`#9F1239` light / `#FB7185` dark), and danger controls
  are ghost/outline styles, while primary controls are solid red.
- Toasts use neutral ink, never red, so a success never looks like an error.
- Success stays green (`--accent`).

UI tokens live in `ui/src/styles.css` (`--brand`, `--brand-bg`, `--primary`, `--focus`).

## 3. Lockups and files

| File | Use |
|------|-----|
| `monolith-symbol.svg` | Master symbol, red |
| `monolith-symbol-black.svg` / `-white.svg` | One-colour |
| `monolith-symbol-square.svg` | Avatars, social |
| `monolith-symbol-app-icon.svg` | White mark on a red rounded tile |
| `monolith-symbol-favicon.svg` | Tight crop for browser tabs |
| `monolith-lockup-light.svg` | Red mark + ink wordmark, light backgrounds |
| `monolith-lockup-dark.svg` | Red mark + white wordmark, dark backgrounds |
| `monolith-lockup-black.svg` / `-white.svg` | One-colour lockups |
| `monolith-wordmark.svg` | Wordmark alone |
| `png/` | PNG exports (symbols 512/1024, lockups 1200 wide) |
| `web/` | `favicon.ico` (16/32/48), `favicon.svg`, `apple-touch-icon.png`, PWA icons, `site.webmanifest` |
| `build_lockup.py` | Regenerates the wordmark and lockups from Inter Bold |

Wordmark: lowercase `monolith` in **Inter Bold** (SIL OFL 1.1, logo use allowed),
tracking −2 %, outlined to paths. In the lockup the ascender of `l` is 62 % of the
slab height, centred on it, with a gap of 0.36 × the slab width.

## 4. Usage

- **Clear space**: keep at least the cut's top width (60 units, ~38 % of the slab
  width) free on every side.
- **Minimum size**: symbol 16 px; lockup 96 px wide on screen, 25 mm in print.
- **Backgrounds**: red mark on white, light grey, Night or any dark neutral. On a
  red or busy background use the white mark or the app-icon tile.
- **Do not**: recolour the mark (except black/white one-colour), round its
  corners, change the cut angle, add gradients, shadows or outlines, stretch it,
  set the wordmark in another font, or put the red mark on a red background.

## 5. Where the logo appears

| Place | Source |
|-------|--------|
| README header | `logo/monolith-lockup-light.svg` / `-dark.svg` via `<picture>` |
| Admin UI rail, sign-in card, project nav | `logo` icon in `ui/src/ui.tsx` (same path, `currentColor` = `--brand`) |
| Browser tab, home screen, PWA | `ui/public/` (copied from `logo/web/`), linked from `ui/index.html`, served at `/_/` |

When the mark changes, update `logo/` first, re-run the exports, then copy
`logo/web/*` (minus `favicon-*.png`) into `ui/public/` and update the path in `ui/src/ui.tsx`.

Trademark clearance has not been checked. Run a professional search before
registering or publishing the mark commercially.
