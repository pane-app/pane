# #93 evidence: type, icon tiles and Windows keycaps

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-04; see [docs/visual-workbench.md](../../visual-workbench.md).

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `a084715` (#92) plus this ticket's uncommitted changes; this ticket's commit is the result. `pane-visual-fixture.exe` SHA-256 `29515506…C14C` (full hash in `workbench-run.json`).

## What changed

- **Keycaps.** A key sequence is one cap per key, in three variants (`ui::keycap`):
  - regular `.kbd`: 20 high, at least 20 wide, padding 5, r5, Geist Mono 11/500 at line height 1, #C9CACE on white 7%, a white 8% ring over a black 35% bottom line;
  - compact `.slot-k .kbd`: 17/17, padding 4, 10px;
  - accent: lime #C9EE6A under #111210, no shadow.

  Caps are 3px apart. The group is one image node named for the whole binding.
- **Shadow order.** CSS paints its first shadow on top and GPUI its last, so the cap lists the reference's two inset shadows reversed: the ring lies over the bottom line, as in the reference.
- **Bindings.** The binding-to-keys adapter moved out of the shared layer into `keyboard::binding_keys`, so `ui/` no longer imports a core type.
  - **Order.** On Windows, Win leads (Windows' own convention, and the reference's Windows labels), then Ctrl, Alt, Shift, the key.
  - **Labels.** Enter and the arrows show their symbols under their full names. Shift+Enter, Ctrl+Enter and Ctrl+Shift+letter are shown and announced whole. The old one-text-cap path, which drew Shift+Enter as Enter, is gone.
- **Where the caps appear.** The footer's primary action uses the accent caps of the effective invoke binding. The menu's Settings hint uses regular caps. The Appearance preview now shows the effective binding instead of a hard-coded Enter.
- **Icons.**
  - New reference glyphs: pen, clipboard, layout, file, moon, lock. `globe` and `sliders` are redrawn to the reference's paths, which also changes the Settings sidebar and documentation icons.
  - New tone: pen.
  - Application tiles draw their glyph at the reference's 2px stroke (`.ic.b`); command tiles stay at 1.6.
  - Tile sizes 28/r7, 42/r11 and 18/r5 are theme tokens. A unit test guards the asset form the 2px derivation depends on.
- **Query field.** It gets a 2px text inset: the reference's `<input>` keeps the browser's default inline padding.

## Results (`compare/summary.md`)

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-native | 618 | 0 | 0 |
| harness-reference | 125 | 0 | 0 |
| parity | 1042 | 109 | 2 |

**Keycaps** (parity: 186 passed, 0 failed, 1 accepted). Labels, font (`11px Geist Mono 500`, compact `10px`), group widths, cap widths and heights, fill alpha, the accent's fill color, the bottom line's alpha and the label ink box all match:

| Group | Labels (both) | Group width native / reference |
|---|---|---|
| Actions | Ctrl \| K | 60 / 59.4 |
| Left Half | Win \| Alt \| ← | 86 / 85.6 |
| Clipboard History | Ctrl \| Shift \| V | 106 / 105.4 |
| primary | ↵ (accent) | 20 / 20 |
| pinned slot | Ctrl \| 1 (compact) | 52 / 52 |

The `←` label sits 2px higher than the reference's. That is an accepted discrepancy: the reference embeds Geist and Geist Mono as **225-glyph subsets** without `←`, `→`, `↑`, `↓` or `↵` (checked with fontTools on the hash-pinned payloads), so Chrome draws those labels in a system fallback face. Pane draws Geist Mono's own glyphs.

The ↵ cap matches within the limits, and the rebound chords (Shift ↵, Ctrl ↵, Ctrl Shift P) are native-only captures.

See `crops/keycaps-x4-native-left-reference-right.png` and `crops/keycaps-side-by-side.png`.

**Row tiles** (parity: 461 passed, 0 failed). Each row's tile edges and placement, app gradient ends (within 4 levels, a gradient's limit), the command tile's fill alpha, the glyph ink box and the glyph's core pixel count (within 25%) all match. See `crops/tile-*-native-top-reference-bottom.png`.

**Tile family** (`crops/tile-sizes-native.png`, native-only). 28, 42 and 18px as an application and as a command, all sizes within 1px. With the same glyph at the same size, the application stroke carries +10% (28px) and +37% (42px) more core pixels than the command's 1.6. At 18px the two strokes (0.9 and 0.7px) are under a pixel and are not compared.

**Fonts.** The manifest records how each face resolved:

- Geist 400 and 500, and Geist Mono 400 and 500, each resolve to their own face (four distinct font ids).
- A family no system has resolves to the fallback.
- The harness fails if any embedded face resolves like that missing family.

On metrics, Pane's TTFs (Geist 1.800, Geist Mono 1.700) and the reference's subsets have the same ascent, descent and units per em (1005, −295, 1000). Cap widths, heights and label positions match within 1px, so the 1.700 vs 1.701 difference is not visible here and no asset was replaced.

**Sensitivity:**
- Row inset +4: the wash's left edge 10 → 14.
- Selected fill: 22 → 64 levels.
- Hover fill: 9 → 51 levels.

## Not matched, and why

- **Query tracking (−.005em) is not applied.** GPUI's editable text element shapes its text with no letter spacing, whatever its style says. The typed query is about 0.1px per character wider than the reference's: under a pixel for the authored queries. This is recorded at `root_search::search_header`.
- **Text rasterization.** Pane renders text with subpixel antialiasing and Chrome's headless captures are grayscale, so colored fringes show in crops. Glyph checks use core pixels.
- **Row metadata and the title/subtitle gap (8 vs 13px)** are #94's: kind, alias, row key groups and the 12px gap. The footer button's size and fill are #95's.
- **Real identities.** Real rows keep the existing identity → icon map. The new glyphs and the pen tone draw the workbench's reference rows. A real Clipboard History or Files identity map needs the default extensions' identity keys, which #94's presentation projection provides.
- **Light keycap colors** are derived like the rest of the light palette. The reference has no light keycap.

## Checks run (local, Windows)

```text
cargo fmt -p pane --check                                       ok
cargo check -p pane --tests --locked -j 1                       ok
cargo build -p pane --locked -j 1                               ok
cargo test -p pane --test window --test keyboard --locked -j 1  69 + 18 passed
cargo test -p pane --lib --locked -j 1                          24 passed
python -m unittest (scripts/visual-workbench/test_compare.py)   18 passed
```

New tests:

- `a_chord_on_the_invoke_action_is_shown_announced_and_pressed_whole`: Shift+Enter and Ctrl+Shift+J are shown and announced whole, the chord invokes, and a bare Enter does not.
- `a_long_invoke_chord_stays_inside_a_narrow_footer`: Ctrl+Alt+Shift+Page Down at 300px.
- The adapter's unit tests.
- `every_glyph_states_the_stroke_the_bold_variant_replaces`.

**Not run:** the real-app smoke (it opens `pane.exe` active, and the operator was using the machine), 125% and 150%, and other OSes.
