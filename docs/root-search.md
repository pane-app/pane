# Root search

Root search is the list Pane opens on: a query field with the matching
results under it. Typing narrows the list, best match first, and Enter
invokes the selected result. This page describes what root search lists,
how it matches and ranks, what it learns from the user, and how
extensions add results to it. It is for people working on the launcher
and for extension authors whose commands answer root search.

Searching reads only command metadata: the commands in each package's
`pane.json` and the commands built into Pane. No extension runs until the
user invokes one of its commands ([activation](#activation)).

## What root search does

- Matching is fuzzy, ignores accents and has an adjustable strictness
  ([#193](https://github.com/pane-app/pane/issues/193)). The comparator
  ranks the results, keywords and other names find them, and rows that
  share a title say what tells them apart
  ([#197](https://github.com/pane-app/pane/issues/197)). See
  [matching and ranking](#matching-and-ranking).
- Extensions can add [results computed from the
  query](#results-computed-from-the-query) (#27).
  [The calculator](#the-calculator) is the default one.
- Extensions can add [results supplied ahead of the
  query](#results-supplied-ahead-of-the-query)
  ([#24, #25 and #26](applications.md)). The installed applications are
  the default one.
- Root search
  [learns from what the user chooses](#learning-from-what-the-user-chooses)
  (ADR 0030, #199). A result the user invokes earns frecency and keeps the
  queries it was chosen with, and these rank above how well titles match.
  The blank query lists the pins, then the commands and applications by
  frecency. The user controls what it learned
  ([#200](https://github.com/pane-app/pane/issues/200)): resetting one
  result from the Actions panel, and resetting everything or turning
  learning off in Settings.
- The selected row's [inline argument fields](#inline-argument-fields)
  show after the query, and an alias followed by a space opens them
  ([#205](https://github.com/pane-app/pane/issues/205)).
- Up on an empty query [recalls the recent queries](#recent-queries), with
  the argument values typed with them
  ([#206](https://github.com/pane-app/pane/issues/206)).
- [Quicklinks](quicklinks.md) open a saved link, file, folder or
  application ([#28](https://github.com/pane-app/pane/issues/28)). Since
  #149 they are indexed results ranked with commands.
- The user gives installed commands [aliases and fallbacks](aliases.md)
  ([#31](https://github.com/pane-app/pane/issues/31)), and
  [global hotkeys](hotkeys.md) (#32 to #34) open a command from any
  application.
- [File search](files.md) ([#29](https://github.com/pane-app/pane/issues/29))
  started within one folder the user chose. Its file results are computed
  results listed after the title matches, and #29 also made a search
  cancel its pending calls for computed results.
  [#175](https://github.com/pane-app/pane/issues/175) moved file search to
  the host's [file index](files.md) of the home folder (below).

Tracking: added for [#23](https://github.com/pane-app/pane/issues/23)
(US09, US14, T01, T02, T03, G2, G4); computed results for
[#27](https://github.com/pane-app/pane/issues/27) (US06, US12, T01, T03).

## What is searched

Root search lists **root results**. For a query that is not blank, the
[computed results](#results-computed-from-the-query) for it come first,
once they arrive. A command whose [alias](aliases.md) the query is, or
starts with, comes before everything, computed results included. The
[fallbacks](aliases.md#making-a-command-a-fallback) come after everything.

A blank query lists the results in the
[no-query order](#matching-and-ranking): the
[pinned home](#the-pinned-home) above, then the commands and applications
by frecency, so what the user opens most is at the top before anything is
typed. These are the [results supplied ahead of the
query](#results-supplied-ahead-of-the-query) minus links.

The list holds:

1. The commands built into this Pane build. There are none in any build
   since [#162](https://github.com/pane-app/pane/issues/162): the Rust,
   JavaScript and TypeScript samples are installed by hand with
   `pane --install`, and a test registers its own.
2. The commands of each enabled installed package, and the [results
   supplied ahead of the query](#results-supplied-ahead-of-the-query),
   such as the installed applications, ranked with them (by kind below
   commands and links, until what the user opens raises them). A
   command's `when` and `matches` say when it is listed at all. The
   commands declared for a typed address or path come
   [below the results found by title](#understanding-the-typed-query)
   instead.
3. An enabled installed package whose managed copy cannot be read, as one
   row explaining the problem.
4. Pane's own rows: "Install extension from folder…", "Install extension
   from npm…" ([npm](npm.md)), "Install extension from Git…" ([Git](git.md)),
   "Check for Extension Updates", "Manage Extensions" and "Settings…".

Extensions are installed and managed in Settings (#168,
[ADR 0043](adr/0043-extensions-are-managed-in-settings-one-page-per-extension.md)).
The launcher itself has no screen for extensions. "Manage
Extensions" opens Settings at its Extensions group, and the install rows
open its install flow there: the folder picker, or the field for an npm
package or a Git repository, then the package's preview with its Install.

"Check for Extension Updates" checks every updatable extension at once,
whatever the cadence, and updates what it finds
([npm](npm.md#updating-by-itself)). Settings' Extensions group has a
"Check for updates" button that starts the same pass.

For a query that matches nothing but fallbacks, the first fallback is
selected, so Enter sends it the query (ADR 0031).

A **disabled** package contributes nothing
([#10](https://github.com/pane-app/pane/issues/10)). Its commands leave
the results at once, even while the choice is being recorded, and come
back when it is enabled again.

A command [unavailable](platform-availability.md) on this system is still
found, with its reason. Invoking it shows the reason and runs nothing.

When an install, update or enable/disable finishes while the user is
searching, the results are rebuilt for the same query. The selection stays
on the same row if it still matches.

## Matching and ranking

Implemented in [`crates/pane-core/src/search.rs`](../crates/pane-core/src/search.rs).

### How text is folded

The query and each result's title, subtitle and, for an installed
command, its package's title are compared after four steps:

1. Unicode NFC normalization, so "é" typed as one character matches "e"
   followed by a combining accent.
2. Transliteration to ASCII through the `any_ascii` table. é is e, đ and Đ
   are d, ß is ss, ligatures split, and every script the table covers is
   romanised: "cafe" finds "Café", "tieng viet" finds "Tiếng Việt" and
   "duong" finds "Đường".
3. Full Unicode lowercasing (`to_lowercase`).
4. Collapsing every run of whitespace, including leading and trailing
   spaces, to a single space, in titles as in the query.

The `any_ascii` table is by Hunter WB, under ISC, which is on the licence
allow list in `deny.toml`, and has no dependencies. It was chosen over
`deunicode`, whose current release declares a BSD-3-Clause with a fourth
clause the allow list does not name.

Lowercasing is not locale-aware case folding. Language rules such as
Turkish dotted and dotless I are out of scope.

The alias the user gave a result is the exception. It is compared
caselessly as it was recorded and never transliterated, so an alias means
exactly what it meant ([#31](aliases.md)).

### When a result matches

A result matches when its alias matches (the query is the alias, or starts
it), or when one of its texts passes the scorer and the user's Search
sensitivity. Its texts are:

- its title and each alternate title;
- its subtitle and each keyword;
- the composites "title subtitle" and "subtitle title", so a query can
  span both: "utub vid" finds a result titled "Search YouTube" with the
  subtitle "Videos";
- for an installed command, its package's title.

A command without its own subtitle still shows its package title as the
subtitle, and one with its own subtitle is still found by its package
title.

A keyword is a text of its own, as the subtitle is: no composite spans a
title and a keyword. A command's keywords are its manifest's
(`"keywords"` in `pane.json`, #197), an author's search terms, distinct
from the user's aliases. An indexed result's keywords come from what
supplies it (#124).

### How a placement is scored

The scorer places the query's letters in a text as a subsequence and
gives the placement a score:

- a letter matched at the text's first position scores 4, at a word start
  3, and elsewhere 2;
- a separator matched to a separator scores 1;
- each gap between two consecutive matched positions costs 1, and
  adjacency is free.

The best placement's score counts. An exact equality of the folded query
and a folded text is its own outcome, the best one.

A query separator that cannot be placed is skipped, never a failure. A
letter that cannot be placed means no match. A query with no letters at
all matches nothing, so a row of separators alone does not list every
result that holds one.

Separators are whitespace and `- . / ( ) [ ]`. Capital letters inside a
word are not word starts. A query starting with "/" treats the first "/"
in a title as a space. A query longer than two characters is checked in
order first, so a text that cannot hold its letters is never scored.

### Search sensitivity

**Search sensitivity** is the Launcher page's choice, recorded in the
settings record as `searchSensitivity`, default High. It decides how good
a score must be, with n the query's characters that are not separators:

- Low accepts any placement.
- Medium requires at least 1.5·(n−2)+4.
- High requires more than 2n, a match that starts the text or a word of
  it.

The choice applies on the next keystroke. The list the current query has
already made stays as it is.

### The comparator

A result that matches is ranked by the **comparator**, the first
difference winning:

1. the query is the result's alias;
2. the query is longer than three characters and is exactly the title or
   an alternate title;
3. the query is exactly one of the result's [learned
   queries](#learning-from-what-the-user-chooses);
4. the query is exactly the subtitle (a keyword counts: keywords rank as
   subtitles);
5. the alias starts with the query;
6. a learned query starts with the query;
7. the query starts with a learned query of at least three characters (an
   "overbounds" match: the longer learned query wins, and it is ignored
   when the query is more than three characters longer than it);
8. the higher of the title, alternate-title and subtitle scores (a
   keyword's counts where the subtitle's does; a match the composites or
   the package title found holds none of them, and ranks below every
   result with a score);
9. higher frecency;
10. higher title score;
11. kind priority: commands above links (quicklinks), above applications,
    above files;
12. the provider's own order: Pane's root list first, then each command
    that supplies results ahead of the query, in the order they were
    first asked. This comes ahead of the title comparison, so rows of one
    provider the title cannot tell apart keep the order the provider gave;
13. the title, with digits compared by their value ("Item 2" before
    "Item 10").

### The no-query order

The **no-query order** is the blank query's own order and the last
tiebreak of every query's. It follows the last step: frecency, then
having an alias, then kind priority, then the provider's order, then the
title. Results the comparator cannot tell apart keep the order they were
listed in. With nothing learned yet, having an alias and the title decide,
so Pane's own rows rank with the commands.

For example, take "download" among Clear cache ("Delete downloaded
files"), Downloader, Download, Recent downloads and Undownloadable files.
Download comes first, because its title is the query. Downloader is next,
a title that starts with the query. Then come Recent downloads and Clear
cache, a word and a subtitle that start with one, scoring 17 to
Undownloadable files' mid-word 16. Recent downloads is first of the two
because its title holds the query.

### Alternate titles and keywords

An indexed result may also have **alternate titles**: an application's
untranslated name or its program's name, such as `wt` for Windows
Terminal. Each is matched as the title is, and the best of them gives the
rank. It may also have **keywords**, matched as the subtitle is. The row
still shows its real title (#170).

### Same-name rows

When two or more listed rows have the same folded title, each shows what
tells it apart (#197):

- An installed command names its package's source after its subtitle, as
  the alias and fallback rows already do.
- An indexed result shows the distinguishing subtitle its provider
  supplies. An application's [distinction](applications.md#names) is the
  subtitle itself, so nothing is added twice.
- A computed result's section names the command that computed it, and a
  file row shows its folder, so neither adds anything.

### Selection and highlighting

A blank query lists every root result, in the no-query order. The best
match is selected after every change of the query; searching the same
query again changes nothing. Each result's text is folded once, when root
search is shown or its results are rebuilt, not on every keystroke.

The letters a title matched are highlighted in the accent on the row. They
are the title characters of the best placement, including a placement
that spans the title and the subtitle. A result found by an alternate
title, a keyword, its subtitle or its package title alone highlights
nothing in its title.

Not done, deliberately: typo tolerance (edit distance) and locale-aware
case folding.

## Learning from what the user chooses

Root search learns from what the user invokes (#199,
[ADR 0030](adr/0030-root-search-learns-from-what-the-user-chooses.md)).
What Pane learns stays on this computer and is never sent
anywhere.

### What counts as a use

A **use** is recorded when the user invokes a root result from root
search (Enter, a click, its number chord) and its action was dispatched,
not refused as unavailable or paused. The use carries the query then in
the field, folded as matching folds it. Invoking a quick slot from the
pinned home records a use with no query.

These are not recorded:

- a result opened by its global hotkey;
- results without a lasting identity: computed answers and other computed
  results, and file results;
- fallback rows;
- Pane's own install and management rows;
- rows the user did not invoke.

### What a use earns

- **Frecency**, a score that decays with a ten-day half-life and never
  falls below the score of a result never used. A use adds 1 to the
  decayed score and re-anchors it at that moment, by the launcher's
  clock. Frecency weighs at the comparator's ninth step and orders the
  blank query. Recording a use never re-sorts the list on screen; the
  next search ranks with it.
- **Learned queries**: the last three distinct non-empty queries the
  result was chosen with, newest first. They weigh at steps 3, 6 and 7
  only while the result's frecency is above 1 and it was last opened
  within 17 days.

A use is keyed by the result's identity, as a quick slot is (ADR 0026),
never by a row's title or position. A registered command is keyed by its
command id. An indexed result is keyed by its own id under the command
that supplies it; for an application that is
[the applications' identity](applications.md#names), which an update into
a new version folder keeps.

### The learned record

The record is Pane's own: `learned.json` beside `installed.json`. It
follows the house record rules of `aliases.json`:

- It is versioned, validated, and written atomically one write at a time.
- An unreadable record is reported on root search's status line and never
  replaced, and nothing is recorded while it cannot be read.
- A write that fails puts back what the record last held.

Uninstalling a package forgets its entries, as its aliases are forgotten;
disabling keeps them. An entry whose score has decayed to 1 and that was
last opened more than 17 days ago is dropped, since it ranks nothing.

### Resetting and turning learning off

The user controls what is learned (#200):

- The Actions panel offers "Reset Ranking" (no default key) for every root
  result that can be learned, which is one a quick slot can hold. It
  clears that result's frecency and its learned queries, keeps every
  other result's, and says "Ranking reset for <title>" in the status line.
- The Launcher page offers "Reset ranking…" for everything. It asks for
  confirmation first: the first press arms the row and the second runs it,
  with Cancel standing it down.
- The Launcher page's "Learn from what I choose" switch is recorded in the
  settings record as `learning`, on by default. Turned off, Pane records
  no use and ranking acts as if none had been recorded. What was learned
  is kept until it is reset, so turning the switch on again uses it.

The same switch also stops the search history being recorded
([below](#recent-queries)); what was recorded is kept until it is reset.
Both controls re-rank the list on screen at once. Nothing is offered while
the learned record cannot be read, since it is never replaced.

## Recent queries

Root search remembers the queries it was cleared of, so Up can bring them
back (#206).

Press **Up** with the query empty and the first row (or no row) selected,
and it restores the most recent query with the [argument
values](#inline-argument-fields) typed with it. The press must not be the
system's repeat of a key still held. While the restored query is
unchanged, Up again restores the one before. Restoring a query searches it
as typing it does.

Any other key ends the walk, such as the query changing or the selection
moving, so the history never hijacks navigation. When a row other than the
first is selected, or a query is typed that the walk did not restore, Up
moves the selection as it always did.

A query is recorded when it is cleared while not blank:

- by Escape;
- by the field being emptied;
- by a command's opening taking it (returning to root always starts
  empty);
- by root search being shown fresh over it, as `window.close` with
  `clearRootSearch` and a pop to root do.

The query is recorded trimmed, with the argument values typed with it. A
password's value is recorded empty, its text never written. Consecutive
duplicates are not added, and at most 64 entries are kept, newest first.
Uninstalling a package forgets the entries that carry its commands'
values; disabling keeps them.

The record is Pane's own: `search-history.json` beside `installed.json`.
It follows the house record rules of `aliases.json` (see
[learning](#learning-from-what-the-user-chooses) for what those rules
are): versioned, validated, written atomically one change at a time, an
unreadable record reported on root search's status line and never
replaced, and nothing recorded while it cannot be read. What Pane
remembers stays on this computer and is never sent anywhere.

The user has two controls over it on the Launcher page. "Reset search
history" clears every entry, asking for confirmation first as "Reset
ranking…" does. The "Learn from what I choose" switch stops queries being
recorded as it stops uses being recorded: one switch for both. What was
recorded is kept until it is reset, so the walk keeps reaching it while
the switch is off.

## Host behavior

The public host interface is [`pane_core::Launcher`](../crates/pane-core/src/launcher.rs):
`Screen::Root`, which carries the query, `set_query`, `move_selection`,
`activate_selected` and `back`. The window renders that state and maps
input to those calls.

| Input | On root search |
| --- | --- |
| Typing, editing keys, clipboard, undo, input-method composition | Edit the query (GPUI CE's single-line editable text element). Every change searches again. |
| Up / Down | Previous / next result (not the caret). Up, with the query empty, the first row (or no row) selected and the press not the system's repeat of a held key, restores the most recent query instead, and, while the query it restored stands, the one before: [recent queries](#recent-queries). |
| Alt+P / Alt+N (the Keyboard page's Emacs navigation bindings) or Alt+K / Alt+J (its Vim Motions), Control instead of Alt on macOS | Previous / next result too, beside Up and Down, while that set is chosen (the default is None). Raycast for Windows puts these sets on Alt as well. Its Alt+B / Alt+F and Alt+H / Alt+L move left and right in its grids; Pane has no left or right selection to give them, so they stay unbound. |
| Moving the pointer over a result | Select it, so the footer's action and Enter act on it. A pointer resting on a result never undoes the keys' selection, and while a layer over the list owns the target (the Actions panel, the Pane menu) the pointer selects nothing. The first pointer event after the window shows only records where the pointer is. |
| A click on a result | On the selected result: invoke it, as Enter does. On an unselected one (the keys moved the selection away while the pointer rested on it): select it; a second click invokes it. |
| Enter | Invoke the selected result; see [what Enter does](#what-enter-does). |
| Tab / Shift+Tab | Focus the next / previous field, as forms and the footer's controls take them. From the query, they go into the selected row's [argument fields](#inline-argument-fields) when they show (the first empty one, or the first; the last for Shift+Tab), between them, and back to the query from the last and the first. On a selected entry of a typed folder, Tab completes the query to its path instead, and Shift+Tab removes the last path component ([understanding the typed query](#understanding-the-typed-query)). After a word that is a command's alias alone, Tab enters what the alias names, as a space after it does. Both wait for the query's list as Enter does ([keys that wait](#keys-that-wait)). |
| Escape | Clear the query; with an empty query, nothing. |
| Ctrl+K (Cmd+K on macOS; the Keyboard page's Open actions), or the footer's Actions button | Open the selected result's Actions panel, or close it, waiting for the query's list as Enter does ([keys that wait](#keys-that-wait)). |

### What Enter does

Enter invokes the selected result:

- it opens the command;
- it explains an unavailable or unreadable one;
- it opens Pane's own screen;
- it copies a computed result's text to the clipboard ("Copied 42 to the
  clipboard"), and root search stays as it was;
- it opens an application, a file, a folder or a link, and the launcher
  closes, as Raycast's does: root search is shown next time, its query
  empty (#468). An open that fails says why ("Could not open Firefox:
  …"), and the launcher stays as it was;
- it sends the text to a command that takes a query, through its alias or
  as a fallback, and shows its answer, and root search stays as it was.
  With nothing else listed, the first fallback is the selected result
  (ADR 0031).

A selected command whose [argument fields](#inline-argument-fields) leave
a required one blank runs nothing. Enter (or a click, the footer's button,
Ctrl and a digit) takes focus to the first blank one. Enter inside a blank
required field marks it and says "Enter <placeholder>" in the status line.

While the query's list is not yet published, Enter waits for it
([keys that wait](#keys-that-wait)).

### The Actions panel

The Actions panel (#95) lists what can be done with the selected result,
from the core's `Launcher::result_actions`:

- its primary action (the footer's, with the same dispatch);
- then, under "Pane":
  - "Pin" for a command or an indexed result, or "Unpin" once it is
    pinned (see [the pinned home](#the-pinned-home));
  - "Reset Ranking" for a result root search can learn from
    ([above](#learning-from-what-the-user-chooses), #200);
  - for an installed command, "Assign Hotkey…"/"Change Hotkey…" and "Add
    Alias…"/"Change Alias…". These open the same hotkey screen and alias
    form the extension list does, and return to this search when they
    end.

Nothing without a working operation is listed: no new window, file
manager, quit or hide (#100).

The panel's search field holds focus. Typing filters the entries by label
("No actions match" when none does), Up and Down move the selection, and
Enter or a click runs the entry once. Escape (or Tab) closes only the
panel, giving focus back to the query. A mouse-down outside the panel
closes it and is consumed, so the result it covered is never invoked.

The panel holds its target. The pointer cannot move the selection while it
is open, and an entry whose target is no longer selected, or no longer has
that action, runs nothing. With no result selected, the panel says so.

### Keys that wait

While the list for the exact query typed is not yet published, the keys
that act on the selection are held until it is, for at most 300 ms (#203).
They are then applied to the selection at that moment. The held keys are:

- Enter, Tab and Shift+Tab, Ctrl+K, and Ctrl and a digit;
- the selected row's action chords and shortcuts;
- a space typed while the query could still turn into a command's alias,
  meaning it is one word that some active alias starts with. Once it
  cannot, a space types as it always did.

Characters typed meanwhile are applied in order, landing in the field
behind the held keys. Typing "ec", a space and "hello" while a provider
answers leaves the field reading "ec hello". An action key held once is
never held twice: the system's repeats of a held Enter run nothing more.

The window owns the holding and the replay. Each held key is dispatched
again as a press, through the same bindings and handlers, while the
launcher only reports whether the current query's list is published. A
hold is keyed by the query its first key was pressed under. A query that
moves on (Escape cleared it, a completion replaced it) drops its keys
rather than running them on another list.

## Inline argument fields

A command that declares arguments shows them **after the query, in the
search field**, while its row is selected (#205). The arguments follow the
"Extension commands like Raycast" specification's contract (#120): at most
three, of text, password and dropdown.

In root search alone, the fields replace the [argument
form](../guests/README.md#arguments). A command invoked any other way (a
global hotkey, a quick slot, another command's launch) asks through the
form as it always did.

### How the fields look

Each field is a Settings-family well sized to its value or placeholder.

- A password masks what is typed, drawn as dots and reading as dots to
  assistive technology.
- A dropdown shows its choice or placeholder on a trigger that Enter,
  Space, Down or a click opens. It lists a leading empty choice, then the
  command's options. The arrows move the highlight, Enter picks, and
  Escape or Tab closes.
- An optional argument carries an "optional" marker inside its field. A
  required one carries none until it has been left blank once; then it
  takes the danger ring until it gets a value.

While the fields show, the query field's placeholder is the command's
title, so the user knows what they are filling in.

### What happens to the values

The values are the search's own state and are never recorded. They survive
the list being rebuilt (a late answer's re-rank, another selection come
back to the same row) while the query stands, and a dropdown's value
survives while it is still a choice. They go when the query does. A
password's value is recorded nowhere, as the form's is not.

The last choice of each command's dropdown argument is remembered per
command, as the form remembers it (`arguments.json`, never a password), and
offered again until it is no longer a choice.

### Keys in the fields

- **Tab** from the query focuses the first empty argument (the first, when
  none is empty); **Shift+Tab** focuses the last.
- Inside an argument, Tab goes to the next, and from the last back to the
  query. Shift+Tab goes to the previous, and from the first back to the
  query.
- Left at the start goes to the field before it, and Right at the end to
  the one after it. That needs the caret's place, so the fields' and the
  query's own navigation keys are bound for the window to count every
  move. Right at the query's end focuses the first field.
- Up and Down do nothing while a field has the keys: the list does not
  move while the user types into one.
- Escape returns focus to the query with its text selected; the next
  Escape is the usual one.
- Backspace in an empty field does nothing special.
- **Enter** with every required argument filled runs the command with the
  values (from its alias, when the query names the command by one). With a
  required one blank, focus moves to the first blank one and nothing runs.
  Enter inside a focused blank required field marks it, the status line
  saying "Enter <placeholder>".

### An alias and a space

An alias followed by a space is the way in. When the query becomes a
command's alias followed by a space (Tab after the alias alone does the
same), the alias stays in the field and focus moves to the command's first
empty argument. Text typed after the space goes into the first argument if
it is text or password, the caret at its end. That text is held with the
query's list when the list is not yet published (#203).

A command **without** arguments opens at once instead, as Raycast does. A
view command opens its screen, a searching one's own field taking what is
typed next, and a no-view command runs.

Some rows never show the fields:

- A command that accepts fallback text but declares no arguments keeps
  today's row that sends the text after the alias on Enter, since that
  text is its input.
- A fallback with arguments receives the query in its first text argument
  as it always did, and never shows the fields.
- Rows that send the query never show them.

## The pinned home

A blank query shows the **pinned home** (#101) above the results: the
"Pinned" label, with the slots' chord, and the **quick slots**. The quick
slots are an ordered list of pins with no gaps and no limit
([ADR 0027](adr/0027-quick-slots-are-an-ordered-list.md)). A query whose
trimmed text is not blank hides the home; clearing the query brings it
back.

The results below keep their "Commands" label and are ordered by what root
search [learned](#learning-from-what-the-user-chooses): the no-query
order, frecency first, as any query's are. The list is one list, with no
section of suggestions of its own.

### What a slot holds

A slot holds an identity, never a row: a registered command by its id, or
an indexed result (an installed application) by its own id under the
command that supplies it. A computed answer, a file or Pane's own rows
cannot be pinned. A fresh installation pins nothing.

A pinned indexed result whose title another result of its command shares
(two applications of one name) says what tells it apart. That is its row's
subtitle (an application's [distinction](applications.md#names)), shown
as its tile's tooltip, its vertical row's subtitle and its accessible
description (#170).

### Pinning, unpinning and moving

The Actions panel's "Pin" adds the selected result at the end of the list
("Pinned …"). There is no slot to choose and nothing is replaced. Pinning
what is already pinned changes nothing, says so and moves focus to its
slot, clearing a typed query so the home and that slot show. On a row
already pinned the panel offers "Unpin" instead.

A slot has its own actions, opened by a secondary click on it or by Ctrl+K
while it has focus:

- open it;
- unpin it ("Unpin", which closes the gap);
- move it ("Move Up"/"Move Down", shown as "Move Left"/"Move Right" in the
  horizontal layout). Each swaps it with its neighbour, never past either
  end.

The default keys (Cmd for Ctrl on macOS): Ctrl+Shift+F pins or unpins the
focused slot, or root search's selected row. Ctrl+Alt+Up or Left and
Ctrl+Alt+Down or Right move the focused slot.

### Layout

The Launcher page chooses the layout:

- Horizontal (the default): a grid of tiles five columns wide that wraps
  onto more rows. One dashed "+ Pin" hint tile sits in the next free cell
  of the last row, never starting a row of its own; with no pins, the hint
  shows alone.
- Vertical: result rows of the pinned results only, and nothing with no
  pins.

In the [compact window mode](../CONTEXT.md) the pins show only if "Show
pinned in Compact mode" is on (`compactPinned` in the settings record, off
by default). They are then a row of small icons under the search field
while the window is collapsed.

### Invoking a slot

A slot runs on a click, on Enter or Space while it has focus, or on its
chord. Only the first five slots are numbered, Ctrl+1 to Ctrl+5. The
numbers after the numbered slots, up to Ctrl+9, pick the first rows below
the home:

- With two slots, Ctrl+3 to Ctrl+9 pick the first seven rows.
- With eight, Ctrl+1 to Ctrl+5 pick slots 1 to 5, slots 6 to 8 have no
  number, and Ctrl+6 to Ctrl+9 pick the first four rows.
- With a query, or in a command's list, Ctrl+1 to Ctrl+9 pick the first
  rows.

Nothing names the chords at rest. Holding Ctrl alone for 400 ms slides
each item's number in: a row's over its right end, a slot's down into its
corner (in compact mode, the first five icons'). Releasing Ctrl slides
them away.

The chords are the root search field's and the slots' own, never
registered with the system. They act only on root search, with no overlay
open, no input-method composition in the query and no action already
running. A held chord's repeats and a double click's second click run
nothing more. The slot is resolved again at that moment, and only a target
that can run is run. A click keeps the query focused.

Each slot is resolved through what is enabled now. A disabled, paused or
missing target, or an application its command has not listed yet, keeps
its slot and its name and says why it cannot run. It can always be
removed, and enabling or installing the same identity resolves it again.
Showing root search asks for the indexed results the home lists, without
typing a query (see [below](#results-supplied-ahead-of-the-query)).

### The quick-slots record

The record is `quick-slots.json` in Pane's data folder, beside
`settings.json` (see
[ADR 0026](adr/0026-host-keeps-quick-slots-by-identity.md) and
[ADR 0027](adr/0027-quick-slots-are-an-ordered-list.md)). It is version 2,
with the pins in order:

```json
{ "version": 2, "pins": [ { "command": "<id>" }, { "command": "<id>", "result": "<id>" } ] }
```

A version 1 record (five positional `slots`, `null` for an empty one) is
read as the list of its pins in order, and the next change writes version
2. The record is written atomically one write at a time. A write that
fails puts back what the record holds and says why. A record of an unknown
version, or one that is not valid, is reported and never replaced.

## The footer, rows and notices

### The footer

The footer shows the Pane mark at its left. The mark is the button of
Pane's own menu (Settings), a Windows/Pane adaptation of the reference's
decorative mark. While the Actions panel is open, the hint "Type to filter
actions · Esc goes back" follows it. At the footer's right are the primary
action and the Actions button, whose keys say how each is pressed. While a
status shows, its message takes the strip and the buttons step aside.

### An opened command

An opened command has no heading line above its content
([#162](https://github.com/pane-app/pane/issues/162)). Its list, its
search, a form or a custom view opened from it, Clipboard History and
Manage extensions start with their content, as Raycast's views do.

The footer's left names it instead, after the Pane mark, while no hint or
status takes that place. It shows the command's icon (its own, as its
package ships it; Pane's tile for Pane's own screens) and the screen's
title: the command's on its list, and a form's or a custom view's on those
("Search Files", "Clipboard History").

Section labels inside a list ("Today", "Commands") stay. The core's own
screens (a package's preview, a confirmation, the details and hotkey
screens) keep the heading that says what they are about.

### What each row shows

Each row shows what the launcher knows beyond its title and subtitle, from
a read-only presentation (`Launcher::presentation`):

- the row's kind (Command, Application, File, Folder, Link or Fallback),
  taken from what activating it does;
- the alias and the registered global hotkey the user gave its command;
- the part of its title the query matched, in the accent: the title
  characters of the best placement. A result found by an alternate title,
  a keyword, its subtitle or its package title alone highlights nothing.

Rows sit under section labels:

- "Commands" over a blank query's list (the no-query order, what the user
  opens most first, with no section of suggestions);
- "Results", with their count, over a query's;
- a computed answer under the title of the command that computed it
  ("Calculator") or, since #196, under the section the answer's own detail
  names (the calculator's colours under "Color", its dates and times under
  "Date & Time");
- the rows declared for the address or path the query is, under
  "Addresses" (#195);
- the files found for the query, with the row searching them all, under
  "Files" (#175);
- the fallbacks under "Fallbacks" (below the no-results notice when
  nothing else matched).

The presentation changes nothing about what is listed, its order, or what
a row does.

### The computed answer card

A computed answer (#96) is a computed result whose action copies text,
such as the calculator's. It is drawn as the reference calculator board's
card: what was typed, an arrow, and the answer. The text is in Geist Mono
at the board's 34px, or at 24 or 18 when the longer of the two would not
fit its column (past that it wraps).

The card shows only what the launcher holds (`ComputedAnswer`): the query,
the text Enter copies, the command and, since #196, the colour of its
swatch when the answer is one (see [the calculator](#the-calculator)).
Since #472 the query is typeset as an expression, `*`, `·` and a `x`
between two numbers as ×, `/` before a number as ÷ and a minus as −, each
operator between two operands with one space either side (`2*3` as
`2 × 3`); `0xff`, `km/h`, `1e-3`, `2026-12-25` and words stay as typed.
The command sends only its answer, so Pane typesets the query it owns.
"Expression" is under the query and "Result" under the answer, as
Raycast's calculator labels its two sides, in the board's caption type;
a colour's swatch takes the answer's caption's place. The board's "Also"
conversions and recent calculations have no provider and are not drawn.

It is a row like any other: selected first, moved to by the keys or the
pointer, with the primary action "Copy answer". It is named "6 × 7 = 42" for
assistive technology, and the swatch is a colour well named by the
colour's value. Its accent ring shows while it is selected.

### The no-results notice

The no-results notice (#96) heads the list while nothing but fallbacks is
listed for a query that is not blank. It reads "Nothing matches “…”",
then "Enter sends it to the first fallback below, or install an extension
that knows about it." With no fallback, it points instead to where one is
offered: Manage extensions.

The notice stays above the fallbacks whichever is selected. The first is
root search's own selection, so Enter sends it the query (ADR 0031).

Pane searches commands, applications and the files of the home folder (and
the folders the user adds), so the notice claims no search of the whole
computer. It suggests no extensions, having no store to suggest them from.

### A missing result is not a failed action

A query that matches nothing shows the no-results notice ("Nothing
matches “…”"). The [fallbacks](aliases.md#making-a-command-a-fallback), if
the user has any, are listed below it with the first selected, so Enter
sends it the query (ADR 0031). With none, nothing is selected, Enter then
does nothing, and the status line stays idle.

A result that matches but fails when invoked (its component is missing,
the runtime is unavailable, the guest reports an error) shows the failure
as the status error, as before this slice.

### Focus

The query field has keyboard focus whenever root search is on screen: when
Pane starts and whenever the user returns to root search. Returning to
root search (Escape from a command, after an install or update) starts
with an empty query. Opening a command moves focus to its list.

## Results computed from the query

A command can also answer the query itself, where matching titles cannot:
a calculator's answer to "6*7" matches no title. Such a **computed result**
comes from the extension, through the same guest boundary as its command.

### Declaring computed results

The command's `pane.json` entry sets `"rootResults": true`, and its
component exports `pane:extension/root-results`
([`wit/root-results.wit`](../wit/root-results.wit)) besides `command`. Pane
checks both at install, without running it. The
[author guide](../guests/README.md#root-results-computed-from-the-query)
covers this in Rust, JavaScript and TypeScript.

### When commands are asked

For every change of a query that is not blank, `Launcher::set_query` ranks
the metadata at once. It ranks once per query, so an answer merging into
the list later splices its section in without ranking the rows again
(#202).

`set_query` also returns a future that asks each enabled command with
`rootResults` for `results-for(query, at)`, one after another in install
order. It asks once the query has been quiet for a short while (40 ms
after the last keystroke, #202). A burst of keystrokes asks once, and
nothing is asked for a query the field has already moved on from. The
window awaits the future off its thread and redraws, so typing never waits
for an extension.

`at` is when the query is asked about: the moment the user stopped at it,
by the clock root search's own dates are shown by, with the local time's
offset from UTC. So a command can answer about the current date or time
(the calculator's "now" and "today", #196). The whole search shares one
moment, whatever a slow command delays.

### Publishing the list

The new query's list is **published** once every command asked has
answered, or 200 ms after the query changed, whichever comes first (#201).
Until then the window keeps showing the previous query's list, while the
search field shows what was typed at once. A slow command holds the list
no longer than the budget and never hides the answers of those asked
before it. The keys that act on the selection are held for the
publication ([Host behavior](#host-behavior)).

A command that misses the budget keeps running until its answer or
cancellation: the budget bounds the wait, not the scheduling. An answer
that arrives after the list was published is merged into it, coalesced
within 16 ms, so answers arriving close together become one update.

Answers for an older query, or for an earlier search of the same query
(or after leaving root search), are discarded. Until a command answers,
the new query lists none of its results, never an older query's.

### Cancelling pending calls

Since [#29](https://github.com/pane-app/pane/issues/29) the search owns
these calls. A call still pending is cancelled when the query changes, or
when root search is left. On a query change it is cancelled only once the
newer query needs the runtime, as its quiet period ends (#202), so a call
that answers within it keeps its instance for the queries after it.

A cancelled call is never started if it was queued, and is dropped with
the command's instance if it was waiting inside the guest (on an async
import). That is not a failure for [pausing](pausing.md), and the query
after a cancelled call starts a fresh instance
([cancelling](files.md#cancelling-a-pending-search)).

Calls still run one at a time on the runtime thread. Since
[#18](https://github.com/pane-app/pane/issues/18) every guest yields at
each epoch tick, so a computing command is cancelled within a tick too. One
that computes for 5 seconds without finishing is stopped as an
unresponsive call ([pausing](pausing.md#when-an-extension-stops-responding)).

### Where computed results are listed

Computed results are listed **above** every title match, in the order the
commands and their answers give them. They are not ranked against titles
(provisional, pending user confirmation; see
[current decisions](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/current-decisions.md)
item 10).

Those that open a file (**open-file**, since #29) are listed **after** the
title matches instead, since a folder can hold many files matching a short
query. Of the file index's entries (#175), at most 5 are listed per
command. A row "<command> for “<query>”" follows when the command searches
in its own field (Search Files for “plan”), and opens it with the query
typed.

When the list is published, and when a late answer merges into it, the
selection moves to the new first row if the first row was selected. A
preselected fallback gives way to a late result (ADR 0031). Otherwise the
selection stays on its row by id, so a merge never takes the selection
from a row the user moved to.

### What a computed result holds

A computed result has an id (`<command id>:<result id>`), a title, an
optional subtitle and an **action** Pane performs without calling the
extension again:

- **copy**: Enter copies the text to the clipboard, which the window
  writes (`Launcher::selected_copy`), and the status says "Copied … to the
  clipboard".
- **open-url** (since #28): Enter opens an address of any scheme (ADR 0037,
  #145) with the launcher's link opener, the system's handler in the
  window.
- **open-file** (since #29): Enter opens an entry of the file index (#175)
  or a file of the package's granted folder, named by the id the host gave
  it. It opens with the system's handler for its type (a folder in the
  file manager), once the host has checked it again. The row shows the
  host's name for the entry, its folder, the system's icon for its path
  and its kind, File or Folder. Enter on a program the index found shows
  it in the file manager and never runs it ([files](files.md#opening)).

### Errors and disabled packages

No result is not a failure. A query the command cannot answer (words, an
incomplete or invalid expression) gives no results, and the status is
untouched.

An error or crash of the extension is shown as a row titled with the
command and "Could not answer: …"; Enter on it shows the whole error.
Other results are listed as usual, and after a crash the next query starts
a fresh instance.

A **disabled** package is not asked, and its computed results leave the
results at once, even while the choice is being recorded. Enabled again,
it answers from the next change of the query.

## Results supplied ahead of the query

Some results do not depend on the query but must be found outside Pane,
such as the installed applications. Matching titles is right for them, but
they are not in any `pane.json`. Such an **indexed result** comes from the
extension, through the same guest boundary as its command.

### Declaring indexed results

The command's `pane.json` entry sets `"indexedResults": true`, and its
component exports `pane:extension/indexed-results`
([`wit/applications.wit`](../wit/applications.wit)) besides `command`. Pane
checks both at install, without running it. The
[author guide](../guests/README.md#root-results-supplied-ahead-of-the-query)
covers this in Rust, JavaScript and TypeScript.

### When they are asked

Showing root search asks each enabled command with `indexedResults` for
`results()`, one after another, after the commands computing results from
the query: the blank query's list below the pins needs them. Pane keeps the
answer and ranks it with the other root results on every later query, so
typing never waits for it. Until it answers, the results kept from an
earlier visit are listed.

They are asked again after each return to root search, after a command ran
and after a command's list handled an action, since nothing tells of such
a change. The exception is a command that asked for the host's list of
installed applications. That list changes by itself, and such a command is
asked for its results then ([applications](applications.md#live-list)):
at once while root search shows a query, listed in place with the selected
row kept on its result, and otherwise at the next query. So a return that
changed nothing asks it for nothing (#202).

The guest's work is not cancelled; calls run one at a time (#29).

### How they rank

They are ranked by title, subtitle and rank exactly as commands are, for
the blank query too (in the no-query order, by kind below commands). On
the same rank they come after commands. A link is the exception: the blank
query does not list it, because what the blank query shows is the commands
and applications the user opens most (#122).

### What an indexed result holds

An indexed result has an id (`<command id>:<result id>`), a title, an
optional subtitle, **alternate titles** and **keywords** (both lists,
empty for none; see [matching](#matching-and-ranking)), and an **action**
Pane performs without calling the extension again:

- **open-application** opens the application through the host's
  [applications adapter](applications.md).
- **open** (#149) opens a target (a URL of any scheme, a file, a folder or
  an application), with an application when one is named, through the
  system the `system` host functions act on, as
  [quicklinks](quicklinks.md#in-root-search) do.

Once the target opens, the launcher closes and root search shows next
time with its query empty (#468); if it does not, Pane says "Could not
open …: <why>".

An error or crash of the extension is listed as a row titled with the
command and "Could not list: …" for every query that is not blank; Enter
on it shows the whole error.

A **disabled** or updated package's indexed results leave at once, and an
answer still on its way is discarded. Enabled again, it is asked with the
next query.

## Root providers

A command whose only job is to answer root search declares
`"mode": "provider"` in its `pane.json` entry
([#164](https://github.com/pane-app/pane/issues/164); the
[author guide](../guests/README.md#root-providers)). Such a **root
provider** computes results from the query (`"rootResults": true`) or
supplies them ahead of it (`"indexedResults": true`).

A provider has no row of its own. Root search never lists it, so typing
its name finds nothing of it. It cannot be pinned, has no alias, fallback
or hotkey, is not offered by the Actions panel or the Shortcuts page, and
cannot be launched by another command. Its results are listed as any
command's are.

Its extension's card in Settings names it under the extension's switch,
which turns its results off and on. The calculator and Applications are
providers, so there is no "Calculator" or "Applications" row.

At install, a provider is refused with the reason if it declares neither
`rootResults` nor `indexedResults`, or if it declares what only a launched
command uses (`search`, `takesQuery`, `arguments`, a `schedule`).

A command can become a provider after the user recorded choices for it.
The calculator and Applications had rows before #164, and an update can
change a command's mode. At the next start, these are removed and their
records written, once:

- its pins (`quick-slots.json`);
- its aliases and fallbacks (`aliases.json`);
- its hotkey (`hotkeys.json`).

A toast names what was removed ("Calculator now only answers root
search"). A pinned application, an indexed result of Applications, keeps
its slot. Until that start, a recorded choice for a provider takes no
effect.

## The calculator

The calculator is a default extension in Rust, in
[its own repository](https://github.com/pane-app/calculator). It is a root
provider whose command, "Calculator", has no row and computes results for
root search. It is not part of the core and can be disabled like any
package.

A Pane release pins its release commits
([`crates/pane/defaults.json`](../crates/pane/defaults.json)), and first
setup acquires it from the commit this Pane release pins
([#278](https://github.com/pane-app/pane/issues/278),
[#53](https://github.com/pane-app/pane/issues/53)). A user can also install
it by hand from its repository:

```
pane --install git:https://github.com/pane-app/calculator
```

### What it calculates

Since #472 it reads what Raycast's calculator reads, offline. Its
repository's README and sources say exactly what; in short:

- arithmetic with `+ - * / ^`, `×`, `x`, `÷`, `−`, `mod` and their words,
  `5!`, `5²`, `√16`, constants (`pi`, `e`), functions (`sqrt`, `sin`, `ln`,
  `log`, `round`…), parentheses and multiplication without a sign (`2pi`,
  `3(4 + 5)`), numbers written `1,000`, `1e3`, `10k`, `0xff`, `0b1010`;
- percentages: `20% of 50`, `50 + 10%`, `10% off 50`, `10 is what % of 50`,
  `what % of 50 is 10`, `10 as a % of 50`;
- units of length, mass, volume, area, time, speed, temperature, data,
  angle and CSS pixels, converted with `in`, `to` or `as`
  (`5 km in miles`, `100 f to c`, `1 GB in MB`), and number bases
  (`255 in hex`, `0xff in decimal`);
- dates and times from the moment of the query (`now + 3 days`,
  `35 days ago`, `next monday`, `days until christmas`, `time in Tokyo`,
  `5pm London in New York`, `1700000000 to date`), its places' time zones
  a table of its own with the daylight-saving rules they follow;
- colours (#196): `#RGB` to `#RRGGBBAA`, `rgb()`, `rgba()`, `hsl()`,
  `hsla()` and `oklch()`.

Currencies are not converted (#475): their rates need a network. A query
has an answer only if it calculates something: "42", "(5)", "pi" or "5 km"
alone is no calculation, and incomplete or invalid input has no answer and
lists nothing. A query longer than 256 characters, or nesting parentheses
more than 64 deep, is not read, so that no query can exhaust the guest's
stack. Arithmetic is IEEE double precision; a number shows at most 15
significant digits, its thousands grouped (`4,096`), and uses scientific
notation from 10^15 up or below 10^-6.

The row shows the answer as its title and "<query> = <answer> · Enter
copies the answer" as its subtitle. Root search draws it as a
[computed answer's card](#the-computed-answer-card), under the section its
answer detail ([`wit/root-results.wit`](../wit/root-results.wit)) names:
"Calculator", "Date & Time" or "Color". Enter copies the answer as it is
shown, as Raycast's does, and the Actions panel offers its further copies:

- a number with grouped thousands, without them ("Copy Unformatted
  Answer");
- a date or time, in the layout Pane's own dates use ("Jun 1, 2025",
  "Jun 1, 2025, 14:23"; `time` alone "14:23"), as ISO 8601 (a date, or the
  moment with its offset) and as a Unix timestamp (a date's at its local
  midnight);
- a colour, answered as uppercase hex (`#RRGGBBAA` while it is not fully
  opaque) with a swatch, as hex, RGB, HSL and OKLCH. A colour name is not
  a colour, as Raycast leaves names out on purpose ("red" is also a word).

The moment is Pane's. The query is asked about at the moment the user
stopped at it, by the clock root search's own dates are shown by, with the
local time's offset from UTC, so a test can hold the clock still.

## Understanding the typed query

Root search understands what is typed beyond the words it holds
([#195](https://github.com/pane-app/pane/issues/195)): a query can be a
**web address** or a **path**. The analysis is the core's (the
launcher's, never an extension's), made once per change of the query.

### Addresses and paths

A query is **URL-like** when it has no spaces, is not path-like, and
either:

- parses as an absolute URL with a scheme (`https://github.com`,
  `mailto:someone@example.com`), or
- contains a dot that is not its last character, and its part before the
  first `/` is a plausible host: labels of letters, digits and `-`, the
  last alphabetic and at least two letters, or four numbers. `https://` is
  inferred before such a host (`github.com`, `notes.example.com/pane`;
  `1.5` is not one).

A query is **path-like** when it starts with:

- a drive letter and a separator (`C:\Windows`, `C:/Program Files`);
- `\\` (a network path);
- `~`, resolved to the user's home folder, alone or followed by a
  separator;
- `/`;
- `file://`, which names the path after it (`file:///etc/hosts`,
  `file:///C:/Windows`).

A path may hold spaces. The home folder is the one the
[file index](files.md) covers.

### The rows for an address or path

The rows such a query lists come from **commands that declare they are for
it** (below), so the core stays small: the default extensions supply them.
They sit below the results found by title and above the files, under
"Addresses", in install order.

Enter on one sends the parsed address or resolved path to its command as
the launch record's **fallback text** (as an alias or a fallback sends
text). It launches the command as any command's row does: a no-view
command runs, a view command opens its screen. The selected row, the
number chords and the Actions panel treat these rows as any row. The first
is selected when nothing else matches, so Enter acts on what was typed.

### Where a command appears

Where a command appears is the command's to declare in its `pane.json`
entry, an additive extension API change:

- `when` says when root search lists it: `"always"` (the default),
  `"blank"` (only while nothing is typed) or `"searching"` (only while
  something is).
- `matches` says what it is matched by: `"title"` (the default), `"url"`
  (only URL-like queries) or `"file-path"` (only path-like ones).

A command declared for URL-like or path-like queries is never matched by
its title, however well it fits. See the
[author guide](../guests/README.md#commands-for-typed-addresses-and-paths).

The default extensions supply the rows for them:

- [Quicklinks](quicklinks.md#in-root-search) adds **Open in Browser** and
  **Create Quicklink** (the address prefilled as its form's link) for
  URL-like queries.
- [Files](files.md#in-root-search) adds **Open** and **Reveal in File
  Explorer** for path-like queries. A program is shown in the file manager
  and never run, as file search's own Enter does
  ([files](files.md#opening)).
- The [samples](../guests/README.md#commands-for-typed-addresses-and-paths)
  show the declarations in Rust, JavaScript and TypeScript.

### A typed folder's entries

A typed folder's entries are listed for a path-like query that ends in a
separator and names an existing folder
([#204](https://github.com/pane-app/pane/issues/204)).
[Files](files.md#in-root-search) asks Pane to list the folder, through the
`typed-folder` host interface, and answers the entries as its file
results, under "Files" below the rows for the path.

No folder is granted: the user named it, and Pane's own bounds apply. Only
the direct entries are listed, folders first and each in name order, at
most 500, with a row saying so when the folder holds more (see
[files](files.md#the-typed-folder)). The rows are file rows with the file
actions. Enter opens the selected entry with the system's handler, and a
program is shown in the file manager, never run.

Browsing stays in the query:

- **Tab** on a selected folder row of the listing completes the query to
  that folder's path with a trailing separator, listing the folder's own
  entries. Tab takes the completion over focusing an argument field only
  when the row is a typed-folder entry (#205's fields do not exist yet).
  On any other row, Tab focuses the next field as usual.
- **Shift+Tab** removes the last path component of the query, keeping a
  separator after what remains, so the folder above is listed. It stops at
  the root, a drive or the home folder itself. The query's form is kept:
  `~` stays `~`.
- Nothing is opened by the keys alone: Enter opens the selected entry.

The calculator's colours, dates and percentages are covered under
[the calculator](#the-calculator), above.

## Activation

Searching reads only what the launcher already holds: built-in command
registrations and the installed packages' managed manifests, read at start
and after installs. It never compiles or starts a component. Invoking a
result starts only that command's guest instance (lazy activation,
ADR 0005).

`Runtime::running` is a diagnostic listing the components with a live
instance. The tests use it to show that twelve installed packages can be
listed and searched with none running, and that invoking one starts only
that one.

Background work declared by an extension does not exist yet (no services,
timers or hotkeys), so there is nothing to keep distinct from it beyond
this.

A command with `rootResults` declares that it answers root search, so its
instance starts with the first query that is not blank. It does not start
at start, at install, or for an empty or blank query, and a disabled one
never starts.

## Minimum search-provider contract

Per [ADR 0006](adr/0006-raycast-style-search-with-extension-providers.md)
the core owns the search interface, matching and ranking, aggregation,
navigation and dispatch; features supply entries. The provider kinds are:

- **command metadata**: contributions indexed from package manifests and
  built-in registrations without running anything;
- since #27,
  [commands that compute results from the query](#results-computed-from-the-query),
  which run to answer and whose results the core lists first;
- since #24 to #26, a third kind:
  [results supplied ahead of the query](#results-supplied-ahead-of-the-query),
  which the core keeps and ranks like titles (the installed applications).

The minimum a root result needs, which later default features (#24
onwards) must supply to plug in:

- an **id**, stable and unique among root results, so a refresh keeps the
  selection (installed commands use `<package identity>#<command id>`);
- a **title** and optional **subtitle**, which are what the query matches;
- an optional **unavailability reason**, which keeps the result listed and
  searchable but stops it from running;
- an **action** the core dispatches when it is invoked (today: open a
  command, explain, open one of Pane's screens, or copy or open the link of
  a computed result).

Since #29 a computed result can come from a provider that searches outside
Pane per query: the [files](files.md) of a granted folder, listed by the
host off the extension thread, the command asked again once the listing
ends. A search also cancels its providers' pending work. Since #175 file
search's provider asks the host's file index, which answers at once from
what is indexed and never waits for a walk, so a busy disk holds up no
other result.

What is *not* settled here is left to the tickets that need it:
provider-supplied ranks and how they mix with title matching, and online
providers. Online providers stay inside their own command (US11, T03):
nothing in root search queries an online service.

### Open: extension points later tickets need

Results computed from the query exist since #27 (above), with discarding
of late answers but no ranks of their own. Since
[#29](https://github.com/pane-app/pane/issues/29) a search cancels their
pending calls when the query changes or root search is left, and file
results are listed after the title matches. Since #18 a provider computing
for 5 seconds without finishing is stopped. Still open: provider ranks.

Aliases and fallbacks exist since [#31](aliases.md): the user's words that
find an installed command, and commands taking a query offered for any
text. The text reaches a command only when the user invokes it.

A package's commands and root results leave root search when it is
disabled or [uninstalled](extension-data.md#uninstalling-an-extension)
([#40](https://github.com/pane-app/pane/issues/40)), and change when it is
updated.

## Accessibility

Checked through GPUI's accessibility tree (`Window::debug_a11y_tree_json`)
in the window tests.

### The field and the results

- The query field and the results form one `EditableComboBox` node
  labelled "Search", with the query as its value and "Search apps and
  commands…" as its placeholder. It tracks the field's keyboard focus.
- The results are a `ListBox` labelled "Results" inside it, of
  `ListBoxOption`s. Each has a label, a description (the subtitle, and the
  reason when unavailable), a selected state, and its position in the
  whole list and the list's size. A computed answer's card is one of them,
  named "6*7 = 42".
- Each [inline argument field](#inline-argument-fields) is a labelled
  editable node inside the combo box's group (#205), named by its
  placeholder ("Name", "Secret", "Tone"). Its description is its required
  or optional state and, once a required one has been left blank, what it
  waits for. A text field carries its value, and a password field carries
  dots, never the value. A dropdown's trigger is a combo box carrying its
  choice, and its open list is a list box of its options, with the
  highlighted one the focused node's active descendant.
- The combo box stays the focused node while the selection moves, so a
  screen reader echoes what is typed. No result reports itself as focused.
  GPUI CE implements an active descendant by reporting the descendant as
  the focused node, so root search no longer uses one
  ([#132](https://github.com/hoangvu12/pane/issues/132)).

### The announcer

The window's **announcer** says the selection instead. It is one hidden,
zero-size `Label` node of Pane's own: a polite live region whose name and
value are both the text said last. AccessKit's Windows and Linux adapters
announce a live region's name, and its macOS adapter the value. Each text
replaces the last, so fast arrowing leaves only where the user stopped.

It speaks in the launcher's language (English today):

- A selection move by the user (any key that moves it, a number chord, the
  pointer): "<title>, <i> of <n>", with ", unavailable" after a row that
  cannot run, and the section's name first when the move enters another
  section ("Fallbacks: Echo, 1 of 1").
- Opening a command or the extension list: "<name>, <n> results", then the
  selected row.
- A screen whose rows are choices (a confirmation, a package before
  installing it, why a build failed or a package paused): its title, then
  the selected row.
- The Actions panel: "Actions for <target>, <n> commands", then its
  selected entry.
- The footer menu: its selected entry, said again each time it opens (the
  same text is cleared for one frame, then set again).
- Root search coming back: nothing, and what was said before is cleared.
- A list that becomes empty: "No results".
- Typing: nothing per keystroke. Once the query's results have settled, or
  300 ms after the last keystroke, whichever is later, it says the
  selected row if it is not the one last said.
- A selection Pane changes itself (a late result, a refreshed list):
  nothing, unless the selected row is another one.
- The footer's message when it is a toast or an outcome (a result, an
  error), which the strip keeps as its name under its `Status` role.
  AccessKit announces only a node with a live setting of its own, so the
  announcer says it too. It also says the busy state, once waited-for work
  has outlasted the loading bar's 300 ms threshold (#248), so a quick
  action is never announced as busy. Progress is the strip's alone.

When the message and the selection change together, the message is said
first and the selection 500 ms later, at most. A later message does not
hold the selection back again, and it is dropped if the list no longer
shows it (a panel closed, another list or selection, typing began).

### Other screens

A command's list keeps the focus on the list (or on its search field), the
Actions panel on its search field ("Search actions"), the footer menu on
the menu, and Clipboard History and Search Files on their fields. In each,
rows carry their position and the list's size, and none claims the focus.
Forms, custom views and Settings keep their own accessibility.

### Open: a native run with each screen reader

G2's remaining condition is a native run with each screen reader (Narrator
and NVDA on Windows, VoiceOver on macOS, Orca on Linux), not a GPUI
change. **No screen reader was run** on any platform.

The window tests check the focus, the rows' positions and the announcer's
name and value. GPUI CE's debug tree does not report a node's live
setting, so that the announcer is polite is read from the code, and what a
screen reader speaks is not established by any test.

A text the announcer says twice in a row (the same menu entry when the
menu opens again) changes nothing in the tree, so it is not said again.
The limits listed for [form text fields](forms.md#accessibility) (no text
details, actions or invalid state) apply to the query field too.

## Checks

### Matching, ranking and same-name rows

- [`crates/pane-core/tests/search.rs`](../crates/pane-core/tests/search.rs),
  through the launcher's public interface:
  - the empty query;
  - each comparator step in order against a pair that differs only there
    (the alias, an exact title, an exact subtitle, the alias prefix, the
    best score, the title score, the title's numeric collation);
  - letter case and blank queries, and spaces inside and around titles not
    lowering their rank;
  - composed and decomposed accents matching each other, and
    transliteration (French accents, Vietnamese including đ, ß);
  - abbreviations, word starts and gaps, a query spanning the title and the
    subtitle, and every letter of the query having to place in order;
  - each sensitivity level admitting and rejecting its cases and applying
    on the next keystroke;
  - highlight ranges, ties keeping order, and the same query searched again
    keeping the selection;
  - selection and invocation among the matches, no match with Enter doing
    nothing versus a match that fails, Escape clearing the query, and a new
    search after returning to root;
  - installed commands found by title or package title (below subtitle
    matches, even with a subtitle of their own) and Pane's own rows;
  - disabling and re-enabling a package under a query, and an update
    finishing while the user searches;
  - an unavailable command found and explained without running;
  - twelve installed packages searched with no guest running and only the
    invoked one started.
- [`crates/pane-core/tests/keywords.rs`](../crates/pane-core/tests/keywords.rs),
  for keywords, alternate titles and same-name rows, with the real
  `sample-keywords` packages in Rust, JavaScript and TypeScript: a command
  found by the keywords its manifest declares, its row keeping its title
  and subtitle, and invoked; the indexed result of a sample provider found
  by its alternate title and by its keywords, never by a query spanning a
  title and a keyword; and two copies of a package naming their sources,
  their indexed results keeping the provider's own order.
- Applications' own other names, keywords and same-name distinctions are
  the applications extension's to test. Those checks left Pane's
  repository with its sources (#285) and live in
  [its repository](https://github.com/pane-app/applications) now. The
  same-name rules for commands are checked in
  [`aliases.rs`](../crates/pane-core/tests/aliases.rs). Between them they
  cover an application found by its program name and by a keyword, two of
  one name showing their distinctions and opening the one chosen, a
  command ranking above an application of the same title with both naming
  what tells them apart, and two copies of a package each naming their
  package's source.

### Typed addresses, paths and folders

- [`crates/pane-core/tests/typed_queries.rs`](../crates/pane-core/tests/typed_queries.rs),
  with the real `sample-matches` packages in Rust, JavaScript and
  TypeScript, the real Quicklinks and Files extensions, and the recording
  system and link-opener fakes:
  - the query analysis: an absolute URL as typed, a bare domain with
    `https://` inferred, `1.5` and words as neither, Windows and Unix
    paths, `~` resolved to the home folder of the file index, `file://`;
  - a command declared `matches: url` or `matches: file-path` listed only
    for such a query, under "Addresses" below the results and above the
    files, never matched by its title, and receiving the parsed address or
    resolved path as its fallback text;
  - `when` honoured with a blank query and while searching;
  - Quicklinks' "Open in Browser" opening the address through the system
    and closing the window, and its "Create Quicklink" opening the form
    with the address prefilled and saving it;
  - Files' "Open" opening a typed path and revealing a program instead of
    running it, and its "Reveal in File Explorer" revealing it.
- `crates/pane-core/src/launcher/typed_query.rs` has unit tests for the
  analysis' grammar, including what Shift+Tab removes of a path.
- Opening a typed URL and a typed path with the system's own handler is a
  release-validation smoke phase to add, not a merge gate (#195).
- [`crates/pane-core/tests/typed_folders.rs`](../crates/pane-core/tests/typed_folders.rs),
  with the real Files extension and the Rust, JavaScript and TypeScript
  files samples, a real index over a fixture home folder and the recording
  opener and system: the entries listed as file results, folders first and
  each in name order, under "Files" below the rows for the path; `~`
  resolved to the home folder of the file index; a missing folder listing
  nothing; Enter opening a document and showing a program in the file
  manager, never running it; the 500-entry bound with the row that says
  so; and Tab's completion and Shift+Tab's query as the launcher answers
  them.
- `crates/pane-core/src/typed_folder.rs` has unit tests for the listing's
  bounds.
- [`crates/pane/tests/file_actions.rs`](../crates/pane/tests/file_actions.rs),
  in the window with real keys: Tab completing to a folder and Enter
  opening its entry, Shift+Tab removing the last path component, and Enter
  on a program showing it and running nothing. Tab and Shift+Tab are also
  held for a typed folder's entries, completing and stepping back once the
  list is published.

### Computed results and the calculator

- The checks of computed results and the calculator are the calculator's
  own. They left Pane's repository with its sources (#285) and live in
  [its repository](https://github.com/pane-app/calculator) now. They run
  the real calculator guest and check:
  - an answer listed first and selected, above a command whose title
    matches too;
  - incomplete, invalid, undefined and operation-free queries listing
    nothing with the status untouched, and completing one answering it;
  - precedence, signs, powers and the number format;
  - parentheses 65 deep, a query over 256 characters and 100,000 leading
    signs or parentheses listing nothing, without an error row or a
    restart of the calculator;
  - Enter reporting the copy and `selected_copy` giving the text;
  - an answer for an older query discarded, the previous query's list held
    until the new query's is published (#201), and none of the new query's
    answers shown before it arrives, nor one for an earlier search of the
    same query;
  - the answer listed and selected while a command asked after it (the
    `faulty` fixture, slow on "0 + 0") is still answering, the list
    published by its budget and the slow result merged below when it
    answers;
  - a selection the user moved kept;
  - the calculator not running until a non-blank query;
  - a ten-character query typed one key at a time at typing speed asking
    the calculator once, its component instantiated at most twice, the
    final list the same as a query typed at once gives (#202);
  - disabling it removing its answer at once, asking it nothing more and
    keeping other results, and enabling it again;
  - a failing and a crashing command (the `faulty` fixture) explained as a
    row while other results stay, and a fresh instance afterwards;
  - a package declaring `rootResults` whose component lacks the interface
    refused at install;
  - the colours (#196): each colour form answering as its uppercase hex;
    the colour card presented with its swatch under "Color" while the
    arithmetic keeps the command's title; colour names and wrong-shaped
    queries listing nothing; the Actions panel offering hex, RGB, HSL and
    OKLCH, each copying its own text;
  - the percentages (#196): each percentage phrase answered within the
    number rules and the length bound, with a query that is not a phrase
    listing nothing;
  - the dates and times (#196): each date and time word answered at the
    clock's moment under "Date & Time" with Enter copying it, in the
    clock's zone (a manual clock in the tests, east and west of UTC,
    across a day boundary), and the panel's ISO 8601 and Unix timestamp
    copying their own text.
- [`samples.rs`](../crates/pane-core/tests/samples.rs): the same computed
  result ("reverse <text>") in Rust, JavaScript and TypeScript.
- [`crates/pane-core/tests/publishing.rs`](../crates/pane-core/tests/publishing.rs),
  for publishing a query's list and merging late answers, with the
  `faulty` fixture's slow provider ("0 + 0", about a second of busy work)
  and the real calculator, all timed by a manual clock:
  - a slow provider holds the previous query's list up to the 200 ms
    budget and publishes without its answer, which then merges late;
  - the selection moving to the new first row when a late answer puts one
    there, with the first row selected, and staying on its row when the
    user moved it;
  - a preselected fallback (ADR 0031) giving way to a late answer;
  - two late answers arriving close together coalescing into one update,
    neither listed before it;
  - a query asking no provider published at once;
  - an answer for an older search discarded as before;
  - a late answer merging without ranking the static rows again, the
    spliced list the same as a full re-rank gives (#202).

  The slow provider's real-clock flow (the budget publishing with the
  calculator's staged answer while the slow call goes on, its answer
  merging within 16 ms) is checked in the calculator's own tests, above.

### Argument fields

- [`crates/pane-core/tests/arguments.rs`](../crates/pane-core/tests/arguments.rs),
  with the real arguments samples in Rust, JavaScript and TypeScript, the
  search and no-view samples, and the `faulty` fixture's slow provider with
  the launcher's clock frozen:
  - the selected row's fields shown with the command's title as the query
    field's placeholder;
  - Enter with a required argument blank marking it, saying "Enter
    <placeholder>" and running nothing, the mark clearing when the
    argument gets a value and the values going when the query does;
  - the values passed to the command on the run, the empty optional ones
    absent and the dropdown with no choice absent too;
  - an alias and a space (or Tab after the alias) entering the fields,
    running the command from its alias with the first argument filled and
    no fallback text, while a command that takes a query keeps its row
    that sends the text and a fallback's query fills the first text
    argument;
  - a command without arguments opened at once: a searching view
    command's own screen, a no-view command run;
  - the last dropdown choice remembered across a restart and dropped once
    it is no longer a choice, and a password recorded nowhere;
  - the typed values surviving a late answer's re-rank, the same row
    selected again with its value kept and the run carrying it;
  - the form still asking wherever the fields do not show (a global
    hotkey, a quick slot, another command's launch), its own contract
    staying as it was.
- [`crates/pane/tests/arguments.rs`](../crates/pane/tests/arguments.rs), in
  the window with real keys: the fields' labelled nodes with their
  required or optional state; Enter with a blank required argument
  focusing it, marking it only once the field was left, and Enter inside a
  blank required one marking it with the status line's "Enter Name"; Tab,
  Shift+Tab and the arrows at the fields' edges moving between the fields
  and the query, with Up and Down moving nothing; Escape returning to the
  query with its text selected; the password drawn and read as dots; "gr
  hello" typed while the query's list is held landing in the first field,
  the alias staying in the query; and a no-view command's hotkey still
  showing the hidden window for the form.

### Learning and recent queries

- [`crates/pane-core/tests/learning.rs`](../crates/pane-core/tests/learning.rs),
  with packages of the no-view sample, the real calculator and
  applications guests, a fake applications system and a manual clock:
  - a use ranking the result first for the blank query and for the query
    it was chosen with (steps 3, 6 and 7, the overbounds bound included);
  - the 17-day and frecency gates;
  - decay over simulated days, a use re-anchoring the score;
  - uses that earn nothing: a global hotkey, a computed answer, a fallback
    row, an unavailable row, Pane's own rows;
  - a quick slot's use with no query;
  - uninstalling a package forgetting what was learned and disabling
    keeping it;
  - survival across a restart over the same data folder;
  - an unreadable record reported on the status line and never replaced;
  - an application keeping its ranking across an update into a new
    version folder;
  - the blank query listing the pins, then the commands and applications
    by frecency under "Commands", with no Suggestions section;
  - the controls over it (#200): Reset Ranking clearing one result only,
    the reset-all clearing everything, and the "Learn from what I choose"
    switch stopping any use being recorded while ranking ignores what was
    kept, kept until it is reset, so turning it on again uses it.
- [`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs), in the
  window with real keys: choosing the second of two equal results a few
  times puts it first for that query, and the Actions panel's Reset
  Ranking clears that result with its toast.
- [`crates/pane/tests/launcher_settings.rs`](../crates/pane/tests/launcher_settings.rs)
  drives the Launcher page's learning controls through the real Settings
  window: "Reset ranking…" asking for its confirmation and clearing
  everything, and the switch found through the Settings search, turned off
  recording nothing while the order ignores what was learned, turned on
  again using what was kept.
- [`crates/pane-core/tests/history.rs`](../crates/pane-core/tests/history.rs),
  for the recent queries Up recalls, with the real Rust arguments sample
  and a registered view command:
  - a query cleared by each path (Escape, the field emptied, a command's
    opening taking it, root search shown fresh over it) recorded with the
    argument values typed with it, the password empty and its text never
    written;
  - a blank query never recorded;
  - consecutive duplicates skipped and the 64 entries kept;
  - the walk restoring the query and its values, one entry back each time
    and nothing past the oldest;
  - the learn switch stopping queries being recorded while what was kept
    stays reachable;
  - the page's reset clearing every entry;
  - the record surviving a restart over the same data folder;
  - an uninstall forgetting the entries that carry its commands' values;
  - an unreadable record reported on the status line and never replaced.
- [`crates/pane/tests/history.rs`](../crates/pane/tests/history.rs), in the
  window with real keys: Up on an empty query restoring the previous query
  with its argument values, and Enter running the command with them;
  repeated Up walking back, and nothing past the oldest; typing ending the
  walk; and Up with another row selected moving the selection, not the
  history.
- [`crates/pane/tests/launcher_settings.rs`](../crates/pane/tests/launcher_settings.rs)
  drives the Launcher page's "Reset search history" through the real
  Settings window: found through the Settings search, asking for its
  confirmation and clearing the record, with the query on screen left
  standing.

### Providers

- [`crates/pane-core/tests/root_providers.rs`](../crates/pane-core/tests/root_providers.rs),
  with the real calculator and Applications guests and a fake system with
  one application:
  - both declare the provider mode;
  - typing "calc", "calculator", "applications" or "appl", or nothing,
    lists no row of either, while "6*7" is still answered under
    "Calculator" and the application is found by name;
  - neither can be given an alias or a hotkey, and the Shortcuts catalog
    lists neither;
  - disabling the calculator stops its answer and enabling it brings it
    back;
  - a provider without `rootResults` or `indexedResults`, or with
    `search`, `takesQuery` or a `schedule`, refused at install with the
    reason;
  - a data folder seeded with a pin, alias, fallback and hotkey for both,
    from before they became providers, losing them at start, with a pinned
    application kept and a toast naming what went, and a second start
    saying nothing.
- `launcher/providers.rs` has unit tests for the toast's wording.

### The window

- [`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs), through
  GPUI's test platform with real key events:
  - typing narrows the results and Enter opens the best match;
  - Up/Down move through the matches while the field keeps focus and
    editing keys still edit it;
  - the no-results state and Escape clearing the field;
  - focus on the field at start and after returning from a command, and
    typing in a command not searching root;
  - input-method composition searching as it composes (driven on the
    field's editing state, with the limits described for
    [forms](forms.md#checks));
  - the matched characters of the best placement highlighted on the row,
    and nothing highlighted for a row found by its subtitle;
  - the accessibility nodes above;
  - typing an expression showing the calculator's answer as the query
    changes, Enter writing it to the clipboard, and an incomplete
    expression showing no results;
  - since #196, a colour answer showing as the card with a swatch under
    "Color", the swatch a colour well named by the colour's value, and
    Enter copying the hex;
  - since #201, typing on from one answered query to another not
    flickering through the intermediate list: the field shows the new
    query at once, the previous list stays while the calculator answers,
    and the published list shows its answer, the launcher saying whether
    the current query's list is published;
  - Clipboard History's field keeping the focus, its rows' places, a move
    and "No results" for a filter that keeps nothing.
- [`crates/pane/tests/held_keys.rs`](../crates/pane/tests/held_keys.rs),
  for the held keys:
  - Enter pressed at once with typing running the published first row,
    not the previous list's;
  - the Open actions chord, a digit chord and Tab waiting the same way;
  - a space typed while the query could still be an alias held, with
    characters typed during the hold landing in order ("ec hello");
  - a held key applied after its 300 ms when a provider never answers (the
    `faulty` fixture, slow on "0 + 0", with the launcher's clock frozen);
  - an Enter repeated during the hold running once;
  - a repeated chord toggling the Actions panel once.
- [`crates/pane/tests/launcher_settings.rs`](../crates/pane/tests/launcher_settings.rs)
  drives the Launcher page's Search sensitivity control through the real
  Settings window: found through the Settings search, its choice recorded,
  and applied by the launcher on the next keystroke, so the same query
  finds more results once it is taken.
- `crates/pane/src/features/announcer.rs` has unit tests for the
  announcer's rules.
- [`crates/pane/tests/announcements.rs`](../crates/pane/tests/announcements.rs),
  window tests of the announcer:
  - the field keeping the focus while the user arrows and no row claiming
    it, and each row's position and the list's size;
  - "<title>, <i> of <n>" as both the announcer's name and value after
    Down, and several fast moves leaving the last row;
  - typing that keeps the first row saying nothing, and typing that
    changes it saying it once after the results settle;
  - "No results";
  - the preselected first fallback said once typing settles, and a move
    into the fallbacks named "Fallbacks";
  - opening a command saying its name and count and then its row, the same
    in a command's list and in the Actions panel (over a command's list and
    over root search);
  - the footer's message said before a selection that changed with it,
    while the footer keeps its status role;
  - a sample's toast said, and a move after it said at once;
  - the footer menu's item said, and said again when it opens again;
  - a selection Pane changes itself said only for another row.

### Native checks

The GUI smoke scripts' search phase (screenshots 24 to 26) types
"typescr", presses Enter, runs "Wait briefly" and asserts the screen is
pixel for pixel the one step 4 reached by Down. Then it types a query that
matches nothing, presses Enter, and asserts root, the search and the
no-results screens all differ. The earlier phases still navigate root with
Down, which moves the selection while the field has focus.

On Linux X11 this ran on 2026-09-28
([evidence](platforms/linux.md#root-search-23)); the macOS and Windows
steps are written but have not run yet. No real input method was used.

A further phase checks [the calculator](platforms/linux.md#calculator-27):
its answer row, and that pasting the copied answer back gives the same
screen as typing it. It ran on Linux X11 on 2026-09-28 and is written but
not run on macOS and Windows.

Results supplied ahead of the query and the installed applications have
their own launcher, window, adapter and native checks, listed in
[applications](applications.md#checks).
