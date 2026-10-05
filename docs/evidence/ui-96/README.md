# #96 evidence: the no-results notice and the computed-answer card

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05, the wave-1 run `run-w1d`; see [docs/visual-workbench.md](../../visual-workbench.md). The same run is the evidence for #97, #101 and #102 (`docs/evidence/ui-97`, `ui-101`, `ui-102`).

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `55882ba`, clean working tree (`workbench-run.json`: `workingTreeDirty: false`). This ticket's commits are `b3474a3` (the port) and `8785794` (root-family workbench green after #96 and #101). `pane-visual-fixture.exe` SHA-256 `8921F47A…D0BE` (full hash in `workbench-run.json`).

**Files:**

- `compare/summary.md`: the whole run's summary, every scenario.
- `compare/report.json.gz`: the run's report filtered to this ticket's scenarios. `meta.filteredTo` lists them, `totals` counts only them, and `runTotals` keeps the whole run's.
- `crops/`: copied from `run-w1d/compare/<scenario>/`, renamed `<scenario>-<file>`. In each side-by-side, Pane's native capture is on the left and the reference is on the right of the magenta bar. In each `crop-parity` image, native is above the bar and reference below.

## What changed

**Core** (`launcher/presentation.rs`):

- A computed result whose action copies text carries a `ComputedAnswer`: the query it answers, the text Enter copies, and the command that computed it. Its row keeps its id, selection and copy action.
- `answer_sections` puts a run of answers under its command's title ("Calculator"), and "Results" keeps counting the rows around it.

**Window** (`ui/result_layouts.rs`, `features/root_search/layouts.rs`):

- **The card:** the query, the arrow disc and the answer in Geist Mono 34/500 at −.03em. A value too long for its column steps down to 24, then 18 (which wraps). The accent ring shows while the card is selected.
- **The notice:** "Nothing matches “…”" and what to do, heading the list while only fallbacks are listed for a query. Fallbacks stay unselected until the user selects one (#100). A command's own search keeps its "No results" line.
- Only backed data is shown: no units, conversions, history, paste or store suggestions (#100).

**Workbench:**

- The empty and calculator boards (static) are compared.
- Three native-only shapes are captured through the launcher's own composition: a plain answer, a long one, and the notice with no fallback.

## Results

Whole run: harness-reference 408/0, harness-native 5760/0, parity 9768 passed, 0 failed, 125 accepted.

This ticket's scenarios:

| Scenario | harness-native | harness-reference | parity passed | parity failed | accepted |
|---|---|---|---|---|---|
| `empty-state` (notice, fallback-selected) | 130 | 48 | 291 | 0 | 18 |
| `calculator-card` (card) | 76 | 11 | 164 | 0 | 4 |
| `answer-plain` (native-only) | 38 | — | — | — | — |
| `answer-long` (native-only) | 38 | — | — | — | — |
| `empty-no-fallbacks` (native-only) | 28 | — | — | — | — |
| **Total** | **310** | **59** | **455** | **0** | **22** |

**Accepted discrepancies** (dispositions recorded in `compare/summary.md`):

- **Notice wording (4).** Pane says "Nothing matches", not "Nothing on this computer matches", because root search looks at commands, applications and a granted folder. It offers installing an extension because there is no plugin store to suggest one from.
- **Fallback titles (6).** The static empty board draws them plain. Pane paints the query's match in the accent, like the live root board.
- **Preselected fallback (2).** The static empty board preselects its first fallback. Pane's fallbacks stay unselected until a deliberate selection (#100).
- **"Reorder in Settings" (2).** Pane offers fallbacks in Manage extensions, with no order to change in Settings, so its Fallbacks label has no note.
- **Footer advertisements (3).** The empty board's footer advertises installing from a store, and the calculator board's advertises pasting the answer into the previous app. Pane does neither and keeps its hint (#100).
- **Glyph stroke weight (5).** The same 2px stroke at the same place. GPUI draws SVG icons through its text contrast and gamma correction, which lifts the anti-aliased edges. Only a heavier native glyph is accepted.

**Sensitivity:** none of the run's four perturbations targets these scenarios. Their summaries are in `docs/evidence/ui-101` and `ui-97`.

## What the images show (looked at by eye)

- `crops/empty-state-notice-side-by-side.png`:
  - The notice's disc, title and line sit at the same place on both sides, and so do the fallback rows and section labels.
  - The text differences are the accepted ones.
  - No fallback is selected on the native side, so its footer shows no primary action. The reference preselects one and shows "Search Web".
  - The "From the Plugin Store" block is board content drawn by the fixture. Production never shows it (#100).
- `crops/empty-state-fallback-selected-side-by-side.png`: after Down, the first fallback carries the selection wash on both sides, and the native footer shows Search Web.
- `crops/empty-state-notice-crop-parity-notice.png`: the notice's disc and lines line up. Only the wording differs.
- `crops/calculator-card-card-side-by-side.png` and `-crop-parity-answer-card.png`: the card's box, accent ring, values, captions, arrow disc, rule and "Also" chips match. The footer hint differs as accepted.
- `crops/answer-plain-answer-native.png` (with its card crop): the production shape for `6*7`, the 80px card with no captions or chips. Its ring shows because the card is selected.
- `crops/answer-long-long-answer-native.png` (with its card crop): a long expression steps both values down to the smaller Mono size, and both fit their columns.
- `crops/empty-no-fallbacks-notice-native.png`: with no fallback configured, the notice reads "Install an extension that knows about it, or offer a fallback in Manage extensions." The footer shows only Actions.

## Tests (local, Windows)

A full `cargo test -p pane-core -p pane --no-fail-fast` ran on the integrated branch before the wave-1 fixes. Its log is `.scratch/test-w1-full.log`, which is not committed. After the fixes, the targeted suites pass:

```text
cargo fmt --check                                         clean
cargo test -p pane --test window                          100 passed
cargo test -p pane --lib                                  51 passed
cargo test -p pane-core --test search --test calculator   22 + 14 passed
```

**New tests:**

- Core:
  - `an_answer_is_presented_with_its_query_under_its_commands_title`;
  - `computed_answers_sit_under_their_commands_title_and_results_keep_their_count`.
- Window: `a_computed_answer_shows_as_the_card_under_its_commands_title`.

During integration, one assertion was dropped from `typing_an_expression_shows_its_answer_and_enter_copies_it`. It expected the footer's "Copy answer" button while a status shows, but since #95 the footer hides the primary action while a status shows.

**Not run or not verified:**

- 125% and 150% DPI.
- The real `pane.exe` smoke: a real arithmetic query copied in the running app, and a configured fallback invoked there. The window tests cover both against the launcher.
- No window test drives the no-results notice itself. Its production shape is covered only by the `empty-no-fallbacks` capture, and the fallback behaviour by core's existing tests.
- Glass and shadow on-screen capture, which needs the user's consent.
