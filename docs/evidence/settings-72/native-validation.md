# Native validation — Settings window (#72)

Native checks for the independent Settings window
([#72](https://github.com/hoangvu12/pane/issues/72)), recorded from a real
Windows run of a development build. The helper is
`capture-settings.ps1`, modeled on the established
[`docs/evidence/ui-prototype/capture-pane.ps1`](../ui-prototype/capture-pane.ps1)
pattern: non-destructive launch of an explicit binary, per-run scratch
`PANE_DATA_DIR` and scratch `LOCALAPPDATA` through spawn-scoped
environment variables (restored immediately), `PANE_ARTIFACTS` cleared for
the child only, focus confirmed with the real foreground HWND
(re-checked before keys are sent), guarded real clicks whose
`WindowFromPoint` guards prove the point belongs to the spawned window
tree before the cursor moves and again before the click, and a close of
only the process it started. Nothing is deleted, and no unrelated window
is touched.

The recorded run: `captures/run-20261002-235736-b6307c66/` (PNGs,
`pane-run.json`, `pane-stderr.log`), DPI 96.

## What was checked natively (Windows)

| Check | Result | Evidence |
| --- | --- | --- |
| The footer ellipsis menu opens on click and closes on Escape | passed | `01-menu-open.png`, `02-menu-closed.png` |
| `Ctrl+,` opens a second top-level window (Settings) beside the launcher | passed | `03-settings-open.png`, `checkOpen: true` |
| The Settings window's layout at its opened size (740×530) and resized small (560×400) | passed | `03`, `04-settings-small.png` (562×401 physical ≈ 560×400 logical + frame) |
| The painted close button closes only Settings: the process and the launcher window stay | passed | `checkClose: { settingsClosed: true, processAlive: true, launcherAlive: true }`, `05-after-settings-close.png` |
| Repeated `Ctrl+,` reopens exactly one Settings window, not a duplicate | passed | `checkReopen: { settingsWindows: 1 }`, `05b-settings-reopened.png` |
| The minimize button routes to the system (iconic) | passed | `checkMin: true` (`IsIconic`) |
| The maximize button routes to the system (zoomed) | passed | `checkMax: true` (`IsZoomed`) |
| Dragging the custom titlebar moves the window | passed | `checkDrag: { before: 589,271; after: 679,331; moved: true }`, `06-settings-dragged.png` |
| Closing the launcher window quits Pane | passed | `checkQuit: true` (the process exited on `WM_CLOSE` to the launcher) |

The caption buttons are marked with `WindowControlArea::Min/Max/Close`,
which Windows' `WM_NCHITTEST` maps to `HTMINBUTTON`/`HTMAXBUTTON`/
`HTCLOSE`: the system takes the clicks itself (the native minimize,
maximize and close above went through that routing, which is why
`IsIconic`/`IsZoomed` answer), while the same buttons' click handlers
answer where the hit test is not consulted — GPUI's test platform, whose
in-window tests cover the close path. The titlebar's drag region is a
sibling of the buttons, never their ancestor, so the control hitboxes are
not swallowed.

## What is not natively verified here

- **macOS**: the traffic-light buttons, AppKit titlebar dragging and the
  window's lifetime under the app activation policy were not run (no
  macOS host yet; the platform's native checks remain open as for the
  earlier milestones). The window keeps the standard `Titled` +
  `FullSizeContentView` + `titlebarAppearsTransparent` configuration, so
  the traffic lights and the titlebar drag area are AppKit's own.
- **Linux**: server-side decorations are requested (the default), so the
  window manager's titlebar and controls are the platform's; no X11 or
  Wayland run was made for this ticket.
- **Perceived visual fidelity** of the Settings window and the menu
  popover: this worker has no vision; the captures are for the
  orchestrator's or the user's review. The window geometry, control
  routing and lifecycle above are structural checks.
- **Display scaling**: the harness itself ran the Settings window at a
  2.0 scale factor (`tests/settings.rs` reports
  `the_settings_window_keeps_its_layout_at_small_sizes` over a 560×400
  logical resize); the native run above was at DPI 96.

## How to reproduce

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File capture-settings.ps1 `
  -Binary <path-to-pane.exe> `
  -ExtensionsDir C:\Users\ADMIN\Desktop\nguyenvu\pane\target\guests `
  -OutputDir .\captures
```

Add `-ClickToFocus` when the console cannot take the foreground itself:
then activation is one guarded real click at the launcher's search header,
with the same WindowFromPoint guards as every other click. Focus is
always confirmed as the real foreground HWND before any key is sent; when
it cannot be, keys are not sent and the run fails.
