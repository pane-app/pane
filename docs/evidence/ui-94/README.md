# #94 evidence: rich result rows and exact pointer/keyboard selection

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-04; see [docs/visual-workbench.md](../../visual-workbench.md).

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `554c4ef` (#93) plus this ticket's uncommitted changes; this ticket's commit is the result. `pane-visual-fixture.exe` SHA-256 `83114AF9…674E` (full hash in `workbench-run.json`). `compare/report.json.gz` is the full report, gzipped.

## What changed

**Core: a read-only presentation projection** (`pane_core::Launcher::presentation`, `presented_view`):

- Each root row carries:
  - its **kind**, from what activating it does (Command, Application, File, Link, Fallback), never from its title;
  - the command's active **alias**;
  - its **registered** global hotkey;
  - the **title ranges** the query matched (`title_matches`, by the matching rule).
- **Sections:**
  - "Commands" over a blank query's rows: root search's own order, no recent-use claim (#100);
  - "Results · N matches" over a query's rows;
  - "Fallbacks" over the fallbacks.

  Nothing about what is listed, its order or its dispatch changes. Off root search nothing is projected.

**The row** (`ui::result_row`):

- explicit slots: tile, title (matched part in the accent), subtitle, then right-aligned alias chip (Mono 11, #B9BABE, 2×6, r5, white 14% ring, 1.3 line height), key sequence and kind (12.5, at least 88 wide);
- 12px between all parts (the title/subtitle gap was 6);
- section labels: 30 high, 8/10/0, 12px/500, .01em tracking;
- one shared list composition (`ui::shell::with_section_labels`) for the launcher and the fixture.

**Root selection** (`app.rs`):

- **Movement.** Real pointer movement selects the row it is over. An event that repeats the position doesn't, and neither does the first event after the window shows. A pointer resting on a row never undoes the keys.
- **Clicks.** A click on an unselected row selects it; a click on the selected row invokes it once.
- **No scrolling.** Pointer selection never scrolls the list; only the keys' selection does.
- **Washes** change at once, with no root fade or press wash.
- **Freeze.** `freeze_pointer_selection` is the selection-freeze input that #95's Actions panel will use. The footer menu (today's Actions entry point) already holds the selection the same way while it is open.

Command and command-search rows keep their existing click-runs semantics and pointer fade.

**The fixture** takes its section labels from core's own rule (`pane_core::root_sections`) rather than a copy, and its pointer selection doesn't scroll, matching the launcher. Its long-content and unavailable rows now carry an alias and keys, so the native-only crops exercise truncation and a wrapping reason beside the right-hand parts.

**The comparison:**

- The alias label's ink top is measured with `ink_top`, which adds up a thin stem split across two pixel columns. The 'b' of "cb" falls on GPUI's subpixel phase across two columns and on Chrome's mostly in one, so core-pixel ink alone read the native label 2px lower. Placed against each chip's top, the two sides' glyphs are identical row for row: ascender at 5, x-height at 7, baseline at 12.
- A new harness check finds no text ink in the 12px gap before an alias chip, so a long title or reason can't run under the chip.

## Results (`compare/summary.md`)

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-native | 1647 | 0 | 0 |
| harness-reference | 155 | 0 | 0 |
| parity | 2415 | 86 | 30 |

**Selection** (parity, every capture). The selected and hover-only rows match by name:

- `root-hover`: the pointer moves onto Clipboard History, which is selected on both sides.
- `root-pointer-keys`:
  - pointed: Clipboard History selected;
  - Down under the resting pointer: Left Half selected, Clipboard History hover-only;
  - moved again: Clipboard History selected.

  See `crops/pointer-keys-*`.

**Rows** (`root-rest`, `root-hover`, `root-pointer-keys`, `root-focus`):

- 791 row checks pass: wash, tile, title ink, the 12px title/subtitle gap, the kind's ink, the alias chip's box and label ink, and the match highlight ("Clip" in the accent, `crops/typed-clip-side-by-side.png`).
- 430 key-sequence checks pass.
- Every section label both sides show matches in height, title ink and note ("Results · 1 match").

**Remaining parity failures**, none of them #94's own:

- 56 "row top in client": the reference's pinned strip (#101) and its second label.
- 30 footer-button checks, height and fill (#95).

The 15 "alias label ink top" checks that failed by 2px in the first #94 runs now pass. They were a measurement artifact (see "The comparison" above), not a placement difference.

**Native-only states** (no reference counterpart, reviewed by eye):

- `crops/long-content-native.png`: the long title ends in an ellipsis short of the `long` chip, whose caps and kind keep their size. The harness measures this row's chip, gap, caps and kind, and finds no text ink in the gap.
- `crops/unavailable-native.png`: the reason wraps onto two lines short of the `cb` chip, and the chip, Ctrl Shift V and kind sit centred in the taller row. The harness doesn't measure this row's trailing parts: a row that grows past its floor is declared at the floor height, so their places can't be read from the declaration. This crop is the evidence.

**Accepted discrepancies:**

- **Section labels.** The reference's "Pinned / Suggested · From your recent use" over its blank list, against Pane's "Commands". Pane claims no recent use (#100), and the pinned strip is #101's.
- **The 1 corner (#92).**
- **The ← label** in the font subset (#93).

**Sensitivity:**
- Row inset +4: the wash's left edge 10 → 14.
- Selected fill: 22 → 64 levels.
- Hover fill, now on the resting-pointer row of `root-pointer-keys`: 9 → 51 levels. It flips the harness and, now that the hover wash matches, parity too.

## Tests (local, Windows)

```text
cargo fmt -p pane -p pane-core --check                                         ok
cargo check -p pane --tests --locked -j 1                                      ok
cargo build -p pane --locked -j 1                                              ok
cargo test -p pane --test window --test command_search --test keyboard --test aliases   75 + 1 + 18 + 1 passed
cargo test -p pane --lib                                                       26 passed
cargo test -p pane-core --test search --test aliases --test command_search     21 + 27 + 18 passed
cargo test -p pane-core --lib                                                  218 passed
python -m unittest (scripts/visual-workbench/test_compare.py)                  20 passed
```

New and changed tests:

- `root_rows_select_under_the_moving_pointer_at_once`: movement selects at once with no frame, the footer follows the selection, Enter opens it, and an opened command's items keep their fade. It replaces the superseded `a_result_row_fades_its_pointer_washes`.
- `a_resting_pointer_leaves_the_keys_selection_alone` and `a_click_selects_an_unselected_row_and_runs_the_selected_one`.
- `a_frozen_selection_ignores_the_pointer` and `an_open_footer_menu_holds_the_selection_against_the_pointer`.
- Comparison: `ink_top`'s tests (a stem split across two columns, a faint ring that isn't ink).
- `the_pointer_selects_a_half_shown_row_without_scrolling`.
- `root_rows_sit_under_their_section_labels`.
- Core: `root_search_presents_its_rows_with_kinds_sections_and_title_matches`, `rows_off_root_search_carry_no_presentation`, the alias presentation, and `title_matches`' unit tests.

**Timing flakes seen in earlier runs of this change, not regressions:**

- `helpers::runner::tests::stopping_an_owner_s_helpers_leaves_the_others_running` (pane-core lib) failed once in the full lib run and passes alone.
- `command_search::reloading_the_package_stops_its_search` (pane-core integration test `command_search`) failed once while a workbench run was building alongside it. It passes at the previous commit `554c4ef` and on this change: alone 3 of 3 times, and in the full `command_search` suite (18 passed).

Neither test touches the presentation projection.

**Not run:** the real-app smoke (it opens `pane.exe` active, and the operator was using the machine), 125% and 150%, and other OSes.
