# Quicklinks

Added for [#28](https://github.com/hoangvu12/pane/issues/28) (US03, US07,
US12, US59; T01, T03, T10, T22; G2, G5, G7, as contributions, not claims that
they pass). A user saves a named web address, finds it by typing into
[root search](root-search.md), also after restarting Pane, and opens it in
the default browser.

## The Quicklinks extension

Quicklinks is a default extension in Rust
([`guests/quicklinks`](../guests/quicklinks), package
`guests/packages/quicklinks`), like [the calculator](root-search.md#the-calculator):
not part of the core, disabled like any package, and installed from its
folder until setup acquires default extensions (#51 to #53):
`pane --install target/guests/packages/quicklinks`.

Its command, **Quicklinks**, lists:

- **Create quicklink**, a [form](forms.md) with *Name* and *URL* and a
  "Save quicklink" button;
- one item per saved quicklink, titled with its name, opening its edit form:
  *Name* and *URL*, each showing the current value while empty, and *On
  submit*, "Save changes" or "Remove this quicklink". An empty field keeps
  its value, so the user changes only what they type (forms have no initial
  values yet).

The list shows the quicklinks as they were when the command opened: after
saving, Esc returns to that list; reopening the command shows the change
(forms do not refresh the list they return to).

### The URL format

A quicklink opens one kind of target: an **absolute `http://` or `https://`
URL** with a host, as the user would paste it from a browser, for example
`https://github.com/hoangvu12/pane/issues?q=is%3Aopen`. The scheme's letter
case does not matter; the address is saved and opened as typed (trimmed),
with no encoding or normalization. Placeholders and query arguments
(`{query}`), other schemes (`file:`, `mailto:`, app links), files, folders
and applications are not quicklinks; each would need its own slice.

The extension validates what is submitted and marks the field, keeping the
form open; none of these is an extension error:

| Field | Rejected | Message |
| --- | --- | --- |
| Name | empty (on create) | Enter a name |
| Name | a tab or line break | The name cannot contain line breaks or tabs |
| Name | over 80 characters | Use at most 80 characters |
| Name | another quicklink's name, in any letter case | A quicklink named “…” already exists |
| URL | no `http://` or `https://` scheme | Enter a web address starting with http:// or https:// |
| URL | a space or control character | The address cannot contain spaces |
| URL | over 2048 characters | Use at most 2048 characters |
| URL | nothing before the first `/`, `?` or `#` after the scheme | The address has no host |

### Storage

The quicklinks are the package's **content**, its durable
[extension data](extension-data.md) (`pane_guest::content`): one value,
`quicklinks`, holding one line per quicklink (`name`, a tab, the URL), in
creation order, in Pane's `content.json`. So they belong to the package's
identity, survive restarts and updates, are kept while the package is
disabled, and are not removed when its cache is cleared.

## In root search

The command declares `"rootResults": true`: for every query that is not
blank, it lists the quicklinks whose name contains every word of the query,
then those where each word is in the name or the URL, ignoring letter case.
Each is a [computed result](root-search.md#results-computed-from-the-query)
titled with the name, with subtitle "Quicklink · <URL>", and like every
computed result it is listed above the title matches, in the extension's
order, not ranked against them. A blank query lists no quicklinks.

Its action is **open-url**, the second root action beside copy
(`wit/root-results.wit`): Pane, not the extension, opens the URL.
Disabled, the package is not asked, so its quicklinks leave root search at
once, together with its command; enabled again, they are back from the next
change of the query.

## Opening a link

`Launcher::activate_selected` on an open-url result opens an address of
any scheme (`https:`, `mailto:`, `ms-settings:`, `file:`, an application's
own), as Raycast does (ADR 0037, #145): the extension is trusted and can
open anything through the `system` host functions anyway. It shows
"Running…" and hands the URL, off the window's thread, to the launcher's
`LinkOpener` (`Launcher::with_link_opener`). The status then
reads "Opened <URL>" or "Could not open <URL>: <reason>"; root search keeps
its query. A launcher given no opener explains that it has none, which is
what every test uses unless it passes a recording fake: no test opens a
browser.

The window's opener, `pane::SystemLinks`, runs the handler the `open` crate
names for the system, without a shell:

| System | Handler | A missing handler |
| --- | --- | --- |
| Linux | `xdg-open`, else `gio open`, `gnome-open`, `kde-open` | none installed: "no program to open web links is installed"; xdg-open finding no browser (status 3): "no program to open web links is set up"; the browser failing (status 4): "the browser or link handler refused or failed to open it" |
| macOS | `/usr/bin/open -- <URL>` | its failure status |
| Windows | PowerShell `Start-Process` (ShellExecute), else `explorer.exe` | its failure status |

A handler still running after three seconds counts as having opened the
link and is left to finish: outside a desktop session, `xdg-open` runs the
browser itself and returns only when the browser exits.

## Checks

Through the launcher's public interface, with the real guest and a
recording link opener
([`crates/pane-core/tests/quicklinks.rs`](../crates/pane-core/tests/quicklinks.rs)):
creating a quicklink in the form, finding it by part of its name or its
address, and Enter opening it; each rejected input above on its field with
the form still open, then the corrected form saving; editing the address
only, renaming, a rename onto another name refused, and removing; a restart
finding and opening it; disabling hiding both the quicklinks and the
command, still disabled after a restart, and enabling bringing them back;
a handler that fails explained; a `file:` address refused without calling
the handler (the `faulty` fixture offers one); and a launcher without a
handler. The open-url action in Rust, JavaScript and TypeScript: each
sample answers "pane website" with a result that opens
`https://github.com/hoangvu12/pane`
([`samples.rs`](../crates/pane-core/tests/samples.rs)).

Window check through GPUI's test platform with real key events
([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)): opening
Quicklinks and its form with Enter, typing a name, Tab, an address without
a scheme rejected on its field, fixing it and saving, then Esc Esc, typing
"pane iss" and Enter opening the link through a recording opener.

Native: the GUI smoke scripts' last phase installs Quicklinks, creates "Pane
issues" through the form with real key events, restarts Pane and types
"pane iss", which must list it selected. On Linux it also presses Enter:
`xdg-open` runs outside any desktop session, with `BROWSER` set to a script
that records the address and every XDG location pointing into the smoke's
output, so the user's browser never starts; the script must have received
the URL and the status must show "Opened …". It ran on Linux X11 on
2026-09-28 ([evidence](platforms/linux.md#quicklinks-28)). On macOS and
Windows the phase stops before Enter, which would open a real browser; it
is written but has not run yet, and opening a link there is so far only
checked through the tests' recording opener, not natively.

## Limits

- No placeholders or query arguments, no other target types, no icons,
  aliases, keywords or hotkeys (#31 and later).
- Quicklinks are computed results: listed above title matches rather than
  ranked with them, and not listed for a blank query.
- The edit form cannot show current values in its fields; empty keeps them.
- The command's list is not refreshed after saving until it is reopened.
- A handler that is slow to fail (over three seconds) is reported as having
  opened the link.
- Screen reader behaviour is unverified, as for all of root search.
