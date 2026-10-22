# #97 evidence: the Settings shell, sidebar and page composition

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05, the wave-1 run `run-w1d`; see [docs/visual-workbench.md](../../visual-workbench.md). The same run is the evidence for #96, #101 and #102.

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `55882ba`, clean working tree. This ticket's commits are `f69ac68` (the port) and `150ce57` (Settings workbench green). `pane-visual-fixture.exe` SHA-256 `8921F47A…D0BE` (full hash in `workbench-run.json`).

**Files:**

- `compare/summary.md`: the whole run's summary.
- `compare/report.json.gz`: the run's report filtered to `settings-shell` and `settings-shell-narrow`, with the whole run's totals in `runTotals`.
- `compare-nav-selected-fill/summary.md`: the sensitivity perturbation for the sidebar.
- `crops/`: copied from `run-w1d/compare/<scenario>/`. In each side-by-side, native is on the left of the magenta bar and the reference on the right. In each `crop-parity` image, native is above the bar and reference below.

## What changed

- **The window** opens at the board's 1120×720, or the primary work area less 24px a side, never below 560×400. It uses the board's .78 glass tint.
- **The shell** (`ui/settings_shell.rs`):
  - the 48px titlebar with its label centred;
  - on Windows, Pane's caption buttons (46 wide, full height) in place of the board's lone close glyph;
  - the 232px sidebar (12/10 padding, black 10%, a 1px rule) with the 34px search well (black 24% under a white 6% ring).
- **The sidebar items** are their own 36px family:
  - a 16px glyph and a 13/500 label, #B3B4B9 at rest, white 5% on hover and white 9% when selected;
  - the Extensions page's installed count at the right end;
  - washes change at once.

  Search results use the same family, with their page and group under the name.
- **The page** is padded 26/32/24 and opens with the 22/600 heading and 13px subtitle. Appearance lays out its controls and preview in 388/36/400 columns, which wrap into one column below the canonical width.
- **The frame loop.** Every sidebar item now attaches a hover style in every state. GPUI only updates an element's remembered hover while a hover style is attached, so a section the pointer left while selected kept a stale hover and retargeted its fade every frame. That loop made `moving_up_the_sidebar_arrives_from_above` and `rapid_section_switches_retarget_the_arrival_from_where_it_is` fail. They had failed since `554c4ef` (#93) and now pass.
- **The search field** (`150ce57`, a production fix):
  - The magnifier keeps its 14px (`flex_none`). The input had squeezed it to about 12.5px and moved the placeholder 1.5px left.
  - The text starts 2px in, as the reference's `<input>` keeps the browser's padding.
  - The placeholder's ink now lands at 45 on both sides; before the fix it was 41 against 45.

## Results

Whole run: harness-reference 408/0, harness-native 5760/0, parity 9768 passed, 0 failed, 125 accepted.

| Scenario | harness-native | harness-reference | parity passed | parity failed | accepted |
|---|---|---|---|---|---|
| `settings-shell` (rest, hover, selected-hover) | 134 | 58 | 295 | 0 | 6 |
| `settings-shell-narrow` (native-only, 760×520) | 47 | — | — | — | — |
| **Total** | **181** | **58** | **295** | **0** | **6** |

**Accepted discrepancies:**

- **Sidebar fill alpha (3).** Both sides draw black 10% (25.5 levels). Pane reads 24.9. Chrome's glass composites the same overlay 0.2–0.3 of a level darker and reads about 28. This is accepted only while Pane matches its declaration and the two differ by less than one level of the page.
- **General's glyph height (3).** It reads 10 against 8, around the same centre. GPUI's sprite contrast lifts the partly covered end rows of a round cap. This is accepted only for a native box larger by at most 2px, around a centre within 1px.

**Sensitivity** (`compare-nav-selected-fill`): raising the selected item's wash flips 6 checks. These are Appearance's "nav wash alpha" in all three captures, harness and parity: 23.0 becomes 63.9 against 23.

## What the images show (looked at by eye)

- `crops/settings-shell-rest-side-by-side.png`:
  - The titlebar, sidebar, search well, items and heading line up.
  - Appearance carries the selected wash on both sides.
  - The native side has minimize, maximize and close where the reference has one close glyph.
  - The native page body is empty below its heading and "Preview" label. The Appearance page's controls are #98's (its `appearance-page` scenario is pending), so this run compares the shell only.
- `crops/settings-shell-hover-side-by-side.png`: General carries the hover wash on both sides.
- `crops/settings-shell-selected-hover-side-by-side.png`: the pointer is over the selected Appearance item. At this size it looks the same as rest on both sides, as the reference's styles say it should.
- **Glyphs:** three sidebar glyphs differ in shape. Appearance is a split circle against the reference's palette, Privacy a lock against a shield, and About a sun against an info circle. The comparison measures glyph boxes, not shapes, so this passes; it is one of the questions below.
- `crops/settings-shell-rest-crop-parity-search.png`: the search well, magnifier and placeholder line up.
- `crops/settings-shell-rest-crop-parity-nav-Appearance.png` and `settings-shell-hover-crop-parity-nav-General.png`: the item washes and labels match, apart from the glyph shapes.
- `crops/settings-shell-narrow-narrow-native.png`: at 760×520 the sidebar keeps its width, every section stays listed, the subtitle fits on one line, and the caption buttons stay at the right.

## Tests (local, Windows)

A full `cargo test -p pane-core -p pane --no-fail-fast` ran on the integrated branch before the wave-1 fixes (log `.scratch/test-w1-full.log`, not committed). After the fixes:

```text
cargo fmt --check                                 clean
cargo test -p pane --test settings                54 passed
cargo test -p pane --test launcher_settings       27 passed
cargo test -p pane --test settings_search         8 passed
cargo test -p pane --test shortcuts               23 passed
cargo test -p pane --test open_pane               12 passed
cargo test -p pane --test keyboard                19 passed
```

**New tests:**

- `the_settings_window_opens_at_the_reference_shell_geometry`
- `the_sidebar_items_are_their_own_family_and_change_at_once`
- `the_appearance_preview_sits_beside_the_controls_and_below_them_when_narrow`
- `all_seven_pages_are_listed_reachable_and_searchable`
- `the_results_are_sidebar_items_not_launcher_rows`

The existing lifecycle tests still pass:

- `the_three_entry_points_converge_on_one_focused_settings_window`
- `closing_settings_reopens_a_new_window_without_ending_the_launcher`
- `the_titlebars_window_controls_close_only_the_settings_window`

**`launcher_settings` repairs.** Two of its tests failed against the new shell. They fail the same way with `ffc86ae`'s own sources, so the failures predate #97.

- The select test measured the trigger while the Launcher page's section arrival was still in flight. Its helper now delivers those frames.
- The toggle test's outside click landed without the pointer moving there. The pointer now moves first.

**Not run or not verified:**

- 125% and 150% DPI.
- The real `pane.exe` smoke, including real minimize, maximize, drag and resize of the Settings window. Only close is driven, by the test above.
- Glass and shadow on-screen capture, which needs the user's consent.
- A keyboard-focus state for sidebar items. The reference authors none (see the questions in the issue comment).
