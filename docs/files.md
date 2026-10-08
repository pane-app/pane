# Files

File search: the user types a file's or a folder's name into
[root search](root-search.md), or into the **Search Files** command (#150),
and opens, reveals, copies or recycles it through the file actions Pane
performs itself. Pane's host keeps an index of the names of the files and
folders under the user's home folder (the **file index**), caught up at
start from what the file system recorded while Pane was not running and
kept current while it runs, so a file is found as quickly as a command
([#126](https://github.com/hoangvu12/pane/issues/126),
[ADR 0034](https://github.com/hoangvu12/pane/blob/2a4f9c43c990656325297a5980f34fa4bddba76e/docs/adr/0034-file-search-indexes-the-users-home-folder.md); built by
[#174](https://github.com/hoangvu12/pane/issues/174),
[#175](https://github.com/hoangvu12/pane/issues/175) and
[#176](https://github.com/hoangvu12/pane/issues/176)). The feature is a
**default extension**, Files, which the user can disable like any package;
while no enabled package uses the index, Pane neither indexes nor watches
anything.

Until #175, file search found only the files of one folder the user granted
Files (#29, [ADR 0017](adr/0017-host-lists-a-granted-folder-for-an-extension.md)).
That capability stays for other packages, unchanged
([The granted folder](#the-granted-folder)); Files no longer uses it.

Search Files works like Raycast's File Search
([#177](https://github.com/hoangvu12/pane/issues/177), spec
[#161](https://github.com/hoangvu12/pane/issues/161)): no folder to choose,
"Recently Used" before typing, a type dropdown, more rows as the list
scrolls and a detail with an image's preview and the file's Metadata
([Search Files](#search-files)).

What is indexed and how the index is doing are shown and changed on the
**File Search** page in Settings, which also lists what the index's
[safety valves](#the-safety-valves) did (#176,
[below](#the-file-search-page)).

## Where it lives

A pure WASI 0.3 guest cannot read the user's folders (its WASI context
preopens none) or open a file, and the walk and watching must never run on
the extension runtime's thread, so the index is the host's, in `pane-core`,
and one index serves every package that uses it:

- **The index**, [`pane_core::file_index`](../crates/pane-core/src/file_index.rs):
  the walker, the engine, the scope and the NTFS change journal (#174,
  [below](#the-engine-and-the-walker)); the reconciling walk
  (`file_index/reconcile.rs`); one change source per system behind the
  `ChangeSource` trait (`file_index/changes.rs` and `changes/{ntfs,macos,linux}.rs`);
  the coordinator, which opens, catches up, walks and watches the index,
  gives each entry an id and checks it again (`file_index/indexer.rs`,
  `Indexer`); and the host side of `pane:extension/file-index`
  (`file_index/host.rs`).
- **The launcher's side**, [`launcher/file_search.rs`](../crates/pane-core/src/launcher/file_search.rs):
  which packages use the index, root search's Files section and its
  "Search Files for “…”" row, the system icons, deleting the index with the
  last package that used it, moving a folder granted under #29 into the
  roots, and the calls the File Search page (#176) and Search Files (#177)
  build on (`Launcher::file_indexer`, `file_index_status`,
  `file_search_problems`, `file_search_packages`, `file_search_rules`,
  `set_file_search_rules`, `include_in_file_search`,
  `rebuild_file_index`). The rows and their actions are
  [`launcher/files.rs`](../crates/pane-core/src/launcher/files.rs) and
  [`launcher/own_actions.rs`](../crates/pane-core/src/launcher/own_actions.rs).
- **Search Files**, [`launcher/search_files.rs`](../crates/pane-core/src/launcher/search_files.rs)
  (`pane_core::search_files`): Pane's registered Files command listed by
  Pane from the index, its types, pages, detail and the index's note
  ([Search Files](#search-files)); drawn by the window's
  [`features/search_files.rs`](../crates/pane/src/features/search_files.rs)
  in the split view ([`ui/split_view.rs`](../crates/pane/src/ui/split_view.rs)).
- **The host interface**, `pane:extension/file-index`
  ([`wit/file-index.wit`](../wit/file-index.wit)): `search` and `status`,
  for any package that declares `"fileIndex": true` ([For authors](#for-authors)).
- **The default extension**, [`guests/files`](../guests/files) (Rust),
  package [`guests/packages/files`](../guests/packages/files) (0.8.0,
  `"fileIndex": true`): its one command, Search Files (id `files`,
  `"search": true` and `"rootResults": true`), answers root search from the
  index with the entries' ids. Installed as Pane's default extension, its
  screen is Pane's own Search Files view; a copy installed from a folder
  lists what is searched and answers its own field (the best 50). It is not a
  [root provider](root-search.md#root-providers): it has a row and a
  screen of its own.
- **The window**, [`crates/pane/src/main.rs`](../crates/pane/src/main.rs):
  `Launcher::with_file_index(IndexerConfig::native(cache, home, own))`
  with Pane's cache folder, the home folder (`USERPROFILE` on Windows,
  `HOME` elsewhere) and Pane's data folder, which is never indexed; and the
  File Search page,
  [`features/settings/file_search.rs`](../crates/pane/src/features/settings/file_search.rs).

Acquiring the package automatically at setup is
[#51](https://github.com/hoangvu12/pane/issues/51) to
[#53](https://github.com/hoangvu12/pane/issues/53); until then it is
installed from its folder like the other default extensions
(`pane --install target/guests/packages/files`).

## The file index

### When it runs

A package declares in `pane.json` that it uses the index:
`"fileIndex": true`. The index is opened, caught up and watched only while
at least one such package is enabled, not paused, and has a command the user
left on in Settings (`Launcher::sync_file_index`, after every change of the
packages): turning off Files' one command, Search Files, stops the index as
disabling Files does. When the last such package is disabled, paused or has
its commands turned off, watching stops at once and the index stays on
disk; turning one on again catches it up from where it stopped.
Uninstalling the last one deletes the index. This keeps lazy activation
([ADR 0005](adr/0005-lazy-activation-and-managed-dependencies.md)) without
running guest code to learn it.

At start Pane catches the index up (it is cheap); a first full walk, or a
reconciling walk the catch-up asks for, waits until the launcher is first
shown (`WindowPresence::Shown`) or 60 seconds after start
(`file_index::FIRST_WALK_DELAY`), whichever comes first. Meanwhile, and
while any walk runs, searches answer from what is indexed so far.

One thread of its own per activation ("pane-file-index"), at background
priority (background mode and EcoQoS on Windows, the background QoS class
on macOS, nice 19 and the idle I/O class on Linux), does all the writing;
the walker's threads run at the same priority. A query never runs at
background priority and never waits for the coordinator.

### Where it is kept

In Pane's cache folder, since it can always be rebuilt:
`%LOCALAPPDATA%\Pane\cache\file-index` on Windows,
`~/Library/Caches/Pane/file-index` on macOS and
`$XDG_CACHE_HOME/pane/file-index` on Linux (`file_index::INDEX_DIR`). The
folder is readable by the user only: mode 0700 (files 0600) on macOS and
Linux, and on Windows a protected DACL for the user and SYSTEM, inherited
by its files, as `credentials.json` is written. It carries a format version;
an index of another version, or one that cannot be read, is deleted and
rebuilt, never read. The folder is locked: a second Pane on the same cache
folder does not index, and its status says "Another Pane is using file
search on this computer". It is never sent anywhere.

The index records with itself whether a first walk finished, each volume's
cursor into the system's change records, and the roots and rules it was
built under, so that changing the rules (even while file search is off) is
applied the next time it opens.

### The index scope

The **roots** are the home folder (`%USERPROFILE%` on Windows, `$HOME` on
macOS and Linux) and the folders the user adds. The **rules**, applied by
the walker, the reconciling walk and every live change alike
([the walker's rules](#the-engine-and-the-walker)):

- Always left out: Pane's own data and cache folders and the index itself;
  the system's recycle and setup folders (`$RECYCLE.BIN`,
  `System Volume Information` and the like); folders tagged with
  `CACHEDIR.TAG`.
- Left out by default, each a switch: hidden entries (a leading `.`; the
  hidden or system attribute on Windows); what `.gitignore` (inside a Git
  repository), `.ignore`, `.git/info/exclude` and the global Git ignore file
  exclude, read as Git reads them without running `git`; `node_modules`,
  folders named `tmp`, `temp`, `cache` or `caches`, `*.tmp` and `*.temp`;
  the home folder's `AppData` (Windows) or `Library` (macOS); network and
  removable volumes mounted under a root.
- The user's own: added roots, excluded folders and excluded patterns (in
  `.gitignore` syntax).

The user's rules are Pane's own record, not extension data:
`file-search.json` beside `installed.json`
(`{"version": 1, "rules": {"addedRoots": [], "excludedFolders": [],
"excludedPatterns": [], "includeHidden": false, "useIgnoreFiles": true,
"defaultExclusions": true, "includeOtherVolumes": false, "quarantined": []}}`,
`file_index::UserRules`; `quarantined` holds the folders taken out for
churn). `Launcher::set_file_search_rules` records and applies them without a
restart, changing only what they affect where it can: a removed root's
entries go and an added root alone is walked; a newly excluded folder's
entries go and a folder no longer excluded is walked alone; any other change
(a pattern, a switch) walks every root again. The folders taken out for
churn stay out whatever the page sends; only
`Launcher::include_in_file_search` puts one back. The File Search page is
what changes them ([below](#the-file-search-page)).

**A folder granted to Files under #29** is kept in what is indexed
(#126 story 60): when the launcher starts with a file index
(`Launcher::with_file_index`), and when a package comes to use the index
while Pane runs (an update of Files), the folder granted to a package that
declares `"fileIndex": true` is added to the roots if the index would not
cover it otherwise (it is outside the home folder, or the rules exclude it
or a folder above it), recorded, and then the grant is forgotten
(`folders.json` no longer holds it). A granted folder the index already
covers is simply forgotten.

Entries are files and folders, named as the system names them (a name that
is not valid Unicode is shown with replacement characters and opened by
its exact name). Links and junctions are indexed as entries and never
followed. Online-only files (OneDrive, iCloud Drive, File Provider) are
indexed from their folder's listing alone, so nothing is downloaded. A
folder that cannot be read is indexed and counted, its contents not. A
root the user added that is away (an unplugged drive) keeps its entries,
hidden from searches until it is back (looked at no more than every two
seconds).

### Catching up and watching

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Catch-up at start | The NTFS change journal of each root's volume, read without administrator rights from the saved cursor ([below](#the-ntfs-change-journal-without-administrator-rights)), resolved through the folder ids the index holds | FSEvents' history, replayed by one stream over the roots from the saved event id, per volume (by its FSEvents UUID) | A reconciling walk |
| Live changes | `ReadDirectoryChangesW` on each root (the `notify` crate) | The same FSEvents stream | inotify, one watch per indexed folder, shallowest first |
| When the records cannot be used | A recreated journal, discarded records, more than a million records, a volume without a journal (FAT, exFAT, a network share) or a refused read: the volume's roots are reconciled | A new volume UUID: its roots are reconciled; a folder FSEvents asks to rescan (history purged or coalesced, events dropped) is reconciled alone; wrapped event ids reconcile every root | The folders past the watch limit (`fs.inotify.max_user_watches`) are reconciled every 5 minutes, and counted in the status |

A **reconciling walk** (`file_index::reconcile`) compares the index's own
folders with the disk and reads again only a folder whose modified time
changed (a folder's time changes when an entry is added, removed or renamed
in it); a folder new to the index is walked whole, and a folder gone takes
everything under it out. A watcher's overflow reconciles the root it
concerns.

Live changes are gathered for about 100 ms (`file_index::SETTLE`) and
applied together: each path reported is looked at again (indexed if it
exists and the rules admit it, removed otherwise, a new folder walked
whole), so a change is visible to queries well within a second of the
system reporting it. While nothing changes, nothing runs, except Linux's
reconciliation of the folders it cannot watch. The status
(`Indexer::status`, `Launcher::file_index_status`, and
`pane:extension/file-index`'s `status`) says whether the index is off,
building (and how many entries the walk found so far), current or stopped
and why, how it last caught up (`CaughtUpBy`: the journal, the event
history, a reconciling walk or a full walk) and when, and how many folders
could not be read or are not watched.

## The File Search page

Pane's own page in Settings
([`features/settings/file_search.rs`](../crates/pane/src/features/settings/file_search.rs)),
after Keyboard and before About; Settings' search finds the page and its
controls (Rebuild Index, Add Folder to Index…, Exclude Folder…, Exclude
Pattern and the switches).

- **Status**: whether the index is up to date, indexing (with how many
  entries it has found so far), waiting for the launcher to be shown,
  stopped (with why: another Pane is using file search, the disk is short of
  space, it could not be opened) or off; how many files and folders it
  holds; when and how it was last caught up ("Last caught up from the change
  journal, 5 minutes ago"). On macOS, before the first walk, it says that
  macOS will ask whether Pane may read Desktop, Documents and Downloads.
  **Rebuild Index** deletes the index and builds it again.
- **While file search is off** it says so and why: which extension that
  uses it is turned off, paused or has its commands turned off, or that
  none is installed. The rules can still be changed; they apply when it
  next runs.
- **Indexed folders**: the home folder, each added folder with Remove, and
  Add Folder… (the system's folder picker).
- **Excluded**: each excluded folder and pattern with Remove, Exclude
  Folder… (the picker) and a field for a pattern in `.gitignore` syntax.
- **Rules**: switches for hidden files and folders, ignore files, the
  default exclusions (caches, temporary folders, `node_modules`, `AppData`
  or `~/Library`) and network and removable drives.
- **Needs attention** (`Launcher::file_search_problems`,
  `file_index::Problem`): folders that could not be read; folders macOS did
  not allow, with how to allow them under System Settings › Privacy &
  Security › Files and Folders; folders not watched on Linux, with the
  setting that raises the limit (`fs.inotify.max_user_watches`); folders
  taken out for churn, each with **Include Again**; folders that did not
  answer; a walk stopped at the ceiling; a stop for free space. Each says
  why and what to do.

Every control goes through `Launcher::set_file_search_rules` (or
`include_in_file_search`, `rebuild_file_index`), off the window's thread;
a failure is the page's status. The page reads the launcher every frame,
and Settings' watcher redraws it when the index's status, problems or rules
change while it shows.

### The safety valves

All four are the coordinator's (`file_index::Indexer`, thresholds in
`file_index::Valves` and `WalkOptions`; #126's proposed values, smaller in
tests), and each is listed on the page:

- **Churn quarantine**: the changes each folder reports are counted, per
  folder they are in, in windows of a minute (`Valves::churn_window`); a
  folder with more than 1,000 changes (`churn_changes`) in 3 windows in a
  row (`churn_windows`) is taken out of the index: its entries go, the rules
  leave it out from then on (it is added to `UserRules::quarantined`,
  recorded in `file-search.json`, so it stays out after a restart), and its
  changes are no longer looked at. A root itself is never taken out. Include
  Again puts it back and walks it alone. Counting costs nothing while
  nothing changes.
- **The ceiling**: a walk stops at 5 million entries
  (`WalkOptions::max_entries`, `file_index::MAX_ENTRIES`) and says so; the
  index keeps what was walked.
- **The free-space floor**: before the index is written (a walk, a batch
  of changes, a reconciling walk), and once a second during a first walk,
  Pane reads the free space of the volume holding its cache
  (`GetDiskFreeSpaceExW` on Windows, `statvfs` elsewhere,
  `file_index::free_space`). Under 1 GB (`Valves::free_space_floor`,
  `FREE_SPACE_FLOOR`: #126 proposes 1 GiB; Pane writes sizes in decimal
  units, so the floor is 1,000,000,000 bytes and the page says "1 GB")
  indexing stops writing: the status is Stopped and says
  why, a first walk under way is given up, and changes reported meanwhile
  are let go. Pane looks again every minute (`space_retry`); once there is
  room it starts again by itself, walking what was not walked and
  reconciling every root if changes were let go (the cursors saved before
  are kept until then).
- **Hung folders**: each walker thread (and each reconciling walk) lists
  folders through a helper thread of its own, and waits at most 10 seconds
  (`WalkOptions::hung_after`, `HUNG_AFTER`) for a folder; one that does not
  answer is skipped for this walk (indexed, its contents not; a reconciling
  walk keeps what the index held of it), listed, and its helper left behind
  to end whenever the system answers it, so a stalled network mount or a
  dying disk holds up only itself.

**Sleep** (#126 "Every system", story 43): indexing pauses while the
computer sleeps and resumes 5 seconds after it wakes (`Valves::resume_after`,
`file_index::power`). Pane learns of a sleep from two clocks every system
keeps, one that runs on through a sleep and one that stops (Windows'
interrupt time and unbiased interrupt time, Linux's `CLOCK_BOOTTIME` and
`CLOCK_MONOTONIC`, macOS's `CLOCK_MONOTONIC` and `CLOCK_UPTIME_RAW`): their
difference grows by exactly each sleep, and the wall clock, which the user
or time synchronization may move, is not read. The coordinator looks before
each batch of changes and each pass of its loop, and each walker thread
before each folder; whoever first sees the difference grow starts the
pause, and everyone waits it out, the status saying "Paused: the computer
slept". The system's own notifications (`WM_POWERBROADCAST`, IOKit's
`IORegisterForSystemPower`, logind's `PrepareForSleep`) are not used: each
needs a window, a run loop or a D-Bus connection of its own, and while the
computer sleeps every thread of Pane is frozen anyway (the write-ahead log
keeps what was half applied); what matters is what follows the wake, which
the first look after it catches. Time limits count awake time only: the
first walk's delay takes the sleep out, a folder whose wait the sleep
interrupted is given its limit again before it counts as hung, and the
churn windows start again after a sleep.

## In root search

Files answers root search through `root-results`, now from the index: its
call returns at once from the host and never waits for a walk, so a busy
disk holds up no other result. A query of one character or more lists:

- at most **5 file rows** per command (`ROOT_FILE_ROWS`), the index's best
  by its own scoring: an exact name above an exact stem, a name prefix, a
  word start and then a match in the folders; case and accents ignored
  ("resume" finds `Résumé.pdf`); the folders' words count ("invoices march"
  finds `Invoices 2026/march.pdf`); newer entries a little higher; folders
  found as well as files;
- then **"Search Files for “<query>”"**, which opens Search Files with the
  query typed in its own field and searched at once.

They are listed **after** the commands, applications and quicklinks found
by title, under the section "Files". Each row is the host's, whatever the
extension's result says: its title is the entry's own name, its subtitle
its folder (below the home folder as `~/…`), its icon the system's icon for
the path (#142, a document's or folder's outline until it is loaded), and
its kind **File**, or **Folder** for a folder. A blank query lists no
files. A result naming an id the index did not give the package is not
listed. No use of a file row is recorded for learning (ADR 0030).

## Search Files

Search Files works like Raycast's File Search (#177, spec #161; it
replaces #126's slice 4 as #161 amends it). Pane draws it itself for its
registered Files default extension (the default extension `files` and its
command `files`, by their verified identity, never by a title), in the
split view Clipboard History uses (#102), and lists the index for it
(`launcher/search_files.rs`); the command's own list and `search` are not
shown. A copy of Files installed from a folder, and any other command that
searches, keep the launcher's [command search](command-search.md).

- **No folder to choose**, nothing explained first: Enter on its row in
  root search opens it on **"Recently Used"**, the most recently modified
  entries the index holds (files and folders, as Raycast's blank query
  lists them).
- **Typing** ranks what the index finds by its own matching
  ([In root search](#in-root-search)). Opened from root search's "Search
  Files for “plan”" row, the field holds "plan" and lists its results at
  once. Escape clears the field (Recently Used comes back, listed in the
  background), and leaves on an empty field; the back button leaves at
  once.
- **The type dropdown** at the search field's right ("Filter by Type"):
  All Types, Folder, Document, Image, Video, Audio, Archive, Text,
  Application, Other (`search_files::FileType`: All Types, Folder, or one
  of the index's categories, whose names it reads). Folder keeps folders;
  the others keep the index's categories, told from the name's extension by
  one table on every system (`file_index::Category`): Document is PDF,
  office and e-book files and web pages; Text is plain text, data,
  configuration and source files (`txt`, `md`, `csv`, `json`, `yaml`,
  `toml`, `log`, `rs`, `py`, `js` and the like); Application is programs,
  scripts, shortcuts, installers and application bundles; Other is a file
  of none of them (`data.bin`, `README`). The type stays while the text
  changes.
- **Pages**: 50 rows at a time (`search_files::PAGE`); the next page loads
  as the list scrolls within ten rows of its end
  (`Launcher::load_more_files`), until the index has no more.
- **Rows**: each file's own name, its folder below the home folder
  (`~/Documents`) and the system's icon for it (#142, a document's or
  folder's outline until it is loaded), drawn as they come into view
  (#165). They are the launcher's own file rows, with
  [the file actions](#the-file-actions): Enter, Ctrl+Enter and the Actions
  panel (Ctrl+K) act as on any file row, and Enter on a program shows it
  and never runs it. A click selects; a double click is Enter.
- **The detail** beside the list, for the selected file: an image's
  preview (`png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `tif`, `tiff`,
  `ico` and `svg`, `icons::DRAWN_IMAGE_EXTENSIONS`, what the window draws;
  the Images category holds these and images Pane cannot draw, such as a
  camera's raw files; at most 32 MB, never a link's target), else the file's
  icon, large; under it the **Metadata**: Name, Where (`~/…`), Type ("PNG
  Image", "PDF Document", "MD Text", "Folder", "Application"), Size (in decimal
  units, as macOS's Finder and the Linux file managers write them: "532
  bytes", "1.2 MB"; `file_index::size_words`, the one formatter for sizes,
  which the File Search page uses too; not for a folder),
  Created (where the system records it) and Modified ("Today at 14:02",
  "Sep 28 at 16:12"), read from the file system when the file is selected
  (`Launcher::search_files_details`).
- **The index's state**, over the list or in its place: "Indexing… (1204
  found so far)" while it is built, with a thin bar under the header while
  a search or the index is in progress; "File search stopped: <why>" or
  "File search is off: <why>", with **Open File Search Settings**, which
  opens Settings at the [File Search page](#the-file-search-page). While the index is built
  the list is asked again each second it grew, keeping the selected file
  selected, until the user scrolled past the first page.
- **The footer**: the command's icon and title (or the status), the
  selected file's primary action and Actions.

The window takes the split view's 940×600 while Search Files shows, as for
Clipboard History.

## The file actions

Each file, in Search Files and in root search's file results alike, has
actions Pane performs itself, without calling the extension, as an item of
a command's list has them: Enter runs the first, Ctrl+Enter the second,
Ctrl+Shift+Enter the third, and the Actions panel (Ctrl+K) lists them all.

| A document | A program or script | A folder |
| --- | --- | --- |
| **Open** (Enter): the system's handler for its type | **Show in Explorer** (Enter) | **Open** (Enter): the file manager |
| **Show in Explorer** (Ctrl+Enter): selected in the file manager | **Open With…** (Ctrl+Enter) | **Show in Explorer** (Ctrl+Enter) |
| **Open With…**: a submenu of the installed applications, by name | **Run** (Ctrl+Shift+Enter): the system's handler, which runs it | **Copy Path** |
| **Copy Path**: its path, as text | **Copy Path** | **Copy Name** |
| **Copy Name**: its name, as text (#177) | **Copy Name** | **Copy File** |
| **Copy File**: the file, as the file manager copies it | **Copy File** | **Move to Recycle Bin** |
| **Move to Recycle Bin** (destructive): after a confirmation | **Move to Recycle Bin** | |

File search's own Enter never runs a program by accident
([ADR 0037](https://github.com/hoangvu12/pane/blob/2a4f9c43c990656325297a5980f34fa4bddba76e/docs/adr/0037-a-command-declares-its-mode-and-host-functions-decide-what-happens-after-it-runs.md)):
a file that would run a program when opened ([below](#opening)) is shown
in the file manager, and only its explicit **Run** runs it; choosing Run is
the confirmation, so nothing more is asked. (On macOS the file manager is
Finder, so the action is "Show in Finder", elsewhere "Show in File
Manager"; the Recycle Bin is the Trash outside Windows.)

Each action closes the window after it acts and says what it did in a
HUD, as the standard actions do: Open, Show in Explorer, Open With… and
Run ("Opened plan.md", "Showed run.bat in Explorer", "Opened plan.md with
Notepad", "Ran run.bat"), Copy Path, Copy Name and Copy File ("Copied
to Clipboard"), and Move to Recycle Bin, once the user confirmed "Move
“plan.md” to the Recycle Bin?" (never remembered) ("Moved to Recycle
Bin"). What fails stays on screen in the status line ("Could not
open todo.txt: it no longer exists"). Opening and running go through the
launcher's link opener (`LinkOpener::open_file`), the others through its
system ([`crate::system`](../crates/pane-core/src/system.rs): reveal, open
with an application, the clipboard, the Recycle Bin), so tests record
them all.

## Opening

A row names its entry by the id the index gave the package that found it
(`i<generation>-<n>`, the newest 10,000 kept per package); an id of an
earlier index (closed, rebuilt or deleted since) or of another package is
not known ("Pane no longer knows it; search again"). Before every action,
off the window's thread, the host checks the entry again
(`Indexer::checked`):

1. The path is not a network path (Windows, before any file system call).
2. `symlink_metadata`: still there ("it no longer exists"), of the kind
   indexed ("it is now a folder", "it is now a file"), not a link where a
   file or folder was indexed ("it is now a link"; a link itself is never
   opened: "it is a link, which Pane does not follow").
3. It is still in the index scope ("it is no longer in the folders file
   search covers"), and its canonical path is under a root's (a folder
   above it replaced by a link outside: "it is no longer inside the folders
   file search covers").
4. Whether it is a **program**: any entry of the types below, the same on
   every system, or on macOS and Linux a file with an executable bit
   ([`files::runs_as_program`](../crates/pane-core/src/files.rs)): the
   Windows types `exe bat cmd com lnk js jse vbs vbe wsf wsh hta msi msp
   scr pif ps1 cpl reg url`, the macOS types `app command tool terminal
   workflow` and anything inside an `.app` bundle, and `.desktop` files.
   A row already knows from the name whether it is one; the executable bit
   is told at this check, so a document that became a program since it was
   found is shown in the file manager rather than opened.

Only then is the checked canonical path acted on: Enter hands a document or
a folder to the system's handler, and shows a program in the file manager;
Run, Show in Explorer, Open With…, the copies and the Recycle Bin act on a
program as on any file.

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

A package that searches the index sets `"fileIndex": true` in `pane.json`;
its commands call `search(query, options)` and `status()` of
`pane:extension/file-index`, and answer `open-file` results with the
entries' ids, or command search results whose `file` is the id
(`SearchResult { file: Some(id), .. }` in Rust, `{ id, title, file }` in
JavaScript and TypeScript) ([author guide](../guests/README.md#panes-file-index)):

- `search` answers at once from what is indexed, never waiting for a walk:
  the entries matching the query (a blank query: the most recently
  modified), filtered by kind (file, folder, link) and category
  (documents, images, audio, video, archives, applications, text, and
  other for a file of none of these, told from the name's extension by one
  table on every system, the one Search Files' dropdown uses), sorted by relevance or
  modified time, paged by `limit` (at most 200 per call) and `offset`. Each
  entry carries its id, absolute path (text, for showing and copying),
  name, folder for people, kind, whether opening it would run a program,
  size, modified time and volume. A package that does not declare
  `"fileIndex": true` is refused.
- `status` answers the state (off, building, current, stopped), the
  entries indexed, the entries the walk in progress found and why.

Rust: `pane_guest::file_index::{search, status}` and
`RootAction::OpenFile(entry.id)`; JavaScript and TypeScript: `search` and
`status` from `"pane:extension/file-index@0.1.0"`
([`guests/js/file-index.d.ts`](../guests/js/file-index.d.ts); WIT's `u64`
numbers are `bigint`), whose `package.json` sets
`"pane": { "fileIndex": true }` so that only such a component imports the
interface, and `{ tag: "open-file", val: entry.id }`. The samples
[`guests/sample-files`](../guests/sample-files),
[`guests/sample-files-js`](../guests/sample-files-js) and
[`guests/sample-files-ts`](../guests/sample-files-ts) do what Files does,
in root search and in their own field, and give the same answers. A command
that chooses to open or run an entry explicitly may use the host's open
functions of ADR 0037 with the path; that is its own explicit action.

## The granted folder

The capability file search used before #175 stays, unchanged, for a
package that wants an exhaustive listing of one folder the user chooses,
including a folder the index leaves out
([ADR 0017](adr/0017-host-lists-a-granted-folder-for-an-extension.md),
superseded for file search by ADR 0034). Files no longer declares it; the
test fixture [`guests/fixtures/folder-files`](../guests/fixtures/folder-files),
what Files was before, keeps it covered. Its own `open-file` results keep
ADR 0017's refusal to open programs and scripts, since such a package never
offered a Run action.

- **The grant**, in the core: a package declaring `"folderAccess": true` in
  `pane.json` gets Pane's own "Choose folder…" row; Pane checks the folder
  and records it in its own `folders.json`
  ([`pane_core::files`](../crates/pane-core/src/files.rs),
  [`launcher/files.rs`](../crates/pane-core/src/launcher/files.rs)).
- **The listing**, in the core: `pane:extension/files`
  ([`wit/files.wit`](../wit/files.wit)), `list-folder()` without a path,
  answered at once from the listing Pane makes on the package's own worker.
- **The file actions** are the ones above, checked again against the grant
  (`FileAccess::checked_file`): the id in the package's latest listing,
  that listing's folder still its grant, not a network path, a regular
  file and not a link, its canonical path inside the grant's, and for Open
  not a program ("it is a program or script, which opening would run").

### Granting the folder

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

### The scan policy

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

#### When it is listed

`list-folder()` never waits. The first call in a visit of root search
starts a listing on the package's **own worker thread** (one per package,
started with its first listing) and answers `listing`; the extension
answers no files yet. The worker waits 100 ms (`files::DEBOUNCE`) before it
starts, and takes only the newest request. The listing is then **kept for
the visit**: every later keystroke gets it at once, and only filters it. It
is dropped when root search is left (a command, Settings › Extensions, a
preview, a restart of the visit) and when the grant changes, so the next
visit lists the folder again; this listing keeps no index and watches
nothing.

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
waiting (the package is then asked again at once); a listing of a visit already
left that stops late does not end that wait.

**A hung folder** (an unresponsive disk or a network mount the host could
not tell apart) holds only its package's worker: no new thread is started
for later requests, which wait (only the newest is kept), other extensions
are unaffected, and the package lists nothing until the listing returns. With
UNC paths refused this should be rare; mapped network drives on Windows and
network mounts on macOS and Linux are not detected.

## The engine and the walker

The measured first slice of #126 ([#174](https://github.com/hoangvu12/pane/issues/174)),
which the coordinator above builds on:

- **The walker** (`file_index/walker.rs`): every folder under the roots the
  rules admit is listed once, by up to 8 threads of its own at background
  priority (`file_index/priority.rs`: background mode and EcoQoS on
  Windows, the background QoS class on macOS, nice 19 and the idle I/O
  class on Linux). On Windows a folder is read with
  `GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`, which gives each
  entry's attributes, size, times and NTFS file id in one call per 64 KB;
  elsewhere with `read_dir` and each entry's own metadata. Links and
  junctions (reparse points whose tag names another file) are indexed as
  entries and never followed; cloud-file placeholders are listed like any
  entry, so nothing is downloaded. A folder that cannot be read is indexed,
  counted and named; a walk stops at 5 million entries.
- **The rules** (`file_index/scope.rs`, `ScopeRules::for_home`): hidden
  entries (a leading `.`; the hidden or system attribute on Windows);
  `.gitignore` inside a Git repository, `.ignore` anywhere, the
  repository's `.git/info/exclude` and the global Git ignore file, matched
  by ripgrep's `ignore` crate as Git matches them; `node_modules`, folders
  named `tmp`, `temp`, `cache` or `caches`, `*.tmp` and `*.temp`; the home
  folder's `AppData` (Windows) or `Library` (macOS); network and FAT or
  exFAT volumes mounted under a root (Linux); and always the system's
  recycle and setup folders, folders tagged with `CACHEDIR.TAG` and Pane's
  own folders. Each but the last is a switch, and the user's folders and
  `.gitignore`-style patterns add to them. `Scope::admits` applies the same
  rules to one path, reading the ignore files above it, for changes.
- **The engine** (`file_index/store.rs`), in the shape of `minidex`: a
  memory table of recent changes, logged first to a write-ahead log
  (`file_index/wal.rs`, records with a CRC, a torn tail dropped); immutable
  segments (`file_index/segment.rs`) holding the entries sorted by path,
  front-coded in blocks of 16, each with a fixed 4-byte hint (day modified,
  depth, kind) and the postings of its terms, whose dictionary is an `fst`
  map read through a memory map; prefix tombstones hiding a deleted or
  renamed folder's entries in older segments; and a merge of all segments
  into one, which drops superseded versions, deletions and what tombstones
  hide, once there are more than 8. A first walk writes segments directly,
  without the log, and merges them at the end. Terms are the folded words
  (case and accents ignored, split at camel case and digits) of the name
  and, separately, of the folders below the root. A query reads, per
  segment, at most 1,000 candidates matching every word, those with every
  word in the name first, ranked by their hints, then scores them as #126's
  "Matching and ranking" describes (`file_index/text.rs`): an exact name,
  then an exact stem, a name starting with the query, every word starting
  a word of the name, then of the folders. A query with `/` or `\`
  matches path segments in order: the words of each part start words of
  one folder below the root, those folders in the parts' order (others may
  lie between), and the last part's the name ("documents/plan",
  `docs\work\` for anything inside); it ranks just below a name prefix.
  When the words' starts find fewer entries than the page asks for, a
  second pass looks for words of three letters or more inside words
  ("port" finds "report", in a name or a folder), reading every term of the
  dictionary, and lists those after every match by the start of words
  (`text::score_inside`, its score put 100 below). Keys are the
  path's exact bytes (WTF-8 on Windows), so a name that is not valid
  Unicode is shown with replacement characters and still opened exactly.
  Each entry keeps its kind, size, modified time, file id and volume.
- **Its folder**: `index.json` (the format version, the live segments, and
  Pane's record: whether a first walk finished, each volume's journal
  cursor, and the roots and rules it was built under), `<n>.seg`, `<n>.wal`, and `lock`, which the index holds locked,
  so a second Pane on the same cache folder gets `IndexError::InUse`. The
  folder is mode 0700 and the files 0600 on macOS and Linux; on Windows
  the folder is made with a protected DACL for the user and SYSTEM, which
  its files inherit (#175). An index of another `FORMAT_VERSION`,
  or one that cannot be read, is deleted and rebuilt, never read. The log
  is handed to the system after each batch but not flushed to the disk: a
  power loss can lose the last changes, which the catch-up finds again from
  the file system's records, since the cursors are saved only with
  segments.

### The engine: Pane's own, not the `minidex` crate

Decided on reading `minidex` 0.38.0's source (MIT, by Joao Neves, the
crate Raycast uses), before the measurement, which is to confirm it:

- **Format.** #126 requires Pane's index format to be Pane's, versioned,
  and rebuilt on any other version. `minidex` writes its own files, and at
  0.38 it is pre-1.0 and changing quickly (each release may change them);
  Pane would version a format it does not control.
- **What an entry keeps.** `minidex` keys an entry by
  `path.to_string_lossy()`, so a name that is not valid Unicode cannot be
  opened from the index; and it keeps kind, modified and accessed times, a
  category and a volume type, but no size and no file id. The NTFS
  catch-up resolves journal records by file id and parent folder id, and
  Search Files shows sizes, so both would need a second store beside it.
- **Tombstones.** Its prefix tombstones compare paths in ASCII lower case,
  so deleting `~/Docs` would also hide `~/docs` on a case-sensitive file
  system (Linux, case-sensitive APFS).
- **Threads and priority.** It starts its own flush, compaction and
  recovery threads with its own priority policy; #126 wants Pane's
  coordinator to own background priority, pausing for sleep and "no
  periodic work while nothing changes".
- **Matching.** Its tokenizer and candidate pruning are fixed; #126 wants
  folding consistent with root search's and weights tuned against Pane's
  fixtures, with name and folder matches told apart.
- **Dependencies.** It brings `zstd` (a C library built by `cc`), `fs4`,
  `arc-swap`, `thiserror` and `log`; Pane's own engine adds only `fst`
  (no dependencies of its own), `memmap2` (already in the tree) and, for
  the rules rather than the engine, `ignore`.
- **The first index.** Its inserts all go through its log; Pane's first
  walk writes segments directly.

What Pane takes from it is the shape: segments with an FST dictionary, a
memory table and log, tombstones, compaction, and a compact per-entry hint
for pruning candidates before reading them.

**The measurement that confirms it** is the benchmark below on the machine
where Raycast was measured, against the home folder: a first index in less
than 12.9 s (aiming for half), an index smaller than 61.8 MB, a query's
95th percentile under 10 ms, and a changed file visible within 10 ms. If
Pane's engine misses the size or query target, `minidex` 0.38 is measured
on the same tree behind the same `FileIndex` calls before Pane's is tuned
further; the result and the decision are recorded in #174's results
comment.

### The benchmark

`cargo xtask file-index-bench [options]` builds
[`crates/pane-core/examples/file_index_bench.rs`](../crates/pane-core/examples/file_index_bench.rs)
in release and runs it; it runs on demand, never in CI. By default it
generates a home-shaped tree of about 450,000 indexable entries (9 files
per folder, paths about 8 folders deep, accented names, and, left out by
the rules, hidden folders, `node_modules` and Git repositories with ignored
`build` folders and logs) in the system's temporary folder, reused by later
runs; `--home` indexes the real home folder instead (it writes nothing
there), `--root <folder>` another folder. It prints, as a table against
#126's targets and Raycast's numbers:

- the first index, run 1 and the median of the warm runs (`--runs`,
  default 3), with the walk's own time; run 1 is cold only when it is the
  first since a restart, or after `--drop-caches` where the system allows
  it (Linux as root, macOS with `sudo purge`; Windows has no way without
  administrator rights);
- the index on disk, and per entry;
- query latency over a fixed set (`--queries`, default 1,000: whole names,
  3-letter prefixes, words, folder and name words, one letter), the first
  pass after opening and a warm pass, with the 50th, 95th and 99th
  percentiles and per kind;
- a changed file re-indexed: written, read, applied and found by a query,
  100 times;
- on Windows with the generated tree, the catch-up after 10,000 files
  created: the journal read from a saved cursor, resolved, looked at,
  applied and found;
- the time to open the index at start, the private memory it adds while
  idle, and the peak memory while indexing.

### The NTFS change journal without administrator rights

`file_index::read_journal` opens the volume's root folder (`C:\`) for
reading attributes only, asks for the journal with
`FSCTL_QUERY_USN_JOURNAL`, and reads it from a saved cursor with
`FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` (Windows 10 1709 and later), never
creating or resizing a journal. It answers the records and the new cursor,
or why a walk is needed: the journal was recreated (another id), the
records were already discarded, there are more than a walk would cost, the
volume keeps none (FAT, exFAT, network shares), or the system refused.
`file_index::resolve` turns the records into paths through the folder ids
the index holds (each folder's NTFS file id, read by the walk), following
renamed folders; a record in a folder the index does not hold resolves to
nothing. `catch_up_changes` then looks at each path and applies the scope.

Read without administrator rights, the records carry no names (Windows
leaves them out; seen on Windows 11 as a non-elevated user). So a record
without a name is named by its file id (`file_index::Names`: the entry
opened with `OpenFileById` for reading attributes only, its name read from
the handle, each id once, at most 100,000 per catch-up); an entry gone has
no name to read, so a folder gone is known by the id the index holds, and
a file gone puts its folder in `CatchUp::listed`, whose entries the disk no
longer holds are removed (`file_index::missing_from`).

The test
`file_index::journal::tests::the_journal_is_read_without_administrator_rights_and_resolves_to_paths`
indexes a temporary folder, makes changes (a file created, one deleted, a
folder renamed, a file in it created), and reads them back from the
cursor. Run as a non-elevated user, it is the evidence #174 asks for (CI's
Windows runner is an administrator).

The other systems' equivalents are FSEvents' history on macOS (no
permission beyond reading the folders, and the privacy prompts for Desktop,
Documents and Downloads) and, on Linux, where no history is readable
without privileges (fanotify needs `CAP_SYS_ADMIN`), the reconciling walk;
see [Catching up and watching](#catching-up-and-watching).

## Checks

Written with #175; none has run yet (tests run once every ticket of the
milestone is merged).

- **The index through the launcher**
  ([`crates/pane-core/tests/file_index.rs`](../crates/pane-core/tests/file_index.rs)),
  with the real Files guest, the real index and the system's own change
  source over a fixture folder standing for the home folder (the test names
  it) and a cache folder of the test's own, a recording opener and system,
  waiting on `Launcher::wait_for_file_index`: typing a file's name lists it
  under "Files", exact name first, at most five, then "Search Files for
  “plan”", with its folder `~/Documents`, and Enter opens it; hidden,
  ignored, `node_modules` and cache-tagged entries absent; a row from the
  first character and none for a blank query; case, accents and folder
  words; a folder found, reading Folder; file rows after a command found
  by title; a program, a script and a shortcut each revealed by Enter and
  never opened; an entry replaced by a folder since it was found explained,
  not opened; the first walk waiting until the launcher is shown;
  disabling Files stopping the index (kept on disk), a file written then
  found once it is enabled again, and uninstalling deleting the index; a
  restart catching up what changed while Pane was stopped by the system's
  records (`CaughtUpBy::Journal` on Windows, `EventHistory` on macOS,
  `ReconcilingWalk` on Linux), not a full walk; a second Pane on the same
  cache folder saying file search is in use; the user's rules recorded in
  `file-search.json` and applied without a restart. For the File Search
  page (#176): the status (state, entries, how and when it last caught
  up); Rebuild Index walking every folder again; turning off Search Files
  stopping the index and saying why; every control (an added and removed
  root, an excluded folder and pattern, each switch) changing what is found
  without a restart; a folder taken out for churn listed and included
  again; a folder granted to Files under #29 outside the home folder added
  to the roots and the grant forgotten, and one the index covers simply
  forgotten.
- **The coordinator through the change source's seam**
  (`file_index::indexer` unit tests): a fake source the test scripts (what
  a catch-up finds) and drives (the live changes it reports), with the real
  index and walker: the first walk waiting to be shown; only a package
  that uses the index searching it, ids its own and checked again
  (removed, replaced by a folder, by a link); kinds, categories, sorting
  and pages; created, moved-in, renamed and deleted entries applied, a
  hidden one not; an overflow reconciling its folder; disabling stopping
  the watch and enabling again catching up without walking; records that
  are gone leading to a reconciling walk with the same result; a restart
  over the same cache folder; a second index refused; deleting the index
  forgetting its ids; the user's rules (hidden entries, an added root, an
  excluded folder, a removed root); an added root away and back; the
  folders a source cannot watch counted, listed and reconciled every few
  minutes. Each safety valve triggered: a folder churning taken out,
  listed, recorded, kept out across a rule change and included again, and
  one busy now and then never taken out; a walk stopped at the ceiling;
  indexing stopped while the disk is short of space (nothing written, a
  change let go) and started again by itself once there is room (the walk,
  then the change caught up); a folder that does not answer skipped and
  listed without holding up the walk; a sleep (a fake computer the test
  puts to sleep, `power::tests::FakeAwake`) pausing a batch of changes and
  a walk under way until the pause after the wake is over, the status
  saying so, the sleep not counted as hanging; a folder macOS refused
  listed apart from one that cannot be read. `file_index::power` tests the
  pause itself and that the system's clocks never say a time asleep that
  goes back; `file_index::privacy` tells a refusal from the system's
  answer (`EPERM` on macOS only) on every system. Excluding a folder takes only it out
  and including it again walks only it. The walker's own tests hold up a
  folder's listing to show the walk does not wait for it.
  `file_index::reconcile` and `store` unit tests cover reading only changed
  folders, a missing root keeping its entries, a folder's children
  across segments and memory, a query found inside words after the words
  it starts (segments and memory alike, not when the words' starts fill
  the page) and a query with `/` or `\` matching path segments in order;
  `file_index::text` tests the ranks of each.
- **Per system**: the NTFS journal read without administrator rights
  (`file_index::journal` tests, #174); inotify reporting a change in a
  watched folder and a folder added later, and stopping when dropped
  (`changes::linux` tests); FSEvents replaying a change made before the
  stream opened from a saved event id, saying the history is done, then
  reporting a live change (`changes::macos` tests). The watch limit cannot
  be lowered without root; its fallback is driven through the seam above.
- **Search Files and the file actions**
  ([`crates/pane-core/tests/file_actions.rs`](../crates/pane-core/tests/file_actions.rs)),
  for Files and the Rust, JavaScript and TypeScript samples alike: the
  entries listed in the command's own field with their folders, a folder
  found; a document's seven actions acting through the fakes and closing the
  window; Move to Recycle Bin confirmed first; a program revealed by Enter,
  Ctrl+Enter its Open With… submenu, only Run running it, in Search Files
  and in root search; root search's Files section with the system's icon
  and the row opening the command with the query typed; a folder opened in
  the file manager; a file created while Pane runs found through the
  system's watcher, and explained at Enter once deleted; the command
  keeping its id. In the window
  ([`crates/pane/tests/file_actions.rs`](../crates/pane/tests/file_actions.rs)),
  with real keys over the index: Enter and Ctrl+Enter on a document and on
  a program.
- **Search Files like Raycast's** (#177;
  [`crates/pane-core/tests/search_files.rs`](../crates/pane-core/tests/search_files.rs)),
  with Files acquired as Pane's default extension from a local artifact
  source, over the real index of a fixture home: it opens with no folder
  to choose on Recently Used, newest first, each row with an icon; typing
  ranks by the index; each type of the dropdown keeps only its files (and
  Folder only folders), with a query too; the detail's Name, Where, Type,
  Size, Created and Modified, an image previewed and a text file not; pages
  of 50 loading until the index has no more, the selection kept; the
  query kept from root search's row; Escape bringing Recently Used back;
  a document's seven actions with Copy Name copying the name, Enter opening
  it, and a program shown, not run; the index being built and a stopped
  index said, the latter leading to the settings; a copy installed from a
  folder keeping its own search. Unit tests: the types, the sizes, the
  Type label, the detail's metadata and the notes
  (`launcher::search_files`), the categories Text and Other
  (`file_index::indexer`). In the window
  ([`crates/pane/tests/search_files.rs`](../crates/pane/tests/search_files.rs)):
  the split view on Recently Used with system icons and the dropdown at
  the field's right; the dropdown's ten types filtering; an image's
  preview and the Metadata rows, a text file's icon instead; typing and
  Escape; Ctrl+K listing Copy Name and Enter showing a program.
- **The File Search page in the window**
  ([`crates/pane/tests/file_search_settings.rs`](../crates/pane/tests/file_search_settings.rs)),
  over the real Files and index: the status and Rebuild Index; the hidden
  switch, the pattern field and Remove, Add Folder… and Exclude Folder…
  through the system's picker (and a cancelled one), each applied without a
  restart and recorded; the page saying file search is off while Files is
  disabled, or with no extension that uses it; each valve listed (a churned
  folder with Include Again, the ceiling, free space, a folder that did not
  answer); Settings' search finding the page and Rebuild Index.
- **The granted folder**
  ([`crates/pane-core/tests/files.rs`](../crates/pane-core/tests/files.rs)),
  with the `folder-files` fixture: the grant, its record and its refusals,
  the scan policy, the listing per visit and its cancellation, the checks
  at Enter, as since #29.
- **Native GUI smokes**, one phase per system (screenshots 220 to 224): a
  debug build's `PANE_TEST_FILE_INDEX_HOME` names a fixture folder for the
  index to cover instead of the home folder (keeping the index in the
  phase's data folder); install Files, type "plan" and open the file found,
  then start Pane again, type "runner" and Enter, which reveals the script
  and runs nothing. Rewritten for the index with #175; not run yet.

## Limits

- A sleep is noticed at the first piece of indexing work after the wake,
  not as it begins (see [Sleep](#the-safety-valves)); a folder's listing
  already asked of the system when the computer slept is not interrupted.
- Churn is counted per folder a change is in, not per subtree: a build
  writing across many folders at once is taken out folder by folder, only
  where one folder alone changes more than the threshold. A root itself is
  never taken out.
- A folder that hangs leaves its helper thread blocked until the system
  answers it; a mount that never answers keeps one thread per walk that met
  it.
- The free space is read on the volume holding the cache, not on the
  volumes being indexed (which the index never writes to).
- Search Files' Recently Used is the most recently modified entries, as
  Raycast's is; Pane records no use of a file (ADR 0030). Created is not
  indexed: it is read when a file is selected, and Linux's file systems
  may not record it. An image is previewed whole, as the window decodes
  it, not as a thumbnail; other files show their icon, without Quick
  Look. Search Files is drawn by Pane for its own default extension only
  until #121's List detail and dropdown let any command draw it.
- The link from Search Files to the File Search page opens Settings at
  #176's page: `features::search_files::FILE_SEARCH_PAGE` is that page's
  title (`features::settings::file_search::TITLE`).
- A query inside a word ("port" in "report") is looked for only when the
  words' starts find fewer entries than the page asks for, and only for
  words of three letters or more; that pass reads every term of the index,
  so it is slower than a match by the start of words on a large index.
- macOS asks before Pane reads Desktop, Documents and Downloads (and
  removable and network volumes); the File Search page says so before the
  first walk and lists a refused folder with how to allow it, telling a
  refusal from another unreadable folder by what the system answered when
  the walker opened it (`EPERM`, "Operation not permitted", is the privacy
  protection; `EACCES` is the account's own permissions;
  `file_index::privacy`), whichever folder it is. Unverified on a real Mac.
- Linux's catch-up compares folders' modified times in whole seconds: a
  change within the second a folder was indexed is not seen until the
  folder changes again, and a file changed in place while Pane was stopped
  keeps its old size and time until it changes again while Pane runs.
- The roots and rules an index was built under are recorded as JSON; a
  root whose path is not valid Unicode cannot be recorded.
- The benchmark's numbers against Raycast's are not measured yet (#174).
- A handler slow to fail (over three seconds) is reported as having opened
  the file. Screen reader behaviour is unverified, as for all of root
  search.
