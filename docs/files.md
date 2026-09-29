# Files

Added for [#29](https://github.com/hoangvu12/pane/issues/29) (US03, US08,
US12, US40, US44, US57; T01, T03, T09, T22; G3, G7, as contributions, not
claims that they pass), and reworked after its security review. A user
grants one folder, finds its files by typing their names into
[root search](root-search.md), and opens one with the system's handler for
its type. The feature is a **default extension**, Files, which the user can
disable like any package.

## Where it lives

A pure WASI 0.3 guest cannot read the user's folders (its WASI context
preopens none) or open a file, so Pane's host does both, and owns every step
that decides what is reached, as recorded (proposed) in
[ADR 0017](adr/0017-host-lists-a-granted-folder-for-an-extension.md):

- **The grant**, in the core: a package declaring `"folderAccess": true` in
  `pane.json` gets Pane's own "Choose folder…" row; Pane checks the folder
  and records it in its own `folders.json`
  ([`pane_core::files`](../crates/pane-core/src/files.rs),
  [`launcher/files.rs`](../crates/pane-core/src/launcher/files.rs)).
- **The listing**, in the core: `pane:extension/files`
  ([`wit/files.wit`](../wit/files.wit)), `list-folder()` without a path,
  answered at once from the listing Pane makes on the package's own worker.
- **Opening**, in the core: a computed root result's action `open-file(id)`
  ([`wit/root-results.wit`](../wit/root-results.wit)), naming a file by the
  id Pane gave it; Pane shows its own name for it, checks it again and opens
  it with the system's handler.
- **Default extension**, [`guests/files`](../guests/files) (Rust), package
  [`guests/packages/files`](../guests/packages/files): it only matches the
  listing Pane gives it against the query and answers `open-file` results.

