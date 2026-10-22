# #99 evidence: the shared controls on the remaining Settings and host screens

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05, the wave-3 run `run-w3`; see [docs/visual-workbench.md](../../visual-workbench.md).

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `35dae93`, clean working tree. This ticket's commits are `40e1b5d` (the port) and `35dae93` (shared-controls workbench green). `pane-visual-fixture.exe` SHA-256 `40413E16…1A1F` (full hash in `workbench-run.json`).

**Files:**

- `compare/summary.md`: the whole run's summary.
- `compare/report.json.gz`: the run's report filtered to #99's eight scenarios, with the whole run's totals in `runTotals`.
- `compare-well-fill/summary.md`: the new sensitivity perturbation for the well.
- `crops/`: copied from `run-w3/compare/<scenario>/`, named `<scenario>-<file>`. Every #99 scenario is native-only, so there are no side-by-sides: the board authors none of these pages.

## What changed

- **`40e1b5d`, the port** (production). The consumer table and the duplicate-value review are in [docs/launcher-presentation.md](../../launcher-presentation.md#control-families-by-consumer-99).
  - General, Launcher, Shortcuts, Keyboard, Extensions and About, and the launcher's form, draw with the Settings board's families (`ui::controls`) instead of the launcher's result row. Only Appearance has an authored board layout; these pages are derived compositions, described as reference-consistent, not as matching a board.
  - The families are:
    - a page is a column of field groups;
    - a setting with a control is a settings row (44px under a white 6% rule);
    - switches are the board's toggle;
    - the Open Pane and Keyboard recorders are wells holding the binding's caps from the launcher's adapters (`keyboard::hotkey_keys`, `binding_keys`), with ghost Reset buttons;
    - actions are buttons (`.pill`) or ghost buttons;
    - text, a filter, alias and hotkey cells and the select's trigger are wells;
    - the select is a field whose list uses the Actions entries in the popover;
    - Launcher's reopening is a segmented choice;
    - Extensions' rows, commands and install sources are list items; Shortcuts' group headers are list headers;
    - notes, refusals and statuses are field descriptions in their tone;
    - the form's fields are field groups with a submit button;
    - an extension's custom view sits in the host's frame, and its drawing is untouched.
  - Washes change at once: the select's and the Settings controls' pointer fades and press washes are gone, and recorders take clicks only on their well.
  - Folded in during verification:
    - Dead code removed: `ui::result_row::result_row` (the launcher's lists, confirmations and details use `result_row_with`) and `Glyph::Download`/`Glyph::Copy` with their SVGs (About's rows became buttons).
    - The last core type in `ui/`, `impl From<&pane_core::Section> for SectionLabel` in `ui/shell.rs` (from #94), moved into the adapter as `app::section_label`. `ui/` no longer names `pane_core`.
    - Three of #99's own tests were fixed:
      - `the_form_draws_the_settings_field_families` read the fills mid-transition (30% opacity); it now settles the view transition first.
      - `a_popup_reopened_during_its_exit_retargets_and_blocks_nothing` clicked the reopening choice's centre, which the content-width popup no longer covers now that the choice is two segments. It now clicks where the popup and the segment overlap.
      - `the_install_rows_from_the_page_open_the_launcher_windows_flows` clicked an install row below the 1100px test window's fold (y 1110). `click_row` now scrolls a row into view first, as a user would.
- **`35dae93`, workbench green.** The first run of #99's scenarios failed 6 harness-native checks. All 6 were measurement faults, confirmed from the pixels; no page's layout changed.
  - **Heading ink left (5).** Pane's pages head with K, L, E and D, whose stems sit 80 units in (1.76px at 22/600), past the 1.5px allowed. The board's A sits 0.42px in. The expected left is now the box plus the first glyph's bearing from Geist's outline (`first_glyph_bearing`). The shell had also declared the board's "Appearance" heading for every page; it now declares each page's own (pages' `TITLE` and `ABOUT`).
  - **Sidebar fill on `settings-launcher/open` (1).** The open select's popover shadow reaches across the sidebar's rule and darkens both sides unevenly to about 130px below the list (23.1 against 26). The page now declares `selectOpen`, and the sidebar's fill is then read in its last 100 rows (`sidebar_rows`).
  - **Fixture.** `form-validation` drew its footer status as a bare div, 14px above the mark. The launcher's status message moved from `LauncherWindow::render` into `ui::footer::status_message` (padding `footer_status_padding_y`), which both now use.
  - **New perturbation** `well-fill`: `theme.field_fill` at black 50% against the authored 24%, driven on `settings-keyboard`.

## Results

Whole run: harness-reference 534/0, harness-native 7197/0, parity 10459 passed, 0 failed, 131 accepted. Every scenario outside #99's has exactly run-w2's counts (harness-native 6541 + 656 new).

| Scenario (capture) | harness-native |
|---|---|
| `settings-general` (general) | 82 |
| `settings-general-recording` (recording) | 84 |
| `settings-launcher` (closed, open) | 114 |
| `settings-keyboard` (keyboard) | 133 |
| `settings-extensions` (list) | 72 |
| `settings-extensions-confirm` (confirm) | 57 |
| `settings-about` (about) | 82 |
| `form-validation` (rejected, 760×518) | 32 |
| **Total** | **656 passed, 0 failed** |

There is no harness-reference or parity count for these: no board authors them. **Accepted discrepancies:** none new.

**Sensitivity** (`compare-well-fill`): the well at black 50% flips 8 checks, every recorder well's "well fill alpha" in `settings-keyboard` (61.1 becomes 129.4 against 61, limit 2 levels). The other five flip as in run-w2: row inset 8, selected 8, hover 2, nav 6, segment-on-fill 8.

## What the images show (looked at by eye)

- `settings-general-general-native.png`:
  - the Open Pane recorder's well with Ctrl Alt Space and a dimmed Reset;
  - the launch-at-login switch on in the accent;
  - the tray row at 40% with its switch off, its reason in the warning colour under it.
- `settings-general-recording-recording-native.png` and its `-well-open-pane-recorder` crop:
  - the well says "Press the keys…" in the accent under a light focus ring;
  - the long refusal wraps over two lines in the danger colour under the row.
- `settings-launcher-closed-native.png`:
  - the Display select as a field (label, well with a chevron, description);
  - Reopening as two equal segments, "Restore the current view" chosen;
  - the dismissal note.
  - The sidebar shows General selected: the board has no Launcher section.
- `settings-launcher-open-native.png`:
  - the list in the popover under the trigger: its search well focused, then Primary display highlighted with the accent dot, Pointer's display, and Active window's display dimmed with its reason;
  - the popover covers the Reopening field.
- `settings-keyboard-keyboard-native.png`:
  - eight rows, each with a recorder well at its right holding the binding's caps;
  - Back is rebound (Alt ←) with a Reset beside it, and its description drops "— the default".
  - The wells keep a 96px minimum and hold their caps at the end, so a one-cap well (↑, ↓, ↵) shows an empty dark area left of the cap.
- `settings-extensions-list-native.png`:
  - three list items for the package (the Develop one with its warning reason, crop `-item-Develop-Settings-sample`), Commands (Greeting) and Install (folder, npm, Git, with their coloured tiles);
  - each item ends in a chevron.
- `settings-extensions-confirm-native.png`:
  - the confirmation's heading, its two lines in the body ink, then "Disable all 2" (production's label) and Cancel;
  - both answers are identical list items: the destructive one has no danger tone (question 3).
- `settings-about-about-native.png`:
  - the version, Updates with "Check for updates", Documentation with "Open documentation" and its green "Opened …" status, and Diagnostics with "Copy diagnostics" and its green status;
  - all three are `.pill` buttons.
- `form-validation-rejected-native.png`:
  - "Greet someone" under the launcher's heading;
  - the Name well focused (light ring, accent caret) with "Enter a name" in the danger colour under it;
  - Greeting as three segments with "Good morning" chosen, and the Greet button;
  - the footer's status "Name: Enter a name" centred beside the mark.

## Tests (local, Windows, at `35dae93`)

```text
cargo fmt -p pane --check                          clean
cargo check -p pane -p pane-core --tests           no warnings
cargo test -p pane --lib                           58 passed
cargo test -p pane --test settings                 62 passed
cargo test -p pane --test launcher_settings        27 passed
cargo test -p pane --test shortcuts                24 passed
cargo test -p pane --test keyboard                 19 passed
cargo test -p pane --test window                   101 passed
cargo test -p pane --test settings_search          8 passed
cargo test -p pane --test open_pane                12 passed
cargo test -p pane --test tray                     9 passed
cargo test -p pane --test install                  12 passed
python -m unittest test_compare                    56 passed
```

`aliases`, `command_search`, `runtime_crash`, `unresponsive` and `update` passed at `8e0f1b6` (the port before the green commit and the adapter move).

**#99's window tests:**

- `settings.rs`:
  - `the_general_page_draws_the_settings_control_families`
  - `the_keyboard_page_shows_bindings_in_recorder_wells`
  - `the_about_pages_actions_are_buttons_whose_washes_change_at_once`
  - `the_extensions_page_lists_its_rows_as_list_items`
- `launcher_settings.rs`: `the_select_is_a_settings_field_whose_washes_change_at_once`, which replaces `the_selects_trigger_fades_its_pointer_washes`.
- `shortcuts.rs`: `the_pages_controls_are_the_settings_families`
- `window.rs`: `the_form_draws_the_settings_field_families`

**Not run or not verified:**

- 125% and 150% DPI.
- The real `pane.exe` smoke.
- Glass and shadow on-screen capture, which needs the user's consent.
- The Shortcuts page has no workbench scenario; `the_pages_controls_are_the_settings_families` covers its families.
- An extension's custom view in the host's frame has no scenario, and no test asserts the frame's ring. The existing custom-view window tests (drawing, input, resize) pass with it.
- The workbench fixture attaches no behaviour to the pages' controls. Their callbacks are proved by the window tests, not by the captures.
