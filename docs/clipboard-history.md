# Clipboard history

Added for [#35](https://github.com/hoangvu12/pane/issues/35) (Windows):
US65, US66, US70, US71; T10, T21, T22; contributions to G5 and G7, not
claims that they pass. Once the user turns it on, Pane keeps the text they
copy on this computer, and the **Clipboard History** default extension lists
it, newest first; Enter on an item pastes it, and its other actions copy
or delete it (#150). It starts
off, can be paused, resumed and turned off again, and disabling the
extension stops it too.
[#36](https://github.com/hoangvu12/pane/issues/36) added expiry and the
remaining deletion controls (US67, US68, US69; T10, T21; G5, again
contributions): items are kept for 7 days unless the user chooses another
time, and Pane deletes them then, whether the extension runs or not
([Expiry](#expiry)); an item, the recent ones, all of them (Clear), or all
of them with history turned off can be deleted ([Deleting](#deleting)).
[#38](https://github.com/hoangvu12/pane/issues/38) added the Linux (X11)
adapter and [#37](https://github.com/hoangvu12/pane/issues/37) the macOS
(pasteboard) adapter, so the package declares Windows, macOS and Linux,
the three systems with an adapter. The architecture is recorded in
[ADR 0020](adr/0020-host-keeps-clipboard-history-for-an-extension.md)
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
  It declares the three systems with an adapter
  (`"platforms": ["windows", "macos", "linux"]`), so its command runs
  wherever Pane runs. Rust commands use the import through
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
  by `clipboard::native()`: the Windows listener, the Linux watcher of the
  X11 `CLIPBOARD` selection ([`linux.rs`](../crates/pane-core/src/clipboard/linux.rs)),
  the macOS watcher of the pasteboard's change count
  ([`macos.rs`](../crates/pane-core/src/clipboard/macos.rs)),
  or on any other system one that says clipboard history is unavailable
  there.

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
| One row per kept item, newest first: its first line with content (at most 80 characters), subtitled "5 min ago · from notepad.exe · 2 lines · Enter pastes it" | its actions (#150), in place of #36's "Copy it again / Delete it" form: **Paste** (Enter) pastes it into the application that was in front before Pane, which closes the window, or, where Pane cannot paste yet (#125), copies it again instead, closes the window and shows "Copied — paste is not available here yet" in a HUD; **Copy** (Ctrl+Enter) puts its text on the clipboard again, closes the window and shows "Copied to Clipboard" (the copy is a change like any other, so it moves to the front); **Delete** (Ctrl+Shift+Enter), destructive and last, deletes that item alone ("Deleted the kept item"), or says "That item is no longer kept" |
| "Nothing kept yet", while on and empty | nothing |

The rows are the command's view when it opens: after an action the status
line answers, and the rows change the next time the command is opened.

Pane's own Clipboard History opens in its split view (#102), the records
beside a preview; its keys are the kept items' actions (#150): **Enter**
(the footer's Paste) pastes the selected record into the application in
front through Pane's system, closing the window, or where Pane cannot
paste yet copies it through the history's own copy and shows "Copied —
paste is not available here yet"; **Ctrl+Enter** (Copy) copies it again
and keeps the view; **Ctrl+D** (Delete) deletes it. Each revalidates the
reading first (`Launcher::paste_clipboard_record`,
`copy_clipboard_record`, `delete_clipboard_record`). Manage (Ctrl+K)
shows the rows above.

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
- **Read once, tried again.** On every system only the listener's thread
  reads the clipboard. A change is marked read only once it was read; on
  Windows, if another program holds the clipboard open, the thread tries
  again 250 ms later, up to five times, before skipping that change, and
  on Linux the watcher waits at most 4 s for the program that copied to
  answer its request for the text (and at most 500 ms for each piece of a
  text sent in pieces), then skips that change; every wait while reading
  is bounded, so an abandoned read always ends by itself. On macOS the
  watcher asks nothing of the program that copied (the pasteboard server
  already holds what it serves), so a read never waits on it; a stop
  still drops the read in progress at the fence and tells the thread to
  stop, waiting at most 1 s for it and leaving a read still going to end
  on its own, as on Linux; a change is noticed within the watcher's poll
  of the pasteboard's change count (250 ms), which stands in for the
  systems' event notifications, since macOS delivers its own only through
  a run loop Pane's threads do not run. A failure in the listener is
  logged, never with what was copied, and it goes on listening.
- **Across restarts.** The capture state is kept with the history: after a
  restart Pane watches again, before any command opens, only where history
  is on and the package enabled; paused stays paused, disabled stays
  disabled and keeps its history (until it expires).
- **What is kept** ([`clipboard::accept`](../crates/pane-core/src/clipboard.rs)),
  the same on every system:
  - plain text only (Windows `CF_UNICODETEXT`, macOS
    `NSPasteboardTypeString`); a copy with text and other formats keeps
    the text; images, files and rich formats alone keep
    nothing;
  - at most 32 KiB of UTF-8 (`MAX_TEXT_BYTES`); longer text is not kept at
    all rather than cut short;
  - not empty or white space only;
  - not marked by the copying application as not to be kept (below, and
    on Linux never: [the X11 clipboard has no such
    formats](#sensitive-markers), so only an excluded program keeps a
    marked copy out);
  - not copied from an excluded program, matched by the owning process's
    file name, ignoring case, with or without its extension (`KeePass`
    excludes `KeePass.exe`) — where the system names the owner: on
    Windows the process whose window owns the clipboard, on Linux the
    process the owner window's `_NET_WM_PID` names (the file `/proc`
    shows, or the process's name) or the window's `WM_CLASS` (usually the
    program's name), and a window that says neither has an unknown owner.
    On macOS no program can be excluded: the pasteboard never names the
    program that copied, so the owner is always unknown (and, as the
    contract says, an unknown owner is never excluded); at most 64
    programs;
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

**macOS has one, de-facto**: the pasteboard convention of
[nspasteboard.org](https://nspasteboard.org) defines the type
`org.nspasteboard.ConcealedType` for exactly this, and password managers
such as 1Password and Strongbox set it when they copy a secret (which
depends on the application and its version, and was not checked here). Pane's
macOS watcher reads the pasteboard's types before the text and, finding that
one, reports the copy as marked and never reads the text. It is not an
Apple-defined format — nothing obliges an application to set it — and macOS
has no equivalent of Windows' history and cloud answers, so those stay
unset there; the convention also defines
`org.nspasteboard.TransientType` for data not worth keeping (an emoji
panel's), which Pane does not read. No program can be excluded by name on
macOS: the pasteboard never names the program that copied, so every copy's
owner is unknown (as on Windows when a copy has no owner), and only the
concealed type keeps a copy out.

**The X11 clipboard has no such formats**: nothing in its protocol lets an
application mark a copy as not to be kept, so Pane's Linux watcher reports
no markers (there is nothing to read before the text) and keeps every text
an excluded program did not copy. A de-facto `x-kde-passwordManagerHint`
selection target exists that KeePassXC sets and Klipper honors on KDE, but
it is no standard, not every password manager sets it and Pane does not
read it; on Linux, excluding the password manager's program (by the name
its window names) is the only supported way to keep a copy out, and
capture stays local by default all the same.

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
| Observed with | `AddClipboardFormatListener` on a message-only window of a thread of Pane's own (`WM_CLIPBOARDUPDATE`), reading the markers first, then `CF_UNICODETEXT`, and the owner through `GetClipboardOwner`, `GetWindowThreadProcessId` and `QueryFullProcessImageNameW` | the pasteboard's `changeCount`, looked at by a thread of Pane's own every 250 ms (macOS' own notification needs a run loop Pane's threads do not run), reading the types first, then `stringForType(NSPasteboardTypeString)`; the owner never, because the pasteboard does not name it | XFIXES selection events (`XFixesSelectSelectionInput`) on a window of a thread of Pane's own, then a selection transfer to that window (`ConvertSelection`): the text as `UTF8_STRING`, or `STRING` (Latin-1) if the owner refuses that, read at most a little over 32 KiB, in pieces (`INCR`) if the owner sends them; the owner through its window's `_NET_WM_PID` and `/proc`, or its `WM_CLASS` |
| Written back with | `SetClipboardData(CF_UNICODETEXT)` | `clearContents` and `setString:forType:` (`NSPasteboardTypeString`); the pasteboard server keeps it, so nothing of Pane's stays behind to serve it | taking the `CLIPBOARD` selection with a window of Pane's own that serves it to whoever pastes until another program copies, and offering it to the clipboard manager when Pane stops |
| Permission | none | none (macOS 15, the baseline written on; newer systems' pasteboard privacy prompts untested) | none |
| Markers | the four Windows formats ([above](#sensitive-markers)) | the de-facto `org.nspasteboard.ConcealedType` ([above](#sensitive-markers)) | none: X11 has no formats for it, so only an excluded program is kept off |
| Excluded programs | by the owning process's file name | never: the pasteboard names no program, so the owner is always unknown | by the owner window's `_NET_WM_PID` process or `WM_CLASS` name |
| Unavailable | | nowhere so far: no permission and no session kind is refused (a system that cannot watch is explained by the same UI, [Checks](#checks)) | on Wayland ("Not available on Linux with Wayland: … Run Pane in an X11 session") or with no display: the command still opens, its first row says why, turning it on answers the reason, and the other rows work |
| Baseline | Windows 10/11; CI `windows-2025` | macOS 15; CI `macos-15`. Intel Macs, macOS 14 or earlier and 26 or later, and a password manager's own copies have not been run | X11 only; CI `ubuntu-24.04` under Xvfb. No Wayland session (with or without XWayland), real desktop or compositor has been run |

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
  watch, and a launcher without a clipboard. With a clock the tests move
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
- Linux adapter ([`crates/pane-core/tests/clipboard_adapter_linux.rs`](../crates/pane-core/tests/clipboard_adapter_linux.rs),
  Linux only) against the real X11 clipboard, with text only the test puts
  there: plain text reported with the markers default (X11 has none) and
  its owner (the test's own process, which its window's `_NET_WM_PID`
  names), a copy no text can be read from (an image) reported as no text
  with an unknown owner, a written text reported, and nothing once the
  watch is dropped; without a display, `clipboard::native` says why. It
  **replaces what is on the clipboard** and does not put it back, so it
  runs only with `PANE_TEST_REAL_CLIPBOARD=1` and an X11 display, which
  CI's Linux runner gives it under Xvfb; elsewhere it passes without doing
  anything. The pure parts (the session's refusals, Latin-1, the WM_CLASS
  and `/proc` reads) are unit tests that run everywhere Linux builds.
- macOS adapter ([`crates/pane-core/tests/clipboard_adapter_macos.rs`](../crates/pane-core/tests/clipboard_adapter_macos.rs),
  macOS only) against the real pasteboard, with text only the test puts
  there: plain text reported with no marker and no source (the pasteboard
  never names the program that copied, so no excluded program matches), a
  copy marked with `org.nspasteboard.ConcealedType` withheld with its
  text never read, a copy no text can be read from (an image) reported as
  no text, a written text reported, and nothing once the watch is dropped.
  It **replaces what is on the pasteboard** and does not put it back, so
  it runs only with `PANE_TEST_REAL_CLIPBOARD=1`, which CI's macOS runner
  sets; elsewhere it passes without doing anything. The pure part (the
  marker decision) is a unit test that runs everywhere macOS builds.
- Native GUI smokes, screenshots 280 to 285: on Windows (with a data folder
  of its own, copying only its own `pane-smoke-...` text through the
  clipboard API and putting back what was on the clipboard, in memory only)
  off, turned on, kept without the four marked texts, paused, resumed,
  Enter copying an item again, disabled, disabled across a restart,
  enabled and kept again across a restart, with `clipboard-history.json`
  checked at each step; on Linux (280 to 287) the same steps with the
  smoke's own copies typed into root search and copied with Ctrl+A and
  Ctrl+C (through the window's X11 clipboard; Xvfb is the smoke's own
  display, so nothing of the user's is touched), no marked texts (X11 has
  no formats for them), and the copy and the clipboard's survival checked
  by pasting into root search and comparing frames; on macOS (280 to 287,
  #37) the same steps with the smoke's own copies put on the pasteboard
  with AppleScript (`set the clipboard to`), no marked texts (a copy with
  the concealed type is checked by the adapter test instead, since
  AppleScript cannot set a custom type), the copy and the pasteboard's
  survival checked by `pbpaste`, and the copy also by pasting into root
  search and comparing frames. For #36, screenshots 400 to 404 on Windows:
  with Pane stopped, the smoke
  makes one kept item 8 days old and one 2 hours old
  (`scripts/clipboard_history.py`); after the restart the first is gone
  before the command shows anything, then one item is deleted through its
  form, the recent ones through Delete recent items (the last hour), the
  2-hour-old one by keeping items for 1 hour, and the last with Turn off
  and delete, after which a copy is not kept, and the clipboard still
  holds what was copied last. Linux (400 to 406) runs the same steps on
  the history it kept, with the clipboard still holding what was copied
  last checked by pasting (there is no direct clipboard read on Linux;
  Windows reads the clipboard API, which is why its phase checks it after
  each deletion). macOS (400 to 404, #37) runs the same steps with the
  pasteboard still holding what was copied last checked by `pbpaste`, as
  on Windows. See the
  [Windows](platforms/windows.md#clipboard-history-35),
  [macOS](platforms/macos.md#clipboard-history-35) and
  [Linux](platforms/linux.md#clipboard-history-35) notes for where they have
  run.

## Limits

- Windows, Linux (X11) and macOS; on Linux a Wayland session is refused
  rather than relied on XWayland's clipboard bridge, and a session with no
  display at all says so. On macOS a copy is noticed within 250 ms (the
  pasteboard is polled) and the text is read whole, the pasteboard offering
  no shorter read, so a longer text is known only after reading it; only
  the first pasteboard item's text is read (a copy of several files that
  offers their paths as text keeps the first), and no program can be
  excluded, because the pasteboard names none.
- Text only; no images, files or rich text, and no text longer than 32 KiB.
  On Linux only `UTF8_STRING` and `STRING` (Latin-1) are read: a copy
  offered only as `COMPOUND_TEXT` or a `text/plain` MIME target is kept as
  no text, and text is read lossily and ends at its first NUL, as the
  Windows reader's does.
- Detection of sensitive content is only what applications declare
  (markers) and the programs the user excludes; on Linux, where nothing
  can be declared, only an excluded program can; the owning process can be
  a helper or unknown, on Linux a window that names neither a process
  nor a class (the window's own clipboard server, say) has an unknown
  owner, and on macOS the owner is always unknown.
- The retention's default (7 days) and choices are provisional. Expiry
  follows the system's time: an item copied while the time was set far
  ahead is kept until then, and setting the time back keeps items longer.
- Paste is not available on any system yet (#125 brings it to Windows),
  so Enter copies the item and says so; the native smokes still drive
  #36's form and need updating to the actions (#150 did not run them).
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
