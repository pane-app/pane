# Clipboard history

Added for [#35](https://github.com/hoangvu12/pane/issues/35) (Windows):
US65, US66, US70, US71; T10, T21, T22; contributions to G5 and G7, not
claims that they pass. Once the user turns it on, Pane keeps the text they
copy on this computer, and the **Clipboard History** default extension lists
it, newest first; Enter on an item copies it again or deletes it. It starts
off, can be paused, resumed and turned off again, and disabling the
extension stops it too.
[#36](https://github.com/hoangvu12/pane/issues/36) added expiry and the
remaining deletion controls (US67, US68, US69; T10, T21; G5, again
contributions): items are kept for 7 days unless the user chooses another
time, and Pane deletes them then, whether the extension runs or not
([Expiry](#expiry)); an item, the recent ones, all of them (Clear), or all
of them with history turned off can be deleted ([Deleting](#deleting)).
macOS and Linux observation are [#37](https://github.com/hoangvu12/pane/issues/37)
and [#38](https://github.com/hoangvu12/pane/issues/38). The architecture is
recorded in [ADR 0020](adr/0020-host-keeps-clipboard-history-for-an-extension.md)
and, for expiry, [ADR 0023](adr/0023-host-expires-clipboard-history-by-its-own-clock.md)
(both proposed).

## Where it lives

- **Host capability**, in the core: `pane:extension/clipboard-history`
  ([`wit/clipboard.wit`](../wit/clipboard.wit)), a host import any command
  of an installed package may use: `status`, `set-capture` (off, on,
  paused), `set-excluded`, `set-retention`, `entries`, `copy`, `clear`,
  `delete-items` and `turn-off-and-clear`. The host watches
  the clipboard and keeps the history itself, so nothing of the extension
  runs while the clipboard changes, and the history is the package's
  [extension data](extension-data.md) whatever the extension does.
- **Default extension**, [`guests/clipboard-history`](../guests/clipboard-history)
  (Rust), package [`guests/packages/clipboard-history`](../guests/packages/clipboard-history):
  its command, "Clipboard History", shows the controls and the kept items.
  It declares Windows only (`"platforms": ["windows"]`), since only Windows
  has an adapter, so on macOS and Linux it is listed as unavailable with
  #19's reason ("Not available on Linux: this command supports only
  Windows") and never runs. Rust commands use the import through
  `pane_guest::clipboard_history`; JavaScript and TypeScript commands import
  it when their package.json sets `"pane": { "clipboardHistory": true }`
  ([`guests/js/clipboard.d.ts`](../guests/js/clipboard.d.ts)), and only
  then, as for `files`. The samples
  [`sample-clipboard-js`](../guests/sample-clipboard-js) and
  [`sample-clipboard-ts`](../guests/sample-clipboard-ts) implement the same
  command in JavaScript and TypeScript, and the launcher tests run the same
  checks on all three.
- **System adapter** behind one small trait
  ([`pane_core::clipboard`](../crates/pane-core/src/clipboard.rs)), chosen
  by `clipboard::native()`: the Windows listener, or on other systems one
  that says clipboard history is unavailable there.

Acquiring the package automatically at setup is
[#51](https://github.com/hoangvu12/pane/issues/51) to
[#53](https://github.com/hoangvu12/pane/issues/53); until then it is
installed from its folder (`pane --install target/guests/packages/clipboard-history`).

## Behavior

The command's rows, in order:

| Row | Enter |
| --- | --- |
| "Turn on clipboard history" (off), "Pause clipboard history" (on) or "Resume clipboard history" (paused), subtitled with the state, the number kept and, if Pane cannot watch the clipboard, why | turns it on, pauses or resumes it; each row does only that, so pressing it again before the command is opened anew changes nothing more |
| "Turn off clipboard history", while on or paused | turns it off: nothing is kept and Pane stops watching for it; the kept items stay until cleared (or expire) |
| "Keep items for 7 days" (the retention now), subtitled "Older items are deleted, also while Pane is stopped or the extension is disabled · Enter changes it" | a form choosing 1 hour, 1 day, 7 days, 30 days or 90 days, listing the retention now first, which is chosen when it opens, so submitting it unchanged (Enter twice) changes nothing; items already older are deleted at once ("Items are kept for 1 hour; deleted 1 older item") |
| "Exclude a program" | a form taking a program's file name, such as `KeePass.exe` |
| "Stop excluding keepass.exe", one per excluded program | removes the exclusion |
| "Clear clipboard history", while items are kept | deletes every kept item; whether history is kept does not change |
| "Turn off and delete clipboard history", while items are kept and history is on or paused | turns history off and deletes every kept item at once ("Clipboard history is off; deleted 2 kept items"): the spec's **Disable and delete history** |
| "Delete recent items", while items are kept | a form choosing the last 15 minutes, hour or day; deletes the items copied then ("Deleted 2 kept items") |
| One row per kept item, newest first: its first line with content (at most 80 characters), subtitled "5 min ago · from notepad.exe · 2 lines · Enter copies or deletes it" | a form, titled with the item, choosing "Copy it again" (first, so Enter twice copies) or "Delete it": copying puts its text on the clipboard again ("Copied to the clipboard"), and the copy is a change like any other, so it moves to the front; deleting deletes that item alone ("Deleted the kept item"), or says "That item is no longer kept" |
| "Nothing kept yet", while on and empty | nothing |

The rows are the command's view when it opens: after an action the status
line answers, and the rows change the next time the command is opened.

- **Off until turned on.** A new package, and one never turned on, keeps
  nothing and Pane does not watch the clipboard at all: no listener is
  registered with the system. Turning it on starts the watch at once.
- **Watched exactly while kept.** Pane watches the clipboard while at least
  one installed package's history is on and the package runs (it is enabled
  and not [paused](pausing.md) after a failure). Pausing the history,
  disabling the package, Pane pausing it, uninstalling it, or turning
  another package's history off when it was the last one drops the
  listener at once: Pane stops taking its reports first, then tells its
  thread to stop and waits at most 1 s for it (on Windows the thread removes
  itself and ends; one stuck in a read, waiting on the program that copied,
  is left to end on its own, and what it reads then is dropped). Enabling
  the package, resuming or turning it on starts it again. A change that
  arrives while the package's code may not run is not kept, even if the
  listener has not stopped yet, and neither is one whose read began before
  the history was cleared.
- **Read once, tried again.** On Windows only the listener's thread reads
  the clipboard. A change is marked read only once it was read; if another
  program holds the clipboard open, the thread tries again 250 ms later, up
  to five times, before skipping that change. A failure in the listener is
  logged, never with what was copied, and it goes on listening.
- **Across restarts.** The capture state is kept with the history: after a
  restart Pane watches again, before any command opens, only where history
  is on and the package enabled; paused stays paused, disabled stays
  disabled and keeps its history (until it expires).
- **What is kept** ([`clipboard::accept`](../crates/pane-core/src/clipboard.rs)),
  the same on every system:
  - plain text only (Windows `CF_UNICODETEXT`); a copy with text and other
    formats keeps the text; images, files and rich formats alone keep
    nothing;
  - at most 32 KiB of UTF-8 (`MAX_TEXT_BYTES`); longer text is not kept at
    all rather than cut short;
  - not empty or white space only;
  - not marked by the copying application as not to be kept (below);
  - not copied from an excluded program, matched by the owning process's
    file name, ignoring case, with or without its extension (`KeePass`
    excludes `KeePass.exe`); at most 64 programs;
  - one item per text: copying a kept text again moves it to the front with
    its new time;
  - at most 100 items per package (`MAX_ITEMS`); beyond that the oldest go.
    This bounds the file; the retention ([Expiry](#expiry)) bounds how long.
- **Local only.** The history stays in Pane's data folder; Pane sends none
  of it anywhere and no other extension can read it through Pane (only the
  package that keeps it). This is not a boundary against trusted extensions
  or other programs running as the user
  ([policy](extension-policy-proposal.md#clipboard-history)).

## Expiry

Each item is kept for its package's **retention** after it was copied, 7
days unless the user chose another time (`DEFAULT_RETENTION_SECONDS`), and
then Pane deletes it ([ADR 0023](adr/0023-host-expires-clipboard-history-by-its-own-clock.md)):

- **Whether the extension runs or not.** Pane removes expired items itself,
  never by running the extension: before anything reads, counts or changes
  any package's history (the command's rows, an uninstall's "Saved data",
  a retained-data row, a copy, Enter on an item), and, while Pane runs, on
  a thread of its own when each item expires (and at least hourly, in case
  the system's time changed). So an item expires while its package is
  disabled, paused after a failure, or uninstalled with its data kept, and
  an item that expired while Pane was stopped is gone before anything shows
  it after the restart.
- **Never restarted.** An item's time is when it was copied (`copiedAt`),
  so disabling and enabling the package, pausing, turning history off and
  on or restarting Pane does not give it more time. Copying the same text
  again keeps it as a new copy, with its new time.
- **Configurable and finite.** The command offers 1 hour, 1 day, 7 days,
  30 days and 90 days; the host accepts any time from 1 minute to 365 days
  (`set-retention`), so history never grows without end. A shorter
  retention deletes the items already older at once; a longer one keeps
  the kept items, and those copied later, longer (what expired stays gone).
  The retention is one of the package's choices, like whether history is
  kept and the excluded programs: it stays when the items go, and in
  retained data ("keeps clipboard history settings").
- The defaults and choices are provisional, pending the user's decision
  ([current decisions](current-decisions.md)).

## Deleting

| Control | Deletes | Afterwards |
| --- | --- | --- |
| Enter on an item, "Delete it" | that item | history stays as it was |
| "Delete recent items" | the items copied in the last 15 minutes, hour or day | history stays as it was |
| "Clear clipboard history" | every item | history stays on (or paused): what is copied next is kept |
| "Turn off and delete clipboard history" | every item | history is off: nothing more is kept, also after a restart, until it is turned on |
| Expiry | each item once its retention passed | unchanged |
| Uninstall and delete saved data, Delete retained data | every item and every choice | the package keeps nothing |

- A deletion is one change of the file, written before the command's
  answer: turning history off and deleting its items happen together, so
  nothing copied in between is kept.
- A clipboard change Pane was still reading when items were deleted (by
  any control but expiry) is dropped rather than kept afterwards, so
  deleting never brings an item back; what is copied after a deletion is
  kept as usual.
- A row shown before a deletion stays until the command is opened again;
  Enter on a deleted item says "That item is no longer kept" and changes
  nothing.
- Deleting is not forensic erasure (the file is replaced; its old blocks
  may remain on the disk), and it never changes what is on the system's
  clipboard.

## Sensitive markers

Before reading the text, Pane's Windows listener reads the formats
applications use to say that what they copied must not be kept, and reads
the text only if none of them does:

| Format | Pane keeps the text |
| --- | --- |
| `ExcludeClipboardContentFromMonitorProcessing` present | never |
| `Clipboard Viewer Ignore` present (older password managers) | never |
| `CanIncludeInClipboardHistory` = 0 | never |
| `CanUploadToCloudClipboard` = 0 | never, although Pane uploads nothing: an application refusing the cloud is taken to mark the text sensitive |
| `CanIncludeInClipboardHistory` or `CanUploadToCloudClipboard` = 1, or absent | yes, unless another rule refuses it |

These are the formats Microsoft documents for clipboard monitors, Windows'
clipboard history and its cloud clipboard; password managers such as KeePass
and KeePassXC set one or more of them when they copy a password (which ones
depends on the application and its version, and was not checked here).
Detection is only as good as what applications declare: an application that
sets none of them is kept like any other, and Pane does not try to detect
secrets in the text itself. A program can also be excluded by name, but the
owning process is the one whose window owns the clipboard, which is
sometimes a helper process or none at all (a copy made without a window has
no owner, so its program is unknown and never excluded).

## Ownership and deletion

- The history is the package's extension data of a kind of its own,
  **clipboard history**, in `clipboard-history.json` beside the other kinds,
  under the package identity's key, written by Pane only (never by the
  extension directly), atomically, and readable only by the user: mode 0600
  on macOS and Linux, and on Windows a protected DACL giving the user and
  SYSTEM only full control, inheriting nothing from the folder, set as each
  new version of the file is created, before it replaces the old one. The
  file is typed and versioned: `{"version": 1, "packages": {<identity key>:
  {"capture", "excluded", "retentionSeconds", "items", "nextId"}}}`, with
  `capture` "on" or "paused" (missing is off), `excluded` lowercase program
  names, `retentionSeconds` the retention the user chose (missing is the
  default), and `items` newest first, each with its `id`, `text`, `copiedAt`
  (milliseconds since the Unix epoch) and `source`. A package whose items
  all went and that has no choices left keeps only its `nextId`, so its
  ids are never given twice (it counts as keeping nothing). A
  `retentionSeconds` outside 1 minute to 365 days, as only an edited file
  can hold, is taken as the nearest bound.
- It is written after each change, outside the lock that captures and
  commands share, so a copy never waits on another's write. A change is on
  disk when the call that made it returns; a crash before that loses only
  that change (the file is replaced atomically, never torn).
- A command reaches it through Pane's extension runtime, like every other
  host interface ([#18](pausing.md#when-an-extension-stops-responding)):
  stopped code (its package disabled, paused, reloaded or uninstalled, or
  its runtime thread given up on) reads and changes nothing, a change is
  made while the runtime thread's fence is held, so none lands after a
  give-up, and each call is a marked host call, so its time (the history's
  lock and file, the system's clipboard) never counts against the guest's
  compute limit.
- It is **saved data**, like settings and content: Clear cache keeps it;
  uninstalling asks, "Saved data: 12 clipboard history items", and
  "Uninstall and delete saved data" removes it, while "keep" keeps it as
  [retained data](extension-data.md#retained-data) ("keeps 12 clipboard
  history items", or "clipboard history settings" when only the state and
  exclusions are kept), which "Delete retained data" removes. Reinstalling
  the same source after keeping it keeps history as it was, on if it was on.
- The command deletes items as [Deleting](#deleting) says, and Pane
  expires them ([Expiry](#expiry)); both remove them from the file.

## Per platform

| | Windows (#35) | macOS (#37) | Linux (#38) |
| --- | --- | --- | --- |
| Observed with | `AddClipboardFormatListener` on a message-only window of a thread of Pane's own (`WM_CLIPBOARDUPDATE`), reading the markers first, then `CF_UNICODETEXT`, and the owner through `GetClipboardOwner`, `GetWindowThreadProcessId` and `QueryFullProcessImageNameW` | not yet | not yet |
| Written back with | `SetClipboardData(CF_UNICODETEXT)` | | |
| Permission | none | | |
| Unavailable | | the command is listed as "Not available on macOS: this command supports only Windows" | "Not available on Linux: this command supports only Windows" |
| Baseline | Windows 10/11; CI `windows-2025` | | |

## Checks

- Capture rules ([`clipboard.rs`](../crates/pane-core/src/clipboard.rs) unit
  tests): plain, marked, withheld, other, blank and long content; excluded
  programs; program names, lowercased also when read from the file; newest
  first, one per text and at most 100; state, exclusions and clearing kept
  apart; the typed, versioned file; a capture begun before a deletion
  keeping nothing. Stopping a thread within a limit, or leaving it
  ([`threads.rs`](../crates/pane-core/src/threads.rs)); on Windows, the
  owner-only DACL ([`atomic.rs`](../crates/pane-core/src/atomic.rs)). The package's generation
  ([`extension_data.rs`](../crates/pane-core/src/extension_data.rs)):
  nothing kept while off, paused by Pane or uninstalled, and each change
  starting or stopping the watch; with #18, no change landing after a
  runtime thread's fence closed, and fenced code reading nothing. The
  runtime ([`runtime.rs`](../crates/pane-core/src/runtime.rs)): a slow
  clipboard history call is Pane's time, never the guest's. Retention and expiry
  ([`history.rs`](../crates/pane-core/src/clipboard/history.rs)): an item
  kept until exactly its retention passed; the retention's bounds; items
  deleted by id, an id no longer kept passed over and counted as a
  deletion; a file left by a downtime counted, read and rewritten without
  what expired (a package left with nothing keeping only its next id,
  also when the file had none), a written retention out of bounds taken as
  the nearest, and a later copy of an expired text kept as new with a new
  id; a shorter retention deleting older items at once; the expiry thread
  removing an item when a test's clock passes its time, with nothing
  reading the store, and ending with it. Tests wait for the expiry thread
  by its own word (a sweep begun after the last change ended), never by
  sleeping or polling.
- Launcher public interface ([`crates/pane-core/tests/clipboard.rs`](../crates/pane-core/tests/clipboard.rs)),
  with the real Clipboard History guest and the JavaScript and TypeScript
  samples alike, and a fake system clipboard (a copy of each package
  declaring every system): nothing watched or kept until turned on, then
  kept, on disk too with mode 0600; markers, blank, other and long content;
  excluding and including a program through the form; pause and resume,
  also across a restart; turning it off keeping the items; a read still
  waiting when the package is disabled or paused neither delaying it nor
  kept, even once a new watch runs; a read begun before Clear not kept; disable stopping the watch, a restart
  while disabled not watching, enable and a restart watching again; Enter
  copying an item again; the 100-item bound and Clear; uninstall deleting
  or keeping (retained, and kept on for a reinstall); a system that cannot
  watch, and a launcher without a clipboard; the real package unavailable on
  macOS and Linux with its reason. With a clock the tests move
  (`Launcher::with_clock`, a `ManualClock`; no test waits for time to pass):
  items expiring 7 days after they were copied, while the package is
  disabled and Pane stopped, gone from the file once Pane starts, and not
  given more time by enabling it again; the retention's form starting on
  the retention now, so submitting it unchanged changes nothing; the
  retention changed through its form, older items deleted at once, kept
  across a restart and applied to later items; expired items removed from the file while Pane runs with
  the package disabled and nothing reading the history; one item deleted,
  a read begun before that not bringing it back, its stale row deleting
  nothing and the clipboard untouched; the recent items deleted together;
  Turn off and delete stopping the watch, dropping a read in progress and
  staying off after a restart; retained history expiring without the
  extension.
- Windows adapter ([`crates/pane-core/tests/clipboard_adapter.rs`](../crates/pane-core/tests/clipboard_adapter.rs),
  Windows only) against the real clipboard, with text only the test puts
  there: plain text reported with its owner (the test's own process), each
  of the four markers read and withholding the text, `CanIncludeInClipboardHistory`
  1 allowing it, a written text reported, and nothing once the watch is
  dropped. It **replaces what is on the clipboard** and does not put it
  back, so it runs only with `PANE_TEST_REAL_CLIPBOARD=1`, which CI's
  Windows runner sets; elsewhere it passes without doing anything. Its
  marked copies also say `CanIncludeInClipboardHistory` 0 where the check
  allows, so Windows' own history (Win+V) does not keep them, and it never
  says `CanUploadToCloudClipboard` 1; withheld reports count only when this
  test's process owns the clipboard with the markers it set.
- Native GUI smokes, screenshots 280 to 285: on Windows (with a data folder
  of its own, copying only its own `pane-smoke-...` text through the
  clipboard API and putting back what was on the clipboard, in memory only)
  off, turned on, kept without the four marked texts, paused, resumed,
  Enter copying an item again, disabled, disabled across a restart,
  enabled and kept again across a restart, with `clipboard-history.json`
  checked at each step; on macOS and Linux (280 and 281) the command listed
  as unavailable and explained, with no history file. For #36,
  screenshots 400 to 404 on Windows: with Pane stopped, the smoke makes one
  kept item 8 days old and one 2 hours old (`scripts/clipboard_history.py`);
  after the restart the first is gone before the command shows anything,
  then one item is deleted through its form, the recent ones through Delete
  recent items (the last hour), the 2-hour-old one by keeping items for 1
  hour, and the last with Turn off and delete, after which a copy is not
  kept, and the clipboard still holds what was copied last. On macOS and
  Linux (400 to 402, where the command never runs) the smoke writes a
  history for the installed package with one item 8 days old and one a day
  old: after the start the first is gone from the file and the uninstall
  confirmation counts "Saved data: 1 clipboard history item"; kept as
  retained data and made 8 days old while Pane is stopped, it is gone once
  Pane starts, leaving "keeps clipboard history settings". See the
  [Windows](platforms/windows.md#clipboard-history-35),
  [macOS](platforms/macos.md#clipboard-history-35) and
  [Linux](platforms/linux.md#clipboard-history-35) notes for where they have
  run.

## Limits

- Windows only; macOS (#37) and Linux (#38) observation are not built.
- Text only; no images, files or rich text, and no text longer than 32 KiB.
- Detection of sensitive content is only what applications declare
  (markers) and the programs the user excludes; the owning process can be a
  helper or unknown.
- The retention's default (7 days) and choices are provisional. Expiry
  follows the system's time: an item copied while the time was set far
  ahead is kept until then, and setting the time back keeps items longer.
- There is no action panel, so deleting one item is a choice in the
  item's form, and Enter twice (not once) copies an item again.
- A disabled package's history cannot be deleted without enabling it
  (uninstalling, or its expiry, can); a retained one has Delete retained
  data.
- The command's rows are read when it opens; they do not change while it is
  open, even as text is copied.
- More than one package may keep history; each keeps its own, and Pane
  watches once for all of them.
- The files are replaced atomically but not locked (as every kind of
  extension data): two Pane processes on one data folder can lose each
  other's last write.
