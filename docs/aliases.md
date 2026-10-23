# Aliases and fallbacks

An alias and a fallback are two ways to reach an installed command from
[root search](root-search.md) in fewer steps. An **alias** is a short word
that finds the command. A **fallback** is a command offered below the
results for any text you type. This page is for users setting them up and
for extension authors whose commands take a query.

You give a command an alias or make it a fallback in Manage extensions or
in Settings' Shortcuts page. Either way it is one record, with the same
rules.

Text typed in root search reaches a command that **takes a query** only
when you invoke it that way. Root search never sends it while you type, so
no command (an online service, say) sees text meant for another.

This is about root-search routing only; [global hotkeys](hotkeys.md) are
separate. It works the same on every system, since no system API is
involved.

## Giving a command an alias

### In Settings › Extensions

In **Settings › Extensions**, after the hotkey rows, every command of an
enabled package has a row "Alias for &lt;command&gt;". Its subtitle is the
alias ("“ec”") or "None · A word that finds it in root search", then the
package's source (copies of a package may share titles).

1. Press Enter on the row. The form "Alias for Echo" opens with one field,
   filled with the current alias.
2. Type the alias and choose Save alias. Pane applies it at once, records
   it and returns to the extension list with "Typing “ec” now finds Echo".

Leaving the field empty removes the alias ("Echo has no alias now").

Some aliases are refused. The reason shows next to the field and the form
stays open:

| Alias | Explanation |
| --- | --- |
| With a space | "An alias is one word, without spaces" |
| Over 32 characters | "An alias has at most 32 characters" |
| Another command's, compared caselessly: NFC, then full Unicode case folding ("STRASSE" is "straße"; not locale-aware, so Turkish dotted and dotless I stay apart) | "“STRASSE” is already the alias of Greeting: change it there first, or choose another" |

### In the Shortcuts page

Settings' **Shortcuts** page lets you edit every command's alias in place.
Click the row's alias (or reach it with Tab and Enter) to open a field
filled with the current alias. Enter applies and records it, and Escape
cancels. The same refusals show beside the field, which stays open.

The page edits the same records, so nothing differs between the two.

### Using an alias in root search

A query that is the alias, compared the same caseless way, lists the
command first, above every other result, computed results included. Enter
opens it as usual.

For a command that takes a query and declares no arguments, type the
alias, a space and more text ("ec hello world"). Root search lists a row
titled with the command and subtitled "Send “hello world” · alias ec",
first and selected. Enter sends "hello world" (the text after the alias,
trimmed) and shows the command's answer as the result, with root search
staying as it was.

The answer is cleared from the status line as soon as the query changes.
An answer that arrives after the query changed is not shown.

Text after the alias of a command that takes no query sends nothing. The
query is then matched as usual.

When another command offered has the same title (copies of a package from
other sources), the row's subtitle also names its source ("Send “hi” ·
alias ec · local folder …"). A fallback row does the same.

### Aliases and arguments

