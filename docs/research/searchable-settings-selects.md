# Searchable settings selects

Bounded research, 2026-10-02. Sources are official Raycast documentation and
the official shadcn/ui repository. This note informs a Pane control; it does
not propose copying React code or changing Pane's visual language.

## Key findings

Raycast has two relevant, extension-facing controls:

- `Form.Dropdown` is a form item: it has an `id`, controlled/uncontrolled
  value, validation error, sections and dropdown items. Its documented
  contract is collecting a form value, not a generic settings-screen widget.
  Its search is explicit: `placeholder` labels the dropdown search field,
  `filtering` controls native filtering, `onSearchTextChange` observes queries,
  and item `keywords` supplement titles for matching.
  [Raycast Form API](https://developers.raycast.com/api-reference/user-interface/form)
- `List.Dropdown` is a search-bar accessory: it adds a second filtering
  dimension, opens from click or `⌘P`, supports sections, `onChange`, persisted
  `storeValue`, and its own search/filtering. [Raycast List API](https://developers.raycast.com/api-reference/user-interface/list)

Raycast's public UI docs say extension UI is declared with React and rendered
into Raycast's native UI. The separate Settings manual documents a first-party
Settings surface with settings search and extension preferences, but does not
publish its implementation as an SDK control. Therefore these APIs are useful
behavioral references, not evidence about the closed-source Settings UI.
[UI API](https://developers.raycast.com/api-reference/user-interface) ·
[Settings manual](https://manual.raycast.com/settings) ·
[Preferences API](https://developers.raycast.com/api-reference/preferences)

## Filtering and selection semantics

Raycast `List` filtering is built in: titles and optional keywords are indexed;
custom filtering is enabled by setting `filtering={false}` (and an
`onSearchTextChange` listener implicitly disables it unless filtering is set
back to true). `List.Dropdown` has the same filtering switch and an
`onSearchTextChange` callback. [List search/filtering](https://developers.raycast.com/api-reference/user-interface/list)

The current shadcn combobox page defines a combobox as an autocomplete input
with suggestions. The old `/docs/components/combobox` URL redirects to
`/docs/components/base/combobox`. The current Base variant is a composed
`Combobox` root with `ComboboxInput`, popup/content, empty state, list, item,
groups/collections, optional chips, multiple selection, and `autoHighlight`.
Its source delegates behavior to `@base-ui/react` and exposes controlled
`value`/`onValueChange`; it is a headless behavior/style recipe, not a
settings persistence model. [Current docs](https://ui.shadcn.com/docs/components/base/combobox) ·
[pinned Base source](https://github.com/shadcn-ui/ui/blob/295a1f114a138f23b5dfee0e0c6812394dfeb90c/apps/v4/registry/bases/base/ui/combobox.tsx)

The pinned repository also retains a distinct older-style example named
`combobox-demo`: a `Popover` trigger contains a `Command` with
`CommandInput`, `CommandList`, `CommandItem`, and `onSelect`; selecting updates
local state and closes the popover. That pattern is not the current Base
primitive: it composes shadcn `Popover` + `Command` (historically backed by
`cmdk`) and the example owns filtering/selection wiring. [pinned example](https://github.com/shadcn-ui/ui/blob/295a1f114a138f23b5dfee0e0c6812394dfeb90c/apps/v4/registry/new-york-v4/examples/combobox-demo.tsx)

The current `/components/radix/combobox` route should not be used as evidence
that this Popover+Command recipe is the current Radix implementation: the
pinned MDX currently presents the same generated Combobox structure and
`@base-ui/react` dependency. The Popover+Command comparison above is the
repository's retained legacy/New York example.

Do not collapse “active option” into “committed value”. While open, query text
filters the candidate set and arrow keys can move an active/highlighted row.
The committed value is the setting already accepted by the form/model. A
highlight is preview/navigation state until Enter or click commits it. This
separation is explicit in the old example's `open`, `value`, and `onSelect`
state, even though that example commits immediately on selection.

## Implications for Pane

Pane is Rust GPUI CE, not React. Treat Raycast/shadcn as interaction evidence;
implement the control with existing Pane visual tokens and GPUI focus/input
primitives. Do not import React, Base UI, Radix, `cmdk`, or their styling.

Use a searchable select only when the option set is large, labels are hard to
scan, or users know a distinctive substring/keyword. For a short stable list,
the existing choice control is faster and clearer. Filtering should be local
and deterministic for settings unless a future contract explicitly needs
remote data; show an empty state without changing the saved setting.

Recommended state:

```text
closed: committed value, focused field
open:   committed value + query + filtered options + active option
```

Opening and typing/filtering never saves. Enter or clicking an option commits
that option, updates the visible field, closes the popup, and returns focus to
the field. Escape cancels query/active-option changes, closes, leaves the
committed value untouched, and returns focus to the field. Tab closes the popup
and continues normal form traversal; it must not implicitly commit a merely
highlighted option. Shift-Tab follows the same no-implicit-commit rule.

Keyboard behavior should support Up/Down (and Home/End where natural), Enter,
Escape, Tab/Shift-Tab, and pointer selection. The query input must retain
focus while filtering; active-row movement must not steal text focus. Preserve
marked text and commit behavior for IME composition: filtering should react to
committed text, not prematurely interpret marked composition as a selection.
When no option is active, Enter does nothing rather than saving an accidental
value.

These semantics intentionally differ from Pane's current form contract, where
choice arrow keys change the choice and Enter submits the form. A searchable
settings select needs an internal draft/commit boundary first; the containing
form's eventual submit remains a separate operation.

## Sources and reproducibility

- Raycast docs were checked directly: [Form](https://developers.raycast.com/api-reference/user-interface/form),
  [List](https://developers.raycast.com/api-reference/user-interface/list),
  [Preferences](https://developers.raycast.com/api-reference/preferences).
- shadcn's current docs and redirect were checked directly: [combobox redirect](https://ui.shadcn.com/docs/components/combobox),
  [Base page](https://ui.shadcn.com/docs/components/base/combobox),
  [Radix page](https://ui.shadcn.com/docs/components/radix/combobox).
- GitHub snippets were fetched with `gh api` at shadcn/ui commit
  `295a1f114a138f23b5dfee0e0c6812394dfeb90c`; the Raycast extension examples
  were checked at `7e67bdb0eb90315c17c07ebd696fd5abdc2a021a`.
