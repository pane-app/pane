# Native validation — Settings search (#83)

Deferred: no native captures were made for this ticket. This file records
the plan for the run that will produce them, following
[`docs/evidence/settings-75/native-validation.md`](../settings-75/native-validation.md)
(the `capture-settings.ps1` helper pattern: scratch `PANE_DATA_DIR` and
`LOCALAPPDATA` per run, guarded real clicks, focus confirmed before keys,
closing only the process it started). The #84 pass runs it.

## Why deferred

The search's behavior is covered in-window by
[`crates/pane/tests/settings_search.rs`](../../../crates/pane/tests/settings_search.rs)
on GPUI's test platform: the one field `Cmd+F`/`Ctrl+F` focuses, typing
matching the registered settings with the core's ranking, arrows and
Enter and pointer navigation to the page and control, Escape clearing
then leaving, no-results, the unavailable entries' reasons, the reveal
scroll of a control that takes no focus, the focus of the one that does
(Shortcuts' filter), and registrations appearing and going with the
launcher's packages. What those tests cannot establish is the field's
real rendered appearance in the narrow sidebar, the reveal's feel on a
native desktop, and the platform key routing of `Ctrl+F` next to IME
input; that is what the captures below are for.

## Planned capture phases (Windows first, one data folder per phase)

| Phase | What is captured | Evidence |
| --- | --- | --- |
| 1 | Settings opened, the sidebar with the search field above the sections: the field's chrome, placeholder and magnifier at the 200px sidebar width, in both palettes | `01-sidebar-search.png`, `02-light.png` |
| 2 | `Ctrl+F` from a page control focusing the field (cursor and focus ring), then typing "dark": the results replacing the sections, the result's title and "Appearance · Theme" subtitle, the selected row's wash | `03-typed.png` |
| 3 | The arrows moving the selection, and Enter jumping: the Appearance page showing with the Dark choice at the top of the page area (the reveal), the sections back in the sidebar, focus on the sections | `04-jump.png` |
| 4 | The reveal with a short window: the Settings window at its 560×400 floor, a search for "solid", the jump scrolling the page area to the choice — before and after frames | `05-before.png`, `06-revealed.png` |
| 5 | No results: the "No settings match" line with the query quoted; Escape clearing it (sections return, field still focused); Escape again handing focus to the sections | `07-empty.png` |
| 6 | An override in force (`PANE_THEME=dark`): the theme choices' search results carrying the override reason, the jump still revealing the disabled choice with the page's notice | `08-overridden.png` |
| 7 | The Extensions page's registrations: a package installed with Settings open, the watcher adding its row to the same query's results; uninstalled, the row gone | `09-found.png`, `10-lost.png` |
| 8 | The Shortcuts filter jump: the page opened with the filter field focused, its caret ready | `11-filter.png` |
| 9 | Display scaling at 100% and 200%: the sidebar's field and result rows at both, checking truncation and the rows' legibility | recorded in the phase's `pane-run.json` notes |
| 10 | The keyboard path alone, with no pointer: `Ctrl+,`, `Ctrl+F`, typing, arrows, Enter, Escape, Escape, and the sections' arrows — each state captured between keys | `12-keys.png` per state |

Each phase also records the window's `pane-run.json` state where the
helper already collects it. macOS and Linux legs of the same phases once
those hosts are available, with `Cmd+F` for the find key on macOS.

## What the captures must also check

- IME composition in the search field: typing a composed sequence on a
  native input method stays composition (no result churn mid-composition,
  Enter committing the composition rather than opening a result), as the
  other Settings fields' captures do.
- `Ctrl+F` in the launcher window behind Settings does nothing (the key
  is the Settings window's own), and typing in the search field never
  reaches the launcher's query.
- The reveal's frame pacing on a native desktop: the scroll lands within
  the two frames the implementation defers, with no visible double-jump
  of the content.

## Not checked natively even then

- Ranking quality (relevance tuning beyond the first ranking, shared
  with root search's own known limits).
- Per-extension command rows in the search: deliberately not indexed
  (this ticket indexes host settings, not extension data; the Shortcuts
  page's own filter finds the commands).
- Any animation of the results or the reveal: none is implemented, per
  the motion policy's rule for query and result updates; #87's section
  transitions are separate.
