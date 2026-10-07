# Files

Added for [#29](https://github.com/hoangvu12/pane/issues/29) (US03, US08,
US12, US40, US44, US57; T01, T03, T09, T22; G3, G7, as contributions, not
claims that they pass), and reworked after its security review. A user
grants one folder, finds its files by typing their names into
[root search](root-search.md) or into its command, **Search Files** (#150),
and opens, reveals, copies or recycles one through the file actions Pane
performs itself. The feature is a **default extension**, Files, which the
user can disable like any package.

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
- **The file actions**, in the core: a computed root result's action
  `open-file(id)` ([`wit/root-results.wit`](../wit/root-results.wit)), or a
  command search result's `file` ([`wit/search.wit`](../wit/search.wit)),
  names a file by the id Pane gave it; Pane shows its own name for it,
  gives it its own actions, and checks it again before each
  ([`launcher/own_actions.rs`](../crates/pane-core/src/launcher/own_actions.rs)).
- **Default extension**, [`guests/files`](../guests/files) (Rust), package
  [`guests/packages/files`](../guests/packages/files): its one command,
  Search Files (id `files`, `"search": true` and `"rootResults": true`),
  only matches the listing Pane gives it against the text typed and answers
  with the files' ids.

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
waits for the same listing, and the older search's wait is cancelled. A
search whose extension was told the folder is listing waits for its own
visit's listing (or a newer one), even if it ended before the search began
waiting (Files is then asked again at once); a listing of a visit already
left that stops late does not end that wait.

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

## Search Files

Search Files (#150) is a view command whose search field is the
launcher's own ([command search](command-search.md)): Enter on its row in
root search opens it with the field empty above its own list (Pane's
folder rows, then "What is searched"), and typing lists the granted
folder's files as root search does (by name, then by folder, at most 20),
each titled with its own name and "File in <folder>". A newer text stops
the search before it. Opening the command is a new visit, so the folder is
listed again then; a search answered while it is still being listed shows
"Running…" and is asked again once the listing ends, unless a newer text
(or leaving the command) stopped it first.

A command may set both `"search"` and `"rootResults"` (#150): root search
asks it, and so does its own field. Root search still never asks a
command that searches unless it says `rootResults` too.

## The file actions

Each file, in Search Files and in root search's file results alike, has
actions Pane performs itself, without calling the extension, as an item of
a command's list has them: Enter runs the first, Ctrl+Enter the second,
Ctrl+Shift+Enter the third, and the Actions panel (Ctrl+K) lists them all.

| A document | A program or script |
| --- | --- |
| **Open** (Enter): the system's handler for its type | **Show in Explorer** (Enter) |
| **Show in Explorer** (Ctrl+Enter): selected in the file manager | **Open With…** (Ctrl+Enter) |
| **Open With…**: a submenu of the installed applications, by name | **Run** (Ctrl+Shift+Enter): the system's handler, which runs it |
| **Copy Path**: its path, as text | **Copy Path** |
| **Copy File**: the file, as the file manager copies it | **Copy File** |
| **Move to Recycle Bin** (destructive): after a confirmation | **Move to Recycle Bin** |

File search's own Enter never runs a program by accident (ADR 0037's
exception, keeping ADR 0017's intent): a file that would run a program
when opened (below) is shown in the file manager, and only its explicit
**Run** runs it.
Whether a file is one is told on the listing's worker, with the listing, so
a row knows at once what Enter does. (On macOS the file manager is Finder,
so the action is "Show in Finder", elsewhere "Show in File Manager"; the
Recycle Bin is the Trash outside Windows.)

Each action closes the window after it acts and says what it did in a
HUD, as the standard actions do: Open, Show in Explorer, Open With… and
Run ("Opened plan.md", "Showed run.bat in Explorer", "Opened plan.md with
Notepad", "Ran run.bat"), Copy Path and Copy File ("Copied to
Clipboard"), and Move to Recycle Bin, once the user confirmed "Move
“plan.md” to the Recycle Bin?" (never remembered) ("Moved to Recycle
Bin"). What fails stays on screen in the status line ("Could not
open todo.txt: it no longer exists"). Opening and running go through the
launcher's link opener (`LinkOpener::open_file`), the others through its
system ([`crate::system`](../crates/pane-core/src/system.rs): reveal, open
with an application, the clipboard, the Recycle Bin), so tests record
them all.

## Checking a file again

Before every action, off the window's thread, the host checks the file
again (`FileAccess::checked_file`):

1. The id is in the package's latest listing, and that listing's folder is
   still the package's grant.
2. The path is not a network path (Windows, before any file system call).
3. `symlink_metadata`: a regular file, not a link ("it is now a link"),
   still there ("it no longer exists").
4. Its canonical path is inside the grant's canonical path (a folder above
   it replaced by a link outside: "it is no longer inside the granted
   folder").
5. For Open (Enter on a document) only: it is not a program or script
   ([`files::runs_as_program`](../crates/pane-core/src/files.rs)), on
   every system: the Windows types `exe bat cmd com lnk js jse vbs vbe
   wsf wsh hta msi msp scr pif ps1 cpl reg url`, the macOS types `app
   command tool terminal workflow` and anything inside an `.app` bundle,
   `.desktop` files, and on macOS and Linux any file with an executable
   bit ("it is a program or script, which opening would run"). A document
   that became a program since it was listed is refused so. Run, Show in Explorer,
   Open With…, the copies and the Recycle Bin act on a program as on any
   file.

Only then is the checked canonical path acted on, with the host's name for
the file in what the status says; root search keeps its query.

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
results with the ids, or command search results whose `file` is the id
(`SearchResult { file: Some(id), .. }` in Rust, `{ id, title, file }` in
JavaScript and TypeScript), in Rust, JavaScript and TypeScript alike
([author guide](../guests/README.md#files-of-a-granted-folder)):
`pane_guest::files::list_folder()` and `RootAction::OpenFile(id)` in Rust;
`listFolder()` from `"pane:extension/files@0.1.0"` and
`{ tag: "open-file", val: id }` in JavaScript and TypeScript, whose
`package.json` sets `"pane": { "files": true }` so that only such a
component imports the interface. The samples
[`guests/sample-files-js`](../guests/sample-files-js) and
[`guests/sample-files-ts`](../guests/sample-files-ts) do what Files does,
in root search and in their own field (`"pane": { "search": true }`), with
a simpler match (every word in the name), and give the same answers.

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
  revealed (through a recording system) rather than opened, a removed file, a file replaced by a link and a folder above it
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
  listing. Unit tests in
  [`pane_core::files`](../crates/pane-core/src/files.rs) hold a left
  visit's listing until the next visit's is queued and waited for (its late
  end must not end that wait), and let a listing end before the wait for it
  is made (the wait must end at once); each race failed a run of the tests
  above once (#29).
- Search Files and the file actions (#150), through the launcher
  ([`crates/pane-core/tests/file_actions.rs`](../crates/pane-core/tests/file_actions.rs)),
  for Files and the JavaScript and TypeScript samples alike, with a
  recording opener, system and window: the files listed in the command's
  own field, a newer text stopping the search waiting for the listing; a
  document's six actions, each acting through the fakes and closing the
  window (Copy Path and Copy File with their HUD), Open With… listing the
  installed applications by name, Move to Recycle Bin confirmed first; a
  program revealed by Enter, Ctrl+Enter its Open With… submenu, only Run
  running it, in Search Files and in root search; the command keeping its
  id. In the window
  ([`crates/pane/tests/file_actions.rs`](../crates/pane/tests/file_actions.rs)),
  with real keys: Enter and Ctrl+Enter on a document and on a program.
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
- Programs and scripts are revealed by Enter; only their Run action runs
  them, and it asks nothing more ("Instant file search", #126, settles the
  rest).
- A handler slow to fail (over three seconds) is reported as having opened
  the file.
- The positive native open ran only on Linux X11 (xdg-open with a recording
  handler); on macOS and Windows the real handler is not run by the smoke.
- Screen reader behaviour is unverified, as for all of root search.
