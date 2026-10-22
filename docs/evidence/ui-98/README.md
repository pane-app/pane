# #98 evidence: the Appearance controls and live preview

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05, the wave-2 run `run-w2`; see [docs/visual-workbench.md](../../visual-workbench.md).

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme (`appearance-light` asks for the light palette itself).

**Revision:** `8cfad9e`, clean working tree. This ticket's commits are `510b7b8` (the port) and `8cfad9e` (Appearance workbench green). `pane-visual-fixture.exe` SHA-256 `0CFDFB2F…36BA2` (full hash in `workbench-run.json`).

**Files:**

- `compare/summary.md`: the whole run's summary.
- `compare/report.json.gz`: the run's report filtered to the six `appearance-*` scenarios, with the whole run's totals in `runTotals`.
- `compare-segment-on-fill/summary.md`: the sensitivity perturbation for the chosen segment.
- `crops/`: copied from `run-w2/compare/<scenario>/`. In each side-by-side, native is on the left of the magenta bar and the reference on the right. In each `crop-parity` image, native is above the bar and reference below.

## What changed

- **`510b7b8`, the port** (production):
  - `ui/controls.rs`: the field group (a 13.5/500 label 8px over its control, a 12.5 description at line height 1.45, groups 18px apart) and the segmented choice (a 36px track, black 24% under a white 6% ring, radius 10, padding 3; 30px segments 2px apart, radius 7, 12.5/500; the chosen one white on white 12% under a white 8% top inset). The board's swatches, range input and toggle are there for the reference fixture only.
  - `ui/preview.rs`: the 400×520 stage (radius 16, white 8% ring) and the 340px miniature 56px below its top, drawn with the theme, material and bindings in effect. It paints no wallpaper.
  - `features/settings/appearance.rs`: Theme (System, Light, Dark) and Material (Glass, Solid) in the segmented family. A choice repaints both windows and saves; a failed save shows and rolls back; an override names itself and disables the choices (labels and tracks at 40%, descriptions at full strength). Segments are tab stops; Enter and Space choose.
  - The heading block's and caption's line boxes are the board's (28, 17, 16).
- **`8cfad9e`, workbench green** (measurement only; no production or fixture change). The first run of #98's scenarios failed 68 checks. Each was a measurement fault, confirmed by dumping both sides' pixels:
  - **Track edges.** The board's glass is 25 levels at a track's left and 38 at its right, so a single page read beside the left end lost the right edge: Material measured 236px wide, and Density not at all. The light ring is darker than the page and was counted with the fill (+2px). An overridden track at 40% moves the page by only 2 levels. Tracks now take their ring's edges per scan line (`local_edges`, 3 levels).
  - **Segment washes.** Each was read against the track's fill at the track's left end, which over the glass read 5–6 levels on the board's unwashed Solid and Roomy. They are now read against the track's fill just left of each segment. The light palette's chosen segment (white 85%) was read as a black overlay (−34 against 217); it is now read as white.
  - **Alpha resolution.** One level of a black 24% over the page's 25 levels is about 10 alpha levels; the board's track reads 61 or 51 by one level. Track fills and segment washes keep the flat-fill limit in channel levels (2 over their background), with the alpha equivalent in each check's note.
  - **The miniature.** The light caret is `theme.accent_text`'s darkened green, not the lime `is_accent` looked for. The light miniature darkens toward its top and levels off at its first row, and the glass one is 2 levels darker mid-row than at its sides, so a row's wash is now read against the gap above it moved by its side padding's change (`mini_row_wash`). The board's fourth slot read 3px high, against glass lighter above it than beside it; slots now use `local_edges`.
  - **A dimmed description.** At 40% its core threshold is about 28 levels over the page, and the glass's bright band at the column's right counted as ink (478 against 482). Descriptions are read along their first 160px.
  - **The light search well.** Its black ring is part of its box and is no longer grown by a pixel.

## Results

Whole run: harness-reference 534/0, harness-native 6541/0, parity 10459 passed, 0 failed, 131 accepted. Every scenario outside #98's has exactly run-w1d's counts.

| Scenario | harness-native | harness-reference | parity passed | parity failed | accepted |
|---|---|---|---|---|---|
| `appearance-page` (rest, segment-hover) | 263 | 84 | 460 | 0 | 4 |
| `appearance-solid` (solid) | 135 | 42 | 231 | 0 | 2 |
| `appearance-production` (native-only) | 101 | — | — | — | — |
| `appearance-light` (native-only) | 101 | — | — | — | — |
| `appearance-override` (native-only) | 102 | — | — | — | — |
| `appearance-narrow` (native-only, 760×520) | 79 | — | — | — | — |
| **Total** | **781** | **126** | **691** | **0** | **6** |

