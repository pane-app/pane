# Launcher presentation ownership

[#62](https://github.com/hoangvu12/pane/issues/62) establishes the organization
accepted by [#61](https://github.com/hoangvu12/pane/issues/61) without changing
appearance or launcher behavior.

| Module in `crates/pane/src` | Responsibility |
| --- | --- |
| `main.rs` | Startup, runtime setup, application/window creation |
| `lib.rs` | Narrow public entry points and window/action re-exports |
| `app.rs` | Window state, navigation, dispatch, screen selection and shared frame; maps core rows to presentation values |
| `features/root_search/` | Editable query wiring and search presentation, also reused by command search |
| `extension_views/` | Form controls, validation/focus wiring and custom-view drawing/input adapters |
| `ui/input.rs` | Existing editable-control bindings, shared by query and forms |
| `ui/result_row.rs` | Existing row chrome and accessibility presentation, used for root and command rows |
| `links.rs` | Native link opening adapter |

Shared UI imports no core types and invokes no launcher actions. The app
passes display values into rows and attaches its existing click dispatch.
Core remains GPUI-free. Existing row/control IDs, selectors, key contexts,
public entry points, focus behavior and extension-authored colors are kept.
Only the fields/method used by sibling adapters have crate visibility;
scroll bookkeeping and window dispatch remain private.

No feature placeholders, new toolkit, separate UI crate, semantic palette,
material policy or visual redesign are introduced. Those have real consumers
in later slices; the present shared layer contains only already-used code.

## Control families by consumer (#99)

The UI port ([#90](https://github.com/hoangvu12/pane/issues/90)) gives every
screen a family from the reference boards. Only the Appearance page has an
authored Settings layout; the other pages and the launcher's form are derived
compositions of the board's families (`ui::controls`), reference-consistent,
not claimed to match a board the reference does not have.

| Consumer | Family |
| --- | --- |
| Root search, command lists and command search (`app.rs`) | Root result row (`ui::result_row`, 44/r10); no heading line above a command's view, whose icon and title the footer's left shows instead (`ui::footer::command_lead`, #162) |
| The launcher's confirmations, package previews and Settings › Extensions screens, their hotkey, pause, build, network and runtime details | Root result row for their choices (a launcher list's own rows), their lines in the 13px body type, the screen heading (`ui::shell::screen_heading`) |
| Status, errors and long errors in the launcher | The footer strip's status (`ui::footer`, #95) |
| The launcher's form (`extension_views::form`) | Field groups: a text field's well, a choice field's segmented choice, the error as a field description; the submit button |
| An extension's custom view | The host's frame (`controls::frame`); the extension's drawing keeps its colors |
| Actions panel, Pane menu | Actions entries (`.arow`, 36/r8) |
| Settings sidebar, its search results | Sidebar item (`.nav`, 36/r8) and the search well (#97) |
| Appearance | Field groups of segmented choices, the preview (#98) |
| General: Open Pane hotkey | Settings row with a recorder's well (the binding's caps from `keyboard::hotkey_keys`) and a ghost Reset |
| General: launch at login, tray | Settings rows whose switch is the board's toggle |
| Launcher: opening monitor | The select as a field: the trigger a well, its list in the L2 popover with Actions entries and a well for its search |
| Launcher: reopening | Segmented choice with the chosen one's description |
| Shortcuts | Filter well; caption type for the column names; group headers (list headers, the sidebar's hover); command rows as settings rows; alias and hotkey cells as inline wells (caps from `keyboard::hotkey_keys`); Clear as a ghost button |
| Keyboard | Settings rows with recorder wells (caps from `keyboard::binding_keys`) and ghost Resets |
| Extensions (incl. its confirmations and details) | List items (the sidebar's hover) for the launcher's rows, commands and install sources; Back as a button; lines as descriptions |
| About | Field groups with buttons (the empty board's `.pill`) |
| Notes, refusals, statuses on every page | Field description in its tone (`controls::status_note`) |
| Windows caption buttons in the Settings titlebar | Platform adaptation (#97), unchanged |

Duplicate values are kept where their roles differ: a button reuses the
result layouts' pill tokens (the same `.pill`), a list item and a group header
the sidebar item's hover (`.nav:hover`, white 5%), a ghost button the footer
button's hover (`.fbtn:hover`, white 6%) — while root search's row hover
(white 3.5%) and selection (white 8.5%), the Actions entry's selection (white
11%) and the segment's chosen wash (white 12%) stay their own tokens. The
launcher's lists keep root search's row family: they are the launcher's own
data rows, not Settings controls.

## Virtualized lists (#165)

Every list the launcher draws lays out and paints only the children in
view, plus three rows' height of overscan past either edge, whatever its
length: root search's results, a command's list and the launcher's other
screens' rows (`app/result_list.rs`), Clipboard History's split view, and
the Actions panel. They share `ui::virtual_list`: GPUI's `list` element,
whose children may differ in height (a section label, an answer card, the
pinned home), measured as they are drawn and taken to be a row high until
then. A list is measured again, from its top, only when what its children
show changes (its rows, its sections, its screen) — never when an icon
arrives, a toast shows or the number hints slide — so nothing moves under
the user. Up and Down, and Page Down and Page Up (the rows in view, stopping
at the first and last rows), keep the selection in view; the wheel scrolls
the list itself and is never undone. GPUI's `list` forgets the row-high
guess for the children it has not drawn whenever it is laid out at a new
width (its first layout included), taking them to have no height; the list
gives the guess back before it reveals a row, or revealing one far past the
rows drawn (the last of ten thousand) would stop short of it. The overscan
is laid out, not painted: its rows are drawn to be measured, so their icons
start loading too, a little ahead of the rows scrolling into view.

A frame reads the launcher's view and what it draws of the whole list
(`Launcher::presented_list`: the sections, whether only fallbacks are
listed, whether a row shows a date) once, and each drawn row's own
presentation as it draws it (`Launcher::present_row`), which is also when
the row's web images and system icons start loading: a row out of view
requests none (`Launcher::load_icons_as_shown`, which the window turns on
when it opens; an item's actions' icons load as the Actions panel lists
them). The window's input and screen sync read the screen and the status
alone (`Launcher::screen`, `Launcher::status`) rather than a copy of the
whole view, which a long list made costly on every key press.

Each drawn row tells assistive technology its place in the whole list and
the list's length (`aria-posinset`/`aria-setsize`), since the rows out of
view are not in the tree; the selected row stays the list's active
descendant. The Actions panel's list is as high as its entries, up to the
room the window leaves above the footer. The Settings Shortcuts page, once
it lists 50 commands or more, draws only the command rows within a view's
height of the page's view, the others standing in as blocks of their last
height (`ui::virtual_list::PageWindow`); the row being edited or recorded,
or holding the focus, is always drawn. The other long lists of Settings
do the same past 50 rows: the Extensions group's entries in the sidebar
(the selected one always drawn), an extension's Commands section (the
command whose alias or hotkey is being edited always drawn), and the File
Search page's indexed folders, exclusions and what needs attention.

The window tests are `crates/pane/tests/virtual_lists.rs` (the File
Search page's long list is tested in `file_search_settings.rs`); its ignored
`benchmark` measures keystroke-to-frame latency in root search over 11,000
results and the frame time of scrolling them (targets: 16 ms at the 95th
percentile, 60 frames per second), run optimized on Windows:
`CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test -p pane --release
--test integration virtual_lists::benchmark -- --ignored --nocapture`.

[Retained prototype evidence](evidence/ui-prototype/README.md) keeps the
initial proof and subsequent glass follow-up independently recoverable,
including the authored reference and native captures.

## Prefactor validation (2026-10-02, Windows)

- `cargo fmt -p pane --check`: pass.
- `cargo check -p pane --tests --locked -j 1`: pass, no warnings.
- `cargo test -p pane --test window --test command_search --locked -j 1`:
  **49 window tests + 1 command-search test pass**; real Rust/JS/TS guest
  fixtures, simulated input/IME/accessibility, forms, custom views, query
  activation, scrolling and resize behavior. [Raw result](evidence/ui-prototype/prefactor-tests.log).
- Prototype bundle verified; all 189 overlay files match the saved manifest.
  The authored HTML's Git blob matches the documented SHA-256; a local
  `.gitattributes` rule prevents checkout newline conversion.

The first broad parallel test build exhausted host memory. Validation was
scoped to the required suites and serial compilation. A running retained
prototype locked the shared output executable; the parent renamed that
executable without stopping its windows, permitting the build to complete.
No compiler-cache cleanup or prototype source modification was needed.

Native startup/query/command smoke is handed to the parent for coordinated
desktop access using this commit's built executable. These automated checks
do not claim that native smoke, native IME/assistive technology, or other
platforms have been verified.
