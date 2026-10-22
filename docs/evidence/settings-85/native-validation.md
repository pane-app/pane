# Native validation — the searchable settings select (#85)

**Status: plan only.** No native captures were made for this ticket: this
machine is the user's, the app is not built or run here, and the
milestone's native and visual validation pass is #84, which this ticket
blocks. This file is the capture plan that pass should follow for the
select, and it must record "passed/failed, evidence" per row once that
run happens. The automated results that *were* run are on CI (this
branch's `[verify]` legs):
[`crates/pane/tests/launcher_settings.rs`](../../../crates/pane/tests/launcher_settings.rs)
drives the real Settings window on GPUI's test platform — the select's
opening, filtering, arrows, Enter, Escape, Tab/Shift-Tab, outside clicks,
the trigger's toggle, no-results, disabled choices, IME composition, the
failed-save rollback and the fresh-application reload — with a **fake
placement** and the real host settings. Neither is a real compositor, a
real IME, a real screen reader or a real display scale.

## What this ticket ships

The Pane-styled searchable select
([`crates/pane/src/ui/select.rs`](../../../crates/pane/src/ui/select.rs)):
one `Select` entity a settings page embeds as a child, which owns its
whole transient state and interaction — the trigger row (a combo box
showing the committed choice), the L2 popover (the local search field
over the choice rows), the draft/commit boundary (opening, filtering and
highlighting save nothing; Enter or a click on an enabled row commits
through the consumer's path; Escape cancels; Tab/Shift-Tab leave without
committing; an outside click cancels and lets the click land), the
deferred-and-anchored popup placement (below the trigger, constrained
inside the window), and the native accessibility exposure (name, value,
expanded state, options' selection/availability, the highlighted option
as the focused field's active descendant). Its first consumer is the
Launcher page's opening-monitor choice
([`crates/pane/src/features/settings/launcher.rs`](../../../crates/pane/src/features/settings/launcher.rs)),
whose supported choices and persistence behavior are unchanged: the same
three preferences, the same per-platform explanations, the same
host-settings commit path and save-failure honesty. The popup's entrance
animation is deliberately absent — the motion policy keeps query and
result updates immediate, and #88 (blocked by this ticket) delivers the
popup micro-transitions on the seams left for it.

## What the harness cannot observe (why native evidence is required)

- **The real popover over the real panel.** The test platform renders
  the deferred/anchored overlay, but only a native run shows the L2
  popover's blur and tint over the real frost, its shadow, and its
  placement over the page's real rows at the platform's scale factor.
- **A real IME.** The test drives the editing state directly, as
  `window.rs`'s composition tests do; a real input method's panel, its
  marked text over the field, and Enter's commit of a composition are
  only observable natively (Japanese, Korean and an accent-marking
  layout are the candidates).
- **A real screen reader.** The a11y JSON the tests assert is the
  AccessKit tree, not a reader: a native pass must have the reader
  announce the trigger's name and value, the expansion, the highlighted
  choice as focus moves, and the no-results status.
- **Window-edge constraint with the real window manager.** The snap
  logic runs in tests against the test window; a native run must show
  the constrained popup near the real window's bottom edge, with the
  window's real frame and shadow.
- **The light palette and material fallback.** The tests cover dark
  glass and dark solid on the test platform; light/dark and
  glass-normalized-to-solid need real captures, as every settings page
  does.

## Capture plan (the #84 pass)

Reuse the harness of
[`settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1)
(scratch `PANE_DATA_DIR` and `LOCALAPPDATA`, guarded clicks, focus
confirmation), aimed at the Launcher page:

1. Capture the closed control: the trigger row with the committed
   choice ("Primary display") and the chevron, in dark and light, at the
   normal and the narrow/scaled window sizes.
2. Click the trigger (guarded): capture the popup open below it — the
   search field, the three rows, the committed one marked — and the
   focus in the field (the caret).
3. Type "mouse": capture the filtered popup (the pointer's row alone)
   and the no-results state for "zzz" (the status line, nothing
   selectable).
4. Press Down/Up/Home/End: capture the highlight moving on the rows
   (the row wash, not the committed mark), the field keeping the caret.
5. Enter: capture the popup closed, the trigger showing the new choice,
   and the scratch `settings.json` holding `"openingMonitor": "pointer"`.
6. Escape with a draft (query and moved highlight): capture the popup
   closed, the trigger unchanged, nothing written.
7. Tab and Shift-Tab with a draft: capture the popup closed and focus
   arriving at the next/previous control, nothing written.
8. Click outside (another page row, the sidebar's search field): capture
   the popup closed and the clicked control holding focus.
9. Click the trigger while open: capture the popup toggled closed.
10. IME: with a Japanese input method, compose in the field ("にほ"),
    capture the marked text filtering the list, then commit ("日本") and
    Enter, capturing the commit path.
11. The constrained popup: resize the Settings window short and scroll
    the page so the trigger sits near the bottom; capture the popup
    snapped inside the window, its rows scrollable, the highlighted row
    visible after Down at the list's end.
12. The search jump: `Cmd+F` / `Ctrl+F`, type "pointer", Enter — capture
    the Launcher page opened with the trigger focused and revealed.
13. The platform's honesty rows: where the system does not answer the
    pointer or the active window, capture those rows listed with their
    reasons and not clickable; where Wayland, the whole choice
    unavailable.
14. A save failure (the record made unwritable): capture the trigger
    rolled back to what was saved and the page's failure status.
15. A screen-reader pass (Narrator/NVDA on Windows, VoiceOver where
    available): the trigger announced as a combo box with its value and
    expansion, the highlighted option announced as focus moves, the
    no-results status announced.

Record per-row passed/failed with the capture files, as `settings-72`'s
table does.

## Not checked natively even then

- Popup entrance animation (absent by design; #88 delivers it).
- Any settings page beyond the Launcher page's opening monitor (the
  component is generic, but one real consumer is this milestone's scope).
