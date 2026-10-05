# #101 evidence: the pinned home with five persistent quick slots

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05, the wave-1 run `run-w1d`; see [docs/visual-workbench.md](../../visual-workbench.md). The same run is the evidence for #96, #97 and #102.

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `55882ba`, clean working tree. This ticket's commits are `774245b` (the port) and `8785794` (root-family workbench green after #96 and #101). `pane-visual-fixture.exe` SHA-256 `8921F47A…D0BE` (full hash in `workbench-run.json`).

**Files:**

- `compare/summary.md`: the whole run's summary.
- `compare/report.json.gz`: the run's report filtered to `pinned-strip`, `pinned-partial` and the root scenarios the strip moved: `root-rest`, `root-hover`, `root-focus`, `root-pointer-keys`, `root-selected`, `root-selected-hover` and `root-actions`. The whole run's totals are in `runTotals`.
- `compare-row-padding-plus-4/`, `compare-selected-fill/`, `compare-hover-fill/`: the sensitivity summaries. They perturb root rows, which sit under the strip.
- `crops/`: copied from `run-w1d/compare/<scenario>/`. In each side-by-side, native is on the left of the magenta bar and the reference on the right. In each `crop-parity` image, native is above the bar and reference below.

## What changed

**Core** (`launcher/quick_slots.rs`, ADR 0026):

- **What a slot holds.** A slot holds an identity, never a row: a registered command by id, or an indexed result under its command. Computed answers, files and Pane's own rows cannot be pinned. A fresh install pins nothing.
- **Resolving.** Slots resolve through the enabled registry each time. A disabled, paused, missing or not-yet-listed target keeps its slot, says why it can't run, and can always be removed. The same identity resolves again once it is back.
- **Cold visits.** A cold visit asks a pinned application's command for its indexed results, without a query.
- **Invoking.** Invoking a slot resolves it again and runs in the generation current then. It runs nothing while another action is running.
- **Pinning:**
  - Pin to Quick Slot fills the first empty slot.
  - With all five taken, the slots to replace are listed and one must be chosen.
  - Re-pinning names the slot it is already in and changes nothing.
- **A slot's own actions** open, remove and move it, never past either end.
- **The record.** `quick-slots.json` sits beside `settings.json`. It is versioned and written atomically, one write at a time. A failed write puts the saved arrangement back and reports why. An unreadable record is reported and never replaced.

**Window:**

- `ui/pinned.rs` draws the "Pinned" label with its Ctrl 1–5 chord, and the five slots.
- `features/quick_slots.rs` handles:
  - click, Enter and Space on a focused slot;
  - the local Ctrl+1–5 chords, which act only on root search with no overlay or IME composition.

  A held chord's repeats and a double click's second click run nothing. An empty slot is a true no-op.
- The Actions panel gains:
  - the pin entries;
  - the replacement choice;
  - the slot's own panel, opened by secondary click or Ctrl+K on a focused slot.
- A query hides the strip, and clearing it brings the strip back. The rows below keep their real order under "Commands", with no claim of recent use.
- `8785794`, a production fix: an unavailable slot overflowed its 100px. Its reason now sits right under the title (`unavailable_gap` 2) and fits.

**Integration fixes, amended into `774245b`:**

- **A real bug.** Right-clicking a slot to open its Actions panel lost focus. That is now prevented. It was caught by `a_slot_whose_target_is_gone_says_why_and_its_own_actions_remove_it`.
- **The footer-menu test** (`settings.rs`) now Tabs through the five slot tab stops.
- **The `install.rs` click** moves the pointer first. That failure predates #101.

## Results

Whole run: harness-reference 408/0, harness-native 5760/0, parity 9768 passed, 0 failed, 125 accepted.

| Scenario | harness-native | harness-reference | parity passed | parity failed | accepted |
|---|---|---|---|---|---|
| `pinned-strip` (rest, query-hides, cleared-restores, slot-hover) | 636 | 37 | 1306 | 0 | 12 |
| `pinned-partial` (native-only) | 124 | — | — | — | — |
| `root-rest` | 199 | 10 | 398 | 0 | 4 |
| `root-hover` | 391 | 20 | 796 | 0 | 8 |
| `root-focus` | 444 | 27 | 908 | 0 | 8 |
| `root-pointer-keys` | 587 | 30 | 1194 | 0 | 12 |
| `root-selected` | 583 | 30 | 1194 | 0 | 12 |
| `root-selected-hover` | 391 | 20 | 796 | 0 | 8 |
| `root-actions` | 527 | 26 | 1026 | 0 | 13 |
| **Total** | **3882** | **200** | **7618** | **0** | **77** |

**Row tops now pass.** In #95's run, 64 "row top in client" checks failed because the reference has a pinned strip and Pane had none. With the strip in place, every "top in client" check in these scenarios passes: 4 in `root-rest`, 8 in `root-hover`, 9 in `root-focus`, 12 in `root-pointer-keys`, 12 in `root-selected`, 8 in `root-selected-hover`, 8 in `root-actions` and 13 in `pinned-strip`.

**Accepted discrepancies:**

