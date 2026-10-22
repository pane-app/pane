# #95 evidence: the launcher footer and the contextual Actions panel

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity) on 2026-10-05; see [docs/visual-workbench.md](../../visual-workbench.md).

**Machine:** Windows 11 build 26200, 96 DPI (100%); 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme.

**Revision:** `d5e22d9` (#94) plus this ticket's uncommitted changes; this ticket's commit is the result. `pane-visual-fixture.exe` SHA-256 `B088A955…33C0` (full hash in `workbench-run.json`). `compare/report.json.gz` is the full report, gzipped.

## What changed

**Core** (`pane-core`):

- **`KeyboardAction::OpenActions`.** Ctrl+K by default (Cmd+K on macOS, where Ctrl+K deletes to the end of a field's line). It appears on the Keyboard page like every action of the set.
- **Reading a keyboard record.** The record's own bindings are read first, against each other. Then each action the record doesn't name takes its first free default: Open actions falls back to Ctrl+Shift+K if a record already uses Ctrl+K. This also fixes a record that moved a binding between two actions, which used to fail to load whenever its fields came in the wrong order.
- **`Launcher::result_actions`** (`launcher/actions.rs`). The selected root result's primary action, then, for an installed command, "Assign Hotkey…"/"Change Hotkey…" and "Add Alias…"/"Change Alias…". Nothing is listed without a working operation behind it (#100).
- **`open_result_action`.** Opens the same hotkey screen and alias form Manage extensions uses. The flow returns to the search it came from, with the same rows and the target still selected, and the outcome shows in the status. Opened from Manage extensions, those flows still return there.
- **`result_action_ready`.** Refuses a target that is no longer selected, or no longer has that action.

**The footer** (`ui::footer`), shared by the launcher and the fixture:

- the Pane mark (18px) at the left padding;
- the hint, "↵ opens instantly · Ctrl K for more", or "Type to filter actions · Esc goes back" while the panel is open, both in the bindings in force;
- on the right, the `.fbtn` buttons: the primary action (accent keys), a 1×16 rule, and Actions (Ctrl K).
  - Both are transparent at rest. The primary takes the 6% hover wash. Actions takes the 10% open wash and a white label while its panel is open, and no hover wash otherwise: in the reference, its inline background overrides `.fbtn:hover`.
- While a status shows, its message takes the hint's place and the primary action steps aside, so a double click on a quick open can't run it twice. Actions stays.
- **The Pane mark is the Pane menu's button.** This is a Windows/Pane adaptation: the reference draws the mark as decoration. The menu holds Settings and is not the contextual Actions panel (#100). Its items now use the Actions panel's entry style. Settings stays reachable by mouse (the mark), by the keyboard (Tab to "Pane menu", or Ctrl+,) and from root search.

**The Actions panel** (`features::actions_panel`):

- **Layout:**
  - the `.pop` popover, 320 wide, 10px in from the right and 8px above the footer, with its outer shadows;
  - the header: the target's 18px tile and title;
  - the entries: 36 high, radius 8, the 11% selected wash and 6% hover wash, and the primary entry's accent keys;
  - the "Pane" group after a rule, dropped while filtering;
  - a 44px search row;
  - "No actions match", or "Select a result to see its actions" with nothing selected;
  - the dimmer over the results (rgba(6,7,8,.34)).
- **Keys:**
  - The Open actions binding and the Actions button open and close it.
  - Typing filters it.
  - Up/Down move over the entries that can run.
  - Enter or a click runs an entry once.
  - Escape or Tab closes only the panel and returns focus to the query.
  - A mouse-down outside closes it, consumed, so the result it covered is never invoked.
- **The target:**
  - The panel holds its target: the pointer can't move the selection while it is open.
  - If the target leaves the results behind it, its entries show unavailable and run nothing.
  - The panel opens and closes at once; the reference authors no motion for it.

**Typography: kerning.** Browsers kern by default, and the reference's Geist is laid out kerned. GPUI on Windows passes DirectWrite an explicit feature list without `kern`. Pane now asks for kerning in its typography (`Typography::features`), at each window's root and in both editable fields. Unkerned, "opens instantly ·" at 12.5px was 94.95px against Chrome's 93; kerned it is 93.0.

**Shadows and draw order.** GPUI orders each primitive by the primitives its box overlaps, and a drop shadow's box leaves out its blur. So the panel's drop would sort above the footer's buttons and darken them, where the reference draws the footer above the shadow. The footer row carries an imperceptible fill (black at 1/255) whose box overlaps the shadow's, so its content sorts above it.

**Rows on a command's screens** keep their click-runs semantics, and now share root search's immediate washes: no press wash, no fade. This follows #100's "Command/command-search rows share visuals".

## Results (`compare/summary.md`)

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-native | 2314 | 0 | 0 |
| harness-reference | 183 | 0 | 0 |
| parity | 3580 | 67 | 39 |

**Footer and Actions** (`root-actions`, `actions-panel`, and every root capture's footer). 1081 checks pass in the two Actions scenarios. They measure:

- the mark's ink;
- each hint part's place and type;
- the rule's place and alpha;
- the Actions button's label, wash and keys;
- the panel's left, width and bottom;
- the header's tile and title;
- each entry's wash, glyph, label and height, paired by what it does;
- the group label, the rule and the search row;
- the empty note;
- the dimmer, each side against its own undimmed list.

**Remaining parity failures:**

- **64 "row top in client"**: the reference's pinned strip (#101).
- **3 "footer-actions label ink left"**, 633 against 635, in the open captures only. The native Actions button is about 1.4px wider than Chrome's fractional 125.6px, because layout puts its Ctrl cap on whole pixels (37 against 36.4). At rest the same check passes.

**Accepted discrepancies** (with their dispositions in the summary):

- **Entries.** The reference lists Pin to Quick Slot (#101), Open New Window, Show in File Manager, Quit and Hide from Results. Pane lists only the operations it can perform (#100).
- **The static Actions board's footer.** It shows a tip where the hint is, and draws its Enter as a plain cap.
- **Accepted earlier:** the section labels (#94), the corner (#92) and the ← label (#93).

**Sensitivity:**

- Row inset +4 flips 674 checks.
- Selected fill flips 44.
- Hover fill flips 22.

**Crops:**

- `crops/root-actions-*-side-by-side.png`: selected, open, filtered, empty and closed.
- `crops/actions-board-side-by-side.png`.
- `crops/open-crop-parity-*`: the panel, each entry, the Actions button and the mark.

## Tests (local, Windows)

```text
cargo fmt -p pane -p pane-core --check                                      ok
cargo check -p pane -p pane-core --tests --locked -j 1                      ok
cargo test -p pane --test window --test keyboard --test command_search --test aliases   84 + 19 + 1 + 1 passed
cargo test -p pane --test settings                                          48 passed, 2 failed*
cargo test -p pane --lib                                                    28 passed
cargo test -p pane-core --test result_actions --test aliases --test hotkeys --test search   9 + 27 + 21 + 21 passed
cargo test -p pane-core --lib                                               222 passed
python -m unittest (scripts/visual-workbench/test_compare.py)               20 passed
```

New tests:

- **Core:** `result_actions.rs` (9):
  - the list for an installed command and for Pane's own rows;
  - nothing off root search;
  - the alias and hotkey flows returning to the search;
  - Back from either;
  - the Manage-extensions path unchanged;
  - a target no longer selected;
  - matching.
- **Core keyboard:**
  - a record from before Open actions existed;
  - a record already holding Ctrl+K;
  - a record that moved a binding between actions;
  - a colliding record still failing whole.
- **Window:**
  - the binding opens the panel and Escape closes only it;
  - the button and the binding toggle one panel;
  - the primary action runs from the panel;
  - typing filters to "No actions match";
  - an outside click invokes nothing;
  - the pointer can't retarget;
  - a target gone from behind the panel runs nothing;
  - with nothing selected the panel says so;
  - an installed command's alias action opens its form and Escape returns to the search;
  - the footer buttons change their washes at once.
- **Keyboard:** `the_actions_binding_follows_the_keyboard_page`.

\* **`settings.rs`:** `moving_up_the_sidebar_arrives_from_above` and `rapid_section_switches_retarget_the_arrival_from_where_it_is` fail with "the window never stopped asking for animation frames". They fail the same way at `554c4ef` (#93) and `d5e22d9` (#94), so they predate this change. They concern the Settings shell (#97).

**Not run:** the real-app smoke (it opens `pane.exe` active while the operator uses the machine), 125% and 150%, and other OSes.
