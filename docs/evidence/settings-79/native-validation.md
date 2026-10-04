# Native validation — Tray or menu-bar entry (#79)

**Status: plan only.** No native captures were made for this ticket: this
machine is not to run the app (a local build froze it before), and the
milestone's native pass is [#84](https://github.com/hoangvu12/pane/issues/84).
Everything below is the per-platform availability record and the capture
plan the next native run on this branch should follow; this file must say
"passed/failed, evidence" per row once that run happens. The automated
window-harness results that *were* run are on CI (this branch's Check
legs); they exercise the General page's toggle, the preference's
persistence and rollbacks, and the menu's dispatch through GPUI's test
platform with a fake native adapter — which is not the real notification
area, the real menu bar, or a real process exit.

## What this ticket ships

The tray or menu-bar entry ([#79](https://github.com/hoangvu12/pane/issues/79)):
Pane's item in the system's tray or menu bar, whose menu offers Open Pane
(summons the launcher — shown and focused, never hidden), Settings (opens
or focuses the one Settings window, sharing the launcher, so no duplicate
window or extension runtime) and Quit (releases the native entry and every
global hotkey registration, then ends Pane through the quit hooks that
stop the runtime helpers and development watches). The General page's
visibility preference is kept in `settings.json` beside the appearance and
Open Pane hotkey choices (as the record's camelCase `trayVisible` field,
named as #80's launch-at-login choice is), applied through the
platform's adapter *before* anything is kept — so only successful
changes persist, a refusal or an unavailable platform is explained on
the page, and a failed save rolls the native entry back to what the
record holds. Where the platform has no entry at all (Linux today), the
switch is not offered and the adapter's own explanation stands in its
place, as the launch-at-login switch is not offered where that
integration cannot manage a registration.

The application-side machinery is tested in
`crates/pane/tests/tray.rs` through real keystrokes, clicks and the
accessibility tree, with a fake native adapter that records what Pane
showed and hides and can be told to refuse: the startup application of
the record's preference, the switch's save, a refusal, an unavailable
platform, the menu dispatch (summon from hidden, never hiding, the one
Settings window), quit's cleanup, and the save-failure rollback. The
record field's defaults and failures are unit-tested in `pane-core`.

## Platform availability (what ships, per system)

| Platform | Adapter | State |
| --- | --- | --- |
| Windows | `Shell_NotifyIconW` on a thread of Pane's own (`pane-core/src/tray/windows.rs`): an icon in the notification area, a right-click menu (Open Pane, Settings, Exit — the platform's own labels), a left click summoning the launcher | Implemented; compiled and clippy-checked on this branch's CI (the Windows legs). Behavior not yet observed natively. |
| macOS | `NSStatusItem` in the menu bar's status area (`pane-core/src/tray/macos.rs`), its `NSMenu` items sending to a target class of Pane's own (Open Pane, Settings, Quit Pane) | Implemented. **Not compiled by this branch's CI** (the fast tier runs Linux and Windows only); it is written against the checked-out `objc2-app-kit` 0.3 / `objc2` 0.6 sources and must be compile-checked on macOS by the merge or #84 before native validation. |
| Linux | No entry: the desktop's tray speaks StatusNotifierItem over DBus, which Pane does not speak yet (an XEmbed tray would be legacy and unimplemented too) | Honestly unavailable: the adapter explains on the General page, the preference stays recorded, never represented as a working toggle. A StatusNotifierItem implementation is separate work. |

The icon the Windows adapter shows is the one the packaging embeds in
Pane's program (resource id 1, the same resource the window icon loads);
a build without one falls back to the system's application icon. The
macOS item's button carries the title "Pane" — an icon would be blank in
a development build, which is what the application icon would be there.
Both choices are honest placeholders recorded here, not confirmed product
decisions: #84's native pass should capture how each actually reads.

## What the harness cannot observe (why native evidence is required)

- **The real tray and menu.** The fake answers `set_visible`; the real
  adapters call `Shell_NotifyIconW` and make an `NSStatusItem`. A run must
  show the icon appearing and disappearing, the menu opening with its
  platform's labels, and each item's dispatch landing where the tests say
  the selection goes.
- **The quit cleanup in the real process.** `cx.quit()` is a no-op on
  GPUI's test platform, so the tests assert the release of the entry and
  the hotkey registrations but not the process ending. A run must show
  Pane exiting on the tray's Quit, the icon gone with it, and the
  registered Open Pane binding free afterwards (another application can
  take it, or a probe registers it).
- **The system's own refusals.** The fakes inject them; only a real run
  shows an add the system refuses (and the General page explaining it
  without saving anything).
- **The Windows menu's foreground behavior.** `TrackPopupMenuEx` needs
  the owner window in the foreground to dismiss on an outside click; that
  is a real-shell behavior the harness cannot approximate.
- **macOS compile and menu behavior.** The macOS adapter is not compiled
  by this branch's CI at all (see the table); the menu item
  target/action wiring, the item's placement and the removal on drop are
  all native-only checks.

## Capture plan (the next native run)

Reuse the harness of
[`settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1)
(scratch `PANE_DATA_DIR`, guarded clicks, focus confirmation), extended
with a tray phase per platform; nothing here mutates the user's real
shortcuts or desktop settings — every change stays inside the run's
scratch data folder.

### Windows first

1. Start Pane with a scratch data dir: the tray icon appears (capture the
   notification area, `01-tray-icon.png`). Record which icon a development
   build fell back to, if it did.
2. Right-click the icon: the menu shows Open Pane, Settings, Exit
   (`02-tray-menu.png`). Choose Open Pane with the launcher hidden by the
   hotkey: the launcher comes up focused with the query caret (type a
   character and capture it, `03-summoned.png`).
3. Left-click the icon: the launcher is summoned the same way (the
   platform's primary action), not a menu.
4. Choose Settings: the Settings window opens focused; choose it again
   from the tray with the launcher hidden: the same window is focused,
   no second one (capture the window list via the taskbar, `04-settings.png`).
5. Choose Exit: Pane's process ends, the icon is gone from the
   notification area, and the Open Pane binding is free (a probe or a
   second Pane instance in another scratch dir can claim it)
   (`05-quit.png`).
6. Toggle Show in tray off in General: the icon disappears with no
   save failure, and the record holds `"tray_visible": false`
   (`06-hidden.png`, the scratch `settings.json`). A restart over the
   same folder keeps it hidden. Toggle it back on: the icon returns.
7. (If a refusal can be produced — an add while Explorer is restarting,
   say) capture the General page's explanation and the record unchanged.

### macOS (compile-check first)

1. Compile the branch on macOS (the adapter is uncompiled by this
   branch's CI): `cargo check --workspace --all-targets`, fixing anything
   the objc2 wiring got wrong.
2. Start Pane with a scratch data dir: the status item appears in the
   menu bar with the title "Pane" (`01-status-item.png`).
3. Click the item: the menu shows Open Pane, Settings, Quit Pane
   (`02-status-menu.png`); each item does what the Windows row above
   captures for its own platform.
4. Quit Pane from the item: the process ends and the status item is gone
   with it (`03-quit.png`).
5. The visibility toggle hides and shows the item, and the record keeps
   the choice across a restart.

### Linux

1. Run the Linux binary on X11 and on Wayland sessions alike: the General
   page carries the same "Not available on Linux: … StatusNotifierItem
   over DBus" explanation (the adapter's message), the toggle is refused
   with it, and nothing is saved (`01-unavailable.png`).
2. Quit and Settings remain reachable through the launcher's footer menu
   and the Settings window, as the explanation says.

Record per-row passed/failed with the capture files, as `settings-72`'s
table does.