**Accepted discrepancies:** none new. The 6 are #97's two dispositions, carried by each capture that shows the Settings shell: the sidebar's black 10% (24.9 against the glass's 28, less than one level of the page apart) and General's glyph box (10 against 8 around the same centre, GPUI's sprite contrast).

**Sensitivity** (`compare-segment-on-fill`): the chosen segment at white 30% instead of 12% flips 8 checks. These are Glass's and Default's "segment on wash alpha" in `appearance-page`, harness and parity, both captures: 31.2 becomes 77.1 against 31, with a 2.1-level limit. The other four perturbations flip as before (row inset 8, selected 8, hover 2, nav 6).

## What the images show (looked at by eye)

- `crops/appearance-page-rest-side-by-side.png`:
  - The fields, tracks, swatches, sliders, toggles, preview and link line up with the board's.
  - The reference's stage shows its miniature wallpaper; Pane's stage is empty behind the miniature (accepted design: no faked translucency).
- `crops/appearance-page-segment-hover-side-by-side.png` and `-crop-parity-track-material.png`: the pointer over Frost lightens its label on both sides; nothing else changes.
- `crops/appearance-solid-solid-side-by-side.png` and `-crop-parity-track-material.png`: Solid takes the wash and white label, its note replaces Glass's, and the Blur and Tint groups dim to 40% on both sides.
  - The range inputs differ: the board's disabled range is the browser's grey control, Pane's slider family keeps its accent at 40%. Sliders are compared by label and value only.
- `crops/appearance-solid-solid-crop-parity-preview.png`: the solid miniature matches row for row; its fill is compared here.
  - Two differences are not compared: the board tints the file row's tile blue (`#2b3542`), a tile kind Pane doesn't have, and the board's slot glyphs differ in shape from the fixture's (the third slot is a ⊖ against a globe, the second `<>` against a diamond).
- `crops/appearance-production-production-native.png`: production's page with Theme and Material and their descriptions, beside the preview (sample rows, four sample slots, "Ctrl+K for more actions" and the Enter cap).
- `crops/appearance-light-light-native.png`: the light palette; the chosen segments are white over the track, and the caret is the darker green.
- `crops/appearance-override-override-native.png`: the override notice in the warning colour; labels and segments at 40%, descriptions at full strength.
- `crops/appearance-narrow-narrow-native.png`: at 760×520 the fields take the page's width and the preview wraps below them, past the window's bottom edge. Scrolling to it is not captured.

## Tests (local, Windows)

```text
cargo fmt -p pane --check                          clean
cargo test -p pane --lib                           56 passed
cargo test -p pane --test settings                 58 passed
cargo test -p pane --test settings_search          8 passed
cargo test -p pane --test launcher_settings        27 passed
cargo test -p pane --test shortcuts                23 passed (at 510b7b8)
cargo test -p pane --test open_pane                12 passed (at 510b7b8)
cargo test -p pane --test window                   100 passed (at 510b7b8)
python -m unittest test_compare                    48 passed
```

`8cfad9e` changes no Rust, so the last three were not re-run.

**New window tests** (`crates/pane/tests/settings.rs`):

- `the_appearance_choices_are_the_reference_segmented_family`
- `the_live_preview_is_the_reference_miniature_in_the_appearance_in_effect`
- `the_keyboard_reaches_and_chooses_a_segment`
- `overridden_segments_take_no_keyboard_focus`

The existing appearance tests still pass:

- `choosing_a_theme_re_renders_both_windows_and_the_preview`
- `the_system_choice_renders_the_appearance_the_system_reports`
- `the_material_choice_switches_the_panel_surface`
- `the_saved_choice_is_reloaded_by_a_fresh_application`
- `a_failed_save_is_reported_and_the_shown_choice_stays_what_was_saved`
- `a_development_override_wins_is_indicated_and_is_never_saved`

**Not run or not verified:**

- 125% and 150% DPI.
- The real `pane.exe` smoke: a real choice, restart and save failure are covered by window tests, not a real process.
- Glass and shadow on-screen capture, which needs the user's consent. The glass miniature's colour is compared only in the Solid state.
- The narrow page scrolled to its preview.
