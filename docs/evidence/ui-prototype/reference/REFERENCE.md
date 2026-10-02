# Pane reference — actual source HTML

**Supersedes** the earlier screenshot-derived DESIGN.md (kept only as history).

- **Source:** `C:/Users/ADMIN/Downloads/Pane — launcher.html` — 1,615,907 bytes, untouched.
- **Copy:** `.scratch/ui-reference/launcher.html` — byte-identical (SHA256 verified):
  `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`
- Analysis extracts live in `.scratch/ui-reference/extracted/` (bounded: templates, CSS, app data).

## What the file is

A **single-file snapshot** (a "bundler" artifact), not app source. HTML shell + unpacker script;
gzip-compressed manifest of **9 iframe pages**, each itself a nested bundle (React 18.3.1 UMD +
ReactDOM from cdn.jsdelivr.net — embedded, no network at runtime — a `dc-runtime` React template
renderer, and 2 embedded woff2 fonts). Page markup is a `{{placeholder}}` template filled by an
inline `text/x-dc` script with data. Root board lists, top to bottom:

1. Launcher — root (type to search)  2. Launcher — ⌘K action panel  3. Launcher — inline calculator
4. Launcher — no results  5. Clipboard history  6. Window manager — snap HUD  7. Plugin Store
8. Settings — Appearance  9. Design language

Root page chrome: background `#f5f4ef`, each board an iframe 1440×900 (light page around dark mocks).

## Authored palette (exact)

From the Design-language page and launcher CSS/JS (`extracted/launcher-root.css`, `launcher-root-dc.js`):

| Token | Value | Source evidence |
| --- | --- | --- |
| Ink | `#EDEDEF` titles | `.row-t`, design page |
| Ink 2 | `#A3A4A9` body | design page |
| Ink 3 | `#8E8F94` labels/meta | `.label`, `.row-s` |
| Selection | `rgba(255,255,255,.085)` | `.row.sel` |
| Accent | `#C9EE6A` caret, ↵, on | `const accent = this.props.accent ?? '#C9EE6A'` (dc.js) |
| Danger | `#FF9A92` quit, delete | design page |

## Authored materials (exact CSS)

- **L1 Panel `.glass`:** `background: linear-gradient(180deg, rgba(255,255,255,.05), rgba(255,255,255,0) 36%), rgba(22,23,26,.7); backdrop-filter: blur(44px) saturate(160%); border-radius: 18px; box-shadow: inset 0 0 0 1px rgba(255,255,255,.075), inset 0 1px 0 rgba(255,255,255,.1), 0 0 0 .5px rgba(0,0,0,.75), 0 50px 120px -30px rgba(0,0,0,.72), 0 16px 40px -16px rgba(0,0,0,.5)`
- **L2 Popover `.pop`:** `background: linear-gradient(180deg, rgba(255,255,255,.06), rgba(255,255,255,0) 40%), rgba(38,39,43,.82); backdrop-filter: blur(30px) saturate(160%); border-radius: 14px; box-shadow: inset 0 0 0 1px rgba(255,255,255,.09), inset 0 1px 0 rgba(255,255,255,.1), 0 0 0 .5px rgba(0,0,0,.8), 0 28px 70px -14px rgba(0,0,0,.75)` (popover example 320px wide)
- **L3 Toast & HUD:** tint 88%, radius 12, no blur (design page text; L3 CSS not yet located in extracts)
- **Wallpaper `.wp`:** `#0a0b0d` base; `.bands` inset −140px, `filter: blur(36px)`; dusk =
  `linear-gradient(122deg, …rgba(255,168,112,.42)… rgba(146,154,255,.34)… rgba(255,140,170,.22)…), radial-gradient(60% 60% at 76% 22%, #3b305e …), radial-gradient(55% 55% at 14% 86%, #4b2b30 …), #130f22`; a `.wp-daylight` variant exists
- **Grain:** inline SVG `feTurbulence` fractalNoise overlay, `opacity:.16`, `mix-blend-mode: overlay`
- Authored readability claim (design page): tint never below 55% supposedly keeps 4.5:1 on any wallpaper. **Not validated, and not a reliable guarantee:** the dark 70% tint over white composites to roughly `#5c5d5f`; muted `#8e8f94` is only about 2.04:1 there. Treat tint and text contrast as separate review concerns.

## Authored icon treatment (exact — `appTone` map in `launcher-root-dc.js`)

All icon gradients are vertical `180deg` top→bottom; second value is the glyph color:

| App | Gradient | Glyph |
| --- | --- | --- |
| term | `#4a4d55 → #1c1e22` | `#c8f5b4` (green terminal glyph) |
| code | `#45a3f5 → #1d62c8` | `#ffffff` |
| web | `#ffa24d → #e2530f` | `#ffffff` |
| note | `#a184ff → #5b3bd0` | `#ffffff` |
| music | `#3ddc78 → #129245` | `#ffffff` |
| pen | `#ff739f → #cf2d63` | `#ffffff` |
| chat | `#86418b → #4a1a4d` | `#ffffff` |
| folder | `#74b6ff → #2f78de` | `#ffffff` |
| cal | `#ff8070 → #d6392a` | `#ffffff` |

Tile chrome (`.tile.app`): `box-shadow: inset 0 0 0 .5px rgba(255,255,255,.28), inset 0 1px 0 rgba(255,255,255,.35), 0 1px 3px rgba(0,0,0,.45)` — thin pale edge, top inset highlight, short bottom shadow.
Sizes: pinned-slot icon **42px / radius 11px** (inline style); row tile 28px / 7px (`.tile`);
toast tile 34px / 9px; selected-row mini tile 18px / 5px. Glyphs are stroke SVGs (`.ic`:
`stroke-width:1.6`, `.ic.b` 2, round caps/joins, `currentColor`).

## Authored geometry & type (launcher root)

- Window (glass panel) width **760px**; search field 64px tall, `font: 19px/400` (`.q`).
- Pinned slots (`.slot`): 100px tall, radius 12, `background: rgba(255,255,255,.035)`, inset 1px
  `rgba(255,255,255,.05)`, gap 9px; label 12.5px/500 `#d9dadd`.
- Rows (`.row`): 44px tall, radius 10, gap 12; `.row.sel` `rgba(255,255,255,.085)` + inset 1px
  `rgba(255,255,255,.05)`; title 14px/500 `#ededef`, subtitle 13px `#8e8f94`, right kind 12.5px
  (min-width 88px).
- Section label (`.label`): 12px/500 `#8e8f94`, 30px tall; separators 1px `rgba(255,255,255,.07)`.
- Keycaps (`.kbd`): 20px tall, radius 5, bg `rgba(255,255,255,.07)`; `.alias` Geist Mono 11px `#b9babe`.
- Fonts: **Geist / Geist Mono** (two embedded woff2 per page; `font-family:'Geist'` / `'Geist Mono'`).
- Keymap (design page): macOS/Windows/Linux share one layout; only modifiers change.

## Unknowns / not verified

- Which woff2 file is Geist vs Geist Mono (font binaries not parsed).
- L3 Toast & HUD exact CSS (tint 88%/radius 12 text only; not found in extracted pages so far).
- Other pages (action panel, calculator, no-results, snap HUD, plugin store, settings) extracted
  but not yet token-mined; same structure expected.
- Fixed panel height: not found — height is content-driven (list area hint 404px).
