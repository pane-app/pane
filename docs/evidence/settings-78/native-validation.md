# Native validation — Launcher placement and reopening (#78)

**Status: plan only.** No native captures were made for this ticket: the
host that recorded the earlier native evidence
([`settings-72`](../settings-72/native-validation.md)) was unavailable,
so this file is the capture plan the next native run on this branch
should follow, and it must say "passed/failed, evidence" per row once
that run happens. The automated results that *were* run are on CI (this
branch's Check legs):
[`crates/pane/tests/launcher_settings.rs`](../../../crates/pane/tests/launcher_settings.rs)
drives the real windows on GPUI's test platform with a **fake placement**
— a display layout the test chooses and a record of every move the
launcher window was asked for — and
[`crates/pane-core/src/placement.rs`](../../../crates/pane-core/src/placement.rs)
unit-tests the resolution's every rule. Neither is the real GDI, RandR or
CoreGraphics display list, nor a real `SetWindowPos`, configure request
or `setFrameTopLeftPoint`.

## What this ticket ships

The Launcher page
([`crates/pane/src/features/settings/launcher.rs`](../../../crates/pane/src/features/settings/launcher.rs))
records two host settings beside the appearance preferences and the Open
Pane hotkey: `openingMonitor` (primary display — the provisional default
—, pointer's display, or active-window's display where the system tells
Pane) and `reopening` (restore-view — the parent specification's
provisional default — or root-search). The resolution of a choice against
the display layout, with its fallback to the primary display when the
chosen one is disconnected or unknown, is renderer-independent core
behavior; the platform halves are the pane crate's placement seam
([`crates/pane/src/placement/`](../../../crates/pane/src/placement/)):
Windows reads the GDI monitor list, `GetCursorPos` and
`MonitorFromWindow(GetForegroundWindow)`, and moves with `SetWindowPos`;
X11 reads the RandR monitors, a root-window pointer query and the window
manager's `_NET_ACTIVE_WINDOW`, and moves with a configure request; macOS
reads the CoreGraphics display list, the main display and a fresh event's
pointer position — the active window's display is honestly *not* offered
there — and moves with `setFrameTopLeftPoint`, flipping the y between
AppKit's bottom-left global space and CoreGraphics' top-left one.
Wayland names the whole choice unavailable: its compositor places windows
itself. The launcher window is placed when it opens and every time a
hidden launcher is reopened; the independent Settings window is never
moved by the launcher's placement. Escape's end at root search with an
empty query now hides the launcher through the same hide path the Open
Pane hotkey uses — hidden, not closed.

## What the harness cannot observe (why native evidence is required)

- **The real display list and its coordinate spaces.** The fakes choose
  the layout; only a native multi-monitor run shows the GDI, RandR and
  CoreGraphics enumerations agreeing with the window manager —
  particularly with mixed-DPI displays, where GPUI CE's own logical
  space is per-display and the placement's physical units have to be
  right by construction.
- **The real move.** `SetWindowPos`, the configure request and
  `setFrameTopLeftPoint` are not compiled into the test platform: a
  native run must show the window actually landing centered in the
  chosen display's usable area, keeping its size, frame and shadow.
- **The active-window's display.** `GetForegroundWindow`/`MonitorFromWindow`
  and `_NET_ACTIVE_WINDOW` need another application with focus and a
  real window manager; the honest absence on macOS needs the page shown
  with its explanation there.
- **macOS at all.** This branch's CI compiles Windows and Linux only; the
  macOS adapter (`placement/macos.rs`) is compiled by the release matrix
  and validated by this run.
- **Per-monitor usable areas.** X11 and macOS name no per-monitor work
  area, so the whole monitor is used there and the Dock, menu bar and
  window-manager panels are the honest limit; Windows' `rcWork` is used
  as the usable area and needs a native check against a taskbar.

## Capture plan (the next native run)

Reuse the harness of
[`settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1)
(scratch `PANE_DATA_DIR` and `LOCALAPPDATA`, guarded clicks, focus
confirmation with the real foreground HWND), extended with a second
display where the host can attach one, or a virtual one (a Windows
"Change display settings" proxy is not part of the helper; a
single-display host still validates the primary and fallback rows):

1. Start Pane with a scratch data dir; confirm the launcher window is
   visible and centered on the primary display. Capture the window's
   `GetWindowRect` (the helper already collects foreground state) and
   the Launcher page showing "Primary display" chosen.
2. Choose "Pointer's display" through the page (guarded click), move the
   pointer to the second display, dismiss the launcher with the Open Pane
   hotkey and summon it again: capture the window's rect centered on the
   second display and the scratch `settings.json` holding
   `"openingMonitor": "pointer"`.
3. With the choice "Pointer's display", summon the launcher with the
   pointer on each display in turn: capture the window following the
   pointer, and the Settings window (opened first) unmoved in the same
   captures.
4. Choose "Active window's display": with another application's window
   focused on the second display, send the Open Pane hotkey from there
   and capture the launcher opening on that display. On macOS, capture
   the choice's row carrying the explanation instead, with the choice not
   offered.
5. Disconnect (or virtually disable) the second display with the choice
   pointing at it: capture the launcher opening on the primary display
   and the page's fallback note naming the reason.
6. Reopening: type a query, dismiss with the hotkey, summon again —
   capture the same view and query restored with the caret in the
   search. Choose "Start at root search" and repeat: capture root search
   with an empty query. Open a command, disable its extension, summon
   again: capture the safe return to root search.
7. Escape's end: at root search with an empty query, press Escape and
   capture the window hidden with the process still running and Settings
   still open (the existing hidden-window checks of the helper).
8. Mixed DPI where the host can arrange it: the second display at a
   different scale, the same phases 2–4, capturing that the window
   arrives centered and keeps a usable size.
9. The Settings search (`Cmd+F` / `Ctrl+F`, typing `monitor`): the
   Launcher page's choices found and jumped to, revealed on the page —
   the same catalog the in-window tests drive.
10. A restart over the same scratch data dir: capture the choices kept and
   applied by the fresh window.
11. (Where a Wayland session is available) run the Linux binary under
    it: capture the page's "Not available" note with the Wayland
    explanation and no choices offered.

Record per-row passed/failed with the capture files, as `settings-72`'s
table does; nothing here mutates the user's real display arrangement (a
scratch data dir and a second display the host already has).

## Not checked natively even then

- Perceived smoothness of the move (the window jumps, as launchers do).
- The pointer's display while the pointer is *dragged*: the placement
  reads the position at opening time only.
- Any window-management feature beyond placing the launcher's own window
  (the specification keeps window management out of scope).
