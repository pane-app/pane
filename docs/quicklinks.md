# Quicklinks

A quicklink is a named link, file, folder or application that you save once
and open from [root search](root-search.md). You type part of its name, it
shows up, also after restarting Pane, and Enter opens it. This page is for
users who want to know what the Quicklinks extension does and for anyone
working on it.

## The Quicklinks extension

Quicklinks is a default extension written in Rust. It lives in
[its own repository](https://github.com/pane-app/quicklinks), and each Pane
release pins its release commits in
[`crates/pane/defaults.json`](../crates/pane/defaults.json). Like
[the calculator](root-search.md#the-calculator), it is not part of the
core. You can disable it like any package, and Pane acquires it at first
setup from its pinned commit (#278, #53).

Its six commands share one component:

| Command | Id | Mode | Does |
| --- | --- | --- | --- |
| **Search Quicklinks** | `quicklinks` | view | lists the quicklinks with their actions; supplies them to root search |
| **Create Quicklink** | `create` | view | the form |
| **Open in Browser** | `browser` | no-view | opens a typed web address (`"matches": "url"`, #195) |
| **Create Quicklink** | `save` | view | the form with a typed web address prefilled (`"matches": "url"`, #195) |
| **Import Quicklinks** | `import` | no-view | adds the quicklinks the clipboard holds as JSON |
| **Export Quicklinks** | `export` | no-view | copies the quicklinks to the clipboard as JSON |

Search Quicklinks keeps the first version's command id. A quick slot or
alias given to the old **Quicklinks** command now reaches Search
Quicklinks.

Open in Browser and the address-prefilled Create Quicklink declare
`matches: "url"`. Root search lists them only for a URL-like query and
never matches them by their titles (see
[root search](root-search.md#understanding-the-typed-query)).

### Search Quicklinks

Each quicklink is an item with its name as the title and its target as the
subtitle. Its icon depends on the target:

- an `http:` or `https:` link shows the site's favicon (a globe while it
  loads and when the site has none, #142);
- a path shows the system's icon of the file, folder or application;
- any other scheme shows a link glyph.

A quicklink that opens with an application names that application as an
accessory. Its actions, in order:

| Action | Keys | Does |
| --- | --- | --- |
| Open | Enter | opens the target (with its application, if it has one), then closes the window |
| Open With… | Ctrl+Enter opens it in the panel | a submenu of the installed applications; the one chosen opens the target, then the window closes |
| Copy Link | Ctrl+Shift+C | copies the target, closes the window and shows "Copied to Clipboard" |
| Edit | Ctrl+E | opens Create Quicklink's form filled in with the quicklink |
| Duplicate | | opens the form filled in as a copy ("Docs copy", "Docs copy 2", …) |
| Delete | | destructive: asks "Delete “Docs”?" first, then deletes it and toasts "Deleted “Docs”" |

Open, Open With… and Copy Link are the SDK's standard actions (#145). Edit
and Duplicate launch Create Quicklink with the quicklink's id in their
context (`{"edit": "3"}`, `{"duplicate": "3"}`).

With no quicklink saved, the list has one item, "Create Quicklink", which
opens the form.

### Create Quicklink

The command's screen is a form (a `form` view, `docs/list-tree.md`) with
three fields: *Name*, *Link* and *Open With*. They start empty, or filled in
for Edit and Duplicate.

- Saving a new quicklink toasts "Created “Docs”" and returns to root search.
- Saving an edit toasts "Saved “Docs”" and opens Search Quicklinks again.
- Escape leaves the command for root search.

A quicklink's target is anything the `open` host function opens
(ADR 0037):

- a **link of any scheme** (`https:`, `mailto:`, `ms-settings:`, an
  application's own scheme);
- the **absolute path** of a file, a folder or an application (`C:\…`,
  `\\server\…`, `/…`).

*Open With* is optional. It takes the name of an installed application as
Pane lists it (any letter case), or an application's absolute path. Left
empty, the target opens with the system's handler.

Targets are saved as typed (trimmed). Whether a path exists is for the
system to say when it is opened.

When the form refuses a value, it marks the field and stays open:

| Field | Refused | Message |
| --- | --- | --- |
| Name | empty | Enter a name |
| Name | a tab or line break | The name cannot contain line breaks or tabs |
| Name | over 80 characters | Use at most 80 characters |
| Name | another quicklink's name, in any letter case | A quicklink named “…” already exists |
| Link | empty | Enter a link, or the path of a file, folder or application |
| Link | neither a scheme nor an absolute path (`example.com`, `notes/a.txt`) | Enter a link with its scheme, such as https:// or mailto:, or the full path of a file, folder or application |
| Link | a tab or line break | The link cannot contain line breaks or tabs |
| Link | over 2048 characters | Use at most 2048 characters |
| Link | a space in a link (paths may have spaces) | A link cannot contain spaces |
| Link | nothing after the scheme (`mailto:`) | Enter what the link opens after “mailto:” |
| Link | an `http(s)` link without a host | The address has no host |
| Open With | not an installed application's name or an absolute path | No installed application is named “…”; enter its name as Pane lists it, or its full path |

### Import and export

Export Quicklinks copies every quicklink to the clipboard as a JSON array
and shows a HUD: "Copied 2 quicklinks as JSON".

```json
[
  { "link": "https://github.com/pane-app/pane/issues", "name": "Pane issues" },
  { "link": "C:\\Notes", "name": "Notes", "openWith": "C:\\Program Files\\Zed\\zed.exe" }
]
```

`openWith` is the application's id (or path) as saved.

Import Quicklinks reads such an array from the clipboard. It adds each
entry whose name no quicklink has yet and whose name and link the form
would accept, skips the others, and toasts "Added 2 quicklinks, skipped 1".
An `openWith` naming an installed application (by name, id or path) is
taken as that application. One this system lacks is kept as given, and
opening the quicklink says why it cannot be used.

If the clipboard text is not a JSON array, or there is no text at all,
import shows a failure toast, "Could not import quicklinks", saying why.

Both commands go through the clipboard until forms gain file fields (#121).

### Storage

The quicklinks are the package's **content**, its durable
[extension data](extension-data.md) (`pane_extension::content`). That is
one value, `quicklinks`, with one line per quicklink in creation order.
Each line holds the name, target, id, application id and application name,
separated by tabs. None of them can hold a control character.

The first version's lines hold only a name and an address. Pane reads them
as quicklinks with no application and gives each the next free id. The ids
are the same on every start until the quicklinks are saved again, so the
upgrade keeps every saved quicklink.

The id stays through a rename. Root search and a quick slot know the
quicklink by it.

## In root search

### Typed web addresses

A query that is a typed web address is one Quicklinks answers (#195). Its
two commands that declare `matches: "url"` are listed under "Addresses",
below the results found by title and above the files. When nothing else
matches, the first is selected, so Enter acts on the address.

- **Open in Browser** (`browser`) opens the address with the system's
  handler, inferring `https://` before a bare domain. The address is its
  launch record's fallback text. Then it closes the window.
- **Create Quicklink** (`save`) is the same form as the `create` command's,
  with the address filled in as its link. Give it a name and press Enter to
  save the quicklink. It toasts "Created “Docs”", exactly as filling the
  form by hand would.

### Saved quicklinks

Search Quicklinks declares `"indexedResults": true`. It supplies one
[indexed result](root-search.md) per quicklink, titled with its name and
with its target as the subtitle. Root search keeps these, matches them by
title and subtitle, and ranks them with the commands (current decisions
item 10). The first version's computed results were instead listed above
every title match.

Root search asks for them again on the first query after it is shown, and
after a no-view command ran or a command's list handled an action (an
import adds some; a delete removes one). A blank query lists none. Their
further ranking belongs to "Root search like Raycast" (#122).

### Opening one

The action is **open** (`wit/applications.wit`, `indexed-action.open`).
Pane, not the extension, opens the target with the system's `open` (the
same one the `system.open` host function uses), with the quicklink's
application if it has one. Once it opens, the launcher closes, and root
search is shown next time with its query empty (#468). If it does not
open, the status reads "Could not open Docs: <reason>", and root search
keeps its query.

A quick slot holds a quicklink by its id under Search Quicklinks
(`{"command": "<key>#quicklinks", "result": "3"}`), so it survives a
restart and a rename. If the quicklink is deleted, it keeps its slot and
says "Search Quicklinks no longer lists it".

When the package is disabled, its quicklinks leave root search along with
its commands. Enabled again, they are back from the next query.

## Checks

The extension's own behaviour is tested in
[the Quicklinks repository](https://github.com/pane-app/quicklinks). Those
tests left Pane's repository with the default extensions' sources (#285).

Through the launcher's public interface, with the real package, a recording
system for opening and the clipboard and a recording window, the
Quicklinks repository checks:

- the six commands and their modes;
- the form saving and returning to root search, and each refusal above on
  its field;
- each row's icon, title, target and accessory, and its actions with their
  shortcuts, Delete destructive;
- Open, Open With… and Copy Link closing the window, Copy Link with its HUD;
- Edit and Duplicate filling the form in;
- Delete dismissed and confirmed;
- a `mailto:` link, an `ms-settings:` link, a file, a folder and an
  application, with and without an application, each opened through the
  system;
- ranking with commands;
- a pinned quicklink through a restart, a rename and its deletion;
- export and import, between two Panes and with entries skipped;
- import's failure toasts;
- the first version's content and quick slot after the upgrade;
- disabling.

Its window checks run through GPUI's test platform with real key events:

- Create Quicklink's form from root search, a link refused on its field,
  fixed and saved, found and opened from root search;
- Escape leaving the form;
- Search Quicklinks' Actions panel listing every action;
- Ctrl+E opening the form filled in;
- Ctrl+Shift+C copying with the HUD;
- Delete asking and Enter confirming;
- Open With… opening with Zed;
- Enter opening.

The rows for a typed web address are checked in Pane's repository, in
[`crates/pane-core/tests/typed_queries.rs`](../crates/pane-core/tests/typed_queries.rs):

- Open in Browser opening the address and closing the window;
- `https://` inferred before a bare domain;
- Create Quicklink opening the form with the address prefilled and saving
  it.

The native GUI smoke scripts have a quicklinks phase. It installs
Quicklinks, creates "Pane issues" through Create Quicklink with real key
events, restarts Pane and types "pane iss", which must list it selected.
On Linux it also presses Enter, and `xdg-open`, with `BROWSER` set to a
recording script, must receive the URL.

## Limits

- No placeholders or query arguments (`{query}`), aliases or keywords per
  quicklink.
- No file-based import or export until forms gain file fields (#121).
- No tags or folders.
- Open With in the form is a text field (the application's name or path),
  not a list to choose from.
- Root search's rows of quicklinks show the row kind's tile, not the
  favicon.
- Back from the form opened by Edit returns to root search, not to the
  list.

## Tracking

Added for [#28](https://github.com/pane-app/pane/issues/28) (US03, US07,
US12, US59; T01, T03, T10, T22; G2, G5, G7, as contributions, not claims
that they pass). Reworked in Raycast's shape for
[#149](https://github.com/pane-app/pane/issues/149) (part of #120). The
commands for a typed web address were added for
[#195](https://github.com/pane-app/pane/issues/195).
