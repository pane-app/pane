# #102 evidence: text clipboard history in the split view

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05, the wave-1 run `run-w1d`; see [docs/visual-workbench.md](../../visual-workbench.md). The same run is the evidence for #96, #97 and #101.

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `55882ba`, clean working tree. This ticket's commits are `a3cc4c7` (the port) and `55882ba` (clipboard workbench green). `pane-visual-fixture.exe` SHA-256 `8921F47A…D0BE` (full hash in `workbench-run.json`).

**Files:**

- `compare/summary.md`: the whole run's summary.
- `compare/report.json.gz`: the run's report filtered to the seven `clipboard-*` scenarios, with the whole run's totals in `runTotals`.
- `crops/`: copied from `run-w1d/compare/<scenario>/`. In each side-by-side, native is on the left of the magenta bar and the reference on the right. In each `crop-parity` image, native is above the bar and reference below.

## What changed

**Core** (`launcher/clipboard_view.rs`) adds a narrow, read-only projection, `Launcher::clipboard_history`:

- **The gate.** The projection is gated on verified identity: the default extension `clipboard-history` and its command of the same id, open on its own list, with its package running.
- **What it reads.** It reads the existing package-scoped history store: the records newest first, with their text, copy time and source. It also reads the actual capture state, problem, retention and copy availability.
- **What it adds.** It adds no store, watcher or capture.
- **Operations.** Copy, delete and capture on/pause/resume are the history's existing operations. Each first checks that the reading still holds: the same screen, the same verified command, the same generation. A record deleted or expired is refused.
- **Browse rules:**
  - search over text and source, ignoring case;
  - All and Text tabs;
  - Today, Yesterday and Older groups in local time;
  - the selection falls back to the first record listed.

**Window** (`features/clipboard_history.rs`, `ui/split_view.rs`):

- **Header:** back, the command's chip, search, and Pause, Resume or Turn on, following the actual state.
- **Tab strip:** its caption says what is kept, as it is kept.
- **Body:** the 360px list beside the preview card (plain text, scrolling).
- **Footer:** 52px. It shows when and where the record was copied, with Copy (Enter), Delete (Ctrl+D) and Manage (Ctrl+K).
- **Keys and clicks:**
  - A click selects without copying.
  - Up and Down keep the selection in view.
  - Manage routes to the extension's own list (retention, exclusions, clear, turn off and delete), and Escape comes back.
  - Escape clears a query first, then leaves.
- **States.** Zero records, no match and an unreadable history are deliberate states, with no preview and no primary action.
- **Size and refresh.** The window takes the view's 940×600 while it shows. The view re-reads the history once a second.
- **Fixture only.** The reference's links, images, colours, Pinned group, Paste and privacy caption stay in the fixture (#100).

**`55882ba`** changed measurement and capture only, with no production change:

- the native capture now waits for the fixture to go idle;
- the reference capture scrolls the board's list as its own logic would;
- the fixture declares tabs and buttons at GPUI's laid-out widths;
- several `compare.py` edge readings were corrected.

## Results

Whole run: harness-reference 408/0, harness-native 5760/0, parity 9768 passed, 0 failed, 125 accepted.

| Scenario | harness-native | harness-reference | parity passed | parity failed | accepted |
|---|---|---|---|---|---|
| `clipboard-rest` | 44 | 8 | 96 | 0 | 0 |
| `clipboard-previews` (text, color, link, image) | 140 | 32 | 369 | 0 | 0 |
| `clipboard-keys` (down, last) | 72 | 17 | 177 | 0 | 10 |
| `clipboard-filter` (text-tab, no-match, cleared) | 98 | 14 | 190 | 0 | 1 |
| `clipboard-production` (native-only: rest, long-text) | 71 | — | — | — | — |
| `clipboard-off` (native-only) | 24 | — | — | — | — |
| `clipboard-narrow` (native-only, 760×518) | 71 | — | — | — | — |
| **Total** | **520** | **71** | **832** | **0** | **11** |

**Accepted discrepancies:**

- **The scroll cushion (10, `clipboard-keys/last`).** The reference scrolls the selected clip 8px past the list's edge. GPUI's `scroll_to_item` scrolls the least that shows it, as root search's list does. So every row and group label in that capture sits 8px lower on the native side. This is accepted only for an offset of exactly 8px.
- **The no-match footer (1, `clipboard-filter/no-match`).** With no clip listed there is nothing to paste or copy, so Pane's footer shows only Actions, as the approved contract says. The reference's static footer lists Paste to Obsidian and Copy anyway.

