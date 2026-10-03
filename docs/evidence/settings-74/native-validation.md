# Native validation — Open Pane hotkey (#74)

**Status: plan only.** The host that recorded the earlier native evidence
([`settings-72`](../settings-72/native-validation.md)) was unavailable for
this ticket, so no native captures were made: everything below is the
capture plan the next native run on this branch should follow, and this
file must say "passed/failed, evidence" per row once that run happens.
The automated window-harness results that *were* run are on CI (this
branch's Check legs); they exercise the General page's recorder and the
launcher's show/focus/hide through GPUI's test platform, which is not
native key delivery, window management, or the real global-shortcut
registration.

## What this ticket ships

The Open Pane hotkey ([#74](https://github.com/hoangvu12/pane/issues/74)):
the application-owned global binding that summons the launcher from any
application — hidden it shows the launcher and focuses its search; visible
but unfocused it brings it forward; focused it hides the window (Pane
keeps running; the Settings window's focus does not count). The choice is
recorded on the Settings window's General page (the recorder and the reset
row), kept in `settings.json` beside the appearance preferences under the
host settings' own rules, and applied through the platform's
global-shortcut registration path (the `Hotkeys` trait the command hotkeys
already use — `RegisterHotKey` on Windows, Carbon hot keys on macOS,
`XGrabKey` on X11). The provisional default is Ctrl+Alt+Space on
Windows/Linux and Option+Space on macOS; the record's rules (missing
field defaults, a value that is not a shortcut fails the record,
unreadable records are never replaced) are unit-tested in `pane-core`.

The window-level behavior is tested in `crates/pane/tests/open_pane.rs`
through real keystrokes, clicks and the accessibility tree, with a fake
native registration that can be told which shortcuts another application
has: startup registration of the default, the three press transitions,
the repeat guard, held keys, Settings-focus isolation, recording, refusal
and rollback, command collisions, save-failure rollback, restart, reset,
Escape cancellation, a failed extension runtime, and the unavailable
adapter's explanation.

## What the harness cannot observe (why native evidence is required)

- **The real OS registration.** The fake answers `register`; the real
  adapters call `RegisterHotKey` (Windows), Carbon (macOS) and `XGrabKey`
  (X11). A real run must show the default binding actually registered
  (and another application's claim actually refused with the taken error
  surfaced on the page).
- **Activation from another application.** The tests press the hotkey
  through the window; only a real run with focus in another application
  shows the OS delivering the press while that application has focus, and
  the launcher coming forward.
- **The three transitions against the real window manager.** Hide
  (`set_visible(false)`) returning focus to the previous application,
  show returning it to the launcher, and "visible but unfocused" being
  told apart from "focused" by the platform's own activation events.
  Windows' `MOD_NOREPEAT` and X11's detectable auto-repeat are already
  used by the adapters; macOS's Carbon hot keys may report a held key
  repeatedly, which the window's 600 ms repeat guard absorbs — a held key
  on each platform must be watched natively.
- **The macOS default (Option+Space).** The harness runs everywhere with
  the platform's default; only a native macOS run shows Option+Space
  registered and usable beside the input-source shortcuts.
- **Wayland.** The adapter explains the limitation; a native Wayland
  session must show the General page's "Not active" note carrying it, with
  the desktop-shortcut guidance.

## Capture plan (the next native run)

Reuse the harness of
[`settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1)
(scratch `PANE_DATA_DIR`, guarded clicks, focus confirmation with the real
foreground HWND), extended with:

1. Start Pane with a scratch data dir; confirm the process runs and the
   launcher window is visible. Confirm the default hotkey is registered:
   focus another application (a Notepad window the script opens) and send
   `Ctrl+Alt+Space` (Windows/Linux) / `Option+Space` (macOS) with focus
   there; capture the launcher foreground and the scratch `settings.json`
   (which may not exist yet — the default is not a save).
2. With the launcher focused, send the hotkey: capture the window hidden
   (no Pane window in the foreground set, process still alive) and the
   Settings window still present if one was open.
3. Send the hotkey again from the other application: capture the launcher
   shown, focused, with the query caret (type a character through the
   keyboard and capture the query containing it).
4. Record a new binding through the General page: click the recorder row
   (guarded click), send `Ctrl+Alt+B` (or another free combination),
   capture the page showing it and the scratch `settings.json` holding
   `"open_pane": "ctrl+alt+b"`; from the other application, send the new
   binding (captures the launcher summoned) and the old default (captures
   nothing happening).
5. Register a deliberate conflict (a second instance of Pane in another
   data dir holding the same shortcut, or a shortcut another program
   owns): record it through the page and capture the refusal note with
   the previous binding still working.
6. Hold the hotkey key down for several seconds: capture that the
   launcher does not toggle repeatedly (one show, no hide-show cycle).
7. Restart Pane over the same scratch data dir: capture the recorded
   binding working before any Settings window opens (the fresh
   application registered what the record holds).
8. Explicit quit (close the launcher window): confirm the process exits
   and a follow-up `RegisterHotKey` probe from a script (or simply another
   application binding the same combination successfully) shows Pane's
   registrations were released with the process.
9. (Where a Wayland session is available) run the Linux binary under it:
   capture the General page's "Not active" note with the Wayland
   explanation and the desktop-shortcut guidance, and no registration
   attempted.

Record per-row passed/failed with the capture files, as `settings-72`'s
table does; nothing here mutates the user's real shortcuts (the scratch
data dir and the second-instance conflict keep every registration inside
the run).
