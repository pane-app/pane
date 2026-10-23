# Extension data

Extensions save values through Pane, and Pane decides what happens to those
values when the user clears an extension's cache, uninstalls it, or deletes
what an uninstall left behind. This page is for extension authors who save
data and for anyone who wants to know what Pane keeps and what it deletes.

Pane does all of this itself, without running the extension. Clearing the
cache follows the accepted
[Clear cache](extension-policy-proposal.md#disable-cache-data-and-uninstall)
behavior: it removes disposable data and keeps settings, user content and
credentials. [Uninstalling](#uninstalling-an-extension) offers a choice to
keep or delete the saved data, and the user can
[delete retained data](#deleting-retained-data) afterwards.

Tracking: added for [#39](https://github.com/pane-app/pane/issues/39) (US60,
US64, T20, G5; contributions, not whole-gate claims). Uninstalling came with
[#40](https://github.com/pane-app/pane/issues/40) and deleting retained data
with [#41](https://github.com/pane-app/pane/issues/41).

## Four kinds of data

An installed package's commands keep string values by key through Pane.
There is one `pane:extension` interface per kind, and each has the same
`get` and `set`. The kind is what decides what Pane's management actions do
with a value.

- WIT: [`wit/data.wit`](../wit/data.wit), world `extension-with-data`.
- Rust: `pane_extension::{settings, content, cache, credentials}`.
- JavaScript and TypeScript: the modules `pane:extension/<kind>@0.2.0`,
  typed in [`guests/js/data.d.ts`](../guests/js/data.d.ts).

Clipboard history is saved data too, but Pane keeps it differently
([below](#clipboard-history)).

| Kind | What it is for | File in `extensions/` | Clear cache | Uninstall |
|---|---|---|---|---|
| Settings | The user's choices for the extension | `settings.json` | kept | the user's choice |
| Content | The extension's own durable records, such as notes or history | `content.json` | kept | the user's choice |
| Cache | Values the extension can compute or download again | `cache.json` | removed | removed |
| Local credentials (`credentials`) | Secrets kept on this computer, such as a sign-in token | `credentials.json` | kept | removed |
| Clipboard history (`clipboard-history`, since #35) | The text the user copied while the package kept [clipboard history](clipboard-history.md), and whether it keeps it | `clipboard-history.json`; the images' PNGs in `clipboard-images/` | kept | the user's choice |

`credentials.json` is readable only by the user. On macOS and Linux that
means mode 0600. On Windows the file has a protected DACL for the user and
SYSTEM only, and each value is encrypted with DPAPI
([below](#protected-credentials)).

### Clipboard history

Clipboard history is not a `get`/`set` interface. Pane itself watches the
clipboard and writes the items for the package, and only Pane writes them.
The package reads and controls them through
`pane:extension/clipboard-history`
([`wit/clipboard.wit`](../wit/clipboard.wit)). For Pane's own Clipboard
History the file also holds copied images and files (#167).

`clipboard-history.json` is typed and versioned, not key-value. Like
`credentials.json`, it is readable only by the user, and on Windows each
item's text and files are encrypted as credentials are. The images' PNGs in
`clipboard-images/` are readable only by the user but not encrypted.

Clipboard history counts as saved data together with settings and content
("Saved data: 1 setting and 12 clipboard history items"). Its items expire
(#36). Pane deletes each item once the package's retention has passed since
it was copied, without running the package. It does this also while the
package is disabled or uninstalled with its data kept, and before counting
or showing anything after downtime, so these counts never include an
expired item ([expiry](clipboard-history.md#expiry)).

### The settings sample

The settings sample in [Rust](../guests/sample-settings/src/lib.rs),
[JavaScript](../guests/sample-settings-js/src/index.js) and
[TypeScript](../guests/sample-settings-ts/src/index.ts) keeps one value of
each kind: its greeting style, a note ("Save a note"), the last greeting
("Greet me") and a token ("Sign in"). "Show what Pane keeps" answers with
all four.

## Ownership

- Pane owns the files. Each file holds every installed package's values of
  its kind under the package identity's key. Values belong to the source
  identity, never to the title or the managed copy, so two copies with the
  same title have separate data, and an update keeps it.
- Nothing is kept for a command built into Pane: `get` and `set` return an
  error.
- The extension owns its keys and the meaning of its values. Pane never
  reads them except to hand them back.
- Everything is kept while the package is disabled or updated, and while
  Pane is not running. A disabled package cannot `set` any kind.
- The files live next to `installed.json` in Pane's data folder. Pane's own
  compile cache (`cache_dir`) is a different thing: it holds compiled
  components keyed by their bytes, Pane manages it itself, and Clear cache
  does not touch it. An extension's cache is its data, owned by its
  identity like the other kinds. It is only disposable.
- Nothing outside these files is Pane's to delete: the package's source
  folder, files an extension wrote elsewhere, remote accounts and sessions.

## Migration

- Pane migrates the files' format. Each file has a `version`, which is 1,
  except that on Windows `credentials.json` is version 2
  ([below](#protected-credentials)). A file of another version, or one that
  cannot be read, is reported to the extension by `get` and `set` and never
  overwritten. An older Pane refuses a version-2 credentials file the same
  way.
- The extension migrates its own values, in its own code, when their
  meaning changes between its versions. Pane keeps values across an update
  and does not transform them.
- A cache needs no migration. The user can clear it at any time, so an
  extension should treat a cache value it cannot use as missing and make it
  again.

## Protected credentials

Extension data has one credential protector per system, between the values
in memory and the file. Values stay plain in memory while Pane runs. Only
the file changes: `credentials.get` and `credentials.set` are unchanged, so
no extension is rebuilt.

| System | How `credentials.json` holds a value |
|---|---|
| Windows | Encrypted with DPAPI (`CryptProtectData`) for the current Windows user, with fixed entropy of Pane's own and the flag that forbids any prompt; the file also keeps its protected DACL |
| macOS | As it is, in a file only the user can read (mode 0600). The macOS Keychain is a later slice of #129, waiting for Pane to be signed |
| Linux | As it is, in a file only the user can read (mode 0600). The Secret Service is a later slice of #129 |

The Windows Credential Manager is not used.

Tracking: added for [#130](https://github.com/hoangvu12/pane/issues/130),
the first slice of [#129](https://github.com/hoangvu12/pane/issues/129).

### The file on Windows

On Windows `credentials.json` is version 2, and each value records how it
is protected:

```json
{"version": 2, "packages": {"local:/…": {"token": {"dpapi": "AQAAANCMnd8B…"}}}}
```

A value written as it is reads `{"plain": "…"}`.

Only values are protected. Package identities and keys stay readable, so
Clear cache, uninstall, deleting retained data, and the counts on Manage
extensions and the uninstall confirmation all work without decrypting
anything.

On macOS and Linux the file stays version 1, as before. The other kinds of
extension data stay at version 1 everywhere.

### Converting an older file

When Pane starts on Windows, it converts a version-1 file (as an earlier
Pane wrote it) to version 2, with every value protected, in one atomic
write. If that write fails, Pane keeps reading the version-1 file and tries
again at the next start. No value is lost.

A later write of the file, such as an uninstall or a deletion, reads the
file again first and protects the values it still holds as they are before
writing it. If Windows could encrypt none of them, the file is written as
version 1 again, never as plain values in a version-2 file.

An older Pane refuses a version-2 file, as it refuses any other version,
and never overwrites it.

### A value that cannot be decrypted

A value can become unreadable when an administrator reset the Windows
password (so the user's DPAPI key was lost), when the data folder came from
another user or computer, or when the bytes were damaged. Pane explains
such a value. It never drops or regenerates it.

- `credentials.get` returns an error the extension can turn into "sign in
  again": "Pane cannot read this credential on this computer: Windows could
  not decrypt it (<reason>). Sign in again."
- `credentials.set` replaces it.
- Pane keeps it as it was through every other write, and the package's
  other values still read.
- Manage extensions counts it, as in "keeps 2 credentials, 1 unreadable".
  The counts themselves need no decryption. Only the row that describes an
  identity tries that identity's values, to say which cannot be read.
  Confirmations count without decrypting anything.
- A password preference that cannot be read is as if it was never set, so
  its Settings page asks for it again.

### What the protection covers

It protects against another user of the computer, a copy of the file (a
backup, a synced or roamed profile, a disk read outside Windows) and an
administrator reading the file without the user's sign-in.

It does not protect against programs running as the same user. They can
call DPAPI too, and Pane's entropy is in its program. The
[data policy](extension-policy-proposal.md#disable-cache-data-and-uninstall)'s
"lifecycle behavior, not secret isolation from trusted code" stands.

Clipboard history gets the same protector on Windows. Each item's text and
files are encrypted, and the file moves to version 2, converted at start in
the same way ([clipboard
history](clipboard-history.md#ownership-and-deletion)).

## Clearing an extension's cache

Settings › Extensions has a row "Clear cache of <title>" for each package,
after the enable/disable rows and in the same order. Its subtitle names the
source, so copies with the same title can be told apart.

Choosing the row asks first. The confirmation shows "Clear the cache of
<title>?", the source, and "Pane deletes the data this extension keeps as
its cache. Its settings, content and credentials are kept, and the
extension does not run."

Choosing Clear cache deletes that identity's entry from `cache.json` and
returns to the list with "Cleared the cache of <title>". Cancel or Esc
returns without deleting. Pane reads `cache.json` again just before, so
values another Pane process saved meanwhile are kept.

No guest runs. The package may be disabled, its component may not load, and
Pane's runtime may not have started. A running instance of the package, if
there is one, is left as it is. The next `cache.get` returns nothing, but
the instance may save a value it still holds, so the outcome then reads
"Cleared the cache of <title>. A running instance may write it again until
it stops."

Other identities' caches, every other kind of data and every file outside
`cache.json` stay as they were.

### When clearing fails

If `cache.json` cannot be read, nothing is deleted and the error says so:
"Could not clear the cache of <title>: Cannot read <path>: <reason>.
Nothing was deleted. That file holds only extension caches: repair or delete
it, then clear the cache again." Pane reads the file again on the next
attempt, so no restart is needed.

If the file cannot be written, the error names the file and asks the user
to check that Pane can write its folder.

## Uninstalling an extension

Uninstalling removes a package's installed copy, its cache and its
credentials, and lets the user keep or delete its saved data. It implements
the accepted
[Uninstall](extension-policy-proposal.md#disable-cache-data-and-uninstall)
behavior for one package.

Uninstalling a package that other installed packages require asks about
them first and uninstalls them together, with the same choice
([#44](https://github.com/pane-app/pane/issues/44),
[dependencies](dependencies.md#uninstalling-a-required-dependency)).

Tracking: added for [#40](https://github.com/pane-app/pane/issues/40)
(US61, US63, US64, T20, G5; contributions).

### The confirmation

Settings › Extensions has a row "Uninstall <title>" for each package, after
the Clear cache rows. Its subtitle names the source. Choosing it asks first,
"Uninstall <title>?", with:

- "From <source>";
- "Pane removes its installed copy, its cache and its credentials on this
  computer, and the extension does not run. Deleting a credential does not
  sign you out of an online service.";
- "Saved data: 1 setting and 1 content record" (or "none");
- "Its source folder <path> and files it saved elsewhere are not touched."

The package's **saved data** is its settings and content (and its
[clipboard history](clipboard-history.md), since #35). That is what the
choice is about. There are three rows:

1. **Uninstall and keep saved data**. It comes first, so Enter keeps.
2. **Uninstall and delete saved data**.
3. **Cancel**, or Esc.

| Kind | Keep saved data | Delete saved data |
|---|---|---|
| Settings | kept | removed |
| Content | kept | removed |
| Cache | removed | removed |
| Local credentials | removed | removed |
| Managed copy (`packages/<n>/`) | removed | removed |
| Source folder, files elsewhere, remote sessions | untouched | untouched |

### What happens

Choosing a row applies at once in the launcher:

- The package leaves root search, Settings › Extensions and the targets of
  [operations](operations.md). A call to it is then "not-found".
- Its instances and pending calls stop as for disable: its generation ends,
  and a call it was serving answers "was uninstalled while serving the
  call".
- An open command, form or view of it closes.
- It can no longer read or save any data.

No guest runs, so a broken or disabled package, or a Pane whose runtime did
not start, uninstalls the same way. Then Pane:

1. records the uninstall in `installed.json`. If it cannot, nothing else
   changes, the package is back as it was and the error ends "It is still
   installed and nothing was deleted.";
2. removes the managed copy. A folder that cannot be removed, as on Windows
   while a file in it is in use, is listed in `installed.json` like an
   update's replaced copy and removed at the next start;
3. removes each kind the choice covers. Like Clear cache, it reads each file
   again first, so other identities' values and another Pane's writes are
   kept.

Recording first means a failure never leaves an installed package whose
data was already deleted.

Pane returns to the extension list with "Uninstalled <title>; its settings
and content are kept" or "Uninstalled <title> and deleted its saved data".
If step 2 or 3 fails, the outcome is an error rather than a success, and it
names what remains: "Uninstalled <title>, but could not delete its cache:
Cannot read <path>: <reason>." or "…, but its installed copy in <path>
could not be removed yet (<reason>); Pane removes it when it next starts."

### Retained data

Extension data kept for an identity that is not installed is **retained
data**. `installed.json` lists it under `retained`, with the source and the
title the package had, so that it stays manageable without the package
(`Launcher::retained_data` returns it):

```json
"retained": [{ "local": "/home/me/greeter", "title": "Greeter" }]
```

- Pane records it in the same write as the uninstall when the user keeps
  saved data and the package has some. It also records it after "delete"
  when a kind could not be removed, so Pane does not lose track of data left
  behind.
- The values stay in the kind files under the identity's key. Installing the
  same source again drops the record, and the package finds its settings and
  content (not its cache or credentials, which were removed). Pane
  restarting in between changes nothing.
- Another source is another identity, even with the same title. It starts
  with nothing and never sees the retained data.

### Deleting retained data

The user can delete retained data from Settings, one identity at a time.
This implements "Remove retained data later" of the accepted
[data policy](extension-policy-proposal.md#disable-cache-data-and-uninstall).
Data of different sources is never merged.

Pane does this alone, after the extension's code is gone. There is no
author example or guest API, and an extension has nothing to call or
handle.

Tracking: added for [#41](https://github.com/pane-app/pane/issues/41)
(US62, US63, US64, T20, G5; contributions).

#### The row and the confirmation

Settings › Extensions is offered in root search while anything is installed
or retained. It lists, last (after the Uninstall and hotkey rows), a row
"Delete retained data of <title>" per retained identity, in the order they
were uninstalled. The subtitle reads "Not installed · keeps 1 setting and 1
content record · <source>", or "keeps nothing". A kind whose file is
missing keeps nothing, and one whose file cannot be read says so.

The counts come from the files as they are when the list or the
confirmation is shown, so a file repaired meanwhile is counted. Two
identities with the same title are told apart by their source.

Choosing the row asks first, "Delete the retained data of <title>?", with:

- "From <source>";
- "<title> is not installed. Pane deletes the data it keeps for this source
  itself; the extension is not downloaded and does not run.";
- "Retained data: 1 setting and 1 content record" (every kind Pane still
  keeps for it);
- "Its source folder <path>, files it saved elsewhere and other extensions'
  data are not touched."

It has two rows: **Cancel** (first and selected, so Enter keeps the data,
as for uninstalling) and **Delete retained data**. Cancel and Esc return to
the row.

#### What deleting does

Deleting first reads `installed.json` again. It then removes that
identity's entry from each kind's file, reading each file again first as
Clear cache does, so other identities' values and another Pane's writes are
kept. Then it drops the identity's `retained` record and returns to the
list with "Deleted the retained data of <title>".

Nothing runs: the package's code is gone and is not downloaded, so a Pane
whose runtime did not start deletes the same way. Installing the same source
afterwards starts with nothing, and a restart changes nothing. The same
source cannot be installed while its data is being deleted.

#### When another Pane changed the list

Another Pane on the same data folder may have changed `installed.json`
since this one read it. Before deleting anything, Pane checks the file as it
is now:

- If the same source was installed again there, its data is that package's
  and nothing is deleted: "<title> was installed again from the same source
  by another Pane using this data folder, so its data is in use and nothing
  was deleted".
- If nothing is kept for it any more: "The data of <title> is no longer
  kept, so nothing was deleted".

Either way the row goes. If the file cannot be read: "Could not delete the
retained data of <title>: <reason>. Nothing was deleted."

Dropping the record also rewrites the file as it is now, so another Pane's
install records are kept. The files are not locked, so a change made by
another Pane in the moment between that check and the deletion is not seen.

#### When deleting fails partway

The record is dropped only once every kind is deleted.

- A file that cannot be read or written (such as a malformed or locked
  `content.json`) leaves that kind's values where they were, and the record
  stays: "Could not delete all the retained data of <title>: could not
  delete its content: Cannot read <path>: <reason>. What could not be
  deleted stays listed: repair or delete that file, then delete it again."
  The row then shows what remains. Deleting again after the repair
  finishes, without a restart.
- If the data is deleted but `installed.json` cannot be written: "Deleted
  the retained data of <title>, but could not remove it from the list:
  <reason>. It stays listed, keeping nothing, until it is deleted again."
  This is checked on macOS and Linux only (a read-only data folder). On
  Windows a read-only folder still accepts new files, so the test that
  replaces `installed.json` with a folder fails on reading instead, before
  anything is deleted.

## Checks

### Uninstall tests

- `crates/pane-core/tests/uninstall.rs`, for each language's settings
  sample: the confirmation, keep and reinstall (with a restart between),
  delete, Cancel and Esc; two copies with the same title, with the source
  folders and a user document unchanged byte for byte; another source with
  the same title not getting retained data; a disabled package with a
  garbage component uninstalled by a launcher without a runtime; an open
  command closed and its instance stopped; an `installed.json` that cannot
  be written (nothing changes); an unreadable `cache.json` (explained, the
  rest deleted, recorded as retained); on Unix, a managed folder that cannot
  be removed (explained, removed at the next start).
- `crates/pane-core/tests/stopping.rs`: a call waiting inside the guest is
  stopped by the uninstall and saves nothing more, in each language.
- `crates/pane-core/tests/operations.rs`: an uninstalled target is not
  found and its instance stops.
- `crates/pane/tests/install.rs`: the rows, the confirmation, Esc and the
  outcome in the native window.
- The native GUI smokes, screenshots 49 to 51: the confirmation, the
  outcome, and after reinstalling the same folder its style and note shown,
  signed out, with the files checked in between.

### Retained data deletion tests

- `crates/pane-core/tests/uninstall.rs`, for each language's settings
  sample: the row and confirmation after a restart, deleted by a launcher
  without a runtime, the source folders and a user document unchanged byte
  for byte, still gone after a restart and a reinstall starting empty;
  Cancel and Esc; three copies with the same title, one of them installed,
  where only the chosen retained identity's data goes; an unreadable
  `content.json` (explained, the rest deleted, still listed, counted again
  and finished once repaired); a cache an earlier uninstall could not delete
  deleted with it; a missing kind file counted as empty; an `installed.json`
  that cannot be read (nothing deleted, on every system) or, on Unix,
  written (explained, still listed keeping nothing); a second launcher on
  the same folder that installed the same source again (nothing deleted,
  its records and data kept) or another package (its record kept); an
  installed package refused.
- `crates/pane/tests/install.rs`: the row, confirmation, Esc and outcome in
  the native window, with Enter on the selected Cancel keeping the data.
- The native GUI smokes, screenshots 63 to 65: the confirmation, the
  outcome, and after reinstalling the same folder nothing shown, with the
  files checked in between.

### Clear cache tests

- `crates/pane-core/tests/clear_cache.rs`, for each language's settings
  sample: each kind before and after clearing, and after a restart; Cancel
  and Esc; two copies with the same title, with the source folders and a
  user document unchanged byte for byte; a disabled package whose managed
  component was replaced by garbage, cleared by a launcher without a
  runtime; an unreadable `cache.json`, unchanged with every other file, then
  cleared once deleted; another Pane's cache write between reading and
  clearing kept; the running instance note; `credentials.json` created, and
  rewritten, with mode 0600 (Unix).
- `crates/pane/tests/install.rs`: the rows, confirmation text, Esc and the
  outcome in the native window.
- The native GUI smokes, screenshots 40 to 43: every kind shown, the
  confirmation, the outcome, and the cached greeting gone with the other
  kinds still shown and still in their files.

### Credential protection tests

These cover [protected credentials](#protected-credentials) (#130).

- `crates/pane-core/tests/credentials.rs`, for each language's settings
  sample: a token read back after a restart, the file's bytes without the
  token's text and the value recorded as DPAPI (Windows; on macOS and Linux
  version 1 as before); a version-1 file converted at start, its token
  still read; a conversion whose write fails (the file held open without
  sharing deletion) leaving the version-1 file readable and converted at
  the next start; a damaged token explained through `credentials.get` while
  the other kinds still read, kept by Pane across restarts and replaced by
  Sign in; Manage extensions counting "2 credentials, 1 unreadable" for
  retained data and deleting it; and, on every system, a file of an unknown
  version refused by `get`, `set` and uninstall and left byte for byte.
- Unit tests in `protection.rs` and `extension_data.rs`: a value protected
  and read back, damaged bytes explained, a value that cannot be read
  counted, kept by other writes and replaced by `set`, and a password
  preference that cannot be read treated as unset.
- The Clear cache and uninstall checks above run unchanged over the
  protected file on Windows.

## Limits

- `get` and `set` only: an extension cannot delete one value or list its
  keys.
- Local credentials are kept in Pane's data folder, not in the system's
  keychain
  ([decision](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/current-decisions.md#cross-cutting-details-preserved)).
  On macOS and Linux `credentials.json` holds them as they are, created with
  mode 0600, so other users of the computer cannot read it. On Windows
  (since #35's review) each version of it is created with a protected DACL
  giving full control to the user Pane runs as and SYSTEM only, inheriting
  nothing from its folder (`atomic.rs`, checked by a Windows unit test).
  Since #130 each value in it is also encrypted with DPAPI for the user
  ([above](#protected-credentials)). `clipboard-history.json` is written
  the same way.
- Nothing protects these files from other extensions or programs running
  as the same user, which can call DPAPI as Pane does. The policy is
  lifecycle behavior, not secret isolation.
- Deleting a local credential never revokes a remote session. A package
  that cannot run cannot be asked to sign out, and the confirmation says
  so.
- Clear cache covers `cache.json` only. An extension that writes cache
  files elsewhere has to manage them itself.
- The files are replaced atomically but not locked, as with settings, so
  two Pane processes on one data folder can lose each other's last write.
- Uninstall and deleting retained data remove an identity's entry from each
  file as Clear cache does. Deleting is not forensic erasure: copies in
  backups or on the disk may remain.
- Only retained data recorded in `installed.json` is listed. Values still
  in the kind files for an identity that was never recorded (as after
  `installed.json` was lost) are not found.
- Uninstall does not remove Pane's own compile cache entries. They are
  keyed by the components' bytes, may be shared with another package, and
  Wasmtime's cache trims them itself. They hold no extension data.
