# Aliases and fallbacks

Added for [#31](https://github.com/hoangvu12/pane/issues/31): US10, US11;
T03; contributions to G2, not claims that it passes. The user gives an
installed command an **alias** or makes it a **fallback** in Manage
extensions or in Settings' Shortcuts page — one record either way, the
same rules — and reaches it from [root search](root-search.md) in fewer
steps. Text typed in root search reaches a command that **takes a query**
only when the user invokes it that way: root search never sends it while
the user types, so no command (an online service, say) sees text meant for
another. Root-search routing only; [global hotkeys](hotkeys.md) are
separate. The same on every system: no system API is involved.

## Giving a command an alias

In **Manage extensions…**, after the hotkey rows, every command of an
enabled package has a row "Alias for &lt;command&gt;", subtitled with its alias
("“ec”") or "None · A word that finds it in root search", then its package's
source (copies of a package may share titles). Enter opens the form "Alias
for Echo" with one field, filled with the current alias; Save alias applies
it at once, records it and returns to the extension list with "Typing “ec”
now finds Echo". Leaving the field empty removes the alias ("Echo has no
alias now"). Refused, with the reason next to the field and the form kept:

| Alias | Explanation |
| --- | --- |
| With a space | "An alias is one word, without spaces" |
| Over 32 characters | "An alias has at most 32 characters" |
| Another command's, compared caselessly: NFC, then full Unicode case folding ("STRASSE" is "straße"; not locale-aware, so Turkish dotted and dotless I stay apart) | "“STRASSE” is already the alias of Greeting: change it there first, or choose another" |

Settings' **Shortcuts** page gives every command the same editing in place
of its alias: clicking the row's alias (or reaching it with Tab and Enter)
opens a field filled with the current alias; Enter applies and records it,
Escape cancels, and the same refusals show beside the field with it kept
open. The page edits the same records, so nothing differs between the two.

In root search, a query that is the alias, compared the same caseless way,
lists the command first, above every other result, computed results
included; Enter opens it as usual.
For a command that takes a query, the alias, a space and more text ("ec
hello world") list a row titled with the command, subtitled "Send “hello
world” · alias ec", first and selected; Enter sends "hello world" (the text
after the alias, trimmed) and shows the command's answer as the result,
root search staying as it was. The answer is cleared from the status line as
soon as the query changes, and an answer that arrives after it changed is
not shown. Text after the alias of a command that takes no query sends
nothing (the query is then matched as usual). When another command offered
has the same title (copies of a package from other sources), the row's
subtitle also names its source ("Send “hi” · alias ec · local folder …"),
and so does a fallback row.

## Making a command a fallback

A command that takes a query also has a row "Fallback: Echo", "Off · Offer
it below the results for any text typed" or "On · …". Enter turns it on
("Echo is now offered for any text typed in root search") or off ("Echo is
no longer a fallback"). For any query that is not blank, each fallback is
listed **below every other result**, in the order the user turned them on,
subtitled "Send “zqx” · fallback". A fallback row is **never selected by
itself**: when nothing else matches, root search shows "No results for
“zqx”" above the fallbacks with nothing selected, so Enter does nothing, as
before; Down (or Up, or a click) selects one and Enter sends the whole
query, trimmed.

## What keeps and removes them

- Pane's own records, not the extension's data: `aliases.json` beside
  `installed.json`, by command id (the package identity's key and the
  manifest's command id), `{ "version": 1, "aliases": { "local:/…#echo":
  "ec" }, "fallbacks": ["local:/…#echo"] }`. Written atomically, one change
  at a time, each write holding the latest choices; a change that cannot be
  written goes back to what was last recorded, unless its package was
  uninstalled meanwhile: an uninstalled package's choices never come back.
  An unreadable file is not overwritten (a change then explains why). The
  global hotkeys' `hotkeys.json` is kept the same way (one shared record
  type, `launcher/choices.rs`).
- A command id is `<package identity key>#<manifest id>`, and a manifest
  command id cannot contain `#`, so the package's part is exact:
  uninstalling `local:/x` forgets `local:/x#echo`, never `local:/x#y#echo`
  (hotkeys alike).
- By command id, so copies of a package from other sources, even with the
  same titles, have their own; each is told apart by its source in Manage
  extensions.
- **Disabling** a package removes its commands' aliases and fallbacks from
  root search at once; they stay recorded, and their rows stay in Manage
  extensions with "Not active: Query sample is disabled". Changing them
  there does not enable the package, and nothing else does: only the user's
  enable in Manage extensions brings them back.
- A **paused** package's command, and a command **unavailable on this
  system**, stay reachable by their alias and fallback rows, which explain
  why and run nothing, as the command's own row does; in Manage extensions
  their rows say "Not active: Query sample is paused after an error; retry
  it in Manage extensions" or "Not active: Not available on Linux: this
  command supports only Windows".
- A package whose managed copy **cannot load** lists its recorded choices
  as "Alias “ec” of `echo` · Not active: query cannot load: …; Enter
  forgets it", not as a missing command.
- An **update** or **reload** keeps them. A command the new copy no longer
  has, or one that no longer takes a query, is shown as not active: "Alias
  “gn” and fallback of a missing command · Not active: Query sample has no
  command `gone` now; Enter forgets it", or "On · Not active: Echo no
  longer takes a query".
- **Uninstall** forgets them, whether its saved data is kept or deleted
  (they are Pane's records). A record for a package that is not installed
  (a record edited by hand) is shown as a missing command too.
- Two commands with the same alias (only in a record edited by hand): both
  rows say "Not active: another command has the same alias", and root
  search uses neither.

## For extension authors

Any installed command can be given an alias, in Rust, JavaScript or
TypeScript alike; nothing is declared. Taking a query needs `"takesQuery":
true` on the command in `pane.json`. Since #138 the text arrives as the
**fallback text** of the command's launch record
([`wit/commands.wit`](../wit/commands.wit)), trimmed, through the entry
point its mode has: a no-view command (`"mode": "no-view"`) runs (`run`)
and opens no screen, its answer shown while root search stays as it was,
which is what users saw of a query-taking command before; a view command
opens its screen (`render`) with the text in the record. The separate
`run-query` interface (#31's `pane:extension/query-command`) is retired.
The [author guide](../guests/README.md#a-command-that-takes-a-query) shows
it in Rust and in JavaScript and TypeScript; Echo, the query sample, is a
no-view command in [`guests/sample-query`](../guests/sample-query),
[`sample-query-js`](../guests/sample-query-js) and
[`sample-query-ts`](../guests/sample-query-ts). An error it answers is
shown and never counts towards [pausing](pausing.md); a trap does.

## Checks

- Launcher public interface ([`crates/pane-core/tests/aliases.rs`](../crates/pane-core/tests/aliases.rs)),
  with the real query sample in Rust, JavaScript and TypeScript for the
  alias, fallback, cleared answer, disable, copies and pausing checks: an alias set in its form is recorded; the
  alias alone lists Echo first and "ec hello  world " lists the row that
  sends "hello  world", with Echo's guest not started while typing; Enter
  shows its answer and keeps root search; an error answer is shown; kept
  after a restart; the form starts with the alias and an empty one removes
  it; an alias ranks above a computed result (the calculator's). A fallback
  is listed last for any text, not selected, Enter then does nothing and
  Echo does not start; Down and Enter send the query; not for a blank query;
  turned off again. The answer cleared when the query changes, and one
  arriving after the change not shown. Three crashes of the query "crash"
  pause the package; its alias row explains it and runs nothing. Refusals
  (another command's alias "straße" given as "STRASSE", a space, 33
  characters); a command that takes no query found by its alias,
  with no row for text after it and no fallback row. A conflict in a record
  edited by hand shown and neither alias used. Disabling removes both from
  root search and shows them as not active; changing them does not enable
  the package, nor does a restart; enabling brings both back. Two copies of
  the package from different folders: the alias finds and runs only the
  copy it was given to (checked with the runtime's running components), and
  the alias and fallback rows name their sources. A command unavailable on
  this system and a package that cannot load shown as not active with why. A
  missing command's choices shown and forgotten; uninstall forgets them,
  exactly its own (a copy from folder `x#y` keeps its alias when `x` is
  uninstalled; the same for hotkeys in `hotkeys.rs`); a write that fails
  after an uninstall does not bring the alias back; Echo opened from its
  row, with no text, runs and opens no screen. The launch record's source
  (alias or fallback) and a view command opened through its alias with
  text are checked in [`no_view.rs`](../crates/pane-core/tests/no_view.rs).
- Window ([`crates/pane/tests/aliases.rs`](../crates/pane/tests/aliases.rs)),
  on GPUI's test platform with real key events: the alias typed in its form
  and saved, the form reopened filled with it, the fallback turned on; "ec
  hello" and Enter show Echo's answer; "zqx" shows "No results" and the
  fallback unselected, Enter changes nothing (checked once the runtime has
  served every call: Echo, stopped before, has not started again), Down and
  Enter send "zqx". Rows are chosen by title.
- Native GUI smokes, one identical phase on all three systems (screenshots
  66 to 74): with data folders of their own, install the query sample, set
  the alias "ec" and the fallback in Manage extensions, send "ec hello" and,
  from the fallback chosen with Down, "zqx"; check `aliases.json`; restart,
  disable the extension and check that "ec hello" gives the same screen as
  a Pane with nothing installed. See the
  [Linux](platforms/linux.md#aliases-and-fallbacks-31),
  [macOS](platforms/macos.md#aliases-and-fallbacks-31) and
  [Windows](platforms/windows.md#aliases-and-fallbacks-31) notes for where
  it has run.

## Limits

- One alias per command, only for installed packages' commands (not the
  samples this build supplies), one word, compared caselessly (above); no
  aliases for indexed or computed results (applications, quicklinks).
- The fallback rows are not ranked or limited: every fallback is listed for
  every non-blank query. No per-command placeholder or argument parsing: the
  command gets one string.
- The answer is shown as text on the status line; a command that takes a
  query cannot open its view with the query, or a form or list for it.
- The query is sent when the row is invoked and the call is not cancelled
  when the user types on (its late answer is discarded); calls run one at a
  time on the runtime thread (#29, #18).
- Screen readers: fallback rows are ordinary options of the results list;
  with none selected, the combo box itself is reported as focused. No
  screen reader was run ([root search](root-search.md#accessibility)).
