# Applications

Added for [#24](https://github.com/hoangvu12/pane/issues/24) (Windows),
[#25](https://github.com/hoangvu12/pane/issues/25) (macOS) and
[#26](https://github.com/hoangvu12/pane/issues/26) (Linux): US03, US05, US12,
US44; T01, T03, T22; contributions to G2 and G7, not claims that they pass.
Typing an installed application's name into root search lists it, ranked
with commands by title, and Enter (or a click) opens it. The feature is a
**default extension**, Applications, which the user can disable like any
package; the three systems share one design and differ only in where Pane
looks for applications and how it opens them.

## Where it lives

The spec and [ADR 0001](adr/0001-small-core.md) make app launching a
disableable default extension, not core; [ADR 0006](adr/0006-raycast-style-search-with-extension-providers.md)
keeps matching, ranking and dispatch in the core. A pure WASI 0.3 guest can
neither read the system's application folders (its WASI context has no
preopened folders) nor start a program. So the split, recorded in
[ADR 0015](adr/0015-host-finds-and-opens-applications-for-an-extension.md), is:

- **Host capability**, in the core: `pane:extension/applications`
  ([`wit/applications.wit`](../wit/applications.wit)), a host import any
  command may use: `installed()` lists the applications, `open(id)` opens one.
  Each system has an adapter behind one small trait
  ([`pane_core::applications`](../crates/pane-core/src/applications.rs)). It
  holds no state and does nothing until a guest asks.
- **Default extension**, [`guests/applications`](../guests/applications)
  (Rust), package [`guests/packages/applications`](../guests/packages/applications):
  its command, "Applications", lists every installed application (Enter
  opens one), and it supplies the applications to root search as
  [indexed results](root-search.md#results-supplied-ahead-of-the-query).
- **Not a native helper** ([ADR 0014](adr/0014-optional-native-extension-helpers.md), #15): listing folders and asking the
  system to open a file are what the host process already does for every
  system; a separately packaged per-OS helper binary would add distribution
  and lifecycle work (#15 is not built) for no capability the host lacks.

Acquiring the package automatically at setup is
[#51](https://github.com/hoangvu12/pane/issues/51) to
[#53](https://github.com/hoangvu12/pane/issues/53); until then it is
installed from its folder like the calculator
(`pane --install target/guests/packages/applications`).

## Behavior

- Nothing runs at start, install or for a blank query. The first query that
  is not blank starts the extension, which asks the host for the installed
  applications; they are kept, and every later query ranks them at once, so
  typing never waits for them (the first query lists the commands at once
  and the applications as soon as they are found).
- They are asked for again on the first query after each return to root
  search (at start, after Escape from a command, after an install), keeping
  the earlier list until the new one arrives. The host keeps its own list
  too ([`Cached`](../crates/pane-core/src/applications/cached.rs)): only
  the very first request scans the system's folders on the runtime thread;
  later ones get the kept list at once, and one older than 10 seconds is
  rescanned on a thread of its own for the next request. So an application
  installed while Pane is open is found from the second return to root
  search after it; there is no file watching. A failed rescan keeps the old
  list.
- A blank query lists no application, so root search's empty list stays the
  commands and Pane's own rows.
- Each application is a root result titled with its name and subtitled
  "Application", matched and ranked like a command's title
  ([root search](root-search.md#matching-and-ranking)): "firefox" finds
  Firefox; an exact or prefix title beats a word inside another title; on
  the same rank commands come first.
- Enter opens it, off the window's thread, and the status says "Opened
  Firefox". A failure stays visible as the status error: "Could not open
  Firefox: cannot find the program firefox", "... not allowed to run ...",
  the system's own message from `open` or `ShellExecuteEx`, or "... no
  longer exists" for an application removed since it was listed.
- If the applications cannot be listed at all, a row "Applications — Could
  not list: ..." is listed for every query that is not blank; Enter shows
  the whole error. A missing or unreadable location only adds nothing.
- **Disabled** (Manage extensions), its applications leave root search at
  once, the kept list is dropped, its instance is stopped, nothing looks for
  applications any more, and a list still on its way is discarded; other
  results (commands, the calculator) are untouched. Enabled again, the next
  query looks again.

## Per platform

| | Windows ([#24](https://github.com/hoangvu12/pane/issues/24)) | macOS ([#25](https://github.com/hoangvu12/pane/issues/25)) | Linux ([#26](https://github.com/hoangvu12/pane/issues/26)) |
| --- | --- | --- | --- |
| Found in | Start menu shortcuts (`.lnk`) in `%APPDATA%\Microsoft\Windows\Start Menu\Programs` then `%ProgramData%\...\Programs`, with subfolders; then the packaged (AppX/MSIX) apps of the shell's Apps folder (`FOLDERID_AppsFolder`), such as Calculator on Windows 11 | Application bundles (`.app`) in `/Applications`, `/System/Applications` and `~/Applications`, and their subfolders two deep (such as `Utilities`), not inside bundles | Desktop entries (`.desktop`) in `$XDG_DATA_HOME/applications` (default `~/.local/share/applications`) then `applications` in each of `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`), with subfolders (Flatpak and Snap add their folders to `XDG_DATA_DIRS`) |
| Name | The shortcut's file name; a packaged app's display name | The bundle's folder name | The entry's `Name` (not localized `Name[..]`) |
| Left out | Shortcuts whose name starts with "Uninstall"; a shortcut at the same place in the all-users menu as in the user's; Apps folder items that are not packaged apps (desktop programs, found by their shortcuts) or that have a shortcut's name | Nothing | `Type` other than `Application`, `NoDisplay` or `Hidden` (a hidden entry also hides a lower one with the same desktop file id), no `Exec`, `OnlyShowIn`/`NotShowIn` against `$XDG_CURRENT_DESKTOP`, a `TryExec` program that is missing, an `Exec` line using field codes against the spec (`%i`, `%F` or `%U` inside an argument, more than one of `%f %u %F %U`, an unknown code or a lone `%`: skipped with a line on standard error, not guessed) |
| Opened by | `ShellExecuteEx` on the shortcut, or on `shell:AppsFolder\<AppUserModelID>` for a packaged app, as Explorer opens them (errors returned, no dialog), with COM initialized for the call and uninitialized after | `/usr/bin/open` on the bundle (Launch Services); its error message is shown | Running the `Exec` program directly (quoting and field codes per the Desktop Entry spec; file and URL codes dropped; `Path` as working folder), in its own process group; a `Terminal=true` entry runs in `$TERMINAL -e`, else the first installed of `x-terminal-emulator -e`, `gnome-terminal --`, `konsole -e`, `xfce4-terminal -x`, `alacritty -e`, `kitty`, `foot`, `xterm -e`, and is refused with an explanation when there is none |
| Not supported yet | `.url` and `.appref-ms` shortcuts | Localized names (`CFBundleDisplayName`), Spotlight-only locations | D-Bus activation, localized names, desktop actions |
| Desktop baseline | Windows 10/11 desktop; CI runs Windows Server 2025 (`windows-2025`) | macOS 15 (`macos-15`, arm64) | freedesktop Desktop Entry 1.5 on any desktop; run on X11 (Xvfb) only, Wayland untested |

The same author-facing contract serves all three: an extension receives
`application` records (`id`, `name`, `location`) and returns
`open-application(id)`; only the adapter differs.

## Checks

- Launcher public interface ([`crates/pane-core/tests/applications.rs`](../crates/pane-core/tests/applications.rs)),
  with the real Applications guest and a fake system (so which applications
  exist and what opening does are deterministic): typing a name lists it and
  Enter opens it; ranking with a command by title; a blank query lists none
  and starts nothing; looked for once per visit of root search and found
  again after coming back; typing lists commands while the system is still
  being asked; opening and listing failures explained; disabling removes the
  applications, stops the instance and asking while the calculator still
  answers, and enabling brings them back; an answer arriving after
  disabling discarded; the command's own list; a package declaring
  `indexedResults` without the interface refused at install; and the
  JavaScript and TypeScript author examples
  ([`guests/sample-applications-js`](../guests/sample-applications-js),
  [`-ts`](../guests/sample-applications-ts)), which supply "Launch <name>"
  for each application and whose commands list and open them through the
  import, errors included.
- Host cache ([`crates/pane-core/tests/application_cache.rs`](../crates/pane-core/tests/application_cache.rs)),
  with a fake system counting scans: a fresh list is not scanned again; an
  old one comes back at once while the rescan waits, one rescan at a time,
  and the next request gets the new list; a failed rescan keeps the list and
  a failed first scan is an error; opening scans nothing.
- Window ([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)):
  typing a name renders the application's row, which assistive technology
  sees as the selected `ListBoxOption`, and Enter opens it with the field
  keeping focus.
- Adapters ([`crates/pane-core/tests/application_adapters.rs`](../crates/pane-core/tests/application_adapters.rs)):
  discovery of each system's fixtures runs on every system (precedence,
  hidden and filtered entries, subfolders, uninstallers, bundles inside
  bundles, `Exec` lines against the spec); `Exec` parsing, field-code
  checks, choosing a terminal emulator and adding the Apps folder's
  packaged apps without duplicating shortcuts have unit tests. On its own system each adapter
  opens a harmless application the test makes, which writes a marker file:
  a desktop entry (Linux), a bundle whose program is a shell script (macOS),
  a shortcut to `cmd.exe` made with `WScript.Shell` (Windows). The native
  lists are also read on each system: on macOS they must include
  Calculator, on Windows an inbox packaged app (Calculator or Settings)
  from the Apps folder.
- Native GUI smokes, one identical phase on all three systems (screenshots
  44 and 45): install the package, type "pane smoke", check the selected
  row, Enter, check "Opened Pane Smoke App" and that the application the
  smoke added (in a data folder, HOME or APPDATA of Pane's own, so the
  system's are searched too) wrote its marker file. See the
  [Linux](platforms/linux.md#applications-24-25-26), [macOS](platforms/macos.md#applications-25)
  and [Windows](platforms/windows.md#applications-24) notes for where it has run.

## Limits

- No icons, no localized names, no keywords or aliases (the user's aliases,
  [#31](aliases.md), are for installed commands only), no frequency
  ranking; applications are not ranked against commands beyond the title
  rank.
- The host imports are synchronous: the very first scan runs on the
  runtime thread, so a guest call made meanwhile waits for it (later scans
  run in the background; #29 owns cancellation).
- Pane does not hide or reset after opening an application; root search
  stays as it was.
- The adapters trust the host's own listing: `open` accepts any existing
  shortcut, bundle or desktop entry path, which any trusted extension could
  pass (Q9's trust model).