An alias and a space is also how you reach a command's
[arguments](root-search.md#inline-argument-fields) (#205). For a command
that declares them, there is no row that sends the text after the alias.
The alias stays in the query and the command's inline fields show after
it, entered at their first empty one. What you type next goes into the
first text or password field. Tab after the alias alone does the same.

A command without arguments that takes no query opens at once instead, as
Raycast does: a view command opens its screen, a no-view command runs.

### When a command takes a query

A command **takes a query** when its manifest says `"takesQuery": true`,
or when its first [argument](../guests/README.md#arguments) is text and
every other argument is optional (Raycast's rule, #144).

The text sent fills its first text or password argument unless that
already has a value. It is also the launch record's fallback text. When
another required argument is still empty, Pane's argument form asks for it
before the command runs. This applies to a fallback row, which never shows
the inline fields, as it does to every launch that does not come from root
search.

## Making a command a fallback

A command that takes a query also has a row "Fallback: Echo", subtitled
"Off · Offer it below the results for any text typed" or "On · …". Enter
turns it on ("Echo is now offered for any text typed in root search") or
off ("Echo is no longer a fallback").

For any query that is not blank, each fallback is listed **below every
other result**, in the order you turned them on, subtitled "Send “zqx” ·
fallback".

When nothing else matches, root search selects the first fallback, so
Enter sends it the whole query, trimmed (ADR 0031). Its notice that
nothing matched shows above the fallbacks. When anything else is listed,
the fallbacks stay below it, unselected, until you move to one.

## What keeps and removes them

### Where they are recorded

Aliases and fallbacks are Pane's own records, not the extension's data.
They live in `aliases.json` beside `installed.json`, keyed by command id
(the package identity's key and the manifest's command id):

```json
{ "version": 1, "aliases": { "local:/…#echo": "ec" }, "fallbacks": ["local:/…#echo"] }
```

The file is written atomically, one change at a time, and each write holds
the latest choices. A change that cannot be written goes back to what was
last recorded, unless its package was uninstalled meanwhile: an
uninstalled package's choices never come back. An unreadable file is not
overwritten, and a change then explains why. The global hotkeys'
`hotkeys.json` is kept the same way (one shared record type,
`launcher/choices.rs`).

A command id is `<package identity key>#<manifest id>`. A manifest command
id cannot contain `#`, so the package's part is exact. Uninstalling
`local:/x` forgets `local:/x#echo`, never `local:/x#y#echo` (hotkeys
alike).

Because they are kept by command id, copies of a package from other
sources have their own, even with the same titles. Manage extensions tells
each apart by its source.

### What happens to them

- A [root provider](root-search.md#root-providers) (#164), such as the
  calculator, has no row, so it has no alias or fallback. Setting one is
  refused. Any recorded before its command became a root provider are
  forgotten at the next start, with a toast saying so.
- **Disabling** a package removes its commands' aliases and fallbacks from
  root search at once. They stay recorded, and their rows stay in Manage
  extensions with "Not active: Query sample is disabled". Changing them
  there does not enable the package, and nothing else does: only your
  enable in Settings › Extensions brings them back.
- A **paused** package's command, and a command **unavailable on this
  system**, stay reachable by their alias and fallback rows. Those rows
  explain why and run nothing, as the command's own row does. In
  Settings › Extensions they say "Not active: Query sample is paused after
  an error; retry it in Settings › Extensions" or "Not active: Not
  available on Linux: this command supports only Windows".
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
TypeScript alike. Nothing is declared for that.

Taking a query needs `"takesQuery": true` on the command in `pane.json`.
Since #138 the text arrives as the **fallback text** of the command's
launch record ([`wit/commands.wit`](../wit/commands.wit)), trimmed, through
the entry point its mode has:

- A no-view command (`"mode": "no-view"`) runs (`run`) and opens no
  screen. Its answer shows while root search stays as it was, which is
  what users saw of a query-taking command before.
- A view command opens its screen (`render`) with the text in the record.

The separate `run-query` interface (#31's `pane:extension/query-command`)
is retired.

The [author guide](../guests/README.md#a-command-that-takes-a-query) shows
it in Rust and in JavaScript and TypeScript. Echo, the query sample, is a
no-view command in [`guests/sample-query`](../guests/sample-query),
[`sample-query-js`](../guests/sample-query-js) and
[`sample-query-ts`](../guests/sample-query-ts).

An error the command answers is shown and never counts towards
[pausing](pausing.md). A trap does.

## Checks

- Launcher public interface,
  [`crates/pane-core/tests/aliases.rs`](../crates/pane-core/tests/aliases.rs).
  The alias, fallback, cleared answer, disable, copies and pausing checks
  use the real query sample in Rust, JavaScript and TypeScript. Checked:
  - an alias set in its form is recorded;
  - the alias alone lists Echo first, and "ec hello  world " lists the row
    that sends "hello  world", with Echo's guest not started while typing;
  - Enter shows its answer and keeps root search; an error answer is
    shown;
  - the alias is kept after a restart; the form starts with the alias and
    an empty one removes it;
  - an alias ranks above a computed result (the calculator's);
  - a fallback is listed last for any text, the first selected when
    nothing else matches, so Enter sends the query without a move, and
    unselected below a match; not for a blank query; turned off again;
  - Echo does not start while typing;
  - the answer is cleared when the query changes, and one arriving after
    the change is not shown;
  - three crashes of the query "crash" pause the package; its alias row
    explains it and runs nothing;
  - refusals: another command's alias "straße" given as "STRASSE", a
    space, 33 characters;
  - a command that takes no query found by its alias, with no row for text
    after it and no fallback row;
  - a conflict in a record edited by hand shown, and neither alias used;
  - disabling removes both from root search and shows them as not active;
    changing them does not enable the package, nor does a restart;
    enabling brings both back;
  - two copies of the package from different folders: the alias finds and
    runs only the copy it was given to (checked with the runtime's running
    components), and the alias and fallback rows name their sources;
  - a command unavailable on this system and a package that cannot load
    shown as not active, with why;
  - a missing command's choices shown and forgotten;
  - uninstall forgets them, exactly its own (a copy from folder `x#y` keeps
    its alias when `x` is uninstalled; the same for hotkeys in
    `hotkeys.rs`);
  - a write that fails after an uninstall does not bring the alias back;
  - Echo opened from its row, with no text, runs and opens no screen.
- [`no_view.rs`](../crates/pane-core/tests/no_view.rs): the launch record's
  source (alias or fallback), and a view command opened through its alias
  with text.
- [`arguments.rs`](../crates/pane-core/tests/arguments.rs): what an alias
  and a space opens. That is the inline fields of a command that declares
  arguments, run from its alias with the first argument filled; the row
  that sends the text, kept for a command that takes a query with none;
  and a command without either, opened at once.
- [`crates/pane/tests/arguments.rs`](../crates/pane/tests/arguments.rs):
  the same with real keys, including a space held with the query's list.
- Window, [`crates/pane/tests/aliases.rs`](../crates/pane/tests/aliases.rs),
  on GPUI's test platform with real key events. Rows are chosen by title.
  Checked:
  - the alias typed in its form and saved, the form reopened filled with
    it, the fallback turned on;
  - "ec hello" and Enter show Echo's answer;
  - "zqx words" shows the notice above the first fallback, preselected
    (its accessibility node is the selection), and typing starts nothing
    (checked once the runtime has served every call: Echo, stopped before,
    has not started again);
  - Enter, and Ctrl+1 as for any row, send the trimmed query.
- Native GUI smokes: one identical phase on all three systems (screenshots
  66 to 74), each with data folders of its own. It installs the query
  sample, sets the alias "ec" and the fallback in Settings › Extensions,
  sends "ec hello", then "zqx", which Enter hands to the preselected first
  fallback (ADR 0031), and checks `aliases.json`. Then it restarts,
  disables the extension and checks that "ec hello" gives the same screen
  as a Pane with nothing installed. See the
  [Linux](platforms/linux.md#aliases-and-fallbacks-31),
  [macOS](platforms/macos.md#aliases-and-fallbacks-31) and
  [Windows](platforms/windows.md#aliases-and-fallbacks-31) notes for where
  it has run.

## Limits

- One alias per command, and only for installed packages' commands (not
  the samples this build supplies). An alias is one word, compared
  caselessly (above). There are no aliases for indexed or computed results
  (applications, quicklinks).
- The fallback rows are not ranked or limited: every fallback is listed
  for every non-blank query.
- No per-command placeholder or argument parsing: the command gets one
  string.
- The answer is shown as text on the status line. A command that takes a
  query cannot open its view with the query, or a form or list for it.
- The query is sent when the row is invoked. The call is not cancelled
  when you type on (its late answer is discarded). Calls run one at a time
  on the runtime thread (#29, #18).
- Screen readers: fallback rows are ordinary options of the results list,
  and the combo box itself is reported as focused, whatever is selected.
  No screen reader was run ([root search](root-search.md#accessibility)).

## Tracking

Added for [#31](https://github.com/pane-app/pane/issues/31): US10, US11;
T03; contributions to G2, not claims that it passes.
