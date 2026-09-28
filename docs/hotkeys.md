# Global hotkeys

Added for [#32](https://github.com/hoangvu12/pane/issues/32) (Windows),
[#33](https://github.com/hoangvu12/pane/issues/33) (macOS) and
[#34](https://github.com/hoangvu12/pane/issues/34) (Linux): US03, US10, US44,
US57; T03, T09, T22; contributions to G2 and G7, not claims that they pass.
The user gives an installed command a **global hotkey** in Pane, and pressing
it while any application has focus opens that command in Pane's window. The
three systems share one design and differ only in how Pane registers the
shortcut with the system. The architecture is recorded in
[ADR 0016](adr/0016-host-registers-global-hotkeys-for-commands.md)
(proposed).

## Assigning one

In **Manage extensions…**, after each package's state, Reload, Clear cache and Uninstall
rows, every command of an enabled package has a row "Hotkey for
&lt;command&gt;", subtitled with its hotkey ("Ctrl+Alt+G · Opens it from any
application"), "None · …", or why it is not active, then its package's source (copies of a package may share titles). Enter opens the hotkey
screen, "Hotkey for Greeting", which says:

- "Press the keys that should open Greeting from any application, such as
  Ctrl+Alt+G." (Control+Option+G on macOS);
- its current hotkey, or that it has none, and why it is not active if it is
  not;
- "Esc goes back without changing it."

Pressing a key with its modifiers there assigns it: Pane registers it with
the system at once, releases the hotkey it replaces, records it and returns to
the extension list with "Ctrl+Alt+G now opens Greeting". If the command has a
hotkey, a "Remove hotkey" row releases and forgets it ("Greeting has no hotkey
now"). Escape leaves without a change. A pressed key that cannot be used is
explained as the status error and the screen stays for another try:

| Pressed | Explanation |
| --- | --- |
| A key other than a letter, digit, F1–F12 or Space | "Pane cannot use Left in a hotkey: end it with a letter, a digit, F1 to F12 or Space" |
| No Ctrl, Alt or Super (Windows key, Command) | "Shift+G needs Ctrl, Alt or Super, so that it does not take over typing." |
| Ctrl (Command on macOS) with only a letter or digit | "Ctrl+C is used by applications for their own commands, such as copying; add Alt or Shift." |
| A shortcut the system keeps (list per system in [`pane_core::hotkeys`](../crates/pane-core/src/hotkeys.rs)) | "Alt+F4 is reserved: Windows closes the active window with it." |
| The hotkey of another command | "Ctrl+Alt+G already opens Greeting: remove it there first, or press another shortcut." |
| Used by another application (the system refuses it) | "Ctrl+Alt+G cannot be used: another application or the system already uses it. Press another shortcut." |
| Any other refusal by the system | "… cannot be used: the system refused it: &lt;reason&gt;. …" |

A refused shortcut leaves the earlier hotkey working. Keys the launcher uses
on that screen (Enter, Escape, Tab, arrows) are not recorded; a modifier
pressed alone is not a key press.

## Pressing it

Pane brings its window to the front (restoring it if minimized) and opens the
command, from whatever it showed: an open command, form or view closes, as
when leaving it, and root search's query is cleared. Escape then returns to
root search. The command runs exactly as when opened from root search. A
hotkey pressed while its command's package is disabled, or after it was
released, reaches Pane no more.

## What keeps and releases it

- The hotkeys are Pane's own records, not the extension's data:
  `hotkeys.json` beside `installed.json`, by command id (the package
  identity's key and the manifest's command id), `{ "version": 1, "hotkeys":
  { "local:/…#greeting": "ctrl+alt+g" } }`. Written atomically; an
  unreadable file is not overwritten (assigning then explains why).
- A hotkey is registered exactly while its command is offered: the package is
  enabled and the command is available on this system. **Disabling** a
  package releases its hotkeys at once and keeps the choice; **enabling** it
  registers them again. They are registered again at every start.
- An **update** or **reload** keeps a command's hotkey; a command the new copy
  no longer has releases its hotkey (the choice stays recorded).
- Each system adapter lives for the whole process; dropping one releases
  every shortcut it registered and ends its thread (checked on X11 and
  Windows by the adapter tests).
- The record is written one change at a time, each write holding the
  choices as they are when it begins, so the last write holds the latest
  choices whatever order changes finish in. A change that cannot be written
  goes back, in Pane, to what was last recorded, and its registration
  follows.
- **Uninstall** (#40) releases the package's hotkeys at once and forgets
  them, whether its saved data is kept or deleted (they are Pane's records):
  exactly its own, by the package part of the command id (a manifest
  command id cannot contain `#`), so uninstalling the copy from folder `x`
  keeps the hotkeys of the copy from `x#y`. A change whose write fails after
  its package was uninstalled does not bring its hotkey back. The record is
  kept as the [aliases](aliases.md)' is.
- A hotkey another application took meanwhile is shown on its row as "Not
  active: another application or the system already uses it" and tried again
  with every change to the installed packages and at the next start; the
  command itself is unaffected.

## Per platform

| | Windows ([#32](https://github.com/hoangvu12/pane/issues/32)) | macOS ([#33](https://github.com/hoangvu12/pane/issues/33)) | Linux ([#34](https://github.com/hoangvu12/pane/issues/34)) |
| --- | --- | --- | --- |
| Registered with | `RegisterHotKey` (`MOD_NOREPEAT`) on a thread of Pane's own with its own message loop | Carbon `RegisterEventHotKey` through the `global-hotkey` crate (0.8, Apache-2.0 OR MIT), on the main run loop | `XGrabKey` on the root window through `x11rb`, on the key whose first level (no Shift) gives the key, with every Caps Lock, Num Lock and Scroll Lock combination (their modifiers read from the server's modifier mapping), XKB detectable auto-repeat; grabs are made again when the keyboard mapping changes (`MappingNotify`) |
| Conflict with another application | `ERROR_HOTKEY_ALREADY_REGISTERED` → "already uses it" (Windows refuses some of its own shortcuts the same way) | Refused only if another application registered it exclusively ("the system refused it: …"); system shortcuts are covered by Pane's reserved list | `BadAccess` → "already uses it" (also a shortcut the window manager grabs) |
| Permission | None | None: Carbon hot keys need no Accessibility or Input Monitoring permission (an event tap would) | None on X11 |
| Unavailable | | | **Wayland** (Pane's window uses Wayland whenever `WAYLAND_DISPLAY` is set): every hotkey row says "Not available on Linux with Wayland: Wayland does not let an application see keys pressed in other applications, and Pane does not use the desktop's global shortcuts portal yet. Assign a shortcut in the desktop's keyboard settings instead, or run Pane on X11." Activating it shows that reason; the commands still open from root search. Without any display, or an X11 display that cannot be reached, the rows say so. |
| Brought to the front | GPUI's `activate_window` (restores a minimized window) | `makeKeyAndOrderFront` and `activateIgnoringOtherApps` | `_NET_ACTIVE_WINDOW`, which a window manager may refuse (focus-stealing prevention) |
| Baseline | Windows 10/11; CI `windows-2025` | macOS 15 (`macos-15`, arm64) | X11: any window manager; run on Xvfb only |

The explanation uses the [unavailable-row mechanism of
#19](platform-availability.md#an-actions-supported-systems): the rows stay
listed and selectable with the reason in amber, and activating one shows the
reason and does nothing. A command unavailable on this system (its
`platforms` list) has an unavailable hotkey row with the command's reason.

Nothing is required of extension authors: any command of an installed package
can be given a hotkey, in Rust, JavaScript or TypeScript alike; an extension
cannot assign, read or declare one (no WIT or manifest change).

## Checks

- Launcher public interface ([`crates/pane-core/tests/hotkeys.rs`](../crates/pane-core/tests/hotkeys.rs)),
  with the real settings-sample guests and a fake system (which shortcuts
  other applications use is deterministic): assigning in Manage extensions
  registers it and a press opens the command from root search or from
  another open command; kept and registered again after a restart; changing
  releases the old one; Remove hotkey; disable releases and enable restores,
  across a restart; taken by another application (explained, not assigned,
  the old one kept), the hotkey of another command, no Ctrl/Alt/Super and
  reserved shortcuts refused; unavailable everywhere (the rows say why, the
  command still opens); taken meanwhile explained after a restart; Escape
  changes nothing; a JavaScript command; an update keeps it and one that
  drops the command releases it.
- Window ([`crates/pane/tests/hotkeys.rs`](../crates/pane/tests/hotkeys.rs)),
  on GPUI's test platform: Enter on the hotkey row, a plain `p` explained,
  `ctrl-alt-p` assigned; a reported press with root search showing a query
  opens the command, whose list has focus.
- Adapters ([`crates/pane-core/tests/hotkey_adapters.rs`](../crates/pane-core/tests/hotkey_adapters.rs)):
  on Linux against an Xvfb of the test's own (`PANE_XVFB` or `PATH`; skipped without one,
  never the desktop the tests run in): a grab, a second client refused as
  taken, a real `xdotool key ctrl+alt+p` reported once, and after release the
  other client can grab it. On Windows: registered, refused to a second
  registration as taken, and free again once released. macOS registers with
  the main run loop, which a test thread does not run, so only the GUI smoke
  checks it.
- Native GUI smokes, one identical phase on all three systems (screenshots 52
  to 58; 48 is the Linux-only opened quicklink): with a data folder of its
  own, install the settings sample, open "Hotkey for Greeting" in Manage
  extensions and press Ctrl+Alt+G (Control+Option+G), checking the hotkey
  screen and "… now opens Greeting"; with Pane unfocused (Linux: focus on the
  root window; Windows: Pane minimized; macOS: Finder in front) press it and
  check Greeting opened and, on Windows and macOS, that Pane is in front;
  check `hotkeys.json`, restart and press again (Greeting opens); disable
  the extension, press again and check nothing changed (and, on Windows and
  macOS, that Pane stayed behind). See the [Linux](platforms/linux.md#global-hotkeys-32-33-34),
  [macOS](platforms/macos.md#global-hotkeys-33) and
  [Windows](platforms/windows.md#global-hotkeys-32) notes for where it has run.

## Limits

- One hotkey per command, only for installed packages' commands (not the
  samples this build supplies), and no hotkey of Pane's own to summon its
  window: a summon hotkey is a separate choice (root search itself).
- Only the listed keys. On X11 the key is the one the current layout gives
  without Shift; a key the layout gives only with Shift (the digits of a
  French AZERTY keyboard) is refused for a hotkey without Shift ("on this
  keyboard layout Ctrl+Alt+1 needs Shift; add Shift or choose another key")
  and grabbed as it is for one with Shift. After a layout change a hotkey
  the new layout cannot give, or another client took meanwhile, is released
  (reported on standard error, not yet on its row). Windows and macOS
  register virtual keys and key codes, whose layout behavior is the
  system's.
- Wayland is unsupported (above); the portal (`org.freedesktop.portal.GlobalShortcuts`)
  would let the desktop ask the user to confirm each shortcut, with
  support that differs between desktops, and is future work.
- On X11 a window manager with focus-stealing prevention may leave Pane's
  window behind another; the command is still opened. The Xvfb smoke has no
  window manager, so the focus transition on Linux is not verified natively.
- macOS reports no conflict with a system shortcut or another application's
  non-exclusive hot key; the reserved list covers common system shortcuts
  only (Spotlight, input sources, screenshots, lock screen).
- The reserved lists are Pane's, not read from the system or desktop
  settings.
- Pane's own window keeps the keys while a hotkey is registered: pressing it
  with Pane focused opens the command too, even on the hotkey screen.
