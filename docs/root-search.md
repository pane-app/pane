# Root search

Added for [#23](https://github.com/hoangvu12/pane/issues/23) (US09, US14, T01,
T02, T03, G2, G4). Root search now has a query field: typing narrows root to
the matching commands, best match first, and Enter invokes the selected one.
Only command metadata from `pane.json` (and the commands built into Pane) is
searched; no extension runs until the user invokes one of its commands. This
is a first matching and ranking, not tuned relevance.
[#27](https://github.com/hoangvu12/pane/issues/27) (US06, US12, T01, T03)
adds [results computed from the query](#results-computed-from-the-query),
with [the calculator](#the-calculator) as a default extension.
[#24, #25 and #26](applications.md) add [results supplied ahead of the
query](#results-supplied-ahead-of-the-query), with the installed
applications as a default extension.
[#28](https://github.com/hoangvu12/pane/issues/28) adds
[quicklinks](quicklinks.md), computed results that open a saved web
address. File search, aliases, fallback actions and hotkeys (#29 to #34)
are not part of it.

## What is searched

Root search lists **root results**, in this order when the query is empty
(for a query that is not blank, the [computed results](#results-computed-from-the-query)
for it come first, once they arrive):

1. the commands built into this Pane build (the three samples);
2. the commands of each enabled installed package, in install order, and,
   for a query that is not blank only, the [results supplied ahead of the
   query](#results-supplied-ahead-of-the-query), such as the installed
   applications, after them;
3. an enabled installed package whose managed copy cannot be read, as one row
   explaining the problem;
4. Pane's own rows: "Install extension from folder…" and "Manage
   extensions…".

A **disabled** package contributes nothing ([#10](https://github.com/hoangvu12/pane/issues/10)):
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
| 1 | is the query | Download |
| 2 | starts with the query | Downloader |
| 3 | has a word starting with each query word | Recent downloads |
| 4 | contains each query word | Undownloadable files |
| 5 | (a word is only in the subtitle) | Clear cache, "Delete downloaded files" |
| 6 | (a word is only in the package title) | a command of package "Downloads" with a subtitle of its own |

Results of the same rank keep root search order. A blank query lists every
root result. The best match is selected after every change of the query;
searching the same query again changes nothing. Each result's text is
normalized once, when root search is shown or its results are rebuilt,
not on every keystroke.

Not done, deliberately: typo tolerance, abbreviations ("ts" for TypeScript
sample), accent folding ("e" finding "é"), locale-aware case folding,
frequency or recency, per-user ranking, keywords or aliases in the manifest,
and ranking results of different kinds (apps, files) against each other.

## Host behavior

The public host interface is [`pane_core::Launcher`](../crates/pane-core/src/launcher.rs):
`Screen::Root`, which carries the query, `set_query`,
`move_selection`, `activate_selected` and `back`. The window renders that
state and maps input to those calls.

| Input | On root search |
| --- | --- |
| Typing, editing keys, clipboard, undo, input-method composition | Edit the query (GPUI CE's single-line editable text element); every change searches again |
| Up / Down | Previous / next result (not the caret) |
| Enter, or a click on a result | Invoke the selected result: open the command, explain an unavailable or unreadable one, open Pane's own screen, copy a computed result's text to the clipboard ("Copied 42 to the clipboard"; root search stays as it was), or open an application ("Opened Firefox"; root search stays as it was) |
| Escape | Clear the query; with an empty query, nothing |

The query field has keyboard focus whenever root search is on screen: when
Pane starts and whenever the user returns to root search. Returning to root
search (Escape from a command, after an install or update) starts with an
empty query. Opening a command moves focus to its list.

A **missing result is not a failed action**: a query that matches nothing
shows "No results for “…”", selects nothing, and Enter then does nothing;
the status line stays idle. A result that matches but fails when invoked
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
  command with `rootResults` for `results-for(query)`, one after another in
  install order, and lists each command's results as soon as it answers, so
  a slow command does not hide the answers of those asked before it. The
  window awaits it off its thread and redraws, so typing never waits for an
  extension. Answers for an older query or an earlier search of the same
  query (or after leaving root search) are discarded; until a command
  answers, the new query lists none of its results, never an older
  query's. The guest's work for an older query is not cancelled, and calls
  run one at a time on the runtime thread, so a slow or hung command still
  delays every command asked after it, for this query and the next ones
  ([#29](https://github.com/hoangvu12/pane/issues/29) owns cancellation,
  [#18](https://github.com/hoangvu12/pane/issues/18) timeouts).
- Computed results are listed **above** every title match, in the order the
  commands and their answers give them; they are not ranked against titles
  (provisional, pending user confirmation; see
  [current decisions](current-decisions.md) item 10).
  When each command's results arrive the first row is selected again, unless the user had
  moved the selection, which stays on its row.
- A computed result has an id (`<command id>:<result id>`), title, optional
  subtitle and an **action** Pane performs without calling the extension
  again. **copy**: Enter copies the text to the
  clipboard, which the window writes (`Launcher::selected_copy`), and the
  status says "Copied … to the clipboard". **open-url** (since #28): Enter
  opens an `http://` or `https://` address with the launcher's link opener,
  the system's handler in the window; any other address is refused
  ([opening a link](quicklinks.md#opening-a-link)).
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
  search. The guest's work is not cancelled; calls run one at a time (#29).
- They are listed only for a query that is not blank, ranked by title,
  subtitle and rank exactly as commands are; on the same rank they come
  after commands.
- An indexed result has an id (`<command id>:<result id>`), title, optional
  subtitle and an **action** Pane performs without calling the extension
  again. The only action is **open-application**: Pane opens the application
  through the host's [applications adapter](applications.md) and says
  "Opened …" or "Could not open …: <why>".
- An error or crash of the extension is listed as a row titled with the
  command and "Could not list: …" for every query that is not blank; Enter
  on it shows the whole error.
- A **disabled** or updated package's indexed results leave at once and an
  answer still on its way is discarded; enabled again, it is asked with the
  next query.

### The calculator

The calculator ([`guests/calculator`](../guests/calculator)) is a default
extension in Rust: package `guests/packages/calculator`, command
"Calculator", which lists the expressions it understands, and computed
results for root search. It is not part of the core and can be disabled
like any package. Acquiring it automatically at setup is
[#51](https://github.com/hoangvu12/pane/issues/51) to
[#53](https://github.com/hoangvu12/pane/issues/53); until then it is
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
- parentheses, nested at most 64 deep, and spaces anywhere;
- at most 256 characters in all.

A query has an answer only if it applies at least one operator: "42" or
"(5)" is not a calculation. Everything else has no answer and lists nothing:
incomplete input ("2 +", "(1 + 2"), invalid input ("2 + * 3", "2 3",
letters, functions, constants, units, percentages, deeper nesting or a
longer query, so that no query can exhaust the guest's stack), and undefined or
unrepresentable values ("1 / 0", "2 ^ 0.5", overflow). Arithmetic is IEEE
double precision; the answer shows at most 15 significant digits and at most
10 decimals, without trailing zeros (0.1 + 0.2 is 0.3, 1 / 3 is
0.3333333333), and scientific notation from 10^15 up or below 10^-6
(`1.00000000000001e15`, `1e-7`). The row shows the answer as its title and
"<query> = <answer> · Enter copies the answer" as its subtitle.

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
What is *not* settled here, and is left to the tickets that need it: results
from a provider that must search something per query outside Pane (files),
provider-supplied ranks and how they mix with title matching,
cancelling a provider's work, and online providers, which stay inside their
own command (US11, T03): nothing in root search queries an online service.

### Open: extension points later tickets need

Results computed from the query exist since #27 (above), with discarding of
late answers but no ranks of their own and no cancellation. Still open, to be
designed by their tickets:

- **Asynchronous, cancellable providers** ([#29](https://github.com/hoangvu12/pane/issues/29)):
  a provider that must run to answer (files), whose late
  answers for an older query are discarded and whose work is cancelled when
  the query changes.
- **Aliases and fallbacks** ([#31](https://github.com/hoangvu12/pane/issues/31)):
  extra words a result is found by, and actions offered when nothing
  matches.

A package's commands and root results leave root search when it is
disabled or [uninstalled](extension-data.md#uninstalling-an-extension)
([#40](https://github.com/hoangvu12/pane/issues/40)), and change when it is
updated.

## Accessibility

Checked through GPUI's accessibility tree
(`Window::debug_a11y_tree_json`) in the window tests:

- The query field and the results form one `EditableComboBox` node labelled
  "Search", with the query as its value and "Search commands" as its
  placeholder. It tracks the field's keyboard focus.
- The results are a `ListBox` labelled "Results" inside it, of
  `ListBoxOption`s with label, description (subtitle, and the reason when
  unavailable) and selected state.
- The selected result is the combo box's active descendant, so it is
  reported as focused while the caret stays in the field; with no result, the
  combo box itself is reported as focused.

**Open: active descendant.** GPUI CE has no real active-descendant support:
it implements it by reporting the descendant as the focused node, not
through AccessKit's `active_descendant` property on the field. So the
selected result is reported as focused while the caret is in the field, and
a screen reader may announce results instead of echoing what the user types.
This needs a GPUI CE change (exposing `active_descendant` on the focused
node) or a workaround before G2 (screen-reader-usable root search) can
pass. **No screen reader was run** on any platform; announcements, echo and
the combo box pattern's behaviour with Narrator/NVDA, VoiceOver and Orca are
unverified. The limits listed for
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
same computed result ("reverse <text>") in Rust, JavaScript and TypeScript
([`samples.rs`](../crates/pane-core/tests/samples.rs)).

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
results.

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
