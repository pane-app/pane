# Native validation — Shortcuts page (#75)

Deferred: no native captures were made for this ticket. This file records
the plan for the run that will produce them, following the pattern of
[`docs/evidence/settings-72/native-validation.md`](../settings-72/native-validation.md)
(`capture-settings.ps1` modeled on the established capture-pane helper:
scratch `PANE_DATA_DIR` and `LOCALAPPDATA` per run, guarded real clicks,
focus confirmed before keys, closing only the process it started).

## Why deferred

The page's behavior is covered in-window by
[`crates/pane/tests/shortcuts.rs`](../../../crates/pane/tests/shortcuts.rs)
on GPUI's test platform: grouping, the columns, inline add/replace/clear
with validation and cancellation, filtering, the keyboard path, the
lifecycle refresh through the window's watcher, and a restart over the
same data folder. What those tests cannot establish is the page's real
rendered appearance and its platform key routing on a native desktop;
that is what the captures below are for. The helper exists and needs only
the Shortcuts phases added to it.

## Planned capture phases (Windows first, one data folder per phase)

| Phase | What is captured | Evidence |
| --- | --- | --- |
| 1 | Settings opened from the launcher, the Shortcuts section chosen in the sidebar: the groups, the column labels, each command's Name/Alias/Hotkey row | `01-shortcuts-page.png` |
| 2 | A package installed beforehand (the query sample) with an alias and a hotkey seeded in the data folder: the alias and hotkey displayed, the hotkey in this system's key names | `02-records-displayed.png` |
| 3 | An alias edited inline: the cell clicked, the field, typing, the Enter commit and the status line | `03-alias-edit.png`, `04-alias-saved.png` |
| 4 | A refused alias (another command's, a space, 33 characters): the reason beside the field, the editor kept open | `05-alias-refused.png` |
| 5 | Escape cancelling an edit, and an empty commit clearing the alias | `06-alias-cancelled.png`, `07-alias-cleared.png` |
| 6 | The filter narrowing the groups; the no-match state | `08-filtered.png` |
| 7 | A group collapsed and expanded by its header | `09-collapsed.png` |
| 8 | The extension disabled from the launcher while Settings stays open: the watcher redraws the page with the not-active reasons, without a click in Settings | `10-disabled.png` (with a wait in the helper for the watcher's interval) |
| 9 | The small-window layout: the Settings window at its 560×400 floor with the page scrolled | `11-small.png` |
| 10 | A restart over the same data folder showing the alias and hotkey kept | `12-restarted.png` |

Each phase also records the page's `pane-run.json` state where the helper
already collects it. The keyboard path (Tab through the filter, a group
header and an alias cell; Enter opening the editor; Enter committing and
Escape cancelling) is exercised with the same guarded real keys the
Settings helper uses.

## What the captures must also check

- Perceptible legibility of the three columns at 100% and 200% display
  scaling (the in-window tests check layout structure, not readability).
- The `Ctrl+,` path to Settings still landing on one window with the
  Shortcuts page available, and the page's keys never reaching the
  launcher's window behind it.
- macOS and Linux legs of the same phases once those hosts are available,
  as the earlier milestones' evidence notes require: the hotkey column's
  platform key names differ, and the Wayland hotkey-unavailable note is
  visible only there.

## Not checked natively even then

- Hotkey **recording** from the page: the Hotkey column is display-only
  in this slice (#76 adds recording).
- Group expand/collapse **animation**: none is implemented; #87 adds it.
- Perceived smoothness or the page's appearance under a screen reader:
  the captures are structural evidence, as the previous milestones'
  notes say of theirs.