Acquiring the package automatically at setup is
[#51](https://github.com/hoangvu12/pane/issues/51) to
[#53](https://github.com/hoangvu12/pane/issues/53); until then it is
installed from its folder like the other default extensions
(`pane --install target/guests/packages/files`).

## Granting the folder

Every command of a package with `"folderAccess": true` starts with Pane's
own rows, above the extension's items:

- **Choose folder…**, subtitled with the folder granted now ("Pane lets
  Files list only /…/Documents") or that none is. Enter opens the system's
  folder picker (`Launcher::folder_to_choose`, then the window calls
  `Launcher::grant_folder`); cancelling changes nothing.
- **Stop sharing the folder with <title>**, once one is granted: the grant
  is taken back and recorded; the folder itself is not changed.

Pane checks the chosen folder before granting it
([`files::check_grant`](../crates/pane-core/src/files.rs)) and says why not
("Pane did not grant the folder: …"), keeping the earlier grant:

| Chosen | Refused because |
| --- | --- |
| `\\server\share`, `\\?\UNC\…`, `//server/share` (Windows) | a network location, told from the text before any file system call |
| `/`, `C:\` | the whole disk |
| the home folder itself (`HOME`, `USERPROFILE`) | choose a folder inside it |
| a folder whose name starts with `.` (or hidden on Windows) | a hidden folder |
| a file, a missing path, a folder the user may not read | as it says |

A granted folder is recorded **canonically** (links resolved; on Windows
without the `\\?\` prefix) in `folders.json` beside `installed.json`, by
package identity key: `{"version": 1, "folders": {"<identity>": "<folder>"}}`.
It is Pane's record, not extension data: the extension never supplies,
saves or sees the path. It survives restarts and disabling; uninstalling
the package forgets it, whether or not its saved data is kept.

## The scan policy

The same on every system, enforced by the host
([`files::walk`](../crates/pane-core/src/files.rs)); the limits are defined
once, as `pane_core::files::MAX_DEPTH`, `MAX_FILES` and `MAX_ENTRIES`, and
extensions read them with `files.limits()`:

- **Regular files only**, from the granted folder and its subfolders,
  **breadth first** (a folder's own files before its subfolders'), each
  folder's entries **in name order** (by the bytes of their names).
- **At most 8 folders deep**, **5,000 files** and **20,000 entries** looked
  at (files, folders, links, anything). Entries are counted, and
  cancellation checked, *while* each folder is read, so a huge folder stops
  at the entry limit rather than being read whole first; only the paths of
  the folders still to list are queued, not open directory handles.
- **Skipped, neither listed nor entered**, at every level: hidden entries (a
  name starting with `.`, and on Windows the hidden and system attributes),
  symbolic links and other links (Windows junctions included) and names
  that are not Unicode.
- A subfolder (or an entry) that cannot be read is skipped and makes the
  listing **partial**: `truncated` is set, as when a limit is reached.

### When it is listed

`list-folder()` never waits. The first call in a visit of root search
starts a listing on the package's **own worker thread** (one per package,
started with its first listing) and answers `listing`; the extension
answers no files yet. The worker waits 100 ms (`files::DEBOUNCE`) before it
starts, and takes only the newest request. The listing is then **kept for
the visit**: every later keystroke gets it at once, and only filters it. It
is dropped when root search is left (a command, Manage extensions, a
preview, a restart of the visit) and when the grant changes, so the next
visit lists the folder again; there is no index and no file watching.

Once a listing ends, the commands whose answer waited for it are asked
again for the query then on screen, **after** every other result of that
query was shown: computed results (the calculator, quicklinks), and the
[indexed results](root-search.md#results-supplied-ahead-of-the-query) (the
applications). A slow or huge folder therefore holds up no other
extension's results; only its own files arrive later.

A listing stops (its answer is dropped) when root search is left, when the
grant changes, and when the package's generation ends (disable, reload,
update, pause, uninstall). A new query does not restart it: the new search
waits for the same listing, and the older search's wait is cancelled.

**A hung folder** (an unresponsive disk or a network mount the host could
not tell apart) holds only its package's worker: no new thread is started
for later requests, which wait (only the newest is kept), other extensions
are unaffected, and Files lists nothing until the listing returns. With
UNC paths refused this should be rare; mapped network drives on Windows and
network mounts on macOS and Linux are not detected.

## In root search

Files are found by name: a file is listed when each word of the query is in
its name, and then, after those, when each word is in its name or the
folders below the granted one ("notes todo" finds `notes/todo.txt`),
ignoring letter case (Unicode lowercasing, no accent folding), at most 20.
That matching is the extension's.

Each row is a
[computed result](root-search.md#results-computed-from-the-query) whose
title and subtitle are **the host's**, not the extension's: the file's own
name, and "File in <granted folder's name>/<subfolders>". A result naming an
id the host did not give in the package's latest listing is not listed.
File results are listed **after the results found by title** (commands and
applications), unlike other computed results. A blank query lists no files,
and none are listed while no folder is granted.

## Opening a file

Enter (or a click) shows "Running…" and, off the window's thread, has the
host check the file again (`FileAccess::checked_file`):

1. The id is in the package's latest listing, and that listing's folder is
   still the package's grant.
2. The path is not a network path (Windows, before any file system call).
3. `symlink_metadata`: a regular file, not a link ("it is now a link"),
   still there ("it no longer exists").
4. Its canonical path is inside the grant's canonical path (a folder above
   it replaced by a link outside: "it is no longer inside the granted
   folder").
5. It is not a program or script ([`files::runs_as_program`](../crates/pane-core/src/files.rs)),
   refused on every system: the Windows types `exe bat cmd com lnk js jse
   vbs vbe wsf wsh hta msi msp scr pif ps1 cpl reg url`, the macOS types
   `app command tool terminal workflow` and anything inside an `.app`
   bundle, `.desktop` files, and on macOS and Linux any file with an
   executable bit ("it is a program or script, which opening would run").

Only then is the checked canonical path handed to the launcher's
`LinkOpener::open_file`. The status reads "Opened <file name>" or "Could not
open <file name>: <reason>", with the host's name for the file; root search
keeps its query.

The window's opener, `pane::SystemLinks`, runs the handler the `open` crate
(5.4.4) names for the system, without a shell:

| System | Handler | A missing handler |
| --- | --- | --- |
| Linux | `xdg-open <path>`, else `gio open`, `gnome-open`, `kde-open` | none installed: "no program to open this kind of file is installed"; xdg-open finding none (status 3): "no program to open this kind of file is set up"; the program failing (status 4): "the program for this kind of file refused or failed to open it" |
| macOS | `/usr/bin/open -- <path>` (Launch Services) | its failure status |
| Windows | PowerShell with the path in an environment variable (not on its command line), which opens an existing path with `Invoke-Item -LiteralPath`; else `explorer.exe` | its failure status; an unassociated type may show the system's "Open with" dialog |

A handler still running after three seconds counts as having opened the
file. Tests replace the opener with a recording fake; a launcher given no
opener says "this Pane has no handler for files".

## For authors

A package that lists a granted folder sets `"folderAccess": true` in
`pane.json`, and its commands call `list-folder()` and answer `open-file`
results with the ids, in Rust, JavaScript and TypeScript alike
([author guide](../guests/README.md#files-of-a-granted-folder)):
`pane_guest::files::list_folder()` and `RootAction::OpenFile(id)` in Rust;
`listFolder()` from `"pane:extension/files@0.1.0"` and
`{ tag: "open-file", val: id }` in JavaScript and TypeScript, whose
`package.json` sets `"pane": { "files": true }` so that only such a
component imports the interface. The samples
[`guests/sample-files-js`](../guests/sample-files-js) and
[`guests/sample-files-ts`](../guests/sample-files-ts) do what Files does,
with a simpler match (every word in the name).

## Checks

- Launcher public interface
  ([`crates/pane-core/tests/files.rs`](../crates/pane-core/tests/files.rs)),
  with the real Files guest, a recording opener and a controlled fixture
  folder named "Pane files — ñ" holding "Résumé plan ü.txt", a subfolder, a
  hidden file and a hidden folder: nothing found before a folder is
  granted; granting through Pane's row; the grant recorded in
  `folders.json` by identity (parsed; both paths canonicalized) and not in
  the extension's settings; a hidden folder refused, keeping the grant;
  "Stop sharing" removing the files; the files found by name, then by
  subfolder, case ignored, hidden ones not; Enter opening the Unicode file
  (the opener's path and the fixture's, both resolved); file results after
  a command whose title matches; a folder gone since it was granted
  explained as a row; at Enter, a `.bat` file and an executable script
  refused, a removed file, a file replaced by a link and a folder above it
  replaced by a link outside the grant each refused, and nothing reaching
  the opener; the grant kept across a restart, hidden while disabled, and
  forgotten by uninstalling; the JavaScript and TypeScript samples; and
  the `faulty` fixture naming `/etc/hosts` itself (not listed) and giving
  each listed file the title "harmless.txt" (shown with the real names,
  and opened as such).
- Grants and the policy, on fixture folders: the root, the home folder, a
  hidden folder, a file, a missing and a relative path refused, and UNC
  forms on Windows; breadth-first name order; links not listed or followed
  (macOS and Linux); hidden attributes and junctions (Windows, written, not
  run); each limit; entries counted while a 50-file folder is read (11
  checks for a limit of 10); a cancelled listing; an unreadable subfolder
  making the listing partial (macOS and Linux, unless run as root); program
  and script types told from documents.
- The listing, with a folder lister the test holds up (same file): while
  the Files folder is listing, the applications' result and the
  calculator's answer are shown, then the files once it is released; one
  listing per visit, later keystrokes filtering it; a new query waiting for
  the same listing, the older search ending at once and only the newer
  query's file shown; leaving root search stopping the listing and the next
  visit listing again; disabling stopping it, with nothing arriving once it
  has returned (waited on, not slept); a new grant stopping the old
  listing.
- Native GUI smokes, one phase per system (screenshots 220 to 223), with a
  data folder of its own and a fixture folder "Pane smoke files" (spaces)
  holding "Résumé plan ü.txt" (non-ASCII) and an executable script or batch
  file: install Files, Enter on "Choose folder…" (the debug build takes
  the folder from `PANE_TEST_CHOOSE_FOLDER` instead of showing the picker),
  type "plan", check the selected row, Enter; then type "runner" and Enter,
  which must be refused, with the script neither handed over nor run. On
  Linux the file opens through the real `xdg-open` outside any desktop
  session, whose only handler for plain text is a script of the smoke's
  that records the path (XDG_CONFIG_HOME, XDG_DATA_HOME and BROWSER of the
  smoke's own), and the recorded path, resolved, must be the fixture
  file's; it ran on Linux X11 on 2026-09-28
  ([evidence](platforms/linux.md#files-29)). On macOS and Windows the debug
  build's `PANE_TEST_OPEN_FILE_LOG` makes the opener record the path
  instead of running `open` or `Invoke-Item` (which could open the user's
  own program or show the "Open with" dialog), and the recorded path must
  be the fixture file's; that is written but has not run yet.

## Limits

- One folder per package; no whole-disk index, other scopes, content search,
  file watching, icons, previews, recent files or ranking beyond name then
  path.
- Each visit of root search lists the folder again; a folder at the limits
  costs up to 20,000 entries per visit, and files beyond the limits are not
  found.
- Mapped network drives (Windows) and network mounts (macOS, Linux) are not
  refused; a hung one holds up only its package's worker.
- Programs and scripts are refused outright; there is no confirmation to
  open one deliberately.
- A handler slow to fail (over three seconds) is reported as having opened
  the file.
- The positive native open ran only on Linux X11 (xdg-open with a recording
  handler); on macOS and Windows the real handler is not run by the smoke.
- Screen reader behaviour is unverified, as for all of root search.
