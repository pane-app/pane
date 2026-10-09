# Clipboard history

Added for [#35](https://github.com/pane-app/pane/issues/35) (Windows):
US65, US66, US70, US71; T10, T21, T22; contributions to G5 and G7, not
claims that they pass. Pane keeps the text the user copies on this
computer, and the **Clipboard History** default extension lists it, newest
first; Enter on an item pastes it, and its other actions copy or delete it
(#150). Since [#166](https://github.com/pane-app/pane/issues/166) it
records **from the first start**, with no step to turn it on
([ADR 0042](adr/0042-clipboard-history-records-from-the-first-start.md),
amending ADR 0020): recording can be paused and resumed, disabling the
extension stops all observation, copies an application marks as concealed
are skipped as before, and the user can name **Disabled Applications**
whose copies are never recorded. Its view looks like Raycast's: a type
dropdown at the search field's right, the records grouped by day, and the
selected record's Information beside them; its controls are in the Actions
panel and on the extension's Settings page. Since
[#167](https://github.com/pane-app/pane/issues/167) it keeps **copied
images and files** as well as text, under the same rules: an image as a PNG
of at most 10 MiB in the history's own folder, files as their paths; rows
show an image's thumbnail or a file's system icon, and Paste and Copy put
back the same kind ([What is kept](#images-and-files)).
[#36](https://github.com/pane-app/pane/issues/36) added expiry and the
remaining deletion controls (US67, US68, US69; T10, T21; G5, again
contributions): items are kept for 7 days unless the user chooses another
time, and Pane deletes them then, whether the extension runs or not
([Expiry](#expiry)); an item, the recent ones, all of them (Clear), or all
of them with history turned off can be deleted ([Deleting](#deleting)).
[#38](https://github.com/pane-app/pane/issues/38) added the Linux (X11)
adapter and [#37](https://github.com/pane-app/pane/issues/37) the macOS
(pasteboard) adapter, so the package declares Windows, macOS and Linux,
the three systems with an adapter. The architecture is recorded in
[ADR 0020](adr/0020-host-keeps-clipboard-history-for-an-extension.md),
for expiry [ADR 0023](adr/0023-host-expires-clipboard-history-by-its-own-clock.md)
(both proposed), and for recording from the first start
[ADR 0042](adr/0042-clipboard-history-records-from-the-first-start.md).

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
  its command, "Clipboard History", which Pane draws in its own split view
  ([Behavior](#behavior)); its own list (what a copy installed from
  another source shows) is Pause or Resume Recording and the kept items.
  Its `pane.json` declares the three preferences its Settings page shows
  ([Settings](#settings)).
  It declares the three systems with an adapter
  (`"platforms": ["windows", "macos", "linux"]`), so its command runs
  wherever Pane runs. Rust commands use the import through
  `pane_extension::clipboard_history`; JavaScript and TypeScript commands import
  it when their package.json sets `"pane": { "clipboardHistory": true }`
  ([`guests/js/clipboard.d.ts`](../guests/js/clipboard.d.ts)), and only
  then, as for `files`. The samples
  [`sample-clipboard-js`](../guests/sample-clipboard-js) and
  [`sample-clipboard-ts`](../guests/sample-clipboard-ts) implement the
  whole contract's controls in their own lists (turning on, retention,
  exclusions, clearing, recent deletion, turning off and deleting) in
  JavaScript and TypeScript, and the launcher tests run the contract's
  checks on both; a sample's history starts off, as every package's but
  Pane's own does.
- **System adapter** behind one small trait
  ([`pane_core::clipboard`](../crates/pane-core/src/clipboard.rs)), chosen
  by `clipboard::native()`: the Windows listener, the Linux watcher of the
  X11 `CLIPBOARD` selection ([`linux.rs`](../crates/pane-core/src/clipboard/linux.rs)),
  the macOS watcher of the pasteboard's change count
  ([`macos.rs`](../crates/pane-core/src/clipboard/macos.rs)),
  or on any other system one that says clipboard history is unavailable
  there.

Acquiring the package automatically at setup is
[#51](https://github.com/pane-app/pane/issues/51) to
[#53](https://github.com/pane-app/pane/issues/53); until then it is
installed from its folder (`pane --install target/guests/packages/clipboard-history`).

## Behavior

Pane's own Clipboard History opens in its split view (#102), as
Raycast's does (#166):

- **The header**: the back button, the search field ("Type to filter
  entries…", matching the text and the program it was copied from), with
  no badge or chip on it, and a **type dropdown** at its right: All Types,
  Text, Images, Files, Links, Colors. Links and colours are text Pane
  recognizes as one URL (`https://…`, `mailto:…`, `www.…`) or one colour
  value (`#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `hsl(…)`), the whole
  text trimmed; Text keeps them too. Images and Files keep the copied
  images and files (#167), which are not text.
- **The list**: the kept records, newest first, grouped by local day:
  Today, Yesterday, then each earlier day by its date ("Thursday, Oct 1";
  "Wednesday, Dec 31, 2025" for another year). A row shows the first line
  of text and the time (today's and yesterday's as 14:02, the week
  before's as Thu, older ones as Sep 28); an image's row shows its
  thumbnail and is titled "Image (1920×1080)", a files row shows the
  system's icon of the first file and is titled by its name, "+2" for two
  more. Only the rows in view ask for their icons.
- **The detail**: the selected record as it was copied — its text, its
  image fit in the card, or its files, one line each with its icon, name
  and folder — over its **Information**: Source (the program's name, and
  its icon where the system gave its path, as Windows does), Type (Text,
  Link, Color, Image, File), Characters (text) or Dimensions (an image,
  "1920×1080"), and Copied ("Today at 14:02", "Yesterday at 23:59",
  "Thursday at 09:00", "Sep 28 at 16:12").
- **The footer**: the command's chip on the left (or the outcome of what
  was just done, until the user moves on), then Paste and Actions.

Its keys are the kept items' actions (#150): **Enter** (the footer's
Paste) pastes the selected record into the application in front through
Pane's system, closing the window, or where Pane cannot paste yet copies
it through the history's own copy and shows "Copied — paste is not
available here yet"; **Ctrl+Enter** (Copy) copies it again, closes the
window and shows "Copied to Clipboard" in a HUD, as every Copy action
does; **Ctrl+D** (Delete) deletes it. **Ctrl+K** (the footer's Actions)
opens the Actions panel: the record's Paste, Copy to Clipboard and Delete
Entry; Pause Recording (or Resume Recording) and Clear History…, which
asks first ("Clear Clipboard History?") and deletes every record while
recording goes on; Keep History For (1 Hour, 1 Day, 7 Days, 30 Days, 90
Days, the one in force marked current); and Disabled Applications…, which
opens the extension's page in Settings. Each revalidates the reading
first (`Launcher::paste_clipboard_record`, `copy_clipboard_record`,
`delete_clipboard_record`, `set_clipboard_capture`,
`set_clipboard_retention`, `clear_clipboard_history`). The command's own
management list (turn on, retention and exclusion forms, clearing rows) is
gone. Copy puts an image back as an image and files as files
(`ClipboardSystem::write_image`, `write_files`); Paste pastes text and a
single file through Pane's system, and copies an image or several files
as what they are instead (saying so as where Pane cannot paste), since the
system's paste takes text or one file.

## Settings

The extension's page in Settings shows three preferences, which its
`pane.json` declares and Pane's [preferences](../crates/pane-core/src/preferences.rs)
controls draw, but whose values are the history's own state, read from it
and written to it by the host (`launcher::clipboard_settings`), so the
page, the Actions panel and what the host honours never disagree:

| Preference | Control | Is |
| --- | --- | --- |
| Keep History For (`keepHistoryFor`) | a select of 1 Hour, 1 Day, 7 Days, 30 Days, 90 Days (a dropdown preference, as every one is drawn) | the retention ([Expiry](#expiry)) |
| Recording (`pauseRecording`) | a switch, "Pause Recording" | paused (or off) while on; recording while off |
| Disabled Applications (`disabledApplications`) | the applications' file names, separated by commas, and "Add…", the system's application picker, which adds the chosen application's file name | the programs whose copies are not recorded ([What is kept](#behavior)) |

`applications` is a preference type of its own (#166): a list of
applications by their file names. The page's card also has **Clear
history**, an operation of the extension (the extension list's
`clear-clipboard-history:<identity key>` row), which asks first, as
clearing a cache does, and deletes every kept item. Only Pane's registered
Clipboard History is so: another package declaring preferences of these
names keeps them as settings. A disabled extension's history cannot be
changed from its page until it is enabled again.

- **Recording from the first start.** Pane's own Clipboard History records
  as soon as it is installed and enabled: while `clipboard-history.json`
  holds no history for it, it is on, and Pane watches the clipboard. Any
  other package that uses the capability, and a copy of this package
  installed from another source, starts off and records once it is turned
  on (`set-capture`). Pausing it stops the watch at once; paused, it stays
  paused across restarts (the file says so), and so does a history turned
  off through the contract. Uninstalling it and deleting its saved data
  forgets that, so a reinstall records again.
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
  - plain text (Windows `CF_UNICODETEXT`, macOS
    `NSPasteboardTypeString`); a copy with text and other formats keeps
    the text; rich formats alone keep nothing; and, for Pane's own
    Clipboard History only, a copied image or copied files
    ([below](#images-and-files)) — every other package keeps plain text
    only, as `wit/clipboard.wit` says;
  - at most 32 KiB of UTF-8 (`MAX_TEXT_BYTES`); longer text is not kept at
    all rather than cut short;
  - not empty or white space only;
  - not marked by the copying application as not to be kept (below, and
    on Linux never: [the X11 clipboard has no such
    formats](#sensitive-markers), so only an excluded program keeps a
    marked copy out);
  - not copied from a disabled application (the contract's excluded
    programs), matched by the owning program's file name, ignoring case,
    with or without its extension (`KeePass` excludes `KeePass.exe`) —
    where the system names the owner: on Windows the program of the
    process whose window owns the clipboard (Pane keeps its full path,
    which gives the Information its icon; a guest is given its file
    name), on Linux the
    process the owner window's `_NET_WM_PID` names (the file `/proc`
    shows, or the process's name) or the window's `WM_CLASS` (usually the
    program's name), and a window that says neither has an unknown owner.
    On macOS no program can be excluded: the pasteboard never names the
    program that copied, so the owner is always unknown (and, as the
    contract says, an unknown owner is never excluded); at most 64
    programs;
  - one item per text, image (by its PNG) or list of files: copying a
    kept one again moves it to the front with its new time;
  - at most 100 items per package (`MAX_ITEMS`); beyond that the oldest go.
    This bounds the file; the retention ([Expiry](#expiry)) bounds how long.
- **Local only.** The history stays in Pane's data folder; Pane sends none
  of it anywhere and no other extension can read it through Pane (only the
  package that keeps it). This is not a boundary against trusted extensions
  or other programs running as the user
  ([policy](extension-policy-proposal.md#clipboard-history)).

### Images and files

Pane's own Clipboard History keeps what is copied as an image or as files
too ([#167](https://github.com/pane-app/pane/issues/167)), under the same
markers, disabled applications, pause and expiry as text
(`clipboard::accept_any`; a package keeping history through the contract
keeps text only, `clipboard::accept`):

- **What a copy is.** Where a copy holds several, files come first, then
  text, then an image: copying files often puts their names beside them as
  text, and copying text in an office application often puts a picture of
  it beside it.
- **Files** (Windows `CF_HDROP`, every path; macOS every pasteboard item's
  `public.file-url`; Linux `text/uri-list`'s `file://` URIs of this
  computer): kept as their absolute paths, in the order copied, at most
  1000 (`MAX_FILES`); a larger selection is not kept. The item's text is
  the paths, one per line, which a search matches and the contract's
  `entries` gives.
- **An image** (Windows the registered `PNG` format as it is, else the
  bitmap `CF_DIBV5` or `CF_DIB` of 24 or 32 bits a pixel made into a PNG;
  macOS `public.png`, else `public.tiff` made into a PNG; Linux
  `image/png`): kept as a PNG of at most 10 MiB (`MAX_IMAGE_BYTES`,
  proposed default); a larger one is not kept, and a bitmap of more than
  7680×4320 pixels is not even read. Its item names the PNG by its SHA-256
  and keeps its width and height; its text is its title, "Image (W×H)".
- **Where the PNG is.** In the history's own folder,
  `clipboard-images/<owner>/<sha256>.png` beside `clipboard-history.json`,
  readable by the user only as the file is (ADR 0020's place for the
  history, its saved data). It is written before its item. Once an item
  holding an image goes, however it goes — expired by the host's clock (ADR
  0023), deleted, cleared, dropped past 100 items, or its history removed
  with the package's saved data — the next write of the file deletes the
  PNGs no item names any more, so the image goes with its item; a write
  that took no image away does not look at the folder (#192). One left
  behind by a stop between the PNG and its item goes when Pane next
  starts.
- **Put back as what it was.** Copy writes an image as an image (Windows:
  the PNG and a 32-bit `CF_DIB`; macOS: `public.png` and `public.tiff`;
  Linux: `image/png`) and files as files (Windows: `CF_HDROP` with
  `Preferred DropEffect` copy; macOS: one file URL per pasteboard item, as
  Finder; Linux: `text/uri-list`, `x-special/gnome-copied-files` and the
  paths as text).

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
- **Configurable and finite.** The Actions panel and the Settings page
  offer 1 hour, 1 day, 7 days, 30 days and 90 days; the host accepts any
  time from 1 minute to 365 days
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
| Delete Entry (Ctrl+D, or the Actions panel) | that item | history stays as it was |
| A sample's "Delete recent items" (`delete-items`) | the items copied in the last 15 minutes, hour or day | history stays as it was |
| Clear History (the Actions panel, or the Settings page's Clear history), once confirmed | every item | recording stays as it was: what is copied next is kept |
| A sample's "Turn off and delete clipboard history" (`turn-off-and-clear`) | every item | history is off: nothing more is kept, also after a restart, until it is turned on |
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
  (milliseconds since the Unix epoch) and `source`, and an image's `image`
  (`width`, `height`, `bytes`, `digest`) or files' `files` (#167; the PNGs
  are in `clipboard-images/`, [above](#images-and-files)). A package whose items
  all went and that has no choices left keeps only its `nextId`, so its
  ids are never given twice (it counts as keeping nothing). A
  `retentionSeconds` outside 1 minute to 365 days, as only an edited file
  can hold, is taken as the nearest bound.
- On Windows each item's text and files are encrypted on disk
  ([#130](https://github.com/hoangvu12/pane/issues/130)) with the same
  DPAPI protector as [local credentials](extension-data.md#protected-credentials),
  for the current Windows user: the file is version 2, and an item holds
  `"protected": {"dpapi": "<base64>"}` in place of `text` and `files`. Its
  `id`, `copiedAt`, `source` and an image's `image` stay readable, so items
  expire, are deleted and are counted without decrypting anything, and an
  image's PNG goes with its item; the PNGs themselves are not encrypted
  (readable by the user only, as before). An item is encrypted once, when it
  is first written, and later writes reuse those bytes. An item Windows
  cannot encrypt is never written as it is: it is left out of that write
  (the log says so), kept in memory, and tried again at the next write,
  while the rest of the change, deletions included, is written. A version-1
  file is converted when Pane starts, every item kept, in one atomic write;
  if that write fails, Pane reads it as it is and tries again at the next
  start. An older Pane refuses a version-2 file and never overwrites it. An
  item that
  cannot be decrypted (the user's password reset by an administrator, a
  folder from another user or computer, damaged bytes) is listed in its
  place as "Pane cannot read this copy on this computer: Windows could not
  decrypt it (<reason>)", is neither copied nor pasted, and is kept as it
  was until it expires or is deleted; the others still read. Programs
  running as the same user can decrypt the file as Pane does. On macOS and
  Linux the file stays version 1, as before.
- It is written outside the lock that captures and commands share, so a
  copy never waits on another's write, compactly (one line, not
  pretty-printed), and replaced atomically, so it holds the history before
  or after a change, never a torn one. Writes are batched (#192): what a
  copy keeps, and what expires, is written by the history's own thread
  500 ms after the first change not written yet (`WRITE_DELAY`, proposed),
  together with every change made meanwhile, so a burst of copies is one
  write. A write that fails is reported in Pane's log and tried again by
  that thread, 500 ms later, then after a pause that doubles with each
  failure in a row, at most a minute, so a change is not left only in
  memory; should that thread end (it panicked), every change is written at
  once again. A change a command or the view makes (deleting, clearing, pausing
  or resuming, a retention, the disabled applications) and the removal of
  a package's history are written before the call returns, with whatever
  waited, since a failure to write is reported to them. A clean quit (the
  tray's or menu bar's Quit, closing the launcher's window) and the system
  ending the session (`WM_ENDSESSION` on Windows, the termination
  notification on macOS, SIGTERM, SIGHUP or SIGINT on Linux and macOS)
  write what waits first, and so does counting the saved data for an
  uninstall. A signal gives that write at most 2 seconds before Pane ends
  ([pausing](pausing.md)). **A copy made less than 500 ms before a crash
  can be lost**, and so can one made just before SIGKILL or a signal whose
  clean quit did not end within those 2 seconds; what was written before
  stays.
- The Clipboard History view reads shared records (#192): the launcher
  makes them from the history once per change of it — a copy kept, a
  deletion, a choice changed, an expiry, each counted by the history — and
  every frame the window draws and the Actions panel read the same records
  until the next change, copying nothing of the history.
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
| Observed with | `AddClipboardFormatListener` on a message-only window of a thread of Pane's own (`WM_CLIPBOARDUPDATE`), reading the markers first, then `CF_HDROP` (files), `CF_UNICODETEXT`, or the registered `PNG`, `CF_DIBV5` or `CF_DIB` (an image), and the owner through `GetClipboardOwner`, `GetWindowThreadProcessId` and `QueryFullProcessImageNameW` | the pasteboard's `changeCount`, looked at by a thread of Pane's own every 250 ms (macOS' own notification needs a run loop Pane's threads do not run), reading the types first, then every item's `public.file-url` (files), `stringForType(NSPasteboardTypeString)`, or `public.png` or `public.tiff` (an image); the owner never, because the pasteboard does not name it | XFIXES selection events (`XFixesSelectSelectionInput`) on a window of a thread of Pane's own, then selection transfers to that window (`ConvertSelection`): what the owner offers (`TARGETS`), then `text/uri-list` (files), the text as `UTF8_STRING`, or `STRING` (Latin-1) if the owner refuses that, read at most a little over 32 KiB, or `image/png`, read at most a little over 10 MiB, in pieces (`INCR`) if the owner sends them; the owner through its window's `_NET_WM_PID` and `/proc`, or its `WM_CLASS` |
| Written back with | `SetClipboardData(CF_UNICODETEXT)`; an image as `PNG` and `CF_DIB`, files as `CF_HDROP` | `clearContents` and `setString:forType:` (`NSPasteboardTypeString`); an image as `public.png` and `public.tiff`, files as one file-URL item each (`writeObjects:`); the pasteboard server keeps it, so nothing of Pane's stays behind to serve it | taking the `CLIPBOARD` selection with a window of Pane's own that serves it (an image as `image/png`; files as `text/uri-list`, `x-special/gnome-copied-files` and text) to whoever pastes until another program copies, and offering it to the clipboard manager when Pane stops |
| Permission | none | none (macOS 15, the baseline written on; newer systems' pasteboard privacy prompts untested) | none |
| Markers | the four Windows formats ([above](#sensitive-markers)) | the de-facto `org.nspasteboard.ConcealedType` ([above](#sensitive-markers)) | none: X11 has no formats for it, so only an excluded program is kept off |
| Excluded programs | by the owning process's file name | never: the pasteboard names no program, so the owner is always unknown | by the owner window's `_NET_WM_PID` process or `WM_CLASS` name |
| Unavailable | | nowhere so far: no permission and no session kind is refused (a system that cannot watch is explained by the same UI, [Checks](#checks)) | on Wayland ("Not available on Linux with Wayland: … Run Pane in an X11 session") or with no display: the command still opens, its first row says why, turning it on answers the reason, and the other rows work |
| Baseline | Windows 10/11; CI `windows-2025` | macOS 15; CI `macos-15`. Intel Macs, macOS 14 or earlier and 26 or later, and a password manager's own copies have not been run | X11 only; CI `ubuntu-24.04` under Xvfb. No Wayland session (with or without XWayland), real desktop or compositor has been run |

## Checks

- Protection on disk (#130): unit tests in
  [`history.rs`](../crates/pane-core/src/clipboard/history.rs) for an item
  written as the system protects it and encrypted once, an item that cannot
  be decrypted explained and kept as it was, a version-1 file converted at
  start (Windows) and a version-3 file refused and kept; and
  [`clipboard.rs`](../crates/pane-core/tests/clipboard.rs)'s
  `the_history_is_encrypted_on_disk_and_a_damaged_item_is_explained`
  (Windows, with the recording clipboard): kept items read back, also after
  a restart, with no plain text in the file; an earlier file converted at
  start with every item kept; a damaged item listed as its explanation while
  the others still read, and kept by a later write. The Windows smoke reads
  the kept texts through `scripts/clipboard_history.py`, which decrypts them
  as the same user.
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
  sleeping or polling, and for a batched write by the store's word that
  nothing waits to be written (`Launcher::wait_for_clipboard_writes`).
- Batched writes (#192): the history store's unit tests for a burst of
  copies written once and read back by another store, a flush writing what
  waits at once, the file written compactly, each item encrypted once over
  several writes, and the images' folder pruned only when an image item
  was deleted or expired (a PNG no item names, left there by the test,
  shows which writes pruned). Through the launcher with the fake clipboard
  ([`clipboard_view.rs`](../crates/pane-core/tests/clipboard_view.rs)):
  several copies within the delay written once and all read back after a
  restart; a clean quit writing the copy that waited; an image's PNG
  deleted when its item is deleted or expires and not otherwise; on
  Windows, items encrypted once each, however many writes follow (a test
  hook counts them), and no copied text in the clear. The sample suites
  ([`clipboard.rs`](../crates/pane-core/tests/clipboard.rs)) read the file
  once the batch was written and restart after a clean quit. A window
  test ([`window.rs`](../crates/pane/tests/window.rs)) draws the view again
  after real key presses with nothing changed and the records are not made
  again (a test hook counts them), while moving, searching and Ctrl+D
  still work and a copy or a deletion makes them once more.
- Pane's own Clipboard History through the launcher
  ([`crates/pane-core/tests/clipboard_view.rs`](../crates/pane-core/tests/clipboard_view.rs),
  #166), acquired as the default extension over a fake system clipboard:
  a fresh data folder records the first copy with no turn-on, disabling
  stops it, and paused it stays paused across a restart; concealed copies
  and copies from a disabled application (by its file name or its path)
  are not recorded; the Settings page's preferences read and change the
  history (pause, keep for 1 hour, disabled applications) and its Clear
  history row asks, then clears; the Actions panel's entries, Clear
  History asking first (dismissed, nothing changes); the type dropdown's
  filters, the day groups with their dates and the Information. For #167:
  a copied image and copied files kept beside text and listed with their
  kinds and titles, the image's PNG in `clipboard-images/`, a disabled
  application's image and oversized copies not kept, Copy putting each
  back as what it was, Paste copying an image instead, and a deleted or
  expired image's PNG deleted. The history store's unit tests: Pane's own
  history on while the file holds none, and off or paused kept so; an
  image kept beside the file by its digest, once, and deleted with its
  item however it goes (expired, deleted, cleared, removed, or left
  behind); files by their paths. The capture rules' unit tests: images and
  files kept under the rules of text by `accept_any` only, oversized ones
  skipped, a PNG's size read from its header. The window tests
  ([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)): no
  badge or tabs, the dropdown filtering by type (Images and Files too),
  the day sections and the Information, an image's thumbnail row, preview
  and Dimensions, a files row's icon and preview, Copy putting each back,
  the Actions panel pausing, resuming, copying, deleting and clearing
  (after the confirmation).
- Launcher public interface ([`crates/pane-core/tests/clipboard.rs`](../crates/pane-core/tests/clipboard.rs)),
  with the JavaScript and TypeScript samples, and a fake system clipboard
  (a copy of each package declaring every system), and the Rust package's
  own list (a copy: Resume Recording, its items, Pause Recording): for the
  samples, nothing watched or kept until turned on, then
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
  there: plain text reported with its owner (the test's own program, by
  its full path), each
  of the four markers read and withholding the text, `CanIncludeInClipboardHistory`
  1 allowing it, a written text reported, a bitmap alone (as Paint copies
  one) read as an image and Pane's written image and files read back as
  what they are (#167; the bitmap and drop-list conversions are unit tests
  in `windows.rs`), and nothing once the watch is
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
  names), a copy no text can be read from (bytes that are no PNG) reported
  as no text with an unknown owner, a PNG as an image and a
  `text/uri-list` as files (#167), a written text, image and files
  reported, and nothing once the
  watch is dropped; without a display, `clipboard::native` says why. It
  **replaces what is on the clipboard** and does not put it back, so it
  runs only with `PANE_TEST_REAL_CLIPBOARD=1` and an X11 display, which
  CI's Linux runner gives it under Xvfb; elsewhere it passes without doing
  anything. The pure parts (the session's refusals, Latin-1, the WM_CLASS
  and `/proc` reads, the URI list and the targets) are unit tests that run
  everywhere Linux builds.
- macOS adapter ([`crates/pane-core/tests/clipboard_adapter_macos.rs`](../crates/pane-core/tests/clipboard_adapter_macos.rs),
  macOS only) against the real pasteboard, with text only the test puts
  there: plain text reported with no marker and no source (the pasteboard
  never names the program that copied, so no excluded program matches), a
  copy marked with `org.nspasteboard.ConcealedType` withheld with its
  text never read, a copy no text can be read from (bytes that are no
  PNG) reported as no text, a PNG as an image (#167), a written text,
  image and files reported as what they are, and nothing once the watch
  is dropped.
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
  the first pasteboard item's text is read (copied files are read from
  every item's file URL instead, #167), and no program can be excluded,
  because the pasteboard names none.
- No rich text, no text longer than 32 KiB, no image whose PNG is larger
  than 10 MiB and no more than 1000 files in one copy. Images and files are
  Pane's own Clipboard History's: the contract (`wit/clipboard.wit`) gives
  other packages text only, and gives Pane's own an image's title and
  files' paths as their text. An image is put back as a PNG and a bitmap
  (Windows), a PNG and a TIFF (macOS) or a PNG alone (Linux, served whole:
  an X server without BIG-REQUESTS may refuse a large one); an image
  copied as a format other than these (a Windows metafile, a macOS PDF, a
  Linux `image/jpeg` alone, a bitmap of fewer than 24 bits a pixel or
  compressed) is not kept. Pasting an image or several files copies them
  instead, the system's paste taking text or one file; a file copied
  without its icon known yet shows a neutral placeholder until the system
  gives it (#142). On macOS a copied image is read whole before its size is
  known; nothing of the image is kept beyond its PNG (no OCR, which Raycast
  has).
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
  the turn-on row and #36's forms, which #166 removed from Pane's own
  Clipboard History: they need updating to its recording from the first
  start and its Actions panel (neither #150 nor #166 ran them).
- Recording from the first start is Pane's own Clipboard History's alone;
  the contract's documentation (`wit/clipboard.wit`,
  `guests/js/clipboard.d.ts`) still says every package starts off, which
  holds for every other package.
- A link or colour is recognized from the whole text only: a sentence
  holding a URL is text.
- A disabled package's history cannot be deleted without enabling it
  (uninstalling, or its expiry, can); a retained one has Delete retained
  data.
- A sample's rows are read when its command opens; they do not change
  while it is open, even as text is copied. Pane's own view looks again
  every second.
- More than one package may keep history; each keeps its own, and Pane
  watches once for all of them.
- The files are replaced atomically but not locked (as every kind of
  extension data): two Pane processes on one data folder can lose each
  other's last write.
