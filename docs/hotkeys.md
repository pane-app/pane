# Global hotkeys

Added for [#32](https://github.com/pane-app/pane/issues/32) (Windows),
[#33](https://github.com/pane-app/pane/issues/33) (macOS) and
[#34](https://github.com/pane-app/pane/issues/34) (Linux): US03, US10, US44,
US57; T03, T09, T22; contributions to G2 and G7, not claims that they pass.
The user gives an installed command a **global hotkey** in Pane, and pressing
it while any application has focus opens that command in Pane's window. The
three systems share one design and differ only in how Pane registers the
shortcut with the system. Pane also has one global hotkey of its own, the
[Open Pane hotkey](#the-open-pane-hotkey) ([#74](https://github.com/hoangvu12/pane/issues/74)),
which summons its window and belongs to no extension. The architecture is
recorded in
[ADR 0016](adr/0016-host-registers-global-hotkeys-for-commands.md)
(proposed).

## Assigning one

A command's hotkey is recorded in one of two places, which change one record
with the same checks, so what one shows the other shows:

- on the command's **extension page** in Settings (Extensions group), in its
  Commands list, where each command has a hotkey recorder beside its alias
  field ([ADR 0043](adr/0043-extensions-are-managed-in-settings-one-page-per-extension.md),
  #168);
- inline on Settings' **Shortcuts page** ([#76](https://github.com/hoangvu12/pane/issues/76)),
  which lists every installed command, grouped by extension, with its Name,
  Alias and Hotkey. Its filter field narrows the list by command or
  extension name.

In either place the Hotkey cell shows the command's hotkey, or "None". Click it,
or press Enter or Space on it, and it listens ("Recording; Hotkey for
Greeting: none" to a screen reader): the keys pressed next are the hotkey,
captured, so they do not navigate or act. A hotkey that cannot be used is
explained under the cell, which keeps listening for another try. Escape, Tab
or a click elsewhere stops listening and changes nothing. A hotkey that lands
is registered with the system at once, replaces the one before it, is
recorded, and the page's status line says "Ctrl+Alt+G now opens Greeting". A
button beside a recorded hotkey ("Clear the hotkey for Greeting") releases and
forgets it ("Greeting has no hotkey now"). A record that cannot be written puts
back what was last recorded, and the status line says "Could not keep the
change: …". A command whose package is disabled, or that is unavailable on
this system, is shown with its hotkey but cannot be recorded for; why is under
the cell as "Not active: …". Where this system has no global hotkeys at all,
the Shortcuts page says "Hotkeys are unavailable here: &lt;reason&gt;" over the
Hotkey column.

A pressed key that cannot be used is explained in these words. The core makes
the same checks, with the same wording, wherever a hotkey is recorded,
including on the hotkey screen ("Hotkey for Greeting") that the launcher's
"Manage extensions" screen used to open and that Pane no longer shows
(ADR 0043):

| Pressed | Explanation |
| --- | --- |
| A key other than the listed ones | "Pane cannot use Left in a hotkey: end it with a letter, a digit, F1 to F24, Space, Enter, Tab, an arrow, a punctuation key, a navigation key or a numpad key" |
| No Ctrl, Alt or Super (Windows key, Command) — a chord, not a tap | "Shift+G needs Ctrl, Alt or Super, so that it does not take over typing." |
| Ctrl (Command on macOS) with only a letter or digit | "Ctrl+C is used by applications for their own commands, such as copying; add Alt or Shift." |
| Ctrl+Alt+Delete | "Ctrl+Alt+Delete is reserved: no program can intercept it" |
| A shortcut no program can intercept (Win+L on Windows; the lists per system are in [`pane_core::hotkeys`](../crates/pane-core/src/hotkeys.rs)) | "Win+L is reserved: Windows locks the computer with it, and no program can intercept it" |
| A shortcut the system keeps, on macOS and Linux (list per system in [`pane_core::hotkeys`](../crates/pane-core/src/hotkeys.rs)) | "Alt+F4 is reserved: Windows closes the active window with it." |
| The hotkey of another command | "Ctrl+Alt+G already opens Greeting: remove it there first, or press another shortcut." |
| A single and a double tap of the same modifier (Windows, #260) | "Win cannot coexist with Greeting: a single tap and a double tap of the same modifier cannot be bound together. Remove it there first, or press another shortcut." |
| A kind this system cannot take (macOS, Linux, #260) | "Not available on macOS: lone modifier taps work only on Windows for now" |
| The [Open Pane hotkey](#the-open-pane-hotkey) | "Ctrl+Alt+Space opens Pane itself: choose another shortcut for Greeting, or change Pane's hotkey in Settings." |
| Used by another application (the system refuses it) — on macOS and Linux | "Ctrl+Alt+G cannot be used: another application or the system already uses it. Press another shortcut." |
| Any other refusal by the system | "… cannot be used: the system refused it: &lt;reason&gt;. …" |

On Windows ([#125](https://github.com/pane-app/pane/issues/125), ADR 0039) a
shortcut another application has, or one Windows keeps for itself (Alt+F4,
Win+D, Win+E, Win+R, Alt+Space and the rest), is not refused any more: Pane
registers the chord with `RegisterHotKey` when Windows accepts it, and its
own `WH_KEYBOARD_LL` low-level keyboard hook recognizes the chord when
Windows does not. The binding works while Pane runs, and its row — the
extension list's, the Shortcuts page's and the General page's — says it is
"through Pane's keyboard hook, which does nothing while an elevated
application is in front", and, for a shortcut Windows keeps, what Windows
does with it, "which Pane takes first while it runs". The hook is installed
only while a binding needs it. Settings' **Keyboard** page says the hook's
state — installed, how many times Windows removed it and Pane installed it
again, and whether its pages are pinned in memory — and Copy Diagnostics
includes it ([#259](https://github.com/pane-app/pane/issues/259)). Still
refused, as the table above says: what no program can intercept, and a
binding another of Pane's bindings already has.

**The binding kinds Windows adds** ([#125](https://github.com/pane-app/pane/issues/125),
#260, ADR 0039): a binding can be a **lone tap** of one modifier — the
Windows key alone, Right Ctrl — pressed and released with no other key
between, within 500 ms; a **double tap** of one modifier (Ctrl Ctrl), two
presses of the same key with nothing between, within 400 ms, its first
press passing through to applications; a chord with a **named side**
(Right Alt+Space, so the left keys keep their usual meaning); and the
**extended key set**: F13 to F24, punctuation by its US-layout name, the
arrows, Home, End, Page Up, Page Down, Insert, Delete, Enter, Tab, and
numpad keys distinct from their counterparts (the numpad's Enter included).
None of these can `RegisterHotKey` express, so the hook recognizes them,
and their rows say so. Display names follow Windows: "Win", "Right Ctrl",
"Ctrl Ctrl". A single tap and a double tap of the same modifier bound
together are refused: they "cannot coexist", one would swallow the other's
presses. A lone tap of the **Windows key** while it is bound is masked: any
other key pressed with it passes through untouched (Win+E, Win+R, Win+D
keep Windows' meaning), and Pane injects a tagged neutral key before the
tap's release reaches Explorer, so the Start menu does not open as well —
it stays reachable from the taskbar's Start button and Ctrl+Esc. The
"Show the taskbar when Pane opens" choice keeps a taskbar that hides
itself on screen while the launcher is open, so the Start button stays
one click away.

**Recording these kinds** (#260): on Windows the recorder — the Open Pane
recorder, the Shortcuts page's hotkey cells and the extension page's — asks
the hook adapter for a recording session while it listens. The hook holds
the keys back from Windows and reports what was pressed, so the Windows key
alone, a double tap and the side of a modifier are recorded without the
Start menu opening. What the session reports takes the same checks a
keystroke does, with the same explanations. Escape and Tab still cancel.
The session ends when the recorder stops listening, the window loses focus
or Pane quits. On macOS and Linux the recorder is unchanged and offers none
of the new kinds: a record naming one (copied from a Windows machine) is
explained on its row as "Not available on …: … work only on Windows for
now", through the [unavailable-row
mechanism](platform-availability.md#an-actions-supported-systems).

A refused shortcut leaves the earlier hotkey working. Keys the recorder uses
(Enter, Space, Escape, Tab, arrows) are not recorded; a modifier pressed alone
is not a key press — on Windows it is a tap the session takes instead (#260).

## Pressing it

Pane brings its window to the front (restoring it if minimized) and opens the
command, from whatever it showed: an open command, form or view closes, as
when leaving it, and root search's query is cleared. Escape then returns to
root search. The command runs exactly as when opened from root search. A
hotkey pressed while its command's package is disabled, or after it was
released, reaches Pane no more.

## The Open Pane hotkey

Pane has one global hotkey of its own, added for
[#74](https://github.com/hoangvu12/pane/issues/74): the **Open Pane hotkey**.
It is the application's, not any extension's or command's, and it is set on
Settings' **General** page, in the "Open Pane hotkey" row.

- **What a press does.** If the launcher is hidden, it is shown and focused:
  it opens on the display the Launcher page's choice resolves to and starts
  from what the Launcher page's reopening choice says (root search, or the
  view it was left on). If it is shown but does not have the focus (another
  application has it, or the Settings window), it is brought forward and
  focused. If it is shown and focused, it is hidden. Hidden is not closed: Pane
  keeps running, the Settings window stays open, and the next press shows the
  same launcher again. A second press within 600 ms of an accepted one is taken
  as the repeat of a key still held and does nothing.
- **Defaults.** On Windows, the Windows key alone — the user's decision
  ([ADR 0039](adr/0039-hotkeys-fall-back-to-a-keyboard-hook-and-the-windows-key-opens-pane.md),
  [#125](https://github.com/pane-app/pane/issues/125) — as Raycast's fresh
  installs are, Pane replacing the Start menu as the place the user starts
  everything — for a **fresh data folder**: one whose `settings.json` does
  not exist. A record an earlier Pane wrote keeps the hotkey it has,
  whatever the fresh default is; only a fresh folder starts with it. Where
  the Windows key alone cannot be taken — an adapter whose own keyboard
  hook cannot be installed, so a tap is not recognized — Ctrl+Alt+Space is
  registered instead, with the reason shown on the row. Linux keeps
  Ctrl+Alt+Space, macOS Option+Space.
- **Recording and reset.** Click the row's recorder, or press Enter or Space on
  it, and it listens ("Recording; Open Pane with Ctrl+Alt+Space" to a screen
  reader): the keys pressed next are the hotkey, captured. Escape, Tab or a
  click elsewhere stops listening and changes nothing. The new hotkey is
  registered with the system before the one it replaces is released, so any
  refusal leaves the earlier one working and nothing is saved; the reason is
  shown in the row and the recorder keeps listening. The reset button beside
  it ("Reset the Open Pane hotkey to …") goes back to the default of this
  system — the Windows key alone on Windows where the adapter's hook can
  take it, Ctrl+Alt+Space or Option+Space elsewhere — through the same
  checks, and is enabled only while the hotkey is not the default.
- **"Use the Windows key"**
  ([#125](https://github.com/pane-app/pane/issues/125) #268, [ADR 0039](adr/0039-hotkeys-fall-back-to-a-keyboard-hook-and-the-windows-key-opens-pane.md);
  Raycast's "Replace Start Menu"). Beside the recorder, while another hotkey
  is set, the General page offers the choice: one step that sets the Windows
  key alone, through the same checks a recording takes, its note saying
  where the Start menu remains — the taskbar's Start button and Ctrl+Esc —
  and that only the lone tap changes, Win+E, Win+D, Win+L and the rest
  keeping Windows' meaning. Where the Windows key alone cannot be used
  (a system whose adapter has no keyboard hook), the choice is explained
  instead of offered, through the same unavailable mechanism as the kinds.
- **Show the taskbar when Pane opens.**
  ([#125](https://github.com/pane-app/pane/issues/125) #268, [ADR 0039](adr/0039-hotkeys-fall-back-to-a-keyboard-hook-and-the-windows-key-opens-pane.md),
  proposed): a General-page choice, Windows only, for a user whose taskbar
  hides itself — while the launcher window is shown the taskbar is kept on
  screen, so the Start button stays one click away, and when the launcher
  hides it goes back exactly as the user had it. Where the platform has no
  taskbar of the kind, the choice is explained rather than offered.
- **Where it is kept.** In Pane's own settings record, `settings.json` in the
  data folder, as `"open_pane": "ctrl+alt+space"` — or the fresh default's
  own textual form, `"tap:win"` — with the other host
  settings: not in `hotkeys.json` and not in any extension's data. It is
  registered at every start, and stays registered while every extension is
  disabled and while the extension runtime has failed. A change whose save
  fails releases what the record does not hold, so what the record names is
  what works. A fresh data folder holds no record at all: its binding is the
  fresh default, resolved against the adapter and registered at start, and
  nothing is written for the resolution itself — the next save of any
  preference is what records the binding the install came to hold (#268).
- **Refusals.** The same checks as a command hotkey (the table above), and
  the explanation is shown in the row. A shortcut a command already has: "Ctrl+Alt+G
  already opens Greeting: remove it there first, or press another shortcut."
  A shortcut another application has: "Ctrl+Alt+B cannot be used: another
  application or the system already uses it." The reverse collision is the
  table's row above: a command hotkey with the Open Pane hotkey's keys is
  refused, and a command whose recorded hotkey names the same keys is shown
  "Not active: the Open Pane hotkey uses it". Where Pane could not register
  the chosen hotkey at start, the row says "Not active: &lt;reason&gt;".
  Nothing else changes: the launcher opens at start as it always does, and
  once hidden it is shown again by the tray or menu bar entry's Open Pane,
  where Pane has one.
- **Same adapters, same limits.** It is registered through the same system
  adapters as command hotkeys (see Per platform below) and carries every
  limit listed under Limits, Wayland's included: on Wayland the row says "Not
  active: Not available on Linux with Wayland: …" with the explanation in
  Per platform, and recording one is refused with it.

## What keeps and releases it

- The command hotkeys are Pane's own records, not the extension's data (the
  Open Pane hotkey's is `settings.json`, above):
  `hotkeys.json` beside `installed.json`, by command id (the package
  identity's key and the manifest's command id), `{ "version": 2, "hotkeys":
  { "local:/…#greeting": "ctrl+alt+g" } }` — a record of version 1, whose
  grammar is this one's, still reads. The value is the binding's id as
  [`Shortcut::parse`](../crates/pane-core/src/hotkeys.rs) reads it, so the
  kinds #260 adds record as their ids too: `tap:win`, `double:ctrl`,
  `rctrl+space` — a side prefix where a side is named (`lctrl`, `ralt`,
  `lwin`), and `tap:` and `double:` kinds; version 2 is unreleased, so its
  grammar grew rather than the version moving again. Written atomically; an
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
- A [root provider](root-search.md#root-providers) (#164), such as the
  calculator, is never launched, so it has no hotkey: recording one is
  refused ("it only answers root search"), it has no hotkey row, and a
  hotkey recorded before its command became one is never registered and is
  forgotten at the next start, with a toast saying so.
- A hotkey another application took meanwhile is shown on its row as "Not
  active: another application or the system already uses it" and tried again
  with every change to the installed packages and at the next start; the
  command itself is unaffected.
- **Game mode** (#125, Windows only, off by default) pauses them all: while
  it is on, each foreground change — a system event, never a timer — is
  decided (Windows reports a full-screen Direct3D application in front
  through its notification state, or the program of the window is one the
  user listed, so windowed games are covered), and while a game is in front
  every hotkey, the Open Pane hotkey included, is released — the keyboard
  hook too, which is installed only while a binding needs it, so it goes
  with them and the game gets every key. They come back by themselves when
  the game leaves the front, through the same registration path. The
  settings are `game-mode.json` beside `installed.json` (the on/off choice
  and the programs to treat as games, matched by program file name), set on
  the Settings window's Keyboard page, which offers the choice only on
  Windows; the tray icon's tooltip says while the hotkeys are paused.

## Per platform

| | Windows ([#32](https://github.com/pane-app/pane/issues/32)) | macOS ([#33](https://github.com/pane-app/pane/issues/33)) | Linux ([#34](https://github.com/pane-app/pane/issues/34)) |
| --- | --- | --- | --- |
| Registered with | `RegisterHotKey` (`MOD_NOREPEAT`) on a thread of Pane's own with its own message loop; a chord Windows refuses — another application's or Windows' own — and the kinds no registration can express — a lone tap, a double tap, a side-specific modifier, a numpad key (#260) — are recognized by Pane's own `WH_KEYBOARD_LL` low-level keyboard hook instead, on a thread of its own at the highest thread priority, installed only while a binding or a recording session needs it, with locked pages, a raw-input watchdog that reinstalls a hook Windows silently removed, modifiers re-read from the system on a session unlock, a resume and a contradiction, and Pane's injected keys tagged and passed through untouched (#125, ADR 0039). While the Windows key alone is bound, the hook injects a tagged neutral key before the key's release reaches Windows when it completes the tap or follows a chord Pane swallowed, so the Start menu does not open; a recorder that listens holds every key back through a recording session with the same hook (#260) | Carbon `RegisterEventHotKey` through the `global-hotkey` crate (0.8, Apache-2.0 OR MIT), on the main run loop | `XGrabKey` on the root window through `x11rb`, on the key whose first level (no Shift) gives the key, with every Caps Lock, Num Lock and Scroll Lock combination (their modifiers read from the server's modifier mapping), XKB detectable auto-repeat; grabs are made again when the keyboard mapping changes (`MappingNotify`) |
| Conflict with another application | `ERROR_HOTKEY_ALREADY_REGISTERED` — Windows refuses some of its own shortcuts the same way — and the hook takes the binding, so it is never an error: the row says it is dispatched "through Pane's keyboard hook" (and does nothing while an elevated application is in front, since Windows does not deliver those keys to a hook of a normal process) | Refused only if another application registered it exclusively ("the system refused it: …"); system shortcuts are covered by Pane's reserved list | `BadAccess` → "already uses it" (also a shortcut the window manager grabs) |
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
  other applications use is deterministic): assigning in Settings › Extensions
  registers it and a press opens the command from root search or from
  another open command; kept and registered again after a restart; changing
  releases the old one; Remove hotkey; disable releases and enable restores,
  across a restart; taken by another application (explained, not assigned,
  the old one kept), the hotkey of another command, no Ctrl/Alt/Super and
  reserved shortcuts refused; unavailable everywhere (the rows say why, the
  command still opens); taken meanwhile explained after a restart; Escape
  changes nothing; a JavaScript command; an update keeps it and one that
  drops the command releases it; and #260's kinds — the tap, double-tap,
  side and numpad kinds binding, firing and round-tripping their textual
  forms through the hook, a single and a double tap of the same modifier
  refused ("cannot coexist"), and the kinds unavailable on the test binary's
  own system explained on their rows. The hook's state is answered
  through the launcher while a binding is dispatched through it, and not
  otherwise ([#259](https://github.com/pane-app/pane/issues/259)); and
  #268's fresh-install default is decided per system against the same
  fake — the Windows key alone on Windows where the fake's hook can take
  the tap, today's default where it cannot, with the reason — registers
  through the same path a recorded choice takes, and a choice the record
  holds is applied as it is, never upgraded to it.
- Window ([`crates/pane/tests/hotkeys.rs`](../crates/pane/tests/hotkeys.rs)),
  on GPUI's test platform: Enter on the hotkey row, a plain `p` explained,
  `ctrl-alt-p` assigned; a reported press with root search showing a query
  opens the command, whose list has focus; and, with a fake recording
  session (#260), the hotkey screen recording the kinds the session reports
  — a refusal explained as a keystroke's is, a tap recorded with its textual
  form round-tripped — and Escape still leaving the screen.
- Open Pane hotkey ([`crates/pane/tests/open_pane.rs`](../crates/pane/tests/open_pane.rs)),
  on GPUI's test platform with a fake system: the default registered at
  start and a press toggling the launcher (shown, focused, hidden), a held key
  not toggling again, the Settings window's focus not counting, recording
  swapping the registration and the record, a shortcut another application has
  and a command's hotkey refused with their explanations, a failed save rolling
  the registration back, a restart registering the record, Escape, reset, the
  hotkey working while the extension runtime has failed, and Wayland's
  explanation on the General page; and, with a fake recording session (#260),
  the recorder showing "Right Ctrl", "Ctrl Ctrl" and the Windows key's name
  from what the session reports, and Escape still cancelling. #268 adds the
  fresh-install default registered at start — the Windows key alone where
  the fake's hook can take the tap, on a fresh data folder only, a record
  keeping its hotkey — the "Use the Windows key" choice beside the recorder
  setting it in one step with its Start-menu note, and the choice explained
  where the Windows key cannot be taken.
- Shortcuts page ([`crates/pane/tests/shortcuts.rs`](../crates/pane/tests/shortcuts.rs)),
  likewise: a hotkey recorded inline and cleared, the collisions with another
  command and with the Open Pane hotkey ("… opens Pane itself: …") explained,
  and the catalog following the package lifecycle and a restart; and, with a
  fake recording session (#260), a cell recording a side-specific tap
  ("Right Ctrl") and a double tap ("Ctrl Ctrl"), the coexistence refusal
  under the cell, and Escape still cancelling.
- Settings' dispatch and health surfaces
  ([`crates/pane/tests/keyboard.rs`](../crates/pane/tests/keyboard.rs),
  [`crates/pane/tests/settings.rs`](../crates/pane/tests/settings.rs)), on
  GPUI's test platform with a fake adapter whose hook takes a refused
  binding: a hook-dispatched binding's row says its route (the General
  page's Open Pane row), the Keyboard page says the hook's state —
  installed, how many times it was reinstalled, pages pinned — and says
  nothing where no hook is in use, and the diagnostics copy holds the
  hook's state ([#259](https://github.com/pane-app/pane/issues/259)).
- The taskbar choice
  ([`crates/pane/tests/settings.rs`](../crates/pane/tests/settings.rs)),
  on GPUI's test platform with a fake taskbar seam (#268): the choice
  toggled on the General page shows the taskbar at once while the
  launcher is open, hiding the launcher puts it back as the user had it,
  showing the launcher again shows it, turning the choice off restores
  it at once, and the record keeps the choice.
- The recognizer ([`crates/pane-core/tests/hotkey_recognizer.rs`](../crates/pane-core/tests/hotkey_recognizer.rs)),
  on every system: the pure state machine the Windows hook feeds, driven by
  synthetic key-event sequences — chords fired with exactly their modifiers
  held and whatever keys between, auto-repeat firing nothing, stuck modifiers
  resynchronized so no phantom chord fires, Pane's own tagged injected keys
  passed through untouched, and other tools' injected keys firing the binding
  without being swallowed or counted as the user's; and #260's kinds — taps
  within and beyond the window, double taps likewise, a key between breaking
  both, sides serving only their own chords, the numpad's Enter told from the
  main one, the Windows key's release masked after a chord Pane swallowed,
  a single and a double tap of the same modifier each firing where a record
  holds both, and a recording session holding the keys back and reporting
  chords, taps and double taps.
- Game mode ([`crates/pane-core/tests/game_mode.rs`](../crates/pane-core/tests/game_mode.rs)
  and the decision's own tests in
  [`crates/pane-core/src/game_mode.rs`](../crates/pane-core/src/game_mode.rs)),
  on every system with a fake foreground source and a fake system: the
  decision over fake facts of the window in front (full-screen or not,
  listed or not); the settings' record; a full-screen game or a listed
  program pausing every hotkey — the Open Pane binding included — and
  their return when it leaves the front, driven by events alone; turning it
  off resuming; the settings kept across a restart. The Keyboard page's
  rows ([`crates/pane/tests/game_mode.rs`](../crates/pane/tests/game_mode.rs)),
  on GPUI's test platform: the choice toggled and recorded, a program
  named and removed, a game in front pausing the launcher's registrations,
  and the choice not offered without a foreground source.
- Adapters ([`crates/pane-core/tests/hotkey_adapters.rs`](../crates/pane-core/tests/hotkey_adapters.rs)):
  on Linux against an Xvfb of the test's own (`PANE_XVFB` or `PATH`; skipped without one,
  never the desktop the tests run in): a grab, a second client refused as
  taken, a real `xdotool key ctrl+alt+p` reported once, and after release the
  other client can grab it. On Windows: a chord another application holds
  is taken through Pane's own keyboard hook instead, released with the
  registration, and the dropped adapter's registrations are free again;
  where `PANE_TEST_REAL_INPUT=1` is set (CI's Windows runner), the hook
  reports a chord injected with a test tag, installs only while a binding
  needs it, and the watchdog repairs a hook removed behind the adapter's
  back; and #260's kinds are checked there too: a lone Windows-key tap
  reported with the Start-menu mask injected before the release passes
  through (seen by a test-owned hook below Pane's), a double tap and a
  side-specific chord recognized, Win+&lt;key&gt; passing through untouched,
  and a recording session holding the keys back — the Windows key never
  reaching the system — reporting what was pressed, with Escape passing
  through and the keys free again once the session ends. macOS registers with
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
  macOS, that Pane stayed behind). On Windows only, a hook-dispatched
  binding (screenshots 77 to 79): the smoke holds a chord from its own
  process first, assigns it to the sample's command in Pane, checks the row
  says the binding is dispatched through Pane's keyboard hook, and pressing
  it — injected, as a tool's keys are — opens the command; and a lone tap of
  the Windows key (screenshots 80 to 83): the recorder's session with the
  hook records it — the Start menu staying closed while it listens — the row
  saying the binding works through the hook, and pressing the bound tap with
  Pane unfocused opens the command, the Start-menu mask keeping the Start
  menu closed. See the
  [Linux](platforms/linux.md#global-hotkeys-32-33-34),
  [macOS](platforms/macos.md#global-hotkeys-33) and
  [Windows](platforms/windows.md#global-hotkeys-32) notes for where it has run.

## Limits

- One hotkey per command, only for installed packages' commands (not the
  samples this build supplies). Pane's own [Open Pane hotkey](#the-open-pane-hotkey),
  set on the Settings window's General page, is separate: one for the whole
  application, not a command's, and an extension cannot read, assign or declare it.
- Only the listed keys — a letter, a digit, F1 to F24, Space, Enter, Tab,
  the arrows, Home, End, Page Up, Page Down, Insert, Delete, a punctuation
  key by its US-layout character, or a numpad key (#260); the kinds beyond
  a chord — a lone tap, a double tap, a side — work only on Windows, where
  the hook recognizes them, and the keys beyond letters, digits, F1 to F12
  and Space too are offered only there for now (the other systems' rows
  explain a record naming them). On X11 the key is the one the current layout
  gives without Shift; a key the layout gives only with Shift (the digits of
  a French AZERTY keyboard) is refused for a hotkey
  without Shift ("on this keyboard layout Ctrl+Alt+1 needs Shift; add Shift or choose another key")
  and grabbed as it is for one with Shift. After a layout change a hotkey
  the new layout cannot give, or another client took meanwhile, is released
  (reported on standard error, not yet on its row). Windows and macOS
  register virtual keys and key codes, whose layout behavior is the
  system's.
- Wayland is unsupported (above), for the Open Pane hotkey as for commands'; the portal (`org.freedesktop.portal.GlobalShortcuts`)
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
  with Pane focused opens the command too, even on the hotkey screen. The
  Open Pane hotkey pressed with the launcher focused hides it.
