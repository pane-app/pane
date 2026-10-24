# Files

Files finds the files and folders under the user's home folder by name.
The user types part of a name into [root search](root-search.md), or into
the **Search Files** command (#150), and can then open, reveal, copy or
recycle what was found. Pane performs those file actions itself.

Behind it, Pane's host keeps an index of the names of the files and
folders under the home folder: the **file index**. At start Pane catches
the index up from what the file system recorded while Pane was not
running, and it keeps the index current while it runs, so a file is found
as quickly as a command.

Files is a **default extension**, and the user can disable it like any
package. While no enabled package uses the index, Pane neither indexes nor
watches anything.

Tracking: [#126](https://github.com/pane-app/pane/issues/126),
[ADR 0034](https://github.com/pane-app/pane/blob/2a4f9c43c990656325297a5980f34fa4bddba76e/docs/adr/0034-file-search-indexes-the-users-home-folder.md);
built by [#174](https://github.com/pane-app/pane/issues/174),
[#175](https://github.com/pane-app/pane/issues/175) and
[#176](https://github.com/pane-app/pane/issues/176).

## Using Files

- Type part of a name into root search. Up to five matching files and
  folders are listed under "Files", after the commands, applications and
  quicklinks found by title. A last row, "Search Files for “…”", opens
  Search Files with the same query ([In root search](#in-root-search)).
- Open Search Files for the whole list. It starts on "Recently Used",
  narrows as the user types, filters by type, loads more rows as the list
  scrolls, and shows the selected file's preview and Metadata
  ([Search Files](#search-files)).
- Act on what was found. For a document, Enter opens it and Ctrl+Enter
  shows it in the file manager; the Actions panel (Ctrl+K) lists every
  action. Enter never runs a program: it shows the program in the file
  manager, and only an explicit Run runs it
  ([The file actions](#the-file-actions)).
- Type a path. Open and Reveal in File Explorer act on it, and a path
  ending in `/` or `\` lists that folder's entries. Tab on a folder row
  completes the path; Shift+Tab removes the last component
  ([In root search](#in-root-search)).
- Choose what is indexed on the **File Search** page in Settings. The
  page also shows how the index is doing and what the index's
  [safety valves](#the-safety-valves) did (#176,
  [The File Search page](#the-file-search-page)).

First setup acquires the extension from the commit this Pane release pins
([#278](https://github.com/pane-app/pane/issues/278),
[#53](https://github.com/pane-app/pane/issues/53)). A user can also install
it by hand from its repository
(`pane --install git:https://github.com/pane-app/files`).

Until #175, file search found only the files of one folder the user granted
Files (#29, [ADR 0017](adr/0017-host-lists-a-granted-folder-for-an-extension.md)).
That capability stays, unchanged, for other packages
([The granted folder](#the-granted-folder)); Files no longer uses it.

## In root search

### A typed path

A query that is a typed path is one Files answers (#195). Files has two
commands declared `matches: "file-path"`:

- **Open** (id `open`) opens the path with the system's handler.
- **Reveal in File Explorer** (id `reveal`) shows it selected in the file
  manager.

They are listed under "Addresses", below the results found by title and
above the files. The first is selected when nothing else matches, so Enter
acts on the path. Each receives the resolved path (as typed, `~` resolved
to the home folder, `file://` taken off) as its launch record's fallback
text, and each closes the window after it acts.

Open never runs a program. A path whose name says it is one is shown in
the file manager instead, as file search's own Enter does
([opening](#opening)). The name is all the extension can see, since a pure
WASI guest reads no file system, and it is what the index knows a program
by.

A command declared `"when": "blank"` or `"searching"` appears only then
(see [root search](root-search.md#understanding-the-typed-query)). The
`sample-matches` package shows the declarations in Rust, JavaScript and
TypeScript.

### File results

Files answers root search through `root-results`, now from the index. Its
call returns at once from the host and never waits for a walk, so a busy
disk holds up no other result. A query of one character or more lists:

- At most **5 file rows** per command (`ROOT_FILE_ROWS`), the index's best
  by its own scoring:
  - an exact name ranks above an exact stem, then a name prefix, a word
    start, and then a match in the folders;
  - case and accents are ignored ("resume" finds `Résumé.pdf`);
  - the folders' words count ("invoices march" finds
    `Invoices 2026/march.pdf`);
  - newer entries rank a little higher;
  - folders are found as well as files.
- Then **"Search Files for “<query>”"**, which opens Search Files with the
  query typed in its own field and searched at once.

These rows come **after** the commands, applications and quicklinks found
by title, under the section "Files". A blank query lists no files.

Each row is the host's, whatever the extension's result says. Its title is
the entry's own name, its subtitle its folder (below the home folder as
`~/…`), its icon the system's icon for the path (#142, a document's or
folder's outline until it is loaded), and its kind **File**, or **Folder**
for a folder. A result naming an id the index did not give the package is
not listed. No use of a file row is recorded for learning (ADR 0030).

### A typed folder

A path ending in a separator lists the folder it names (#204). When the
query is a path that ends in `/` or `\`, Files asks Pane to list that
folder's entries (the host's `typed-folder` interface,
[below](#the-typed-folder)) and answers them as it answers the index's, by
the ids Pane gave them.

The entries are listed the same way the index's are: they are the host's
own rows, with the file actions, and Enter opens an entry, showing a
program rather than running it. They come after the rows declared for the
path and beside nothing else. A folder that cannot be listed, a missing
one among them, lists nothing, and the rows for the path stay.

Browsing happens in the query itself. Tab on a selected folder row
completes the query to that folder's path with a trailing separator, and
Shift+Tab removes the last path component
([root search](root-search.md#understanding-the-typed-query)).

## Search Files

Search Files works like Raycast's File Search
([#177](https://github.com/pane-app/pane/issues/177), spec
[#161](https://github.com/pane-app/pane/issues/161); it replaces #126's
slice 4 as #161 amends it). There is no folder to choose. It lists
"Recently Used" before typing, has a type dropdown, loads more rows as the
list scrolls, and shows a detail with an image's preview and the file's
Metadata.

Pane draws it itself for its registered Files default extension, the
default extension `files` and its command `files`, recognised by their
verified identity and never by a title. It appears in the split view
Clipboard History uses (#102), and Pane lists the index for it
(`launcher/search_files.rs`); the command's own list is not shown. A copy
of Files installed from a folder, and any other command that searches,
answers a designed List whose search field it owns
([designed tree](designed-tree.md)).

### Recently Used and typing

Enter on its row in root search opens Search Files with nothing explained
first, on **"Recently Used"**: the most recently modified entries the
index holds, files and folders, as Raycast's blank query lists them.

Typing ranks what the index finds by its own matching
([In root search](#in-root-search)). Opened from root search's "Search
Files for “plan”" row, the field holds "plan" and lists its results at
once.

Escape clears the field, and Recently Used comes back, listed in the
background. On an empty field Escape leaves. The back button leaves at
once.

### Filtering by type

The type dropdown sits at the search field's right ("Filter by Type"). It
offers All Types, Folder, Document, Image, Video, Audio, Archive, Text,
Application and Other (`search_files::FileType`: All Types, Folder, or one
of the index's categories, whose names it reads). The type stays while
the text changes. Ctrl+P (Command+P on macOS) opens it from the search
field, as Raycast's dropdowns open, and its tooltip names the chord.

While the index is stopped or off, the note over the list (or the empty
list) links to the File Search page in Settings: "Open File Search
Settings" is a tab stop after the type dropdown, and Enter or Space
follows it, as a click does.

Folder keeps folders. The others keep the index's categories, told from
the name's extension by one table on every system
(`file_index::Category`):

- Document is PDF, office and e-book files and web pages.
- Text is plain text, data, configuration and source files (`txt`, `md`,
  `csv`, `json`, `yaml`, `toml`, `log`, `rs`, `py`, `js` and the like).
- Application is programs, scripts, shortcuts, installers and application
  bundles.
- Other is a file of none of them (`data.bin`, `README`).

### Rows and pages

The list loads 50 rows at a time (`search_files::PAGE`). The next page
loads as the list scrolls within ten rows of its end
(`Launcher::load_more_files`), until the index has no more.

Each row shows the file's own name, its folder below the home folder
(`~/Documents`) and the system's icon for it (#142, a document's or
folder's outline until it is loaded), drawn as rows come into view (#165).

They are the launcher's own file rows, with
[the file actions](#the-file-actions). Enter, Ctrl+Enter and the Actions
panel (Ctrl+K) act as on any file row, and Enter on a program shows it and
never runs it. A click selects; a double click is Enter.

### The detail

The detail beside the list shows the selected file. An image gets a
preview if it is one the window draws: `png`, `jpg`, `jpeg`, `gif`,
`webp`, `bmp`, `tif`, `tiff`, `ico` and `svg`
(`icons::DRAWN_IMAGE_EXTENSIONS`), at most 32 MB, and never a link's
target. The Images category holds these and also images Pane cannot draw,
such as a camera's raw files. Any other file shows its icon, large.

Under it is the **Metadata**, read from the file system when the file is
selected (`Launcher::search_files_details`):

| Row | Shows |
| --- | --- |
| Name | The file's name |
| Where | `~/…` |
| Type | "PNG Image", "PDF Document", "MD Text", "Folder", "Application" |
| Size | Decimal units, as macOS's Finder and the Linux file managers write them: "532 bytes", "1.2 MB" (`file_index::size_words`, the one formatter for sizes, which the File Search page uses too). Not shown for a folder. |
| Created | Where the system records it |
| Modified | "Today at 14:02", "Sep 28 at 16:12" |

### When the index is not ready

The index's state shows over the list or in its place:

- "Indexing… (1204 found so far)" while the index is built, with a thin
  bar under the header while a search or the index is in progress.
- "File search stopped: <why>" or "File search is off: <why>", with
  **Open File Search Settings**, which opens Settings at the
  [File Search page](#the-file-search-page).

While the index is built, the list is asked again each second it grew,
keeping the selected file selected, until the user scrolled past the
first page.

### The footer and the window

The footer shows the command's icon and title (or the status), the
selected file's primary action and Actions. The window takes the split
view's 940×600 while Search Files shows, as for Clipboard History.

## The file actions

Each file, in Search Files and in root search's file results alike, has
actions Pane performs itself, without calling the extension, as an item of
a command's list has them. Enter runs the first, Ctrl+Enter the second and
Ctrl+Shift+Enter the third. The Actions panel (Ctrl+K) lists them all.

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
([ADR 0037](https://github.com/pane-app/pane/blob/2a4f9c43c990656325297a5980f34fa4bddba76e/docs/adr/0037-a-command-declares-its-mode-and-host-functions-decide-what-happens-after-it-runs.md)).
A file that would run a program when opened ([below](#opening)) is shown
in the file manager, and only its explicit **Run** runs it. Choosing Run
is the confirmation, so nothing more is asked.

The names differ by system. On macOS the file manager is Finder, so the
action is "Show in Finder", elsewhere "Show in File Manager". Outside
Windows the Recycle Bin is the Trash.

### What each action says

Each action closes the window after it acts and says what it did in a
HUD, as the standard actions do:

- Open, Show in Explorer, Open With… and Run: "Opened plan.md", "Showed
  run.bat in Explorer", "Opened plan.md with Notepad", "Ran run.bat".
- Copy Path, Copy Name and Copy File: "Copied to Clipboard".
- Move to Recycle Bin, once the user confirmed "Move “plan.md” to the
  Recycle Bin?" (never remembered): "Moved to Recycle Bin".

What fails stays on screen in the status line ("Could not open todo.txt:
it no longer exists").

Opening and running go through the launcher's link opener
(`LinkOpener::open_file`). The others go through its system
([`crate::system`](../crates/pane-core/src/system.rs): reveal, open with an
application, the clipboard, the Recycle Bin), so tests record them all.

## Opening

A row names its entry by the id the index gave the package that found it
(`i<generation>-<n>`; the newest 10,000 are kept per package). An id of an
earlier index (closed, rebuilt or deleted since) or of another package is
not known ("Pane no longer knows it; search again").

Before every action, off the window's thread, the host checks the entry
again (`Indexer::checked`):

1. The path is not a network path (Windows, before any file system call).
2. `symlink_metadata` says it is still there ("it no longer exists"), of
   the kind indexed ("it is now a folder", "it is now a file"), and not a
   link where a file or folder was indexed ("it is now a link"). A link
   itself is never opened: "it is a link, which Pane does not follow".
3. It is still in the index scope ("it is no longer in the folders file
   search covers"), told with the ignore rules the coordinator keeps
   between batches ([above](#catching-up-and-watching); read again when
   the global ignore file changed). Its canonical path is under a root's;
   a folder above it replaced by a link outside gives "it is no longer
   inside the folders file search covers".
4. Whether it is a **program**: any entry of the types below, the same on
   every system, or on macOS and Linux a file with an executable bit
   ([`files::runs_as_program`](../crates/pane-core/src/files.rs)). The
   types are the Windows types `exe bat cmd com lnk js jse vbs vbe wsf wsh
   hta msi msp scr pif ps1 cpl reg url`, the macOS types `app command tool
   terminal workflow` and anything inside an `.app` bundle, and `.desktop`
   files. A row already knows from the name whether it is one. The
   executable bit is told at this check, so a document that became a
   program since it was found is shown in the file manager rather than
   opened.

Only then is the checked canonical path acted on. Enter hands a document
or a folder to the system's handler, and shows a program in the file
manager. Run, Show in Explorer, Open With…, the copies and the Recycle Bin
act on a program as on any file.

### The system's handler

The window's opener, `pane::SystemLinks`, runs the handler the `open` crate
(5.4.4) names for the system, without a shell:

| System | Handler | A missing handler |
| --- | --- | --- |
| Linux | `xdg-open <path>`, else `gio open`, `gnome-open`, `kde-open` | none installed: "no program to open this kind of file is installed"; xdg-open finding none (status 3): "no program to open this kind of file is set up"; the program failing (status 4): "the program for this kind of file refused or failed to open it" |
| macOS | `/usr/bin/open -- <path>` (Launch Services) | its failure status |
| Windows | PowerShell with the path in an environment variable (not on its command line), which opens an existing path with `Invoke-Item -LiteralPath`; else `explorer.exe` | its failure status; an unassociated type may show the system's "Open with" dialog |

A handler still running after three seconds counts as having opened the
file. Tests replace the opener with a recording fake. A launcher given no
opener says "this Pane has no handler for files".

## The File Search page

The File Search page is Pane's own page in Settings
([`features/settings/file_search.rs`](../crates/pane/src/features/settings/file_search.rs)),
after Keyboard and before About. Settings' search finds the page and its
controls (Rebuild Index, Add Folder to Index…, Exclude Folder…, Exclude
Pattern and the switches).

The page has these parts:

- **Status** says whether the index is up to date, indexing (with how many
  entries it has found so far), waiting for the launcher to be shown,
  stopped or off. A stop gives its reason: another Pane is using file
  search, the disk is short of space, or it could not be opened. The
  status also gives how many files and folders the index holds, and when
  and how it was last caught up ("Last caught up from the change journal,
  5 minutes ago"). On macOS, before the first walk, it says that macOS
  will ask whether Pane may read Desktop, Documents and Downloads.
  **Rebuild Index** deletes the index and builds it again.
- **While file search is off**, the page says so and why: which extension
  that uses it is turned off, paused or has its commands turned off, or
  that none is installed. The rules can still be changed; they apply when
  it next runs.
- **Indexed folders** lists the home folder and each added folder with
  Remove, and has Add Folder… (the system's folder picker).
- **Excluded** lists each excluded folder and pattern with Remove, and has
  Exclude Folder… (the picker) and a field for a pattern in `.gitignore`
  syntax. Enter in the field excludes the pattern, as its Exclude button
  does, and Exclude Folder… and Exclude are tab stops that Enter or Space
  press.
- **Rules** has switches for hidden files and folders, ignore files, the
  default exclusions (caches, temporary folders, `node_modules`, `AppData`
  or `~/Library`) and network and removable drives.
- **Needs attention** (`Launcher::file_search_problems`,
  `file_index::Problem`) lists, each with why and what to do:
  - folders that could not be read;
  - folders macOS did not allow, with how to allow them under System
    Settings › Privacy & Security › Files and Folders;
  - folders not watched on Linux, with the setting that raises the limit
    (`fs.inotify.max_user_watches`);
  - folders taken out for churn, each with **Include Again**;
  - folders that did not answer;
  - a walk stopped at the ceiling;
  - a stop for free space.

Every control goes through `Launcher::set_file_search_rules` (or
`include_in_file_search`, `rebuild_file_index`), off the window's thread,
and a failure becomes the page's status. The page reads the launcher every
frame. Settings' watcher redraws it when the index's status, problems or
rules change while it shows.

### The safety valves

All four valves belong to the coordinator (`file_index::Indexer`, with
thresholds in `file_index::Valves` and `WalkOptions`; #126's proposed
values, smaller in tests). Each is listed on the page.

#### Churn quarantine

The changes each folder reports are counted per folder they are in, in
windows of a minute (`Valves::churn_window`). Only entries the index scope
admits are counted (#184). A change in `.git`, `node_modules`, a cache or
temporary folder, a folder an ignore file ignores, or a hidden or excluded
folder never counts, so such a folder is never taken out, recorded or
listed. Whether a change counts is told with the ignore rules the scope
keeps between batches (`Scope::admits_kept`, #186), which the batch is
then looked at with.

A folder with more than 1,000 changes (`churn_changes`) in 3 windows in a
row (`churn_windows`) is taken out of the index:

- its entries go;
- the rules leave it out from then on: it is added to
  `UserRules::quarantined` and recorded in `file-search.json`, so it stays
  out after a restart;
- its changes are no longer looked at.

A root itself is never taken out. Include Again puts a folder back and
walks it alone. Counting costs nothing while nothing changes.

#### The ceiling

A walk stops at 5 million entries (`WalkOptions::max_entries`,
`file_index::MAX_ENTRIES`) and says so. The index keeps what was walked.

#### The free-space floor

Before the index is written (a walk, a batch of changes, a reconciling
walk), and once a second during a first walk, Pane reads the free space of
the volume holding its cache (`GetDiskFreeSpaceExW` on Windows, `statvfs`
elsewhere, `file_index::free_space`).

Under 1 GB (`Valves::free_space_floor`, `FREE_SPACE_FLOOR`) indexing stops
writing. #126 proposes 1 GiB; Pane writes sizes in decimal units, so the
floor is 1,000,000,000 bytes and the page says "1 GB". The status is
Stopped and says why, a first walk under way is given up, and changes
reported meanwhile are let go.

Pane looks again every minute (`space_retry`). Once there is room it
starts again by itself, walking what was not walked and reconciling every
root if changes were let go. The cursors saved before are kept until
then.

#### Hung folders

Each walker thread, and each reconciling walk, lists folders through a
helper thread of its own and waits at most 10 seconds
(`WalkOptions::hung_after`, `HUNG_AFTER`) for a folder. One that does not
answer is skipped for this walk (indexed, its contents not) and listed. A
reconciling walk or a re-check keeps what the index held of it, however
many folders do not answer. Its helper is left behind to end whenever the
system answers it, so a stalled network mount or a dying disk holds up
only itself.

#### Sleep

Indexing pauses while the computer sleeps and resumes 5 seconds after it
wakes (#126 "Every system", story 43; `Valves::resume_after`,
`file_index::power`).

Pane learns of a sleep from two clocks every system keeps, one that runs
on through a sleep and one that stops: Windows' interrupt time and
unbiased interrupt time, Linux's `CLOCK_BOOTTIME` and `CLOCK_MONOTONIC`,
macOS's `CLOCK_MONOTONIC` and `CLOCK_UPTIME_RAW`. Their difference grows by
exactly each sleep. The wall clock, which the user or time synchronization
may move, is not read.

The coordinator looks before each batch of changes and each pass of its
loop, and each walker thread looks before each folder. Whoever first sees
the difference grow starts the pause, and everyone waits it out, the
status saying "Paused: the computer slept".

The system's own notifications (`WM_POWERBROADCAST`, IOKit's
`IORegisterForSystemPower`, logind's `PrepareForSleep`) are not used. Each
needs a window, a run loop or a D-Bus connection of its own, and while the
computer sleeps every thread of Pane is frozen anyway (the write-ahead log
keeps what was half applied). What matters is what follows the wake,
which the first look after it catches.

Time limits count awake time only. The first walk's delay takes the sleep
out, a folder whose wait the sleep interrupted is given its limit again
before it counts as hung, and the churn windows start again after a
sleep.

## The file index

### When it runs

A package declares in `pane.json` that it uses the index:
`"fileIndex": true`. The index is opened, caught up and watched only while
at least one such package is enabled, not paused, and has a command the
user left on in Settings (`Launcher::sync_file_index`, after every change
of the packages). Turning off every one of Files' commands (Search Files
and, since #195, its Open and Reveal in File Explorer) stops the index as
disabling Files does.

When the last such package is disabled, paused or has its commands turned
off, watching stops at once and the index stays on disk. Turning one on
again catches it up from where it stopped. Uninstalling the last one
deletes the index. This keeps lazy activation
([ADR 0005](adr/0005-lazy-activation-and-managed-dependencies.md)) without
running guest code to learn it.

At start Pane catches the index up, which is cheap. A first full walk, or
a reconciling walk the catch-up asks for, waits until the launcher is
first shown (`WindowPresence::Shown`) or 60 seconds after start
(`file_index::FIRST_WALK_DELAY`), whichever comes first. Meanwhile, and
while any walk runs, searches answer from what is indexed so far.

One thread of its own per activation ("pane-file-index") does all the
writing, at background priority: background mode and EcoQoS on Windows,
the background QoS class on macOS, nice 19 and the idle I/O class on
Linux. The walker's threads run at the same priority. So does the index's
own merge thread ("pane-file-index-merge", one per open index), which
merges segments in the background (#187,
[below](#the-engine-and-the-walker)). A query never runs at background
priority and never waits for the coordinator or for a merge, and the
coordinator never waits for a merge either.

### Where it is kept

The index is kept in Pane's cache folder, since it can always be rebuilt
(`file_index::INDEX_DIR`):

- Windows: `%LOCALAPPDATA%\Pane\cache\file-index`
- macOS: `~/Library/Caches/Pane/file-index`
- Linux: `$XDG_CACHE_HOME/pane/file-index`

The compiled extension code is kept beside it in a folder of its own
(`code`), since Wasmtime deletes every file it does not recognize in the
folder its cache is given. Up to Pane 0.1.1 that folder was the cache
folder itself, and installing or updating an extension deleted the index
(see [the applications' icon cache](applications.md#the-cache)).

The folder is readable by the user only: mode 0700 (files 0600) on macOS
and Linux, and on Windows a protected DACL for the user and SYSTEM,
inherited by its files, as `credentials.json` is written. It is never sent
anywhere.

It carries a format version (`file_index::FORMAT_VERSION`, 2 since #185
added the fragment index and key filters). An index of another version, or
one that cannot be read, is deleted and rebuilt, never read.

The folder is locked. A second Pane on the same cache folder does not
index, and its status says "Another Pane is using file search on this
computer".

The index records with itself whether a first walk finished, each volume's
cursor into the system's change records, and the roots and rules it was
built under. A change of the rules, even while file search is off, is
therefore applied the next time it opens.

### The index scope

The **roots** are the home folder (`%USERPROFILE%` on Windows, `$HOME` on
macOS and Linux) and the folders the user adds. The walker, the
reconciling walk and every live change apply the same **rules**
([the walker's rules](#the-engine-and-the-walker)):

- Always left out:
  - Pane's own data and cache folders and the index itself;
  - the system's recycle and setup folders (`$RECYCLE.BIN`,
    `System Volume Information` and the like);
  - folders tagged with `CACHEDIR.TAG`.
- Left out by default, each a switch:
  - hidden entries (a leading `.`; the hidden or system attribute on
    Windows);
  - what `.gitignore` (inside a Git repository), `.ignore`,
    `.git/info/exclude` and the global Git ignore file exclude, read as Git
    reads them without running `git`;
  - `node_modules`, folders named `tmp`, `temp`, `cache` or `caches`,
    `*.tmp` and `*.temp`;
  - the home folder's `AppData` (Windows) or `Library` (macOS);
  - network shares and removable drives, a root on one as well as one
    mounted under a root
    ([below](#network-shares-and-removable-drives)).
- The user's own: added roots, excluded folders and excluded patterns (in
  `.gitignore` syntax).

#### The user's rules

The user's rules are Pane's own record, not extension data. They live in
`file-search.json` beside `installed.json` (`file_index::UserRules`):

```json
{"version": 1, "rules": {"addedRoots": [], "excludedFolders": [],
"excludedPatterns": [], "includeHidden": false, "useIgnoreFiles": true,
"defaultExclusions": true, "includeOtherVolumes": false, "quarantined": []}}
```

`quarantined` holds the folders taken out for churn.

`Launcher::set_file_search_rules` records and applies the rules without a
restart, changing only what they affect where it can:

- a removed root's entries go, and an added root alone is walked;
- a newly excluded folder's entries go, and a folder no longer excluded is
  walked alone;
- any other change (a pattern, a switch) walks every root again.

The folders taken out for churn stay out whatever the page sends; only
`Launcher::include_in_file_search` puts one back. The File Search page is
what changes the rules ([above](#the-file-search-page)).

#### A folder granted to Files under #29

A folder granted to Files under #29 is kept in what is indexed (#126
story 60). This happens when the launcher starts with a file index
(`Launcher::with_file_index`), and when a package comes to use the index
while Pane runs (an update of Files).

The folder granted to a package that declares `"fileIndex": true` is added
to the roots if the index would not cover it otherwise: it is outside the
home folder, or the rules exclude it or a folder above it. It is recorded,
and then the grant is forgotten (`folders.json` no longer holds it). A
granted folder the index already covers is simply forgotten.

#### What is indexed

Entries are files and folders, named as the system names them. A name
that is not valid Unicode is shown with replacement characters and opened
by its exact name.

- Links and junctions are indexed as entries and never followed.
- Online-only files (OneDrive, iCloud Drive, File Provider) are indexed
  from their folder's listing alone, so nothing is downloaded.
- A folder that cannot be read is indexed and counted, its contents not.
- A root the user added that is away (an unplugged drive) keeps its
  entries, hidden from searches until it is back. Pane looks no more than
  every two seconds.

### Network shares and removable drives

Two cases are left out unless the user turns on **Include network and
removable drives** (`includeOtherVolumes`, #184):

- a root on a network share or a removable drive (a folder the user added
  there, or a home folder redirected to a share);
- a network share or a removable drive mounted in a folder under a root.

Nothing of them is walked, watched or looked at, and what an earlier index
held of such a root goes.

With the switch on they are indexed like any folder, except that a network
share is never watched (#126 leaves live watching of shares out). A root
on one is caught up by a reconciling walk at start and every 5 minutes
(`file_index::RECONCILE_UNWATCHED`, as Linux's unwatched folders), so a
change made there is found by the next one.

#### How Pane tells what a folder is on

What a folder is on is the system's answer (`file_index::volume_kind`,
`VolumeKind`). Pane asks once for each root when the index opens, and for
a folder the walker meets on another volume than the folder it is in. The
coordinator asks through `IndexerConfig::volumes`, which tests replace.

The question runs on a helper thread of its own, given 2 seconds to answer
(`VOLUME_ANSWER`), so a stalled network mount holds up only that thread. A
root that does not answer in time is taken for a network share (left out
unless the switch is on, never watched) and the status says so. A mounted
folder that does not answer is taken for one too.

The answers:

| | A network share | A removable drive |
| --- | --- | --- |
| Windows | a network path (`\\server\share`, told from its text, before any call); any other path is resolved to the root of the volume holding it (`GetVolumePathNameW`: a drive letter, a mapped drive's letter, the folder a volume is mounted in) and `GetDriveTypeW` says that root is remote | `GetDriveTypeW` says removable or CD-ROM |
| macOS | `statfs`'s flags (`f_flags`) without `MNT_LOCAL` | `MNT_REMOVABLE` (removable media) |
| Linux | `statfs`'s type (`f_type`): NFS, SMB, CIFS, FUSE (sshfs, rclone and the like), AFS, Lustre | FAT or exFAT |

On Windows another volume under a root is reached only through a mount
point or a junction, which is a link and never followed, so only roots are
asked there.

A folder the system says nothing about (a call that failed) is of an
unknown kind (`VolumeKind::Unknown`). It is left out as a share or a
removable drive is unless the switch is on, as Linux's walker left out a
mount it could not ask about before #184.

A root that is away (an unplugged drive) is not asked. It keeps its
entries, hidden from searches, and is asked once it is back. So a
removable drive plugged in after Pane started is left out from then on
(its entries hidden, and gone the next time the index opens) unless the
switch is on.

### Catching up and watching

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Catch-up at start | The NTFS change journal of each root's volume, read without administrator rights from the saved cursor ([below](#the-ntfs-change-journal-without-administrator-rights)), resolved through the folder ids the index holds | FSEvents' history, replayed by one stream over the roots from the saved event id, per volume (by its FSEvents UUID) | A reconciling walk |
| Live changes | `ReadDirectoryChangesW` on each root (the `notify` crate) | The same FSEvents stream | inotify, one watch per indexed folder, shallowest first |
| When the records cannot be used | A recreated journal, discarded records, more than a million records, a volume without a journal (FAT, exFAT, a network share) or a refused read: the volume's roots are reconciled | A new volume UUID: its roots are reconciled; a folder FSEvents asks to rescan (history purged or coalesced, events dropped) is reconciled alone; wrapped event ids reconcile every root | The folders past the watch limit (`fs.inotify.max_user_watches`) are reconciled every 5 minutes, and counted in the status |

Only the roots the rules keep are caught up and watched. A root on a
network share the user included is not watched on any system: it is
reconciled at start and every 5 minutes instead
([above](#network-shares-and-removable-drives)).

#### The folder-id table

The **folder-id table** holds each indexed folder's file id and path
(`FileIndex::folder_ids`). Building it takes reading every entry of every
segment, so it is read at most once each time the index opens (#187,
`file_index::FolderIds`):

- by the NTFS catch-up, the first time a volume's records need resolving;
  it is then brought up to the changes the catch-up applied and handed to
  the watch setup;
- by the watch setup, only where the source watches each folder (Linux).

Windows and macOS watch each root whole and never read it for watching,
nor after a first walk. The table is let go once watching has started. It
is not kept on disk, since that would change the index's format; that is
left for when the benchmark shows the catch-up still needs it.

#### Reconciling walks

A **reconciling walk** (`file_index::reconcile`) compares the index's own
folders with the disk. It reads again only a folder whose modified time
changed; a folder's time changes when an entry is added, removed or
renamed in it. A folder new to the index is walked whole, and a folder
gone takes everything under it out.

A folder it reads again that holds a `.gitignore`, an `.ignore` or a
`.git` is then re-checked whole ([below](#the-ignore-rules-kept-between-batches)),
since the rules below it may have changed with it. A watcher's overflow
reconciles the root it concerns, then re-checks it whole, because an
ignore file changed in place while the buffer overflowed changes no
folder's time.

#### Live changes and the status

Live changes are gathered for about 100 ms (`file_index::SETTLE`) and
applied together. Each path reported is looked at again: indexed if it
exists and the rules admit it, removed otherwise, and a new folder walked
whole. A change is therefore visible to queries well within a second of
the system reporting it.

While nothing changes, nothing runs, except Linux's reconciliation of the
folders it cannot watch and that of network shares the user included.

The status (`Indexer::status`, `Launcher::file_index_status`, and
`pane:extension/file-index`'s `status`) says:

- whether the index is off, building (and how many entries the walk found
  so far), current, or stopped and why;
- how it last caught up (`CaughtUpBy`: the journal, the event history, a
  reconciling walk or a full walk) and when;
- how many folders could not be read or are not watched.

#### The ignore rules kept between batches

To tell whether the rules admit a path, each folder above it is judged and
its ignore files are read (`.ignore`, `.gitignore`, `.git/info/exclude`,
whether it holds `.git` or `CACHEDIR.TAG`). Since #186 the scope keeps
what it learned of each folder (`Scope::admits_kept`), so the next batch
reads only what changed. A file changed deep in a repository costs its own
folder's files, not every folder's above it.

The live batches, the catch-up at start (`catch_up_changes`) and the check
before a file is acted on ([Opening](#opening)) use the same record.
Correctness comes first, so the record drops more than it strictly has
to.

A folder that a change names (made, deleted, renamed, its attributes
changed; Windows also names a folder whose entries changed) is dropped
with every folder under it.

A `.gitignore`, `.ignore`, `CACHEDIR.TAG` or `.git/info/exclude` that
changes, or a `.git` that appears or goes, drops its folder and every
folder under it, and the folder is **re-checked**:

- Every folder under it is read again whatever its time, under the rules
  as they are now (no ignore file read before is used), and compared with
  what the index holds there (`reconcile::recheck`, the reconciling walk's
  comparison).
- What the rules leave out now goes (a whole folder at once if the folder
  itself is left out), and what they admit now comes in. A folder that
  does not answer keeps what the index holds of it.
- A re-check reads 64 folders (`RECHECK_FOLDERS`) and lets the changes
  reported meanwhile be applied before it goes on, so re-checking a whole
  root never holds up a live change. It runs with the other walks, once
  the launcher was shown or a minute passed.

Windows also reports what Git does inside a repository as a change of its
`.git`, so a `.git` counts only when it appears or goes since a change
last named it. The first time one is named, a `.git` made before Pane
started counts as there already (the index was built or caught up with
it); one made since, or whose time the system does not say, is
re-checked.

On Linux, where inotify watches each folder, a repository's `.git/info` is
watched with its folder (and with a repository made while Pane runs), so a
change of its `exclude` is reported too. Where that watch cannot be added,
the folder counts as not watched (below).

At start, a catch-up that names one of these files re-checks its folder
with the walks it waits for. So does one whose records say less:

- Windows' records read without administrator rights carry no names. A
  folder from which an entry the index did not hold went (a hidden
  `.gitignore` or `.git` deleted; its id is not among the folder's indexed
  entries and names nothing now) is re-checked. So is a repository whose
  `.git/info` a record was in, the folder looked up by its id, at most
  4,096 such folders per catch-up.
- A folder Linux's reconciling catch-up reads again that holds an ignore
  file or a repository is re-checked.

A folder the records make visible (its hidden attribute taken off) is
walked whole.

The global Git ignore file, and the files Git reads to find it
(`core.excludesFile` in `~/.gitconfig`, `git/config` under
`XDG_CONFIG_HOME` or `~/.config`, `GIT_CONFIG_GLOBAL`, the system's
`gitconfig`), are looked at (their size and modified time) before each
batch. When one changed, the rules are read again with nothing kept, and
every root is re-checked if the global ignore file itself changed.

A change of the user's rules (`file-search.json`, through the File Search
page or a folder taken out for churn) also builds the rules again with
nothing kept. So does a watcher's overflow, which drops everything kept
and re-checks the root it concerns. So does the start of watching, which
drops what the catch-up learned, since a change made between the two is
reported by neither.

Nothing is kept of a folder no change is reported from
(`Scope::keep_nothing_under`): a root on a network share the user
included, a folder past Linux's watch limit, every root when watching
could not start, a root away when watching started. Its ignore files are
read each time a path under it is judged, the check at Enter included,
and a folder its reconciling walk reads again holding an ignore file is
re-checked.

The record holds at most 20,000 folders; past that it is dropped whole.

## Where it lives

The index belongs to the host, in `pane-core`, and one index serves every
package that uses it. A pure WASI 0.3 guest cannot read the user's folders
(its WASI context preopens none) or open a file, and the walk and the
watching must never run on the extension runtime's thread.

- **The index** is
  [`pane_core::file_index`](../crates/pane-core/src/file_index.rs):
  - the walker, the engine, the scope and the NTFS change journal (#174,
    [below](#the-engine-and-the-walker));
  - what kind of volume holds a folder (`file_index/volume.rs`, #184,
    [above](#network-shares-and-removable-drives));
  - the reconciling walk (`file_index/reconcile.rs`);
  - one change source per system behind the `ChangeSource` trait
    (`file_index/changes.rs` and `changes/{ntfs,macos,linux}.rs`);
  - the coordinator, which opens, catches up, walks and watches the index,
    gives each entry an id and checks it again (`file_index/indexer.rs`,
    `Indexer`);
  - the host side of `pane:extension/file-index` (`file_index/host.rs`).
- **The launcher's side** is
  [`launcher/file_search.rs`](../crates/pane-core/src/launcher/file_search.rs):
  which packages use the index, root search's Files section and its
  "Search Files for “…”" row, the system icons, deleting the index with
  the last package that used it, and moving a folder granted under #29
  into the roots. It also has the calls the File Search page (#176) and
  Search Files (#177) build on: `Launcher::file_indexer`,
  `file_index_status`, `file_search_problems`, `file_search_packages`,
  `file_search_rules`, `set_file_search_rules`, `include_in_file_search`,
  `rebuild_file_index`. The rows and their actions are
  [`launcher/files.rs`](../crates/pane-core/src/launcher/files.rs) and
  [`launcher/own_actions.rs`](../crates/pane-core/src/launcher/own_actions.rs).
- **Search Files** is
  [`launcher/search_files.rs`](../crates/pane-core/src/launcher/search_files.rs)
  (`pane_core::search_files`): Pane's registered Files command listed by
  Pane from the index, its types, pages, detail and the index's note
  ([Search Files](#search-files)). The window's
  [`features/search_files.rs`](../crates/pane/src/features/search_files.rs)
  draws it in the split view
  ([`ui/split_view.rs`](../crates/pane/src/ui/split_view.rs)).
- **The host interface** is `pane:extension/file-index`
  ([`wit/file-index.wit`](../wit/file-index.wit)), with `search` and
  `status`, for any package that declares `"fileIndex": true`
  ([For authors](#for-authors)).
- **The default extension** is the
  [Files repository](https://github.com/pane-app/files), written in Rust. A
  Pane release pins its release commits
  ([`crates/pane/defaults.json`](../crates/pane/defaults.json)). Its
  package declares `"fileIndex": true`:
  - its command Search Files (id `files`, `"rootResults": true`) answers
    root search from the index with the entries' ids;
  - its two commands declared `"matches": "file-path"` (#195), Open and
    Reveal in File Explorer, act on a path typed into root search
    ([In root search](#in-root-search)).

  Installed as Pane's default extension, its screen is Pane's own Search
  Files view. A copy installed from a folder lists what is searched, with
  no field of its own (extension API 0.2 retired a command's own `search`
  export, #240). It is not a
  [root provider](root-search.md#root-providers): it has a row and a
  screen of its own. The [files samples](../guests/packages) hold the same
  contract in this repository's tree.
- **The window** is [`crates/pane/src/main.rs`](../crates/pane/src/main.rs).
  It calls `Launcher::with_file_index(IndexerConfig::native(cache, home, own))`
  with Pane's cache folder, the home folder (`USERPROFILE` on Windows,
  `HOME` elsewhere) and Pane's data folder, which is never indexed. The
  File Search page is
  [`features/settings/file_search.rs`](../crates/pane/src/features/settings/file_search.rs).

## For authors

A package that searches the index sets `"fileIndex": true` in `pane.json`.
Its commands call `search(query, options)` and `status()` of
`pane:extension/file-index`. They answer `open-file` results with the
entries' ids, or command search results whose `file` is the id
(`SearchResult { file: Some(id), .. }` in Rust, `{ id, title, file }` in
JavaScript and TypeScript)
([author guide](../guests/README.md#panes-file-index)).

`search` answers at once from what is indexed, never waiting for a walk.
A package that does not declare `"fileIndex": true` is refused. It
returns:

- the entries matching the query (a blank query: the most recently
  modified);
- filtered by kind (file, folder, link) and category (documents, images,
  audio, video, archives, applications, text, and other for a file of none
  of these, told from the name's extension by one table on every system,
  the one Search Files' dropdown uses);
- sorted by relevance or modified time;
- paged by `limit` (at most 200 per call) and `offset`.

Each entry carries its id, absolute path (text, for showing and copying),
name, folder for people, kind, whether opening it would run a program,
size, modified time and volume.

`status` answers the state (off, building, current, stopped), the entries
indexed, the entries the walk in progress found and why.

In Rust, use `pane_extension::file_index::{search, status}` and
`RootAction::OpenFile(entry.id)`. In JavaScript and TypeScript, use
`search` and `status` from `"pane:extension/file-index@0.2.0"`
([`guests/js/file-index.d.ts`](../guests/js/file-index.d.ts); WIT's `u64`
numbers are `bigint`) and `{ tag: "open-file", val: entry.id }`. The
package's `package.json` sets `"pane": { "fileIndex": true }` so that only such a
component imports the interface.

The samples [`guests/sample-files`](../guests/sample-files),
[`guests/sample-files-js`](../guests/sample-files-js) and
[`guests/sample-files-ts`](../guests/sample-files-ts) do what Files does,
in root search and in their own field, and give the same answers.

A command that chooses to open or run an entry explicitly may use the
host's open functions of ADR 0037 with the path; that is its own explicit
action.

## The typed folder

A folder the user **typed** into root search, ending in a separator, is
listed for the Files extension (and any other command that answers root
results for a path) through `pane:extension/typed-folder`
([`wit/typed-folder.wit`](../wit/typed-folder.wit)). This follows what
[ADR 0034](adr/0034-file-search-indexes-the-users-home-folder.md) decided
about file access: no folder is granted, since the user named it, and the
host, not the extension, reads the file system. It is separate from the
granted folder's `list-folder` ([below](#the-granted-folder)), which stays
as it is for the packages that ask for a folder the user chooses.

### The listing

The command passes what the user typed, and Pane resolves it as root
search resolves a typed path (`~` to the home folder of the file index,
`file://` taken off). Pane then lists the folder's **direct entries only,
folders first and each in name order**, at most **500** of them, with
`truncated` set when the folder holds more.

Hidden entries (a name starting with `.`, and on Windows the hidden and
system attributes), links and names that are not Unicode are neither
listed nor counted, as the granted folder's walk skips them.

An error explains a folder that cannot be listed: not a path, a network
location, a file, or one Pane cannot read. The command then lists nothing
for it.

A partial listing says so in the answer. Root search adds a row at the end
of the entries ("…and more entries", the bound named), which cannot be
activated; it only informs.

### The entries

The entries are named by the ids Pane gives them in the package's latest
typed listing. Each new listing replaces the one before it, so an id of an
earlier one is not found.

An `open-file` result names one, and Pane checks it again before acting on
it, exactly as it checks the index's entries and a granted folder's files:

- it is still in the latest listing;
- it is still the kind it was listed as;
- it is not a link;
- it is still inside the folder the user typed.

### The thread it runs on

The listing runs on a thread of its own, since a folder may block. The
runtime thread awaits it, serving other packages' calls meanwhile, and the
wait is Pane's time, never the extension's computing, as the system host
functions are. A listing whose call was cancelled (the query changed)
still finishes, but cannot replace a newer one's listing.

### Using it

Rust commands use it through pane-extension
(`pane_extension::typed_folder`). JavaScript and TypeScript ones use the
module "pane:extension/typed-folder@0.2.0"
([`guests/js/typed-folder.d.ts`](../guests/js/typed-folder.d.ts)), which
only a command whose package.json sets `"pane": { "typedFolder": true }`
imports.

## The granted folder

The capability file search used before #175 stays, unchanged, for a
package that wants an exhaustive listing of one folder the user chooses,
including a folder the index leaves out
([ADR 0017](adr/0017-host-lists-a-granted-folder-for-an-extension.md),
superseded for file search by ADR 0034). Files no longer declares it. The
test fixture [`guests/fixtures/folder-files`](../guests/fixtures/folder-files),
what Files was before, keeps it covered. Its own `open-file` results keep
ADR 0017's refusal to open programs and scripts, since such a package
never offered a Run action.

It has three parts:

- **The grant**, in the core. A package declaring `"folderAccess": true`
  in `pane.json` gets Pane's own "Choose folder…" row. Pane checks the
  folder and records it in its own `folders.json`
  ([`pane_core::files`](../crates/pane-core/src/files.rs),
  [`launcher/files.rs`](../crates/pane-core/src/launcher/files.rs)).
- **The listing**, in the core: `pane:extension/files`
  ([`wit/files.wit`](../wit/files.wit)), `list-folder()` without a path,
  answered at once from the listing Pane makes on the package's own
  worker.
- **The file actions** are the ones above, checked again against the grant
  (`FileAccess::checked_file`): the id is in the package's latest listing,
  that listing's folder is still its grant, it is not a network path, it
  is a regular file and not a link, its canonical path is inside the
  grant's, and for Open it is not a program ("it is a program or script,
  which opening would run").

### Granting the folder

Every command of a package with `"folderAccess": true` starts with Pane's
own rows, above the extension's items:

- **Choose folder…**, subtitled with the folder granted now ("Pane lets
  Files list only /…/Documents") or that none is. Enter opens the system's
  folder picker (`Launcher::folder_to_choose`, then the window calls
  `Launcher::grant_folder`); cancelling changes nothing.
- **Stop sharing the folder with <title>**, once one is granted. The grant
  is taken back and recorded; the folder itself is not changed.

Pane checks the chosen folder before granting it
([`files::check_grant`](../crates/pane-core/src/files.rs)). When it
refuses, it says why ("Pane did not grant the folder: …") and keeps the
earlier grant:

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
saves or sees the path. It survives restarts and disabling. Uninstalling
the package forgets it, whether or not its saved data is kept.

### The scan policy

The scan policy is the same on every system and enforced by the host
([`files::walk`](../crates/pane-core/src/files.rs)). The limits are defined
once, as `pane_core::files::MAX_DEPTH`, `MAX_FILES` and `MAX_ENTRIES`, and
extensions read them with `files.limits()`.

- **Regular files only**, from the granted folder and its subfolders,
  **breadth first** (a folder's own files before its subfolders'), each
  folder's entries **in name order** (by the bytes of their names).
- **At most 8 folders deep**, **5,000 files** and **20,000 entries** looked
  at (files, folders, links, anything). Entries are counted, and
  cancellation checked, *while* each folder is read, so a huge folder stops
  at the entry limit rather than being read whole first. Only the paths of
  the folders still to list are queued, not open directory handles.
- **Skipped, neither listed nor entered**, at every level: hidden entries
  (a name starting with `.`, and on Windows the hidden and system
  attributes), symbolic links and other links (Windows junctions included)
  and names that are not Unicode.
- A subfolder (or an entry) that cannot be read is skipped and makes the
  listing **partial**: `truncated` is set, as when a limit is reached.

#### When it is listed

`list-folder()` never waits. The first call in a visit of root search
starts a listing on the package's **own worker thread** (one per package,
started with its first listing) and answers `listing`; the extension
answers no files yet. The worker waits 100 ms (`files::DEBOUNCE`) before
it starts, and takes only the newest request.

The listing is then **kept for the visit**: every later keystroke gets it
at once, and only filters it. It is dropped when root search is left (a
command, Settings › Extensions, a preview, a restart of the visit) and
when the grant changes, so the next visit lists the folder again. This
listing keeps no index and watches nothing.

Once a listing ends, the commands whose answer waited for it are asked
again for the query then on screen, **after** every other result of that
query was shown: computed results (the calculator, quicklinks), and the
[indexed results](root-search.md#results-supplied-ahead-of-the-query) (the
applications). A slow or huge folder therefore holds up no other
extension's results; only its own files arrive later.

#### Cancelling a pending search

A listing stops, and its answer is dropped, when root search is left,
when the grant changes, and when the package's generation ends (disable,
reload, update, pause, uninstall).

A new query does not restart it: the new search waits for the same
listing, and the older search's wait is cancelled. A search whose
extension was told the folder is listing waits for its own visit's
listing (or a newer one), even if it ended before the search began
waiting; the package is then asked again at once. A listing of a visit
already left that stops late does not end that wait.

#### A hung folder

A hung folder (an unresponsive disk or a network mount the host could not
tell apart) holds only its package's worker. No new thread is started for
later requests, which wait (only the newest is kept). Other extensions are
unaffected, and the package lists nothing until the listing returns. With
UNC paths refused this should be rare; mapped network drives on Windows
and network mounts on macOS and Linux are not detected.

## The engine and the walker

The walker, the rules and the engine are the measured first slice of #126
([#174](https://github.com/pane-app/pane/issues/174)), which the
coordinator above builds on.

### The walker

The walker (`file_index/walker.rs`) lists every folder under the roots the
rules admit once, using up to 8 threads of its own at background priority
(`file_index/priority.rs`: background mode and EcoQoS on Windows, the
background QoS class on macOS, nice 19 and the idle I/O class on Linux).

On Windows a folder is read with
`GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`, which gives each
entry's attributes, size, times and NTFS file id in one call per 64 KB.
Elsewhere it uses `read_dir` and each entry's own metadata.

Links and junctions (reparse points whose tag names another file) are
indexed as entries and never followed. Cloud-file placeholders are listed
like any entry, so nothing is downloaded. A folder that cannot be read is
indexed, counted and named. A walk stops at 5 million entries.

### The rules

The rules are in `file_index/scope.rs` (`ScopeRules::for_home`):

- hidden entries (a leading `.`; the hidden or system attribute on
  Windows);
- `.gitignore` inside a Git repository, `.ignore` anywhere, the
  repository's `.git/info/exclude` and the global Git ignore file, matched
  by ripgrep's `ignore` crate as Git matches them;
- `node_modules`, folders named `tmp`, `temp`, `cache` or `caches`,
  `*.tmp` and `*.temp`;
- the home folder's `AppData` (Windows) or `Library` (macOS);
- network shares and removable drives, a root on one or one mounted under
  a root (`file_index/volume.rs`,
  [above](#network-shares-and-removable-drives));
- and always the system's recycle and setup folders, folders tagged with
  `CACHEDIR.TAG` and Pane's own folders.

Each but the last is a switch, and the user's folders and
`.gitignore`-style patterns add to them. `Scope::admits` applies the same
rules to one path, reading the ignore files above it, for changes.
`Scope::admits_kept` does so with what the scope keeps of those folders
between batches (#186, [above](#catching-up-and-watching)).

### The storage engine

The engine (`file_index/store.rs`) has the shape of `minidex`:

- a memory table of recent changes, logged first to a write-ahead log
  (`file_index/wal.rs`, records with a CRC, a torn tail dropped);
- immutable segments (`file_index/segment.rs`) holding the entries sorted
  by path, front-coded in blocks of 16, each with a fixed 4-byte hint (day
  modified, depth, kind) and the postings of its terms, whose dictionary
  is an `fst` map read through a memory map;
- prefix tombstones hiding a deleted or renamed folder's entries in older
  segments;
- **tiered merges in the background** (#187).

Each entry keeps its kind, size, modified time, file id and volume. Keys
are the path's exact bytes (WTF-8 on Windows), so a name that is not valid
Unicode is shown with replacement characters and still opened exactly.

#### Merges

A segment is written when the memory table reaches 65,536 entries, at a
save of the cursors, and when Pane stops. Each time, the index's merge
thread looks for 4 neighbouring segments of similar size (the largest at
most 4 times the smallest, a segment under 4,096 entries counted as
4,096). It takes the smallest such run and merges it into one segment that
takes its place. With more than 8 segments it merges the smallest 4
neighbours whatever their sizes, so a query reads about 8 at most.

Only neighbours are merged, so each segment's entries stay newer than
every older one's. A merge keeps the newest version of each path and drops
what tombstones hide. It drops deletions and the tombstones its segments
carry only when it starts at the oldest segment, since only then is
nothing older left for them to hide; a merge above it carries them on.

Tombstones could pile up above a first index's large segment, which the
tiers seldom reach. To prevent that, every segment is merged into one once
the segments above the oldest carry more than 4,096 tombstones
(`TOMBSTONES_DUE`) and more than a 32nd of the oldest segment's entries.

A merge holds the writer only to name its file and to put the merged
segment in place, never while writing it. The coordinator applies changes
and writes segments meanwhile, and queries read the old segments until the
new one replaces them.

When a merge is cut short:

- by the index's closing, it stops reading at once and deletes its file
  without sorting or writing what it read, so closing waits for it no
  longer than that;
- by Pane's stopping, its file is left out of `index.json`, and the next
  open deletes it. What changed meanwhile is in the log or a segment of
  its own, so nothing is lost.

A merge that fails is tried again when a segment is next written. One that
panics is given up the same way and said so on the standard error, and
the merge thread goes on.

A first walk writes segments directly, without the log, with no merge
running, and merges them into one at its end. So does
`FileIndex::compact`, which the benchmark uses; it also holds the merges
off (`FileIndex::hold_merges`) to time queries on unmerged segments.

#### Queries

Terms are the folded words (case and accents ignored, split at camel case
and digits) of the name and, separately, of the folders below the root.

A query reads, per segment, at most 1,000 candidates matching every word,
those with every word in the name first, ranked by their hints. It then
scores them as #126's "Matching and ranking" describes
(`file_index/text.rs`): an exact name, then an exact stem, a name starting
with the query, every word starting a word of the name, then of the
folders.

A query with `/` or `\` matches path segments in order. The words of each
part start words of one folder below the root, those folders in the
parts' order (others may lie between), and the last part's words start
words of the name ("documents/plan", `docs\work\` for anything inside). It ranks
just below a name prefix.

When the words' starts find fewer entries than the page asks for, a
second pass looks for words of three letters or more inside words ("port"
finds "report", in a name or a folder). It lists those after every match
by the start of words (`text::score_inside`, its score put 100 below).

That pass does not read the whole dictionary (#185). Each segment, and the
memory table, keeps a **fragment index**: the terms having each run of 3
bytes of their words ("rep", "epo", "por", "ort" for "report"), name and
folder terms apart. The pass takes the terms having every fragment of the
word (the shortest list first, the others only keeping what it found),
checks that each really holds the word, and reads their entries in the
dictionary's order. That is what reading every term did, so the same
entries are found and stop at the same place. A word no term holds
usually ends at its first fragment.

Checking that a candidate is the current version of its path looks up the
folders above it among the tombstones (kept by prefix, not read one by
one). It asks a newer segment for the path only when the segment's **key
filter** (a Bloom filter of its paths, about 1 in 100 wrong) does not
rule it out.

### A segment's file

A segment's file (`file_index/segment.rs`, format version 2 since #185)
holds, in order:

- the entries, front-coded in blocks of 16;
- each block's offset and each entry's hint;
- each term followed by its postings (the term stored again so that its
  ordinal, its place in the dictionary's order, leads to it);
- the `fst` dictionary;
- the tombstones it carries;
- each term's offset by ordinal (4 bytes);
- the fragment lists (each fragment's term ordinals as gaps) and the
  fragment table (4-byte fragment, 4-byte offset, sorted, searched by
  halves);
- the key filter (10 bits per entry, 7 bits set by each path);
- a footer of 21 numbers.

Next to version 1 this adds, per term, about 4 bytes of offset, the term
again (its length and bytes) and a byte or two per fragment of it, and
1.25 bytes per entry for the filter. An index of version 1 is rebuilt.

### The index's folder

The folder holds:

- `index.json`: the format version, the live segments, and Pane's record
  (whether a first walk finished, each volume's journal cursor, and the
  roots and rules it was built under);
- `<n>.seg` and `<n>.wal`;
- `lock`, which the index holds locked, so a second Pane on the same cache
  folder gets `IndexError::InUse`.

The folder is mode 0700 and the files 0600 on macOS and Linux. On Windows
the folder is made with a protected DACL for the user and SYSTEM, which
its files inherit (#175). An index of another `FORMAT_VERSION`, or one
that cannot be read, is deleted and rebuilt, never read.

The log is handed to the system after each batch but not flushed to the
disk. A power loss can lose the last changes, which the catch-up finds
again from the file system's records, since the cursors are saved only
with segments.

### Why the engine is Pane's own

Pane has its own engine instead of using the `minidex` crate. This was
decided on reading `minidex` 0.38.0's source (MIT, by Joao Neves, the
crate Raycast uses), before the measurement, which is to confirm it. The
reasons:

- **Format.** #126 requires Pane's index format to be Pane's, versioned,
  and rebuilt on any other version. `minidex` writes its own files, and at
  0.38 it is pre-1.0 and changing quickly (each release may change them);
  Pane would version a format it does not control.
- **What an entry keeps.** `minidex` keys an entry by
  `path.to_string_lossy()`, so a name that is not valid Unicode cannot be
  opened from the index. It keeps kind, modified and accessed times, a
  category and a volume type, but no size and no file id. The NTFS
  catch-up resolves journal records by file id and parent folder id, and
  Search Files shows sizes, so both would need a second store beside it.
- **Tombstones.** Its prefix tombstones compare paths in ASCII lower case,
  so deleting `~/Docs` would also hide `~/docs` on a case-sensitive file
  system (Linux, case-sensitive APFS).
- **Threads and priority.** It starts its own flush, compaction and
  recovery threads with its own priority policy. #126 wants Pane's
  coordinator to own background priority, pausing for sleep and "no
  periodic work while nothing changes". (Pane's index has one merge thread
  of its own since #187, at the same background priority, which waits
  without waking while no segment is written.)
- **Matching.** Its tokenizer and candidate pruning are fixed. #126 wants
  folding consistent with root search's and weights tuned against Pane's
  fixtures, with name and folder matches told apart.
- **Dependencies.** It brings `zstd` (a C library built by `cc`), `fs4`,
  `arc-swap`, `thiserror` and `log`. Pane's own engine adds only `fst` (no
  dependencies of its own), `memmap2` (already in the tree) and, for the
  rules rather than the engine, `ignore`.
- **The first index.** Its inserts all go through its log; Pane's first
  walk writes segments directly.

What Pane takes from it is the shape: segments with an FST dictionary, a
memory table and log, tombstones, compaction, and a compact per-entry hint
for pruning candidates before reading them.

**The measurement that confirms it** is the benchmark below, run on the
machine where Raycast was measured, against the home folder. The targets
are a first index in less than 12.9 s (aiming for half), an index smaller
than 61.8 MB, a query's 95th percentile under 10 ms, and a changed file
visible within 10 ms. If Pane's engine misses the size or query target,
`minidex` 0.38 is measured on the same tree behind the same `FileIndex`
calls before Pane's is tuned further. The result and the decision are
recorded in #174's results comment.

### The benchmark

`cargo xtask file-index-bench [options]` builds
[`crates/pane-core/examples/file_index_bench.rs`](../crates/pane-core/examples/file_index_bench.rs)
in release and runs it on demand.

By default it generates a home-shaped tree of about 450,000 indexable
entries in the system's temporary folder, reused by later runs. The tree
has 9 files per folder, paths about 8 folders deep and accented names. It
also has entries the rules leave out: hidden folders, `node_modules` and
Git repositories with ignored `build` folders and logs. `--home` indexes
the real home folder instead (it writes nothing there), and
`--root <folder>` another folder.

It prints a table against #126's targets and Raycast's numbers, with
these rows.

**The first index.** Run 1 and the median of the warm runs (`--runs`,
default 3), with the walk's own time. Run 1 is cold only when it is the
first since a restart, or after `--drop-caches` where the system allows it
(Linux as root, macOS with `sudo purge`; Windows has no way without
administrator rights).

**The index on disk**, in total and per entry.

**Query latency** over a fixed set (`--queries`, default 1,000, about as
many of each kind), on two shapes of the index:

- one segment, as the first index leaves it;
- several segments with changes in memory, as a stream of changes leaves
  it: 5 batches, about one change for every 50 entries a batch, between
  500 and 10,000 (files added, entries changed, files and a folder
  deleted), 4 written as segments and the last in memory. The background
  merges are held off while it is timed (`FileIndex::hold_merges`), so it
  holds 5 segments and the changes in memory however fast the merges would
  have been.

Each shape gets a first pass and a warm pass. A third row times the same
set once more while changes arrive (#187). Another thread applies streams
of 5 batches of a tenth of that size, each but the last written as a
segment, 100 ms apart, so that segments are merged in the background
while the queries run. The row says how many changes arrived meanwhile.

The overall 95th percentile covers every kind (#183):

- whole names, 3-letter prefixes, a word, folder and name words, one
  letter;
- misses: a word no entry holds (3 to 8 letters, so the pass inside words
  runs too), and a real word followed by such a word, each checked to find
  nothing;
- typed-out names: every prefix of a name, from its first letter to the
  whole name, one keystroke after the other;
- the kind filters Folder (the index's own) and Document (run as Search
  Files runs it: at least 500 asked for and the documents kept, asking
  again for more while fewer than a page are kept);
- three or four letters from inside a word of a name.

The set is built from the names the walk found, sorted, and a seeded
generator, so runs over the same tree time the same queries. It takes the
first entry of each batch the walk hands over, which is each folder
walked, and every 400th after it in a large folder. A table gives each
kind's 50th, 95th and 99th percentiles on both shapes.

**A changed file re-indexed**, 100 times, as the indexer takes a change:
what the index holds at its path, the file read, the rules applied to it
as a batch applies them, the change applied and found by a query. Applying
the rules follows #186: the global ignore file is looked at, the file's
own folder is dropped from what the scope keeps and its ignore files read
again (as when Windows reports that folder too), and the folders above it
are kept from the time before. The first of the 100 reads every folder
above it. There are three rows:

- a file near the root;
- a file in the deepest folder the walk found with ignore files on the way
  down, with its depth and how many of its folders hold ignore files. That
  file is written beside the first and indexed as if it were in that
  folder, which is not written to;
- that deep file with nothing kept, every folder above it read again each
  time, as the first change after a start, or after what was kept of the
  folders above was dropped (and as #183 measured every change).

**The catch-up**, on Windows with the generated tree, after 10,000 files
created: the journal read from a saved cursor, resolved (the folder-id
table read once, as the coordinator does since #187), looked at, applied
and found.

**Start and memory**: the time to open the index at start, the private
memory it adds while idle, and the peak memory while indexing.

The row labels stay as they are, so runs compare.

#### The regression guard

`cargo xtask file-index-guard` runs the same benchmark in CI
(`ci-branch.yml`'s Linux tests, shard 2). It uses a generated tree of
about 20,000 entries, one run, built in the development profile as the
tests built it, with `--guard`.

It fails when any of these is over its ceiling: the first index, the
index's size per entry, any kind's 95th percentile on either shape, or any
re-index row's 95th percentile. The ceilings are set for that unoptimized
build, far above what it should take, so they catch only a large
regression; [CI](agents/ci.md) lists them.

### The NTFS change journal without administrator rights

`file_index::read_journal` opens the volume's root folder (`C:\`) for
reading attributes only and asks for the journal with
`FSCTL_QUERY_USN_JOURNAL`. It reads it from a saved cursor with
`FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` (Windows 10 1709 and later), never
creating or resizing a journal.

It answers the records and the new cursor, or why a walk is needed: the
journal was recreated (another id), the records were already discarded,
there are more than a walk would cost, the volume keeps none (FAT, exFAT,
network shares), or the system refused.

`file_index::resolve` turns the records into paths through the folder ids
the index holds (each folder's NTFS file id, read by the walk), following
renamed folders. A record in a folder the index does not hold resolves to
nothing. `catch_up_changes` then looks at each path and applies the scope.

#### Records without names

Read without administrator rights, the records carry no names. Windows
leaves them out; this was seen on Windows 11 as a non-elevated user. So:

- A record without a name is named by its file id (`file_index::Names`):
  the entry is opened with `OpenFileById` for reading attributes only and
  its name read from the handle, each id once, at most 100,000 per
  catch-up.
- An entry gone has no name to read. A folder gone is known by the id the
  index holds. A file gone puts its folder in `CatchUp::listed`, and the
  entries of that folder the disk no longer holds are removed
  (`file_index::missing_from`).
- An entry gone that the index did not hold (`CatchUp::gone`: a hidden
  `.gitignore` or `.git` deleted, say) has its folder re-checked.
- A record in a folder the index does not hold has that folder looked up
  by its id (`CatchUp::unresolved_folders`, `Names::path`) in case it is a
  repository's `.git/info` ([above](#catching-up-and-watching)).

The test
`file_index::journal::tests::the_journal_is_read_without_administrator_rights_and_resolves_to_paths`
indexes a temporary folder, makes changes (a file created, one deleted, a
folder renamed, a file in it created), and reads them back from the
cursor. Run as a non-elevated user, it is the evidence #174 asks for (CI's
Windows runner is an administrator).

The other systems' equivalents are FSEvents' history on macOS (no
permission beyond reading the folders, and the privacy prompts for
Desktop, Documents and Downloads) and, on Linux, the reconciling walk,
since no history is readable there without privileges (fanotify needs
`CAP_SYS_ADMIN`). See [Catching up and watching](#catching-up-and-watching).

## Checks

Written with #175; none has run yet (tests run once every ticket of the
milestone is merged).

### Through the launcher

- [`crates/pane-core/tests/typed_queries.rs`](../crates/pane-core/tests/typed_queries.rs)
  checks the rows for a path typed into root search (#195), with the real
  Files package, a recording link opener and system and a home folder of
  the test's own:
  - Open and Reveal in File Explorer listed under "Addresses" only for a
    path-like query;
  - Open opening the file, and revealing a program instead of running it;
  - Reveal in File Explorer revealing it.
- [`crates/pane-core/tests/file_index.rs`](../crates/pane-core/tests/file_index.rs)
  checks the index through the launcher. It uses the real Files guest, the
  real index and the system's own change source over a fixture folder
  standing for the home folder (the test names it), a cache folder of the
  test's own and a recording opener and system, waiting on
  `Launcher::wait_for_file_index`.
  - Searching: typing a file's name lists it under "Files", exact name
    first, at most five, then "Search Files for “plan”", with its folder
    `~/Documents`, and Enter opens it; hidden, ignored, `node_modules` and
    cache-tagged entries absent; a row from the first character and none
    for a blank query; case, accents and folder words; a folder found,
    reading Folder; file rows after a command found by title.
  - Opening: a program, a script and a shortcut each revealed by Enter and
    never opened; an entry replaced by a folder since it was found
    explained, not opened.
  - Running and stopping: the first walk waiting until the launcher is
    shown; disabling Files stopping the index (kept on disk), a file
    written then found once it is enabled again, and uninstalling deleting
    the index; a restart catching up what changed while Pane was stopped
    by the system's records (`CaughtUpBy::Journal` on Windows,
    `EventHistory` on macOS, `ReconcilingWalk` on Linux), not a full walk;
    a second Pane on the same cache folder saying file search is in use;
    the user's rules recorded in `file-search.json` and applied without a
    restart.
  - The File Search page (#176): the status (state, entries, how and when
    it last caught up); Rebuild Index walking every folder again; turning
    off Search Files stopping the index and saying why; every control (an
    added and removed root, an excluded folder and pattern, each switch)
    changing what is found without a restart; a folder taken out for churn
    listed and included again; a folder granted to Files under #29 outside
    the home folder added to the roots and the grant forgotten, and one the
    index covers simply forgotten.
  - The ignore rules kept between batches (#186), each over several
    batches of changes: a `.gitignore` line added hiding what it matches
    (and what a later batch adds) and removed showing it again; a new
    `.git` folder applying its `.gitignore`, and deleting it lifting it; a
    repository's own `.git/info/exclude` applied and lifted while Pane
    runs; the user's excluded pattern applied and lifted without a
    restart; a file an ignore file hides since it was found explained at
    Enter, not opened.
- [`crates/pane-core/tests/files.rs`](../crates/pane-core/tests/files.rs)
  checks the granted folder with the `folder-files` fixture: the grant,
  its record and its refusals, the scan policy, the listing per visit and
  its cancellation, the checks at Enter, as since #29.

### The coordinator and the index's modules

- `file_index::indexer` unit tests check the coordinator through the
  change source's seam: a fake source the test scripts (what a catch-up
  finds) and drives (the live changes it reports), with the real index and
  walker.
  - Searching: the first walk waiting to be shown; only a package that
    uses the index searching it, ids its own and checked again (removed,
    replaced by a folder, by a link); kinds, categories, sorting and
    pages; the categories Text and Other.
  - Changes: created, moved-in, renamed and deleted entries applied, a
    hidden one not; an overflow reconciling its folder; disabling stopping
    the watch and enabling again catching up without walking; records that
    are gone leading to a reconciling walk with the same result; a restart
    over the same cache folder; a second index refused; deleting the index
    forgetting its ids.
  - Rules and roots: the user's rules (hidden entries, an added root, an
    excluded folder, a removed root); excluding a folder taking only it
    out and including it again walking only it; an added root away and
    back; the folders a source cannot watch counted, listed and reconciled
    every few minutes.
  - Volumes (#184): an added root the system says is on a network share or
    a removable drive (a fake answer through `IndexerConfig::volumes`,
    standing for Windows' and macOS's) left out, indexed once other
    volumes are included (the removable drive watched, the share never, a
    change on it found by the reconciling walk) and left out again; a home
    folder on a share left out until then.
  - The folder-id table (#187, through `FileIndex`'s count of reads): read
    once for a catch-up that reads it and a watch given each folder; the
    watch given the folders as the catch-up's changes left them; on
    Windows a restart through the real NTFS journal reading it once.
  - The ignore rules (#186): an ignore file changed in place re-checked
    after an overflow; a folder a catch-up asks to re-check; every folder
    re-checked after an ignore file in the home folder changed, a few at a
    time with a change reported meanwhile applied too; the rules of a
    folder not watched read again at Enter.
  - Each safety valve triggered: a folder churning taken out, listed,
    recorded, kept out across a rule change and included again; one busy
    now and then never taken out; the same burst of changes in
    `node_modules`, a repository's `.git`, an ignored and a hidden folder
    taking nothing out while it takes out an indexed folder (#184); a walk
    stopped at the ceiling; indexing stopped while the disk is short of
    space (nothing written, a change let go) and started again by itself
    once there is room (the walk, then the change caught up); a folder
    that does not answer skipped and listed without holding up the walk; a
    sleep (a fake computer the test puts to sleep,
    `power::tests::FakeAwake`) pausing a batch of changes and a walk under
    way until the pause after the wake is over, the status saying so, the
    sleep not counted as hanging; a folder macOS refused listed apart from
    one that cannot be read.
- `file_index::scope` tests check that what is kept of a folder holds until
  it, a folder above it, or everything is forgotten, and that nothing is
  kept of a folder not watched live. They also check which roots are kept,
  watched and reconciled as each volume's kind and the switch say, that a
  root away is asked about again once it is back, and that a volume that
  does not answer in time, or of which the system says nothing, is left
  out.
- `file_index::reconcile` tests check a re-check reading every folder a
  few at a time under the rules as they are now, a folder read again
  holding an ignore file named for one, and a folder that does not answer
  keeping its entries.
- `file_index::reconcile` and `store` unit tests cover reading only changed
  folders, a missing root keeping its entries, a folder's children across
  segments and memory, a query found inside words after the words it
  starts (segments and memory alike, not when the words' starts fill the
  page) and a query with `/` or `\` matching path segments in order.
- `file_index::text` tests the ranks of each.
- `store`'s reference test (#185) answers a fixed set of queries
  (prefixes, words inside words, misses, accents and letter case, several
  words, paths), on pages of several sizes and the Folder kind, by brute
  force over a model of the changes. It checks that the index gives the
  same entries in the same order on one segment, on several segments with
  changes in memory and tombstones, after opening again and after a merge;
  on the way, that the fragment index finds what reading every term finds,
  in the same order.
- Other `store` and `segment` tests cover:
  - the tombstones looked up by prefix against reading each one, the term
    ordinals, the fragment lists, the key filter, and an index of the
    previous and of a later format version rebuilt;
  - the merges (#187): which run of segments is merged (similar sizes, the
    smallest run, any run past 8 segments); a merge held before it puts
    its segment in place while changes arrive (a batch written as a
    segment, one in memory), the same reference queries answering from
    what is current during it and after it, and after opening again, for
    a merge from the oldest segment (its tombstones dropped) and one above
    it (carried on); a restart from the folder as a merge cut short leaves
    it, its merged segment deleted and every change found through the log;
    many small segments merged a few at a time; tombstones piling up above
    the oldest segment merging every segment; a merge that panics given
    up, the next one running; and a segment's write given up leaving no
    file;
  - a word of one or two letters stopping gathering entries at 50,000, in
    memory and in a segment, and a query matching more entries than the
    1,000 it reads still finding the best first (#185).
- The walker's own tests hold up a folder's listing to show the walk does
  not wait for it, and walk a root on a share only once other volumes are
  included.
- `file_index::volume` tests what each system's answer (a drive type,
  `statfs`'s flags or type) means, that a temporary folder is on a local
  disk, that a system that does not answer holds up only the helper asking
  it, and (Windows) that a network path is a share by its text (#184).
- `file_index::power` tests the pause itself and that the system's clocks
  never say a time asleep that goes back.
- `file_index::privacy` tells a refusal from the system's answer (`EPERM`
  on macOS only) on every system.

### Per system

- `file_index::journal` tests read the NTFS journal without administrator
  rights (#174) and check which folders Windows' nameless records have
  re-checked.
- `changes::linux` tests check inotify reporting a change in a watched
  folder and a folder added later, and stopping when dropped, and that a
  repository's `.git/info` is watched with its folder. The watch limit
  cannot be lowered without root; its fallback is driven through the seam
  above.
- `changes::macos` tests check FSEvents replaying a change made before the
  stream opened from a saved event id, saying the history is done, then
  reporting a live change.

### Search Files, the file actions and the typed folder

- [`crates/pane-core/tests/file_actions.rs`](../crates/pane-core/tests/file_actions.rs)
  checks Search Files and the file actions for Files and the Rust,
  JavaScript and TypeScript samples alike: the entries listed in the
  command's own field with their folders, a folder found; a document's
  seven actions acting through the fakes and closing the window; Move to
  Recycle Bin confirmed first; a program revealed by Enter, Ctrl+Enter its
  Open With… submenu, only Run running it, in Search Files and in root
  search; root search's Files section with the system's icon and the row
  opening the command with the query typed; a folder opened in the file
  manager; a file created while Pane runs found through the system's
  watcher, and explained at Enter once deleted; the command keeping its
  id.
- [`crates/pane-core/tests/typed_folders.rs`](../crates/pane-core/tests/typed_folders.rs)
  checks the typed folder's entries for Files and the Rust, JavaScript and
  TypeScript samples alike, over a fixture folder and a real index of the
  home: the entries listed as file results, folders first and each in
  name order, under "Files" below the rows for the path; `~` resolved to
  the home folder; a missing folder listing nothing; Enter opening a
  document and showing a program in the file manager, never running it;
  the 500-entry bound with the row that says so; Tab's completion and
  Shift+Tab's query.
- [`crates/pane-core/src/typed_folder.rs`](../crates/pane-core/src/typed_folder.rs)
  unit-tests the listing's own bounds and checks.
- [`crates/pane-core/tests/search_files.rs`](../crates/pane-core/tests/search_files.rs)
  checks Search Files like Raycast's (#177). Files is acquired as Pane's
  default extension from its pinned commit in a repository served on this
  computer (the Rust files sample's component standing in for the
  extension's own, which lives in its repository), over the real index of
  a fixture home:
  - it opens with no folder to choose on Recently Used, newest first, each
    row with an icon;
  - typing ranks by the index;
  - each type of the dropdown keeps only its files (and Folder only
    folders), with a query too;
  - the detail's Name, Where, Type, Size, Created and Modified, an image
    previewed and a text file not;
  - pages of 50 loading until the index has no more, the selection kept;
  - the query kept from root search's row; Escape bringing Recently Used
    back;
  - a document's seven actions with Copy Name copying the name, Enter
    opening it, and a program shown, not run;
  - the index being built and a stopped index said, the latter leading to
    the settings;
  - a copy installed from a folder keeping its own search.
- `launcher::search_files` unit tests cover the types, the sizes, the Type
  label, the detail's metadata and the notes.

### In the window

- [`crates/pane/tests/file_actions.rs`](../crates/pane/tests/file_actions.rs),
  with real keys over the index: Enter and Ctrl+Enter on a document and on
  a program; Tab completing to a folder and Enter opening its entry,
  Shift+Tab removing the last path component, and Enter on a program
  showing it and running nothing.
- [`crates/pane/tests/search_files.rs`](../crates/pane/tests/search_files.rs):
  the split view on Recently Used with system icons and the dropdown at
  the field's right; the dropdown's ten types filtering; an image's
  preview and the Metadata rows, a text file's icon instead; typing and
  Escape; Ctrl+K listing Copy Name and Enter showing a program.
- [`crates/pane/tests/file_search_settings.rs`](../crates/pane/tests/file_search_settings.rs),
  over the real Files and index: the status and Rebuild Index; the hidden
  switch, the pattern field and Remove, Add Folder… and Exclude Folder…
  through the system's picker (and a cancelled one), each applied without
  a restart and recorded; the page saying file search is off while Files
  is disabled, or with no extension that uses it; each valve listed (a
  churned folder with Include Again, the ceiling, free space, a folder
  that did not answer); Settings' search finding the page and Rebuild
  Index.

### Native GUI smokes

One phase per system (screenshots 220 to 224). A debug build's
`PANE_TEST_FILE_INDEX_HOME` names a fixture folder for the index to cover
instead of the home folder, keeping the index in the phase's data folder.
The phase installs Files, types "plan" and opens the file found, then
starts Pane again, types "runner" and presses Enter, which reveals the
script and runs nothing. Rewritten for the index with #175; not run yet.

## Limits

### Indexing and the safety valves

- A sleep is noticed at the first piece of indexing work after the wake,
  not as it begins (see [Sleep](#the-safety-valves)). A folder's listing
  already asked of the system when the computer slept is not interrupted.
- A merge does not follow the sleep pause: one under way when the
  computer sleeps finishes after the wake, at background priority. A first
  walk waits for a merge under way to finish before it writes.
- Churn is counted per folder a change is in, not per subtree. A build
  writing across many folders at once is taken out folder by folder, only
  where one folder alone changes more than the threshold. A root itself is
  never taken out.
- A folder that hangs leaves its helper thread blocked until the system
  answers it. A mount that never answers keeps one thread per walk that
  met it, and one per question about its volume.
- The free space is read on the volume holding the cache, not on the
  volumes being indexed (which the index never writes to).
- The roots and rules an index was built under are recorded as JSON; a
  root whose path is not valid Unicode cannot be recorded.
- The benchmark's numbers against Raycast's are not measured yet (#174),
  nor the catch-up after 10,000 changes and the queries while changes
  arrive since #187's merges and shared folder-id table.

### Volumes

- A disk the system reports as fixed is indexed as a local disk even when
  it is plugged in by USB. Windows' `GetDriveTypeW` says removable of
  memory sticks and cards, not of most USB hard disks, and macOS sets
  `MNT_REMOVABLE` only for removable media.
- A root left out for being on a network share or a removable drive is
  still listed among the indexed folders on the File Search page, and not
  under Needs attention. A home folder on a share (a Linux home on NFS
  included) is left out until the switch is on.
- With the switch on, a network share mounted under a root (macOS, Linux)
  is watched with that root; only a root on a share is reconciled rather
  than watched. An entry under a root given as a network path
  (`\\server\share`) is found but not opened, since the check before any
  action refuses network paths ([Opening](#opening), step 1). One under a
  mapped drive's letter is opened.
- Which volume a root is on is asked when the index opens (a start, a
  change of the rules), not again while it runs, except for a root that
  was away, which is asked once it is back. A root that did not answer in
  time is taken for a network share until the index opens again. With the
  switch on it is reconciled like one, and a reconciling walk looks at the
  root itself without a time limit, so a share that stays stalled can
  still hold up the coordinator there (only listing its folders is
  bounded).

### Catching up and the ignore rules

- macOS asks before Pane reads Desktop, Documents and Downloads (and
  removable and network volumes). The File Search page says so before the
  first walk and lists a refused folder with how to allow it, whichever
  folder it is. It tells a refusal from another unreadable folder by what
  the system answered when the walker opened it: `EPERM`, "Operation not
  permitted", is the privacy protection; `EACCES` is the account's own
  permissions (`file_index::privacy`). Unverified on a real Mac.
- Linux's catch-up compares folders' modified times in whole seconds. A
  change within the second a folder was indexed is not seen until the
  folder changes again, and a file changed in place while Pane was stopped
  keeps its old size and time until it changes again while Pane runs.
- The ignore rules kept between batches (#186) can miss changes made while
  Pane was stopped:
  - Linux's reconciling catch-up re-checks a folder it reads again (its
    time changed) that holds an ignore file or a repository. An ignore
    file edited in place, or one deleted (its folder no longer holds one),
    is not seen there until the folder changes again or the index is
    rebuilt.
  - Windows' records read without administrator rights name no file. An
    ignore file deleted has its folder re-checked, but one renamed away
    (its id still names an entry) does not. A repository's
    `.git/info/exclude` is found by looking its folder up by id, at most
    4,096 such folders a catch-up; those past it are not looked at.
  - A folder not watched live (a share, past Linux's watch limit) keeps
    nothing, but its reconciling walk sees an ignore file edited in place
    there only as on Linux's catch-up.
  - A share mounted under a root (macOS, Linux) is watched with its root,
    though the system may report nothing of what other computers change
    there.
- The global ignore file is looked at with each batch of changes, so a
  change of it alone is applied with the next change under a root.
- The check before a file is acted on uses what the coordinator keeps. For
  about the settle time (100 ms) after an ignore file changes, before its
  change is handled, it judges by the rules from before. Until a re-check
  under way reaches a folder, the index holds what it held there before.
- Re-checking a folder reads every folder under it again. An ignore file
  changed in the home folder itself, the global ignore file, or an
  overflow therefore reads every root again (at background priority, 64
  folders at a time between batches of changes, the index answering
  meanwhile from what it holds).
- Where the system does not say when a `.git` was made, the first change
  named in it re-checks its repository once.

### Search and Search Files

- Search Files' Recently Used is the most recently modified entries, as
  Raycast's is; Pane records no use of a file (ADR 0030).
- Created is not indexed. It is read when a file is selected, and Linux's
  file systems may not record it.
- An image is previewed whole, as the window decodes it, not as a
  thumbnail. Other files show their icon, without Quick Look.
- Search Files is drawn by Pane for its own default extension only until
  #121's List detail and dropdown let any command draw it.
- The link from Search Files to the File Search page opens Settings at
  #176's page: `features::search_files::FILE_SEARCH_PAGE` is that page's
  title (`features::settings::file_search::TITLE`).
- A query inside a word ("port" in "report") is looked for only when the
  words' starts find fewer entries than the page asks for, and only for
  words of three letters or more. That pass reads the terms having every
  fragment of the word, so a word made of common fragments ("ing", "pdf")
  reads long lists and is slower than a match by the start of words on a
  large index.

### Handlers and accessibility

- A handler slow to fail (over three seconds) is reported as having opened
  the file.
- Screen reader behaviour is unverified, as for all of root search.
