# Native validation — Inline hotkey recording in Shortcuts (#76)

Deferred: no native captures were made for this ticket. This file records
the plan for the run that will produce them, following the pattern of
[`docs/evidence/settings-75/native-validation.md`](../settings-75/native-validation.md)
(the `capture-settings.ps1` helper with its scratch `PANE_DATA_DIR` and
`LOCALAPPDATA` per run, guarded real clicks, focus confirmed before keys,
closing only the process it started). #84's release-validation pass owns
the run; this file must say passed/failed with the capture files per row
once it happens.

## Why deferred

The recorder's behavior is covered in-window by
[`crates/pane/tests/shortcuts.rs`](../../../crates/pane/tests/shortcuts.rs)
on GPUI's test platform, through real keystrokes, clicks and the
accessibility tree, with a fake hotkey system that can be told which
shortcuts another application holds: inline recording and replacement
(registering the new binding before releasing the old one), captured keys
not acting (the sidebar's navigation and Tab's traversal), Escape
cancelling, the key-without-a-modifier refusal, collisions with another
command's binding and with the Open Pane binding, a registration another
application holds, the save-failure rollback (registration and record
back to what was last recorded), clearing by keyboard, the unavailable
command's plain label, the package lifecycle (disable stops registration,
enable restores it), a restart over the same data folder, and a press of
the recorded hotkey opening the command. What those tests cannot
establish is the real OS registration, the OS's key delivery while
another application has focus, and the page's rendered appearance; that
is what the captures below are for.

## Planned capture phases (Windows first, one data folder per phase)

| Phase | What is captured | Evidence |
| --- | --- | --- |
| 1 | The Shortcuts page's Hotkey cell for an installed command: the chip (or "None") as a button, and the Clear button beside a recorded hotkey | `01-hotkey-cell.png` |
| 2 | Clicking the cell starts the recorder: the listening mark, the hint under the cell, and the focus on the cell | `02-recording.png` |
| 3 | While the recorder listens, pressing navigation keys (arrows, Tab): the page does not move and the recorder stays listening — the keys are captured, not acted on | `03-captured-keys.png` |
| 4 | Recording a real combination (e.g. `Ctrl+Alt+G`): the cell shows the chip, the status line says what it now opens, and the scratch `hotkeys.json` holds it | `04-recorded.png` |
| 5 | The recorded hotkey pressed with focus in another application (a Notepad window the script opens): the command opens in the existing launcher | `05-opens-command.png` |
| 6 | Replacing the hotkey with another combination: the old registration released (pressing the old keys does nothing), the new one working | `06-replaced.png` |
| 7 | A collision: recording another command's keys, and the Open Pane default — both refusals explained under the cell, the recorder kept listening, both bindings untouched | `07-collision.png`, `08-open-pane-collision.png` |
| 8 | A registration the OS refuses (a second Pane instance in another data dir holding the same shortcut): the refusal explained, the previous binding still working | `09-taken.png` |
| 9 | Escape cancelling the recorder: nothing changed on the page or in the record | `10-cancelled.png` |
| 10 | Clearing through the Clear button: the registration released, the record rewritten, the keys opening nothing afterwards | `11-cleared.png` |
| 11 | The package disabled with Settings open: the watcher redraws the cell as a plain label with the not-active reason, and the registration is released; re-enabled, it is registered again | `12-disabled.png`, `13-re-enabled.png` |
| 12 | A restart over the same data folder: the hotkey registered at startup, shown on the page, and opening the command | `14-restarted.png` |
| 13 | A command unavailable on this system (a seeded record for a command declaring no platforms): the plain label with the reason, no recorder | `15-unavailable.png` |

## What the captures must also check

- The real OS registration through the adapters (`RegisterHotKey` on
  Windows, Carbon on macOS, `XGrabKey` on X11) — the fake in the tests
  answers `register` itself.
- Key delivery while another application has focus, including the
  modifier-only edge (pressing a modifier alone must not record).
- The macOS and Linux legs of the same phases once those hosts are
  available, as the earlier milestones' evidence notes require: the
  modifier names differ (Command/Option, Super), and the Wayland
  unavailable note — where recording is refused with the adapter's
  explanation — is visible only there.
- The Open Pane binding's independence on the real system: after a
  command hotkey is recorded, pressing the Open Pane default still
  summons and hides the launcher, and recording its keys for a command is
  refused.

## Not checked natively even then

- The record's concurrent write discipline (two windows of one Pane
  racing on `hotkeys.json`): single-instance is a later concern, as the
  domain notes say.
- Perceived smoothness or the page's appearance under a screen reader:
  the captures are structural evidence, as the previous milestones'
  notes say of theirs.