- **Section label (18).** Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown (#94, #100). Both sides show "Pinned".
- **Glyph stroke weight (36).** The same 2px stroke at the same place. GPUI's SVG contrast and gamma lift the anti-aliased edges. Only a heavier native glyph is accepted.
- **The ← keycap symbol (18).** The reference's Geist Mono subset lacks it, so Chrome draws a fallback face (#93).
- **Actions entries (2)** (#95, #100). The reference lists Open New Window, Show in File Manager, Quit and Hide. Pane lists only what it can do, which now includes Pin to Quick Slot.
- **The open Actions label (3)** (#95). The reference's two boards disagree on its colour. Pane draws it white, as the static Actions board does.

**Sensitivity** (from `workbench-run.json`):

- **Row inset +4.** 8 "wash left" checks flip, the property it targets: 10 becomes 14. Altogether 482 checks flip (212 harness, 270 parity), because the inset also moves every row's trailing parts.
- **Selected fill.** 8 checks flip: 22 becomes 63.6.
- **Hover fill.** 2 checks flip: 8.8 becomes 51.1.

The selected and hover counts are lower than #95's (44 and 22). The integration notes put this down to #101's layout, because HEAD's `compare.py` gives the lower counts on the same captures. Each perturbation still flips the property it targets.

## What the images show (looked at by eye)

- `crops/pinned-strip-rest-side-by-side.png`:
  - The Pinned label and Ctrl 1–5 chord, five equal slots with tiles, titles and per-slot Ctrl n caps, and the rows below all sit at the same places on both sides.
  - Native shows "Commands" where the reference shows "Suggested · From your recent use" (accepted).
  - Native's placeholder reads "Search apps and commands…" against the reference's "Search apps, commands, plugins…". Text content is not a measured check.
  - Below Search Files, native shows a fifth row (Plugin Store, alias `store`) where the reference shows its second label, "Commands". Rows are paired by name, so that row is not compared.
- `crops/pinned-strip-query-hides-side-by-side.png`: typing "clip" hides the strip on both sides, leaving "Results · 1 match" with Clipboard History selected.
- `crops/pinned-strip-cleared-restores-side-by-side.png`: clearing the query brings the strip back. It looks the same as rest.
- `crops/pinned-strip-slot-hover-side-by-side.png` and `-slot-hover-crop-parity-slot-2.png`: the pointer over Visual Studio Code lightens that slot's fill on both sides by about the same amount.
- `crops/pinned-strip-rest-crop-parity-slot-1.png` and `-keys-quick-slots.png`: the slot box, tile, title and caps, and the label's chord, line up.
- `crops/pinned-partial-partial-native.png` (native-only), the production shapes:
  - Terminal in slot 1.
  - Empty slots 2, 4 and 5, showing "Empty".
  - Obsidian in slot 3, unavailable: its tile and title at reduced opacity, with "Notes is disabled" in the warning colour right under the title.

  Notes on the crops:

  - In `-crop-slot-2.png` the empty slot's outline is visible, but at this size I could not make out its dashes.
  - In `-crop-slot-3.png` the unavailable slot's Ctrl 3 caps stay at full strength while its tile is dimmed.
- `crops/root-rest-rest-side-by-side.png`: the same as the pinned rest capture. The root scenarios now carry the board's five pins.
- `crops/root-selected-selected-side-by-side.png`: after two Downs, Left Half carries the selected wash at the same top on both sides.

## Tests (local, Windows)

A full `cargo test -p pane-core -p pane --no-fail-fast` ran on the integrated branch before the fixes (log `.scratch/test-w1-full.log`, not committed). It failed:

- `install.rs` ×3, caused by an un-moved pointer;
- the `settings.rs` footer-menu test, which met the new slot tab stops;
- `window.rs` `a_slot_whose_target_is_gone_says_why_and_its_own_actions_remove_it`, the focus bug.

All three are fixed above. After the fixes:

```text
cargo fmt --check                                       clean
cargo test -p pane --test window                        100 passed
cargo test -p pane --test settings                      54 passed
cargo test -p pane --test launcher_settings             27 passed
cargo test -p pane --test install                       12 passed
cargo test -p pane --test keyboard                      19 passed
cargo test -p pane --lib                                51 passed
cargo test -p pane-core --test quick_slots              14 passed
cargo test -p pane-core --test result_actions           9 passed
```

**New tests:**

- **Core** (`quick_slots.rs`, 14). They cover:
  - fresh install;
  - pinning fills the first empty slot;
  - order survives a restart;
  - re-pin names its slot;
  - remove and move with bounds;
  - the full-slots replacement choice;
  - Pane's rows can't be pinned;
  - invoking, with an empty slot running nothing;
  - disabled, missing, and replaced-or-uninstalled generations;
  - an unreadable record kept;
  - a failed write put back;
  - an application pinned by identity and resolved cold.
- **Window:**
  - `a_blank_query_shows_the_pinned_home_and_a_query_hides_it`
  - `pinning_from_the_actions_panel_fills_a_slot_whose_chord_opens_it`
  - `a_click_on_a_slot_opens_what_it_holds_and_an_empty_one_runs_nothing`
  - `a_slots_chord_runs_nothing_while_an_overlay_or_a_composition_has_the_keys`
  - `a_slot_whose_target_is_gone_says_why_and_its_own_actions_remove_it`
  - `with_every_slot_taken_pinning_asks_which_slot_to_replace`
  - `pinning_what_a_slot_holds_names_that_slot_and_focuses_it`
  - `a_slot_runs_once_per_press_and_an_empty_one_ignores_its_keys`
  - `a_pin_that_cannot_be_recorded_is_put_back_and_reported`

**Not run or not verified:**

- 125% and 150% DPI.
- The real `pane.exe` smoke.
- A real restart with pins persisting. Persistence and restart are exercised only by core tests over the record file.
- Glass and shadow on-screen capture, which needs the user's consent.
