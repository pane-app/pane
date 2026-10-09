# Root search

Added for [#23](https://github.com/pane-app/pane/issues/23) (US09, US14, T01,
T02, T03, G2, G4). Root search now has a query field: typing narrows root to
the matching commands, best match first, and Enter invokes the selected one.
Only command metadata from `pane.json` (and the commands built into Pane) is
searched; no extension runs until the user invokes one of its commands. This
is a first matching and ranking, not tuned relevance.
[#27](https://github.com/pane-app/pane/issues/27) (US06, US12, T01, T03)
adds [results computed from the query](#results-computed-from-the-query),
with [the calculator](#the-calculator) as a default extension.
[#24, #25 and #26](applications.md) add [results supplied ahead of the
query](#results-supplied-ahead-of-the-query), with the installed
applications as a default extension.
[#28](https://github.com/pane-app/pane/issues/28) adds
[quicklinks](quicklinks.md), which open a saved link, file, folder or
application; #149 made them indexed results ranked with commands. [#31](https://github.com/pane-app/pane/issues/31) adds
[aliases and fallbacks](aliases.md), which the user gives installed
commands; [global hotkeys](hotkeys.md) (#32 to #34) open a command from any
application. [#29](https://github.com/pane-app/pane/issues/29) adds
[file search](files.md) within one folder the user chose, whose file
results are computed results listed after the title matches, and makes a
search cancel its pending calls for computed results;
[#175](https://github.com/pane-app/pane/issues/175) moves file search to
the host's [file index](files.md) of the home folder (below).

## What is searched

Root search lists **root results**, in this order when the query is empty
(for a query that is not blank, the [computed results](#results-computed-from-the-query)
for it come first, once they arrive):

1. the commands built into this Pane build: none in any build since
   [#162](https://github.com/pane-app/pane/issues/162) (the Rust,
   JavaScript and TypeScript samples are installed by hand with
   `pane --install`, and a test registers its own);
2. the commands of each enabled installed package, in install order, and,
   for a query that is not blank only, the [results supplied ahead of the
   query](#results-supplied-ahead-of-the-query), such as the installed
   applications, after them;
3. an enabled installed package whose managed copy cannot be read, as one row
   explaining the problem;
4. Pane's own rows: "Install extension from folder…", "Install extension
   from npm…" ([npm](npm.md)), "Install extension from Git…" ([Git](git.md)),
   "Manage Extensions" and "Settings…". Extensions are installed and
   managed in Settings ([ADR 0043](adr/0043-extensions-are-managed-in-settings-one-page-per-extension.md),
   #168): "Manage Extensions" opens Settings at its Extensions group, and
   the install rows open its install flow there — the folder picker, or
   the field for an npm package or a Git repository, then the package's
   preview with its Install. The launcher itself has no screen for
   extensions.

For a query that is not blank, a command whose [alias](aliases.md) the
query is, or starts with, comes before everything (computed results
included), and the [fallbacks](aliases.md#making-a-command-a-fallback)
after everything; a fallback is never selected by itself.

A **disabled** package contributes nothing ([#10](https://github.com/pane-app/pane/issues/10)):
its commands leave the results at once, even while the choice is being
recorded, and come back when it is enabled again. A command
[unavailable](platform-availability.md) on this system is still found, with
its reason; invoking it shows the reason and runs nothing. When an install,
update or enable/disable finishes while the user is searching, the results
are rebuilt for the same query and the selection stays on the same row if
it still matches.

## Matching and ranking

Implemented in [`crates/pane-core/src/search.rs`](../crates/pane-core/src/search.rs).
The query and each result's title, subtitle and, for an installed command,
its package's title are compared after three steps: Unicode NFC
normalization, so "é" typed as one character matches "e" followed by a
combining accent; full Unicode lowercasing (`to_lowercase`); and collapsing
every run of whitespace, including leading and trailing spaces, to a single
space, in titles as in the query. Lowercasing is not locale-aware case
folding: language rules such as Turkish dotted and dotless I are out of
scope. A result matches when **every word** of the query appears in its
title, subtitle or package title. An installed command without its own
subtitle shows its package title as the subtitle; one with its own subtitle
is still found by its package title, below everything else. Matches are
ranked by how well the title matches:

| Rank | The title… | Query "download" |
| --- | --- | --- |
| 0 | (the query is the alias the user gave it; listed above computed results) | a command with the alias "download" |
| 1 | is the query | Download |
| 2 | starts with the query | Downloader |
| 3 | has a word starting with each query word | Recent downloads |
| 4 | contains each query word | Undownloadable files |
| 5 | (a word is only in the subtitle) | Clear cache, "Delete downloaded files" |
| 6 | (a word is only in the package title) | a command of package "Downloads" with a subtitle of its own |

An [indexed result](#results-supplied-ahead-of-the-query) may also have
**alternate titles** (an application's untranslated name or its program's
name, such as `wt` for Windows Terminal), each matched as the title is,
the best of them giving the rank, and **keywords**, matched as the subtitle
is (rank 5). The row still shows its real title (#170).

Results of the same rank keep root search order. A blank query lists every
root result. The best match is selected after every change of the query;
searching the same query again changes nothing. Each result's text is
normalized once, when root search is shown or its results are rebuilt,
not on every keystroke.

Not done, deliberately: typo tolerance, abbreviations ("ts" for TypeScript
sample), accent folding ("e" finding "é"), locale-aware case folding,
frequency or recency, per-user ranking, keywords or aliases in the manifest
(aliases are the user's, [#31](aliases.md); keywords and alternate titles
exist only for indexed results), and ranking results of
different kinds (apps, files) against each other.

## Host behavior

The public host interface is [`pane_core::Launcher`](../crates/pane-core/src/launcher.rs):
`Screen::Root`, which carries the query, `set_query`,
`move_selection`, `activate_selected` and `back`. The window renders that
state and maps input to those calls.

| Input | On root search |
| --- | --- |
| Typing, editing keys, clipboard, undo, input-method composition | Edit the query (GPUI CE's single-line editable text element); every change searches again |
| Up / Down | Previous / next result (not the caret) |
| Alt+P / Alt+N (the Keyboard page's Emacs navigation bindings) or Alt+K / Alt+J (its Vim Motions), Control instead of Alt on macOS | Previous / next result too, beside Up and Down, while that set is chosen (the default is None). Raycast for Windows puts these sets on Alt as well; its Alt+B / Alt+F and Alt+H / Alt+L move left and right in its grids, and Pane has no left or right selection to give them, so they stay unbound |
| Moving the pointer over a result | Select it, so the footer's action and Enter act on it; a pointer resting on a result never undoes the keys' selection, and while a layer over the list owns the target (the Actions panel, the Pane menu) the pointer selects nothing. The first pointer event after the window shows only records where the pointer is |
| A click on a result | The selected result: invoke it, as Enter does. An unselected one (the keys moved the selection away while the pointer rested on it): select it; a second click invokes it |
| Enter | Invoke the selected result: open the command, explain an unavailable or unreadable one, open Pane's own screen, copy a computed result's text to the clipboard ("Copied 42 to the clipboard"; root search stays as it was), open an application ("Opened Firefox"; root search stays as it was), or send the text to a command that takes a query, through its alias or as a fallback, and show its answer (root search stays as it was) |
| Escape | Clear the query; with an empty query, nothing |
| Ctrl+K (Cmd+K on macOS; the Keyboard page's Open actions), or the footer's Actions button | Open the selected result's Actions panel, or close it |

**The Actions panel** (#95) lists what can be done with the selected
result, from the core's `Launcher::result_actions`: its primary action
(the footer's, with the same dispatch), then, under "Pane", "Pin" for a
command or an indexed result, or "Unpin" once it is pinned (see [the
pinned home](#the-pinned-home)) and, for an installed command, "Assign
Hotkey…"/"Change Hotkey…" and "Add Alias…"/"Change Alias…", which open the
same hotkey screen and alias form the extension list does and return to this
search when they end. Nothing without a working operation is listed: no new
window, file manager, quit or hide (#100). Its search field holds focus: typing filters
the entries by label ("No actions match" when none does), Up and Down move
the selection, Enter or a click runs the entry once, and Escape (or Tab)
closes only the panel, giving focus back to the query. A mouse-down outside
it closes it and is consumed, so the result it covered is never invoked.
The panel holds its target: the pointer cannot move the selection while it
is open, and an entry whose target is no longer selected, or no longer has
that action, runs nothing. With no result selected it says so.

### The pinned home

A blank query shows the **pinned home** (#101) above the results: the
"Pinned" label, with the slots' chord, and the **quick slots**, an ordered
list of pins with no gaps and no limit
([ADR 0027](adr/0027-quick-slots-are-an-ordered-list.md)). A query
whose trimmed text is not blank hides it; clearing the query brings it
back. The results below keep their own order and their "Commands" label:
Pane lists no suggestions of recent use.

- **What a slot holds** is an identity, never a row: a registered command
  by its id, or an indexed result (an installed application) by its own id
  under the command that supplies it. A computed answer, a file or Pane's
  own rows cannot be pinned. A fresh installation pins nothing.
- **Same-title pins:** a pinned indexed result whose title another result
  of its command shares (two applications of one name) says what tells it
  apart, its row's subtitle (an application's
  [distinction](applications.md#names)), as its tile's tooltip, its
  vertical row's subtitle and its accessible description (#170).
- **Pinning:** the Actions panel's "Pin" adds the selected result at the
  end of the list ("Pinned …"); there is no slot to choose and nothing is
  replaced. Pinning what is already pinned changes nothing, says so and
  moves focus to its slot, clearing a typed query so the home and that
  slot show. On a row already pinned the panel offers "Unpin" instead.
- **A slot's own actions** — a secondary click on it, or Ctrl+K while it
  has focus — open it, unpin it ("Unpin", which closes the gap) or move it
  ("Move Up"/"Move Down", shown as "Move Left"/"Move Right" in the
  horizontal layout; each swaps it with its neighbour, never past either
  end).
- **Keys** (default keys; Cmd for Ctrl on macOS): Ctrl+Shift+F pins or
  unpins the focused slot, or root search's selected row; Ctrl+Alt+Up or
  Left and Ctrl+Alt+Down or Right move the focused slot.
- **Layout:** the Launcher page chooses horizontal (the default), a grid
  of tiles five columns wide that wraps onto more rows, with one dashed
  "+ Pin" hint tile in the next free cell of the last row (never starting
  a row of its own; with no pins, the hint alone), or vertical, result rows
  of the pinned results only (nothing with no pins). In the
  [compact window mode](../CONTEXT.md) the pins show only if "Show pinned
  in Compact mode" is on (`compactPinned` in the settings record, off by
  default): a row of small icons under the search field while the window
  is collapsed.
- **Invoking a slot:** a click, Enter or Space while it has focus, or its
  chord: only the first five slots are numbered, Ctrl+1 to Ctrl+5, and the
  numbers after the numbered slots, up to Ctrl+9, pick the first rows
  below the home. With two slots, Ctrl+3 to Ctrl+9 pick the first seven
  rows; with eight, Ctrl+1 to Ctrl+5 pick slots 1 to 5, slots 6 to 8 have
  no number, and Ctrl+6 to Ctrl+9 pick the first four rows. With a query,
  or in a command's list, Ctrl+1 to Ctrl+9 pick the first rows. Nothing
  names the chords at rest: holding Ctrl alone for
  400 ms slides each item's number in — a row's over its right end, a
  slot's down into its corner (in compact mode, the first five icons') —
  and releasing it slides them away. The
  chords are the root search field's and the
  slots' own, never registered with the system, and act only on root
  search, with no overlay open, no input-method composition in the query
  and no action already running; a held chord's repeats and a double
  click's second click run nothing more. The slot is resolved again then,
  and only a target that can run is run; a click keeps the query focused.
- **Resolution:** each slot is resolved through what is enabled now. A
  disabled, paused or missing target, or an application its command has
  not listed yet, keeps its slot and its name and says why it cannot run;
  it can always be removed, and enabling or installing the same identity
  resolves it again. Showing root search asks a pinned application's
  command for its results if it never answered, without typing a query.
- **The record** is `quick-slots.json` in Pane's data folder, beside
  `settings.json` (see [ADR 0026](adr/0026-host-keeps-quick-slots-by-identity.md)
  and [ADR 0027](adr/0027-quick-slots-are-an-ordered-list.md)), version 2:
  `{ "version": 2, "pins": [ { "command": "<id>" }, { "command": "<id>", "result": "<id>" } ] }`,
  the pins in order. A version 1 record (five positional `slots`, `null`
  for an empty one) is read as the list of its pins in order, and the next
  change writes version 2. It is written atomically one write at a time; a
  write that fails puts back what the record holds and says why, and a
  record of an unknown version, or one that is not valid, is reported and
  never replaced.

**The footer** shows the Pane mark at its left — the button of Pane's own
menu (Settings), a Windows/Pane adaptation of the reference's decorative
mark — then, while the panel is open, the hint "Type to filter actions ·
Esc goes back" — and at its right the primary action and the Actions
button, whose keys say how each is pressed. While a status shows, its
message takes the strip and the buttons step aside.

**An opened command** has no heading line above its content
([#162](https://github.com/pane-app/pane/issues/162)): its list, its
search, a form or a custom view opened from it, Clipboard History and
Manage extensions start with their content, as Raycast's views do. The
footer's left names it instead, after the Pane mark, while no hint or
status takes that place: the command's icon (its own, as its package
ships it; Pane's tile for Pane's own screens) and the screen's title — the
command's on its list, a form's or a custom view's on those ("Search
Files", "Clipboard History"). Section labels inside a list ("Today",
"Commands") stay. The core's own screens — a package's preview, a
confirmation, the details and hotkey screens — keep the heading that says
what they are about.

Each row shows what the launcher knows beyond its title and subtitle, from a read-only presentation (`Launcher::presentation`). That is:

- the row's kind (Command, Application, File, Folder, Link or Fallback), taken from what activating it does;
- the alias and the registered global hotkey the user gave its command;
- the part of its title the query matched, in the accent.

Rows sit under section labels: "Commands" over a blank query's list (root search's own order, with no claim of recent use), "Results" with their count over a query's, a computed answer under the title of the command that computed it ("Calculator") or, since #196, under the section the answer's own detail names (the calculator's colours under "Color", its dates and times under "Date & Time"), the files found for the query with the row searching them all under "Files" (#175), and the fallbacks under "Fallbacks" (below the no-results notice when nothing else matched). The presentation changes nothing about what is listed, its order, or what a row does.

**A computed answer** (#96) — a computed result whose action copies
text, such as the calculator's — is drawn as the reference calculator
board's card: what was typed, an arrow, and the answer, in Geist Mono at
the board's 34px, or at 24 or 18 when the longer of the two would not fit
its column (past that it wraps). The card shows only what the launcher
holds (`ComputedAnswer`: the query, the text Enter copies, the command
and, since #196, the colour of its swatch when the answer is one — see
[the calculator](#the-calculator)): the board's units, "Also" conversions
and recent calculations have no provider and are not drawn. It is a row
like any other — selected first, moved to by the keys or the pointer, its
primary action "Copy answer", named "6*7 = 42" for assistive technology
(the swatch is a colour well named by the colour's value) — and its
accent ring shows while it is selected.

**The no-results notice** (#96) heads the list while nothing but
fallbacks is listed for a query that is not blank: "Nothing matches
“…”", then "Pick a fallback below, or install an extension that knows
about it." (or, with no fallback, where one is offered: Manage
extensions). It stays above the fallbacks whichever is selected; Pane
searches commands, applications and the files of the home folder (and
the folders the user adds), so it claims no search of the whole computer, and it suggests no extensions, having no
store to suggest them from.

The query field has keyboard focus whenever root search is on screen: when
Pane starts and whenever the user returns to root search. Returning to root
search (Escape from a command, after an install or update) starts with an
empty query. Opening a command moves focus to its list.

A **missing result is not a failed action**: a query that matches nothing
shows the no-results notice ("Nothing matches “…”"), selects nothing, and
Enter then does nothing; the status line stays idle. The
[fallbacks](aliases.md#making-a-command-a-fallback), if the user has any,
are listed below it, unselected: Down selects the first, and the notice
stays above it. A result that matches but fails when invoked
(its component is missing, the runtime is unavailable, the guest reports an
error) shows the failure as the status error, as before this slice.

## Results computed from the query

A command can also answer the query itself, where matching titles cannot: a
calculator's answer to "6*7" matches no title. Such a **computed result**
comes from the extension, through the same guest boundary as its command:

- The command's `pane.json` entry sets `"rootResults": true`, and its
  component exports `pane:extension/root-results`
  ([`wit/root-results.wit`](../wit/root-results.wit)) besides `command`;
  Pane checks both at install, without running it
  ([author guide](../guests/README.md#root-results-computed-from-the-query),
  in Rust, JavaScript and TypeScript).
- For every change of a query that is not blank, `Launcher::set_query`
  ranks the metadata at once and returns a future that asks each enabled
  command with `rootResults` for `results-for(query, at)`, one after another
  in install order, and lists each command's results as soon as it answers,
  so a slow command does not hide the answers of those asked before it.
  `at` is when the query is asked about — the moment the user stopped at
  it, by the clock root search's own dates are shown by, with the local
  time's offset from UTC — so a command can answer about the current date
  or time (the calculator's "now" and "today", #196); the whole search
  shares one moment, whatever a slow command delays. The window awaits it
  off its thread and redraws, so typing never waits for an extension.
  Answers for an older query or an earlier search of the same
  query (or after leaving root search) are discarded; until a command
  answers, the new query lists none of its results, never an older
  query's. Since [#29](https://github.com/pane-app/pane/issues/29) the
  search owns these calls: once the query changes or root search is left,
  a call still pending is cancelled, never started if it was queued, and
  dropped with the command's instance if it was waiting inside the guest
  (on an async import); that is not a failure
  for [pausing](pausing.md), and the next query starts a fresh instance
  ([cancelling](files.md#cancelling-a-pending-search)). Calls still run one
  at a time on the runtime thread; since
  [#18](https://github.com/pane-app/pane/issues/18) every guest yields at
  each epoch tick, so a computing command is cancelled within a tick too,
  and one that computes for 5 seconds without finishing is stopped as an
  unresponsive call ([pausing](pausing.md#when-an-extension-stops-responding)).
- Computed results are listed **above** every title match, in the order the
  commands and their answers give them; they are not ranked against titles
  (provisional, pending user confirmation; see
  [current decisions](current-decisions.md) item 10). Those that open a
  file (**open-file**, since #29) are listed **after** the title matches
  instead, since a folder can hold many files matching a short query; of
  the file index's entries (#175), at most 5 per command, followed by a
  row "<command> for “<query>”" when the command searches in its own field
  (Search Files for “plan”), which opens it with the query typed.
  When each command's results arrive the first row is selected again, unless the user had
  moved the selection, which stays on its row.
- A computed result has an id (`<command id>:<result id>`), title, optional
  subtitle and an **action** Pane performs without calling the extension
  again. **copy**: Enter copies the text to the
  clipboard, which the window writes (`Launcher::selected_copy`), and the
  status says "Copied … to the clipboard". **open-url** (since #28): Enter
  opens an address of any scheme (ADR 0037, #145) with the launcher's link
  opener, the system's handler in the window. **open-file** (since
  #29): Enter opens an entry of the file index (#175) or a file of the
  package's granted folder, named by the id the host gave it, with the
  system's handler for its type (a folder in the file manager), once the
  host has checked it again; the row shows the host's name for the entry,
  its folder, the system's icon for its path and its kind, File or Folder.
  Enter on a program the index found shows it in the file manager and
  never runs it ([files](files.md#opening)).
- **No result is not a failure**: a query the command cannot answer (words,
  an incomplete or invalid expression) gives no results and the status is
  untouched. An error or crash of the extension is shown as a row titled
  with the command and "Could not answer: …"; Enter on it shows the whole
  error. Other results are listed as usual, and after a crash the next query
  starts a fresh instance.
- A **disabled** package is not asked, and its computed results leave the
  results at once, even while the choice is being recorded. Enabled again, it
  answers from the next change of the query.

## Results supplied ahead of the query

Some results do not depend on the query but must be found outside Pane,
such as the installed applications: matching titles is right for them, but
they are not in any `pane.json`. Such an **indexed result** comes from the
extension, through the same guest boundary as its command:

- The command's `pane.json` entry sets `"indexedResults": true`, and its
  component exports `pane:extension/indexed-results`
  ([`wit/applications.wit`](../wit/applications.wit)) besides `command`;
  Pane checks both at install, without running it
  ([author guide](../guests/README.md#root-results-supplied-ahead-of-the-query),
  in Rust, JavaScript and TypeScript).
- The first change to a query that is not blank after root search is shown
  asks each enabled command with `indexedResults` for `results()`, one after
  another, after the commands computing results from the query. Pane keeps
  the answer and ranks it with the other root results on every later query,
  so typing never waits for it; until it answers, the results kept from an
  earlier visit are listed. They are asked again after each return to root
  search, and when the host's list of installed applications changes by
  itself for a command that asked for it: at once while root search shows
  a query, listed in place with the selected row kept on its result,
  otherwise at the next query ([applications](applications.md#live-list)).
  The guest's work is not cancelled; calls run one at a time (#29).
- They are listed only for a query that is not blank, ranked by title,
  subtitle and rank exactly as commands are; on the same rank they come
  after commands.
- An indexed result has an id (`<command id>:<result id>`), title, optional
  subtitle, **alternate titles** and **keywords** (both lists, empty for
  none; see [matching](#matching-and-ranking)), and an **action** Pane
  performs without calling the extension
  again: **open-application**, which opens the application through the
  host's [applications adapter](applications.md), or **open** (#149), which
  opens a target (a URL of any scheme, a file, a folder or an application),
  with an application when one is named, through the system the `system`
  host functions act on, as [quicklinks](quicklinks.md#in-root-search) do.
  Pane says "Opened …" or "Could not open …: <why>".
- An error or crash of the extension is listed as a row titled with the
  command and "Could not list: …" for every query that is not blank; Enter
  on it shows the whole error.
- A **disabled** or updated package's indexed results leave at once and an
  answer still on its way is discarded; enabled again, it is asked with the
  next query.

### Root providers

A command whose only job is to answer root search declares `"mode":
"provider"` in its `pane.json` entry
([#164](https://github.com/pane-app/pane/issues/164); the
[author guide](../guests/README.md#root-providers)). Such a **root
provider** computes results from the query (`"rootResults": true`) or
supplies them ahead of it (`"indexedResults": true`), and has no row of its
own: root search never lists it, so typing its name finds nothing of it,
and it cannot be pinned, has no alias, fallback or hotkey, is not offered by
the Actions panel or the Shortcuts page, and cannot be launched by another
command. Its results are listed as any command's are. Its extension's
card in Settings names it under the extension's switch, which turns its
results off and on. The calculator and Applications are providers, so
there is no "Calculator" or "Applications" row.

At install, a provider that declares neither `rootResults` nor
`indexedResults`, or declares what only a launched command uses (`search`,
`takesQuery`, `arguments`, a `schedule`), is refused with the reason. A
command can become a provider after the user recorded choices for it (the
calculator and Applications had rows before #164, and an update can change
a command's mode): at the next start, its pins (`quick-slots.json`),
aliases and fallbacks (`aliases.json`) and hotkey (`hotkeys.json`) are
removed and those records written, once, and a toast names what was
removed ("Calculator now only answers root search"). A pinned application,
an indexed result of Applications, keeps its slot. Until that start, a
recorded choice for a provider takes no effect.

### The calculator

The calculator ([`guests/calculator`](../guests/calculator)) is a default
extension in Rust: package `guests/packages/calculator`, a root provider
whose command, "Calculator", has no row and computes results for root
search. It is not part of the core and can be disabled like any package. Acquiring it automatically at setup is
[#51](https://github.com/pane-app/pane/issues/51) to
[#53](https://github.com/pane-app/pane/issues/53); until then it is
installed from its folder like any package
(`pane --install target/guests/packages/calculator`).

Its expression scope is deliberately small (US06; no symbolic algebra and no
arbitrary code evaluation):

- numbers with an optional decimal point: `12`, `3.5`, `.5` (no thousands
  separators, exponents or other bases);
- `+`, `-` (or `−`), `*` (or `×`), `/` (or `÷`) and `^` for a power with a
  whole-number exponent; `^` binds tightest and right to left, then `*` and
  `/`, then `+` and `-`, left to right; a leading `-` or `+` applies to what
  follows, so `-2^2` is -4, and any number of signs may lead;
- the percentage phrases (#196), within those number rules: `p% of x`,
  `p% off x` (x reduced by p%), `p% on x` (x increased), `x + p%`, `x − p%`
  and `x as a % of y`, their words the language's own, lowercase;
- parentheses, nested at most 64 deep, and spaces anywhere;
- at most 256 characters in all.

A query has an answer only if it applies at least one operator or is a
percentage phrase: "42" or "(5)" is not a calculation. Everything else has
no answer and lists nothing: incomplete input ("2 +", "(1 + 2"), invalid
input ("2 + * 3", "2 3", letters, functions, constants, units, other
percentages, deeper nesting or a longer query, so that no query can
exhaust the guest's stack), and undefined or unrepresentable values
("1 / 0", "2 ^ 0.5", "1 as a % of 0", overflow). Arithmetic is IEEE
double precision; the answer shows at most 15 significant digits and at most
10 decimals, without trailing zeros (0.1 + 0.2 is 0.3, 1 / 3 is
0.3333333333), and scientific notation from 10^15 up or below 10^-6
(`1.00000000000001e15`, `1e-7`). The row shows the answer as its title and
"<query> = <answer> · Enter copies the answer" as its subtitle; root search
draws it as a computed answer's card (above).

Since #196 the calculator answers three more kinds of query, each through
its answer detail ([`wit/root-results.wit`](../wit/root-results.wit)),
which names the section the answer sits under, a swatch and further ways
to copy it:

- **Colours**: `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` (either case, each
  short digit standing for two of it), `rgb()`, `rgba()`, `hsl()`, `hsla()`
  (comma-separated, as CSS writes them) and `oklch()` (space-separated, its
  lightness 0-1, with an optional `/ a` alpha). An argument out of range is
  clipped as CSS clips it; a colour name is not a colour, as Raycast leaves
  names out on purpose ("red" is also a word). The answer is the colour as
  uppercase hex — `#RRGGBBAA` while it is not fully opaque — drawn with a
  swatch under the section "Color", and Enter copies it. The Actions panel
  offers copying it as hex, RGB, HSL and OKLCH.
- **Date and time words**: "now", "time", "today", "date", "tomorrow" and
  "yesterday" answer at once with the local date or time in the layout
  Pane's own dates use ("Jun 1, 2025, 14:23"), under "Date & Time"; Enter
  copies it, and the Actions panel offers ISO 8601 (a date for a date word,
  the whole moment otherwise, in the local zone) and a Unix timestamp (a
  date word's at its local midnight). The moment is Pane's: the query is
  asked about at the moment the user stopped at it, by the clock root
  search's own dates are shown by, with the local time's offset from UTC,
  so a test can hold the clock still. Date arithmetic and time zones stay
  out: a query about another moment than now is not answered.

Units, currencies and conversion between them stay out (CONTEXT: Pane has
no unit conversion).

## Activation

Searching reads only what the launcher already holds: built-in command
registrations and the installed packages' managed manifests, read at start
and after installs. It never compiles or starts a component. Invoking a
result starts only that command's guest instance (lazy activation, ADR
0005). `Runtime::running` is a diagnostic listing the components with a live
instance; the tests use it to show that twelve installed packages can be
listed and searched with none running, and that invoking one starts only
that one. Background work declared by an extension does not exist yet
(no services, timers or hotkeys), so there is nothing to keep distinct from
it beyond this. A command with `rootResults` declares that it answers root
search, so its instance starts with the first query that is not blank, not
at start, install or an empty or blank query, and a disabled one never
starts.

## Minimum search-provider contract

Per [ADR 0006](adr/0006-raycast-style-search-with-extension-providers.md) the
core owns the search interface, matching and ranking, aggregation,
navigation and dispatch; features supply entries. There are two kinds of
provider: **command metadata**, contributions indexed from package
manifests and built-in registrations without running anything, and, since
#27, [commands that compute results from the query](#results-computed-from-the-query),
which run to answer and whose results the core lists first. The minimum
a root result needs, which later default features (#24 onwards) must supply
to plug in:

- an **id**, stable and unique among root results, so a refresh keeps the
  selection (installed commands use `<package identity>#<command id>`);
- a **title** and optional **subtitle**, which are what the query matches;
- an optional **unavailability reason**, which keeps the result listed and
  searchable but stops it from running;
- an **action** the core dispatches when it is invoked (today: open a
  command, explain, open one of Pane's screens, or copy or open the link of
  a computed result).

Since #24 to #26 a third provider kind exists:
[results supplied ahead of the query](#results-supplied-ahead-of-the-query),
which the core keeps and ranks like titles (the installed applications).
Since #29 a computed result can come from a provider that searches outside
Pane per query (the [files](files.md) of a granted folder, listed by the
host off the extension thread, the command asked again once the listing
ends), and a search cancels its providers' pending work. Since #175 file
search's provider asks the host's file index, which answers at once from
what is indexed and never waits for a walk, so a busy disk holds up no
other result. What is *not*
settled here, and is left to the tickets that need it: provider-supplied
ranks and how they mix with title matching, and online providers, which
stay inside their own command (US11, T03): nothing in root search queries
an online service.

### Open: extension points later tickets need

Results computed from the query exist since #27 (above), with discarding of
late answers but no ranks of their own; since
[#29](https://github.com/pane-app/pane/issues/29) a search cancels their
pending calls when the query changes or root search is left, and file
results are listed after the title matches; since #18 a provider computing
for 5 seconds without finishing is stopped. Still open: provider ranks.

Aliases and fallbacks exist since [#31](aliases.md): the user's words that
find an installed command, and commands taking a query offered for any
text; the text reaches a command only when the user invokes it.

A package's commands and root results leave root search when it is
disabled or [uninstalled](extension-data.md#uninstalling-an-extension)
([#40](https://github.com/pane-app/pane/issues/40)), and change when it is
updated.

## Accessibility

Checked through GPUI's accessibility tree
(`Window::debug_a11y_tree_json`) in the window tests:

- The query field and the results form one `EditableComboBox` node labelled
  "Search", with the query as its value and "Search apps and commands…" as its
  placeholder. It tracks the field's keyboard focus.
- The results are a `ListBox` labelled "Results" inside it, of
  `ListBoxOption`s with label, description (subtitle, and the reason when
  unavailable), selected state, and their position in the whole list and
  its size (a computed answer's card is one of them, named "6*7 = 42").
- The combo box stays the focused node while the selection moves, so a
  screen reader echoes what is typed. No result reports itself as focused:
  GPUI CE implements an active descendant by reporting the descendant as
  the focused node, so root search no longer uses one
  ([#132](https://github.com/hoangvu12/pane/issues/132)).
- The window's **announcer** says the selection instead: one hidden,
  zero-size `Label` node of Pane's own, a polite live region whose name and
  value are both the text said last (AccessKit's Windows and Linux adapters
  announce a live region's name, its macOS adapter the value). Each text
  replaces the last, so fast arrowing leaves only where the user stopped.
  It says, in the launcher's language (English today):
  - a selection move by the user (any key that moves it, a number chord,
    the pointer): "<title>, <i> of <n>", with ", unavailable" after a row
    that cannot run, and the section's name first when the move enters
    another section ("Fallbacks: Echo, 1 of 1");
  - opening a command or the extension list: "<name>, <n> results", then
    the selected row; a screen whose rows are choices (a confirmation, a
    package before installing it, why a build failed or a package paused):
    its title, then the selected row; the Actions panel: "Actions for
    <target>, <n> commands", then its selected entry; the footer menu: its
    selected entry, said again each time it opens (the same text is cleared
    for one frame, then set again);
  - root search coming back: nothing, and what was said before is cleared;
  - a list that becomes empty: "No results";
  - typing: nothing per keystroke; once the query's results have settled,
    or 300 ms after the last keystroke, whichever is later, the selected row
    if it is not the one last said;
  - a selection Pane changes itself (a late result, a refreshed list):
    nothing, unless the selected row is another one;
  - the footer's message when it is a toast or an outcome (a result, an
    error), which the strip keeps as its name under its `Status` role:
    AccessKit announces only a node with a live setting of its own, so the
    announcer says it too. "Running…" and progress are the strip's alone.
    When the message and the selection change together, the message is said
    first and the selection 500 ms later, at most: a later message does not
    hold it back again, and it is dropped if the list no longer shows it (a
    panel closed, another list or selection, typing began).
- A command's list keeps the focus on the list (or on its search field),
  the Actions panel on its search field ("Search actions"), the footer menu
  on the menu, and Clipboard History and Search Files on their fields; in
  each, rows carry their position and the list's size, and none claims the
  focus. Forms, custom views and Settings keep their own accessibility.

**Open: G2's remaining condition is a native run with each screen reader**
(Narrator and NVDA on Windows, VoiceOver on macOS, Orca on Linux), not a
GPUI change. The window tests check the focus, the rows' positions and the
announcer's name and value; GPUI CE's debug tree does not report a node's
live setting, so that the announcer is polite is read from the code, and
what a screen reader speaks is not established by any test. **No screen
reader was run** on any platform. A text the announcer says twice in a row
(the same menu entry when the menu opens again) changes nothing in the
tree, so it is not said again. The limits listed for
[form text fields](forms.md#accessibility) (no text details, actions or
invalid state) apply to the query field too.

## Checks

Through the launcher's public interface
([`crates/pane-core/tests/search.rs`](../crates/pane-core/tests/search.rs)):
the empty query, each rank in order, letter case and blank queries, spaces
inside and around titles not lowering their rank, composed and decomposed
accents matching each other, every word having to match, ties keeping
order, the same query searched again keeping the selection, selection and
invocation among the
matches, no match with Enter doing nothing versus a match that fails,
Escape clearing the query, a new search after returning to root, installed
commands found by title or package title (below subtitle matches, even
with a subtitle of their own) and Pane's own rows, disabling and
re-enabling a package under a query, an update finishing while the user
searches, an unavailable command found and explained without running, and
twelve installed packages searched with no guest running and only the
invoked one started.

For computed results and the calculator
([`crates/pane-core/tests/calculator.rs`](../crates/pane-core/tests/calculator.rs)),
with the real calculator guest: an answer listed first and selected, above a
command whose title matches too; incomplete, invalid, undefined and
operation-free queries listing nothing with the status untouched, and
completing one answering it; precedence, signs, powers and the number
format; parentheses 65 deep, a query over 256 characters and 100,000
leading signs or parentheses listing nothing, without an error row or a
restart of the calculator; Enter reporting the copy and `selected_copy` giving the text; an
answer for an older query discarded and none shown before the new one
arrives, nor one for an earlier search of the same query; the answer listed
and selected while a command asked after it (the `faulty` fixture, slow on
"0 + 0") is still answering, its result added below when it answers; a
selection the user moved kept; the calculator not running until
a non-blank query; disabling it removing its answer at once, asking it
nothing more and keeping other results, and enabling it again; a failing
and a crashing command (the `faulty` fixture) explained as a row while other
results stay, and a fresh instance afterwards; and a package declaring
`rootResults` whose component lacks the interface refused at install. The
colours, dates and percentages (#196): each colour form answering as its
uppercase hex; the colour card presented with its swatch under "Color"
while the arithmetic keeps the command's title; colour names and
wrong-shaped queries listing nothing; the Actions panel offering hex, RGB,
HSL and OKLCH, each copying its own text; each percentage phrase answered
within the number rules and the length bound, with a query that is not a
phrase listing nothing; each date and time word answered at the clock's
moment under "Date & Time" with Enter copying it, in the clock's zone —
a manual clock in the tests, east and west of UTC, across a day boundary —
and the panel's ISO 8601 and Unix timestamp copying their own text. The
same computed result ("reverse <text>") in Rust, JavaScript and TypeScript
([`samples.rs`](../crates/pane-core/tests/samples.rs)).

For root providers
([`crates/pane-core/tests/root_providers.rs`](../crates/pane-core/tests/root_providers.rs)),
with the real calculator and Applications guests and a fake system with
one application: both declare the provider mode; typing "calc",
"calculator", "applications" or "appl", or nothing, lists no row of either,
while "6*7" is still answered under "Calculator" and the application is
found by name; neither can be given an alias or a hotkey, and the
Shortcuts catalog lists neither; disabling the calculator stops its answer
and enabling it brings it back; a provider without `rootResults` or
`indexedResults`, or with `search`, `takesQuery` or a `schedule`, refused
at install with the reason; and a data folder seeded with a pin, alias,
fallback and hotkey for both, from before they became providers, losing
them at start, with a pinned application kept and a toast naming what
went, and a second start saying nothing. The toast's wording is unit-tested
in `launcher/providers.rs`.

Window checks through GPUI's test platform with real key events
([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)): typing
narrows the results and Enter opens the best match; Up/Down move through the
matches while the field keeps focus and editing keys still edit it; the
no-results state and Escape clearing the field; focus on the field at start
and after returning from a command, and typing in a command not searching
root; input-method composition searching as it composes (driven on the
field's editing state, with the limits described for
[forms](forms.md#checks)); the accessibility nodes above; and typing an
expression showing the calculator's answer as the query changes, Enter
writing it to the clipboard, and an incomplete expression showing no
results; since #196, a colour answer showing as the card with a swatch
under "Color", the swatch a colour well named by the colour's value, and
Enter copying the hex. The announcer's rules have unit tests in
`crates/pane/src/features/announcer.rs`, and window tests in
[`crates/pane/tests/announcements.rs`](../crates/pane/tests/announcements.rs):
the field keeping the focus while the user arrows and no row claiming it,
each row's position and the list's size, "<title>, <i> of <n>" as both the
announcer's name and value after Down, several fast moves leaving the last
row, typing that keeps the first row saying nothing and typing that changes
it saying it once after the results settle, "No results" and a move into
the fallbacks named "Fallbacks", opening a command saying its name and
count and then its row, the same in a command's list and in the Actions
panel (over a command's list and over root search), the footer's message
said before a selection that changed with it while the footer keeps its
status role, a sample's toast said and a move after it said at once, the
footer menu's item said and said again when it opens again, and a
selection Pane changes itself said only for another row; in
`crates/pane/tests/window.rs`, Clipboard History's field keeping the focus,
its rows' places, a move and "No results" for a filter that keeps nothing.

Native checks: the GUI smoke scripts' search phase (screenshots 24 to 26)
types "typescr", presses Enter, runs "Wait briefly" and asserts the screen
is pixel for pixel the one step 4 reached by Down; then it types a query that matches nothing, presses
Enter, and asserts root, the search and the no-results screens all differ.
The earlier phases still navigate root with Down, which moves the selection
while the field has focus. On Linux X11 this ran on 2026-09-28
([evidence](platforms/linux.md#root-search-23)); the macOS and Windows steps
are written but have not run yet. No real input method was used. A further
phase checks [the calculator](platforms/linux.md#calculator-27): its answer
row, and that pasting the copied answer back gives the same screen as
typing it; it ran on Linux X11 on 2026-09-28 and is written but not run on
macOS and Windows.

Results supplied ahead of the query and the installed applications have
their own launcher, window, adapter and native checks, listed in
[applications](applications.md#checks).
