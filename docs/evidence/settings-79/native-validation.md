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

Since [#131](https://github.com/hoangvu12/pane/issues/131), both entries
show Pane's mark, embedded in the program, so a development build shows
it too (`crates/pane-core/assets/tray`, drawn from the footer's mark by
`scripts/icons/tray-icons.py`):

- Windows: a black or a white variant of the mark, as an `.ico` of every
  small-icon size from 100% to 400% scaling. The adapter picks the variant
  from the taskbar's theme (`SystemUsesLightTheme`, not the applications'
  theme and not Pane's Appearance choice), swaps it in place when the
  theme changes, and makes it at the notification area's small-icon size
  for the display's DPI. Under high contrast it picks the variant from
  the high-contrast theme's window colour (proposed). The icon is added
  with a GUID derived from the program's canonical path, so Windows keeps
  the user's "always show" choice across restarts and updates; a refused
  GUID falls back to the numeric id with a diagnostic. When Explorer
  restarts and broadcasts `TaskbarCreated`, the icon is added again if
  Pane last asked for it to show, and stays hidden otherwise.
- macOS: the mark as a template image, which the system tints for light
  and dark menu bars and the selected state, instead of the title "Pane"
  (kept only as the fallback if the image cannot be read). The item has
  an autosave name, so a place the user Command-dragged it to is kept.

The unit tests in `crates/pane-core/src/tray/windows.rs` check the
Windows behaviour through a fake of the adapter's inner shell seam. What
they cannot show is the real notification area and menu bar, which the
Explorer restart and theme phases below capture.

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
   notification area, `01-tray-icon.png`). It is Pane's mark, in the
   variant for the taskbar's theme; record it if standard error says the
   system's application icon was shown instead.
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

#### Windows: Explorer restart, identity and theme (#131)

These phases restart Explorer and change the taskbar's theme, which the
phases above do not. Run them on a release-validation machine or runner,
not on someone's working desktop, and put the theme back at the end. They
are release-validation evidence, not a merge gate.

8. Explorer restart, icon shown: with the icon shown, capture the
   notification area (`08-before-restart.png`), then restart Explorer
   (`Stop-Process -Name explorer -Force`; Windows starts it again, or
   `Start-Process explorer.exe` after 5 s if it does not). Wait until the
   taskbar is back and capture the notification area again: Pane's icon
   is there without Pane restarting (`09-after-restart.png`), and its
   menu and left click still work (step 2 and 3, once). Pane's standard
   error has no refusal for the re-add.
9. Explorer restart, icon hidden: toggle Show in tray off, restart
   Explorer the same way: no Pane icon comes back
   (`10-hidden-after-restart.png`), the General page's switch is still
   off, and the scratch `settings.json` is unchanged.
10. Identity: toggle the icon back on, and in Settings > Personalization >
    Taskbar > Other system tray icons turn Pane on, so the icon sits on
    the taskbar rather than in the overflow (`11-always-show.png`). Quit
    and start Pane again from the same program path: the icon is still on
    the taskbar, not in the overflow (`12-kept-after-restart.png`).
    Replace the program at the same path with another build (an update)
    and start it: still on the taskbar (`13-kept-after-update.png`).
    Record that standard error shows no "refused Pane's tray icon
    identity" diagnostic.
11. Theme: in Settings > Personalization > Colors, set "Choose your
    mode" to Custom with Windows mode Dark and app mode Light: the icon
    is the white mark on the dark taskbar while Pane's own Appearance is
    left alone (`14-dark-taskbar.png`). Switch Windows mode to Light: the
    icon becomes the black mark in place, without disappearing
    (`15-light-taskbar.png`). Set Pane's Appearance to the opposite of
    the taskbar and confirm the icon does not change.
12. Scaling: at 150% (or the highest the display offers), capture the
    icon at 1:1 (`16-scaled.png`): sharp edges, no blur from scaling a
    smaller image up. Turn high contrast on (Aquatic, then Desert) and
    capture the icon on each (`17-high-contrast-*.png`): the mark is the
    one that stands out from the theme's background.

### macOS (compile-check first)

1. Compile the branch on macOS (the adapter is uncompiled by this
   branch's CI): `cargo check --workspace --all-targets`, fixing anything
   the objc2 wiring got wrong.
2. Start Pane with a scratch data dir: the status item appears in the
   menu bar showing Pane's mark, not the title "Pane"
   (`01-status-item.png`).
3. Click the item: the menu shows Open Pane, Settings, Quit Pane
   (`02-status-menu.png`); each item does what the Windows row above
   captures for its own platform.
4. Quit Pane from the item: the process ends and the status item is gone
   with it (`03-quit.png`).
5. The visibility toggle hides and shows the item, and the record keeps
   the choice across a restart.
6. Template image (#131): capture the item in a light menu bar
   (`04-light-menu-bar.png`), in a dark one (System Settings >
   Appearance > Dark, `05-dark-menu-bar.png`) and with its menu open, the
   selected state (`06-selected.png`): the system tints the mark each
   time, as it tints its own items.
7. Autosave name (#131): Command-drag the item to another place in the
   menu bar, quit Pane and start it again: the item comes back where it
   was put (`07-kept-place.png`).
8. Restart the menu bar (`killall SystemUIServer`, and `killall
   ControlCenter` on macOS 11 and later): the item is still there, with
   no action from Pane (`08-after-menu-bar-restart.png`).

### Linux

1. Run the Linux binary on X11 and on Wayland sessions alike: the General
   page carries the same "Not available on Linux: … StatusNotifierItem
   over DBus" explanation (the adapter's message), the toggle is refused
   with it, and nothing is saved (`01-unavailable.png`).
2. Quit and Settings remain reachable through the launcher's footer menu
   and the Settings window, as the explanation says.

Record per-row passed/failed with the capture files, as `settings-72`'s
table does.