**Sensitivity:** none of the run's four perturbations targets the clipboard scenarios. Their summaries are in `docs/evidence/ui-101` and `ui-97`.

## What the images show (looked at by eye)

- `crops/clipboard-rest-rest-side-by-side.png`:
  - The header, tab strip, privacy caption, list groups, rows with timestamps, the code preview with line numbers, and the footer line up on both sides.
  - This is the board's fixture content. The code preview, the Links, Images and Colors tabs, the Pinned group, Paste to Obsidian and the privacy caption are fixture-only (#100).
- `crops/clipboard-rest-rest-crop-parity-clip-const-pane---createPane--.png` and `-clip-tab-All.png`: the selected row's wash, tile, title and time, and the selected tab, match.
- `crops/clipboard-previews-color-side-by-side.png`: **an unmeasured difference.** The swatch's hex, rgb and hsl lines sit about 10–14px higher on the native side than on the reference. The comparison checks only which preview branch shows for a colour clip, not where its text sits, so this passes unmeasured. Colour previews are fixture-only, but the gap is real in the fixture.
- `crops/clipboard-previews-image-side-by-side.png`: the hatched image placeholder, glyph and caption match.
- `crops/clipboard-keys-last-side-by-side.png`:
  - After moving to the last clip, #8FD3FF is selected and fully in view on both sides. Native scrolled 8px less (accepted).
  - The colour preview's text sits higher on the native side here too.
- `crops/clipboard-filter-no-match-side-by-side.png`: "No clips match. Try another filter." on both sides, with no preview and "Nothing selected". The native footer has only Actions (accepted).
- `crops/clipboard-production-rest-native.png`, the production shape:
  - "Clipboard History" chip, "Search 5 items…", Pause.
  - All and Text tabs, with the caption "Text is kept for 7 days · copies marked private are skipped".
  - Today, Yesterday and Older groups.
  - A plain-text preview in the proportional face.
  - Footer: "Copied today, 14:02 from WindowsTerminal.exe", with Copy, Delete (Ctrl D) and Manage (Ctrl K).
- `crops/clipboard-production-long-text-native.png`: a multi-paragraph record wraps inside the preview card, with the footer naming Thursday and notepad.exe. The capture shows the top of the text, so I can see the wrap but not the scroll.
- `crops/clipboard-off-off-native.png`: with history off, the header offers "Turn on" and the caption says nothing is kept. The list explains that history is off, there is no preview, and the footer shows only Manage.
- `crops/clipboard-narrow-narrow-native.png`: at 760×518 the list keeps its 360px and the preview card narrows. The caption, footer and buttons still fit.

## Tests (local, Windows)

A full `cargo test -p pane-core -p pane --no-fail-fast` ran on the integrated branch before the wave-1 fixes (log `.scratch/test-w1-full.log`, not committed). `clipboard_view` passed 11/11 there. After the fixes:

```text
cargo fmt --check                               clean
cargo test -p pane --test window                100 passed
cargo test -p pane --lib                        51 passed
cargo test -p pane-core --test clipboard_view   11 passed
```

**New tests:**

- **Core** (`clipboard_view.rs`, 11). They cover:
  - titles and groups by local day;
  - search over text and source;
  - the selection staying in range as the filter and deletions change the list;
  - only the registered default extension being projected;
  - copy and delete on a record still kept;
  - a stale reading running nothing;
  - a clipboard that can't be watched.
- **Window:**
  - `a_click_selects_a_record_without_copying_it_and_enter_copies_it`
  - `the_keys_move_the_selection_and_the_footer_copies_and_deletes`
  - `a_query_that_matches_nothing_previews_nothing_and_copies_nothing`
  - `the_capture_button_pauses_and_resumes_the_actual_history`
  - `manage_routes_to_the_commands_own_controls_and_escape_comes_back`
  - `a_similarly_titled_package_keeps_its_generic_list`

**A flaky test.** `window` `the_mouse_wheel_scrolls_away_until_the_rows_reload` failed once in #102's integration run (`.scratch/test-102.log`, under load). It passed alone, and it passed in the full run.

**Not run or not verified:**

- 125% and 150% DPI.
- The real `pane.exe` smoke with real copies on this machine. The window tests drive the view through the launcher in a test app.
- Glass and shadow on-screen capture, which needs the user's consent.
