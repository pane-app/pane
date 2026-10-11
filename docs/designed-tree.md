# A designed view as a tree

A command whose `pane.json` entry says `"mode": "designed"` opens a
designed view: a screen the extension describes as a **tree of layout
nodes and UI components that Pane renders**, in Pane's design, with
Pane's keyboard behaviour and accessibility (#235, #237, part of #121).
The tree reaches Pane through the typed envelope of
[ADR 0036](https://github.com/hoangvu12/pane/blob/main/docs/adr/0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md),
which extends the one the list tree uses (`render` and `handle-event`,
[list-tree.md](list-tree.md)) without redefining it.
`pane:extension/command` (`wit/extension.wit`) has what a designed view
needs:

- **`open-view(command, launch) -> result<view, string>`** opens the
  designed view of the command `command` (its id in `pane.json`, so one
  component can serve several commands), passing its launch record, and
  answers a `view` resource. Pane calls this at the command's launch —
  Enter on its row, its alias, a fallback, its hotkey, its quick slot,
  another command — and never calls the command's `render`. A component
  that serves no designed command answers with an error; Pane never calls
  it then. (The custom view's own opener, `open-custom-view(item-id)`, is
  retired: what a custom view drew is a canvas in the tree now, #242.)
- **`view.render(context) -> result<rendered, string>`** draws the view:
  `rendered` carries the tree as JSON, naming the version of the UI
  component set it uses, and `refresh-after-ms`, how long Pane waits
  before asking again (#236, see [Refreshes](#refreshes)). `context` is
  JSON too: the sequence number of this render, the id of the view it
  draws, why Pane asks (`"open"`, the view's first drawing; `"event"`,
  the render an event's answer asks for; `"refresh"`, the drawing the
  view asked to wait for; `"push"`, the drawing the view's own work asked
  for, #243), the version of the component set Pane supports, and the
  sizes its canvases were laid out at (#242), such as `{"render": 1,
  "view": 7, "why": "open", "ui": "1.3", "canvases": {"grid":
  {"width": 376, "height": 108}}}`; it can grow without WIT changes.
  While it renders, the view's canvases may ask Pane to measure text
  (`pane:extension/view.measure-text`, wit/view.wit): the width and
  height a text run occupies at the size and weight the tree's text
  styles resolve to, so text drawn beside shapes fits what it says —
  `pane_extension::view::measure_text` in Rust, `measureText` in
  JavaScript and TypeScript.
- **`view.handle-event(event) -> result<outcome, string>`** handles the
  user's input: `ui-event` carries the callback id the tree named
  (`callback`), the sequence number of the render whose tree the user saw
  (`render`), the key of the node the event was raised on (`key`, `""`
  when the tree gave it none) and the event's details as JSON
  (`payload`: `"{}"` for a press, `{"value": …}` for a change or an
  input — the value the user chose or typed, a boolean, a string or a
  number — and `{"key": "…"}` for a key). Pane then calls `view.render`
  again and draws the answer. `outcome` (`push`, `replace`, `pop`)
  changes the view's navigation stack, below.

The event set: press (a button, a link, a row, a span), change (a
committed value — a field's, a control's), input (a field's value as the
user types it, only when the field asks for it), focus and blur (a
focusable node taking and losing the keyboard), key (a key pressed while
a node that asks for them is focused), and the pop event of the
navigation stack. Tab, Enter and Escape stay with Pane, and so does any
key that acted.

**Stale events:** an event is raised on the tree the user saw — each
control carries the render it was drawn from — and is delivered even if
the view has rendered since, while the view holds no render more than
one past it (the two the SDKs keep) and the node with that key still
names a handler of the event's kind; otherwise it is dropped, and
reported in the extension's log while its package is developed.

Events of one view are delivered one at a time, in order; an answer that
arrives after its view left the screen is discarded, and a tree is shown
only if no later event's tree is on screen yet. An error the extension
answers with — from `open-view`, `render` or `handle-event` — is shown on
the screen while the view stays open with its last good tree; a crash
closes it, as a crash closes a custom view.

The envelope is the only part an extension's component is type-checked
against, so a UI component, a property or an event can be added with no
WIT change, and a component built against an older component set keeps
working.

Authors never see the JSON or the callback ids. In Rust
(`pane-extension`) the command's `open_designed_view` answers with its
view's state, a type implementing `pane_extension::view::View`, whose
`render` builds the tree with `column()`, `row()`, `stack()`, `text()`,
`button()` and every other constructor of `pane_extension::view`, naming
each control's closure with `cx.listener(|this: &mut V| ...)` and its
style through the same-named methods every builder carries; the SDK
writes the tree. In JavaScript and TypeScript (`@pane-app/extension`) the
exported command's `openView` resolves with `createView(Component)` from
`@pane-app/extension/view`, the component written as JSX (`Column`,
`Row`, `Stack`, `Text`, `Button`, every property type-checked; the
`jsx-runtime` module is the JSX import source) or as the elements
`jsxs(...)` builds directly, with `useState`, `useRef` and `useMemo` for
state; the SDK's runtime writes the tree. Both SDKs keep the callbacks of
the last two renders, so a press of what the user could see is delivered
even if the view has rendered since, and an older event is dropped.

The custom views of #21 (see the retired `docs/custom-views.md`, folded
into this document's [canvas](#the-canvas)) became the canvas: their
rectangles and text are canvas operations, their keys and pointer input
the canvas's events, their one accessibility node the canvas's one.

The samples are a counter and a components gallery, in
[Rust](../guests/sample-view/src/lib.rs),
[JavaScript](../guests/sample-view-js/src/index.js) and
[TypeScript](../guests/sample-view-ts/src/index.tsx), held alike by
`crates/pane-core/tests/designed_views.rs`.

## The navigation stack

A designed view's screen holds a **stack** of views: the root view the
command opened, and each view a view's answer pushed above it, the top of
the stack on screen (#239). The stack is Pane's, the extension's state
living in each view's resource; a popped or replaced view's resource is
dropped, never used again. One command's stack holds at most 32 views: a
push that would go beyond it is the extension's error ("the navigation
stack already holds 32 views; a further push is refused"), the view
keeping its last good tree, as it does for any error.

What `handle-event` answers changes the stack, at most one field of the
`outcome` acted on — a `pop` first, then a `replace`, then a `push` (the
SDKs answer exactly one):

- **`push`**: the view the answer names is pushed above the answering
  one, and drawn — its first render. The answering view stays below,
  keeping its state and its tree, which shows again when the pushed view
  pops.
- **`replace`**: the view the answer names takes the answering one's
  place, at its depth; the replaced view is dropped.
- **`pop`**: the answering view pops itself, dropped with its resource,
  answering the string as the pop's result to the view below. Popping
  the root view drops the whole stack, leaving the command as the back
  key's own pop of the root does. A pop that carried no result answers
  the empty string.

The back key (Escape, whatever the Keyboard page binds back to) pops the
top view without asking: the view below's last tree shows **at once**
first — no guest call — and then the view below is told the view above
it popped, re-rendering as after any event. Before popping, the back key
does what it does on every screen: it clears a non-empty search field,
closes an open dropdown or menu, and cancels an input method's
composition, in that order (the "Launcher polish" specification, #123,
owns that order; a designed view shows no search field of its own today,
the List's arriving with #240). Backspace pops the top view too, but not
on key repeat. When the stack holds only the root view, the back key
leaves the command, as it leaves a list command. `window.pop-to-root`
(wit/feedback.wit, the host function) and Shift+Esc drop the whole stack,
leaving for root search at once.

The event that tells a view the one above it popped is the **pop event**:
callback id 0, an id no tree's `onPress` names (the SDKs' ids start at
1), with the pop's result as its payload — `{"pop": "…"}` for a pop that
answered one, `{"pop": null}` for a pop that carried none (the back
key's, so no view's result reached the one below). An answer to it is an
answer like any other: it may itself push, replace or pop.

Authors write the stack through the SDKs: in Rust `cx.push(open)` (the
`on_pop` of `cx.push_with` answering the pushed view's pop),
`cx.replace(open)`, `cx.pop()` and `cx.pop_with(result)`; in JavaScript
and TypeScript `push(target, onPop)`, `replace(target)`, `pop(result)`
and the Raycast-style `Push(target, onPop)` as a button's `onClick`. The
`onPop` handler lives on the pushing side and is never sent over the
wire.

The sample is a navigation sample, in
[Rust](../guests/sample-nav/src/lib.rs),
[JavaScript](../guests/sample-nav-js/src/index.js) and
[TypeScript](../guests/sample-nav-ts/src/index.tsx), held alike by
`crates/pane-core/tests/navigation_stack.rs`.

## Refreshes

A render's answer may ask Pane to draw the view again after some
milliseconds (`refresh-after-ms`), which is how a screen changes by
itself — a timer, a clock, a progress bar, a poll of a service — with no
change to the extension runtime (#236). The ask is clamped to a floor of
100 ms and a ceiling of 24 hours; each refresh is an ordinary guest call
(`render` again, with no event), so it holds other extensions' calls
while it computes, exactly as any call does. Pane sends it numbered with
the view's events — at most one refresh in flight at a time, and a late
answer never replaces a newer tree — and wakes the window for each
answer through the change channel the continuing services use.

- Each answer's own ask paces the view: the refresh happens the delay
  after the answer that asked for it (the period is the delay plus the
  render's own time, as "draw me again after this" reads). Time that
  passes while a refresh runs, or in one jump of the clock, is served by
  the next refresh, not replayed.
- Refreshes run only while the view is on top of its stack — the view
  the screen shows; a view below another, or one whose screen the user
  left, refreshes nothing — and while the launcher's window is shown
  expanded, not
  hidden or collapsed to its search field, where no view is drawn: a
  hidden screen refreshes nothing, so it uses no CPU or battery. A
  refresh that fell due otherwise waits for the next showing, which runs
  it at once — one refresh from the clock's current time, not the ticks
  it missed. Leaving the view cancels its refresh; a view opened afresh
  asks anew.
- An answer that asks for no refresh — one that failed, or the view's
  done — ends any asked before it: the ask belongs to the answer, and
  every answer rules.

The SDKs build their asynchronous helpers on this for timers and
polling. The JavaScript and TypeScript SDK has `useInterval` (run once
each time `ms` passes while the view is open); the Rust SDK has
`.refresh_after(Duration)` on the render's root, and `cx.refreshed()`,
true on the render that answers the view's own ask — the one whose
context says `"refresh"` — where an interval's work runs. A view with
pending data does not ask for a prompt refresh: its arrival asks for a
drawing itself (below). The sample is a timer, in
[Rust](../guests/sample-timer/src/lib.rs),
[JavaScript](../guests/sample-timer-js/src/index.js) and
[TypeScript](../guests/sample-timer-ts/src/index.tsx), held by
`crates/pane-core/tests/view_refresh.rs`.

## Keys and host-owned state

What must react within a frame is Pane's, kept by key (#238): each new
tree is reconciled against the previous one by path key — a parent's key
composed with the child's own, or the child's position when it gives
none — and per key the window keeps:

- the text, the caret, the selection and the input method's composition
  of a field (GPUI's editable text, with undo and the clipboard), and
  the select's open state, query and highlight;
- the focus of every focusable control, its hover and pressed, and each
  scroll region's position;
- stable accessibility ids: the paths themselves.

A key that disappears loses its state, and a node of a different kind
under the same key is new. Static nodes may omit keys and are matched by
position; interactive and stateful nodes (fields, selects, toggles,
scroll regions, anything with a callback) should take one — keys shared
by siblings and stateful nodes without one are matched by position
instead, and reported in the extension's log while the package is
developed. This generalises the form's reconciliation of its fields'
ids and focus.

**Partially controlled inputs:** a field edits at once; the extension
hears `input` as the user types (coalesced to the latest while one is in
flight, throttled when the field asks) and `change` when a value is
committed (Enter, a blur). A value the extension sets wins — but only
when it differs from the node's value in the extension's *previous*
render: an unchanged value is no instruction, and neither is one the
field itself reported (its own text, or a value an input or change event
carried), so echoing a field's value back never fights fast typing. A
value that counts as set replaces the text and moves the caret to its
end.

## Pushes

A view asks Pane to draw it again itself (`pane:extension/view`,
#243): `ask-to-render(view)` names the view by the id its render context
carries (`"view"`), from inside a call or between them. Between them is
the point: the runtime keeps a guest that still has work running while
one of its designed views is open — its store's event loop is driven, as
a call's is — so work a view's answer started (the load a loading state
waits for, a call the view has not awaited) lands in the background, and
the view asks for a drawing the moment it does. The guest's computing
between calls is bounded as any call's is: metered against the compute
limit, yielding at every epoch tick, and a guest that computes for too
long is stopped as unresponsive, its instance dropped and its package
reported. The operation calls it makes meanwhile are served as a call's
are, and its generation's end stops it as it stops a call.

- The drawing a push asks for is sent as a refresh's is: numbered with
  the view's events, at most one drawing of a view in flight at a time,
  asks that arrive meanwhile coalesced into the drawing that follows its
  answer, and a late answer never replaces a newer tree — an answer
  arriving after the view left is dropped with it.
- A push is served only while the view is the top of its stack and the
  launcher's window is shown, as a refresh is; one that arrives
  otherwise waits for the next showing, and an ask for a view no longer
  in the stack is dropped.
- A push is served ahead of a refresh the clock made due — data landing
  is newer than a timer — and the drawing it asks for rules the view's
  ask: its answer carries the next one, exactly as any answer's does.
- The render a push asks for says so in its context (`"why": "push"`),
  so an interval's work does not run in it as a refresh's does: the
  drawing shows what landed, without ticking.

The SDKs' loading helpers are built on this: the JavaScript and
TypeScript SDK's `usePending(load)` starts the work in the view's first
render, answers its loading state, and asks for a drawing when the work
answers; the Rust SDK's `Pending::loading(work)` does the same, running
the work beyond the render that started it. One cycle of loading state,
no flicker, and no timer: the data is drawn the moment it arrives. The
work must be a real future — one that awaits something the host answers
(a file, a service, the clock) or finishes on its own — for nothing
schedules a future that wakes no one. The `loading` command of the
designed view sample shows it, in [Rust](../guests/sample-view/src/lib.rs),
[JavaScript](../guests/sample-view-js/src/index.js) and
[TypeScript](../guests/sample-view-ts/src/index.tsx), and
`crates/pane-core/tests/view_push.rs` holds the pushes: ordered against
event answers, dropped after the view left, drawn while another
extension waits on a slow service.

## The document

A document is one JSON object: its version and its root node.

```json
{
  "version": "1.3",
  "root": {
    "type": "column",
    "key": "main",
    "gap": "m",
    "padding": { "x": "l", "y": "s" },
    "children": [
      { "type": "text", "text": "Count: 3", "style": "title", "level": "secondary" },
      { "type": "row", "gap": "s", "wrap": true, "children": [
        { "type": "button", "key": "up", "label": "Increment", "onPress": 1 },
        { "type": "button", "key": "down", "label": "Reset", "tone": "destructive", "onPress": 2 }
      ] }
    ]
  }
}
```

- `version`: the version of the UI component set the document targets, as
  `"<major>.<minor>"`. A document of Pane's major and any minor is read;
  one of another major is refused as the extension's error, naming both
  versions. Additive changes (a component, a property, an event) bump the
  minor; 1.1 added the layout primitives, the shared UI components, the
  tokens' colour grammar and Markdown (#237); 1.2 added the keyed state
  — fields that edit, the select's searchable state, scroll by key —
  with the inputs' partial control and the focus, blur and key events
  (#238); 1.3 added the standard views — the List with its sections,
  keywords, host filtering, controlled search text and selection, the
  search-bar dropdown, pagination, the empty view and the detail pane;
  the Grid; the Detail, and Markdown images (#240) — the canvas (#242)
  — and the form: the `form` node whose submission is an action, the
  fields' titles, notes, errors and remembered values, the date, tag,
  file and folder pickers, and a select's sections and
  extension-handled search (#241).
- Every node has a `type`, and may have:
  - `key`: the node's stable identity among its siblings, which Pane
    keeps node state under (the keyed reconciler, below: the focus of a
    control, a field's text and caret, a select's open state, a scroll
    region's position, each surviving a re-render that still draws the
    node, wherever it moved);
  - `name`: what assistive technology reads the node by, when the node's
    own content does not name it (a column's or row's);
  - `navigationTitle`: what names the view, read from the root node only
    — the screen's title, shown where a screen's title is (the footer's
    left names it as it names a custom view's; the search header, once
    the List's search field owns it, #240). Ignored on any other node;
  - `focus`: `true` asks for the keyboard — an ask that is new (the tree
    the user saw did not name it) focuses the node, so a view's opening
    ask is an auto-focus and a later one a focus moved from code; an
    unchanged ask leaves the focus wherever the user moved it;
  - `onFocus`, `onBlur`: callback ids run when a focusable node takes and
    loses the keyboard;
  - `onKey`: a callback id run for a key pressed while the node is
    focused, its payload `{"key": "…"}` naming the key as a key sequence
    spells it (its modifiers, then its key). Tab, Enter and Escape stay
    with Pane, and so does any key that acted;
  - `requires`: the minimum minor version of the component set the node
    needs; a node Pane cannot satisfy degrades as an unknown node does;
  - `fallback`: the node drawn instead when Pane does not know the node;
  - `children`: the node's children, in order;
  - the **style** every node carries, below.

### The style every node carries

Any node, whichever its type, may set how it takes space and the surface
it draws — and Pane applies the `hover` and `pressed` variants of that
surface itself, with no call into the extension (Figma-style):

- **Sizing**: `grow` and `shrink` (numbers), `basis`, `width`, `height`,
  `minWidth`, `maxWidth`, `minHeight` and `maxHeight` (a length), and
  `aspectRatio` (a number, its width over its height; a non-positive one
  is left out).
- **Surface**: `background` (a colour), `border`
  (`{ "width": <length>, "color": <colour> }`, either alone allowed),
  `radius` (a radius) and `opacity` (0 to 1).
- **Variants**: `hover` and `pressed`, each an object restating any of
  the surface's properties, drawn while the pointer is over the node or
  the node is pressed.
- **In a stack**: `place` (one of the nine places) and `offset`
  (a length, or `{ "x": ..., "y": ... }`), which move the node from its
  place.

A **length** is a space token (`"xs"`–`"xxl"`), a number of pixels, a
string of pixels (`"12px"`), or a fraction of the parent (`"1/2"` or
`"50%"`). A **radius** is a radius token (`"s"`, `"m"`, `"l"`, `"full"`)
or pixels. Raw lengths are clamped to 0–4096 px; raw opacity to 0–1.

### Tokens and raw values

A **colour** is accepted wherever a token is, in any of its forms:

- a **tone token** by its name: `neutral` (the primary ink), `accent`,
  `success`, `warning`, `danger`, `primary`, `secondary`, and the seven
  palette colours `red`, `orange`, `yellow`, `green`, `blue`, `purple`,
  `magenta`;
- a **raw colour**: `#RGB`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`,
  `hsl()` or `hsla()`, channels as numbers or percentages;
- a **pair**: `{ "light": <colour>, "dark": <colour> }`, one per
  appearance;
- an **exact** colour: `{ "raw": <colour or pair> }`, drawn as it is.

**Contrast correction:** a colour used for *text or an icon* — a text's
`color`, a span's, a link's, a tag's, a tint — is corrected against the
surface it is drawn on, by moving its lightness until it reads at **2.5:1**
(the ratio the spec chose, deliberately below Pane's own icon 3.0 and
text 4.5), unless it is given as `raw`. A colour used for a *background*
is drawn as it is; the text and icons on it are corrected against it,
composited over what is behind.

The token set also names the **space** tokens (`xs`–`xxl`), the **text
style** (`heading`, `title`, `body`, `caption`, `mono`, `small-mono`) and
**text level** (`primary`, `secondary`, `tertiary`, `quaternary`) of #235,
the **radius** tokens (`s`, `m`, `l`, `full`), and the **icon size**
tokens (`s`, `m`, `l`, `xl`) — a stable mapping layer over Pane's
private theme, resolving per appearance (and over the background image),
so the theme can change without renaming a token.

Buttons are focusable, in tree order: Tab and Shift+Tab move through
them, and Enter and Space press the focused one. Escape stays with Pane,
as it does for a custom view: it pops the navigation stack (above), and
leaves the screen when only the root view is on it.

### Layout primitives

- `column`, `row`: children below or beside each other, with `gap`,
  `padding` (as #235), `align`, `justify` and `wrap`.
- `stack`: children drawn over each other, each placed at one of the nine
  places — `place` on the child, the stack's `align` the default — with
  an optional `offset` from it.
- `scroll`: a scrolling region, `orientation` `"vertical"` (the default)
  or `"horizontal"`, whose position Pane keeps by key.
- `spacer`: space, growing to fill what it is given unless its style says
  otherwise.
- `divider`: a hairline rule, `orientation` as a scroll's.
- `card`: children on Pane's own card surface, with a column's layout.

Layout is computed by GPUI's flex layout; nothing is laid out by the
extension.

### Text

One text: `text` (a string), or `spans` — an array of
`{ "text", "style", "level", "color", "code", "onPress" }` — where a span
with `onPress` is a **link**, its callback run by a press. A text takes
`style`, `level`, `color` (a colour), `size` (pixels), `weight`
(100–900) and `truncate` (one line with an ellipsis). In the SDKs the
children spell the `text`, and `Span` children make the spans.

### Shared UI components

Pane's own, drawn from Pane's own families; each has one accessibility
mapping (role, name, value, state) owned by the host. All are focusable
where they name a callback, in tree order: Tab and Shift+Tab move through
them, Enter and Space press or change the focused one, and the segmented
control's and slider's arrows move them. Escape stays with Pane, as it
does for a custom view: it leaves the screen.

- **`button`** — `label`, `tone` (`default`, `secondary`, `ghost`,
  `accent`, `destructive`), `icon`, `keys` (a key sequence drawn after
  its label), `enabled` and `onPress`. Without `onPress` it draws its
  label as a text.
- **`link`** — `label`, `color`, `onPress`: its label underlined in the
  accent.
- **`icon`**, **`icon-tile`** — the icon model below, `size`; the tile
  draws it on Pane's own tile.
- **`image`** — the icon model below as `image`, `size`, `fit`
  (`contain`, the default, `cover`, `fill`); its children are its
  placeholder, drawn while it loads or cannot be read.
- **`rich-row`** — `title`, `subtitle`, `icon`,
  `accessories` (`{ "text", "tag", "color" }`), `onPress`: the
  launcher's own result row as a component.
- **`keycap`** — `key`: the key its cap shows.
- **`key-sequence`** — `keys`: one cap per key, drawn as the launcher's
  shortcuts are.
- **`tag`**, **`badge`** — `text`, `color`: a short label in a chip, a
  short count in a filled one.
- **`toggle`** — `on`, `label`, `onChange`: the Settings board's switch.
  A change tells the extension `{"value": true|false}`.
- **`checkbox`** — `checked`, `label`, `onChange`: alike, a box with a
  check.
- **`segmented`** — `options` (`{ "value", "label" }`), `value`,
  `label`, `onChange`: the Settings board's track; its arrows move the
  choice, a change telling the extension `{"value": "…"}`.
- **`select`** — `options`, `value`, `label`, `onChange`: Pane's
  searchable select, keyed, so its open state, query and highlight
  survive a re-render that still draws it. Enter, Down and a click open
  its popup — the choices as rows, narrowed by a local search, the
  arrows and Enter choosing — and a choice commits through `onChange`;
  Escape and Tab close it without choosing. The trigger shows the choice
  the tree names, read live each frame.
- **`slider`** — `value`, `min` (0), `max` (1), `step` (0.1), `label`,
  `onChange`: its arrows adjust it by its step, a click moves it to
  where its rail was clicked, and a change tells the extension
  `{"value": <number>}`.
- **`progress`** — `value` (0 to 1), `label`.
- **`loading`** — `label`: an indeterminate bar, its highlight sweeping
  it.
- **`markdown`** — `markdown` (the source; the children spell it): a
  CommonMark subset with GitHub's tables and task lists, drawn in the
  theme's own typography. LaTeX and images are out of scope here.
- **`section-header`** — `title` (the children spell it), `note`.
- **`metadata-list`** — `items`:
  `{ "label", "value", "onPress" (making the value a link), "tags",
  "separator" }`.
- **`empty-state`** — `title`, `description`, `icon`, its children its
  actions: the notice the launcher's own empty board draws.
- **`text-input`**, **`password-input`**, **`text-area`** — `value`,
  `placeholder`, `label`, `onInput`, `throttleMs`, `onChange`: a well
  holding GPUI's own editable text — typing, the input method's
  composition, the selection, undo and the clipboard never wait for the
  extension — keyed, so its editing state survives a re-render that
  still draws the field. The field is partially controlled: the value
  the tree names is the value it starts from and the one an echo of it
  never fights, `onInput` hears its value as the user types (coalesced
  to the latest while one is in flight, throttled to `throttleMs` when
  given), and `onChange` runs on its commits — Enter (a text area's
  Enter inserts a newline; its commits are blurs') and a blur.

### The canvas

`canvas` is a leaf the extension draws into, for what the UI components
cannot show (#242, the custom view's successor): a chart, a colour wheel,
a game board, beside ordinary controls. It is **keyed** like every
stateful node (its focus, its drag and its bounds surviving a re-render
that still draws it), and **sized by its style** as any node is — a fixed
`width` and `height`, or the space the layout gives it (`grow`), which
the render context names for it by its key, a change of which is the
resize event.

- **`ops`**: the drawing operations, painted in order (later ones over
  earlier ones), clipped to the canvas's size. At most 20,000; a canvas
  over it is the extension's error, the view keeping its last good tree.
  Each text operation at most 64 KiB.
- **`role`**, **`label`**, **`value`**: what the canvas is to assistive
  technology — **one node**, whatever it draws. The role is one of the
  widened set `color-well`, `slider`, `image`, `figure`, `group`,
  `generic` (the default); the value is what it holds, as a colour well
  names its chosen colour. The drawing itself adds no nodes.
- The input handlers, each naming a callback id: **`onPointerDown`**,
  **`onPointerUp`**, **`onPointerMove`** (a drag's, coalesced to the
  latest while one is in flight, as the custom view's were),
  **`onPointerEnter`** and **`onPointerLeave`** (the hover: enter carries
  no point, and neither is a drag), **`onWheel`**, **`onDoubleClick`**,
  **`onSecondary`** (the right button), and **`onResize`**. A press
  outside the drawing area is no press of the canvas; a drag continues
  outside it; the window's deactivation ends one the window can no
  longer see the release of.
- The semantic handlers, for a control-like canvas that parses no keys:
  **`onIncrement`** (the up arrow), **`onDecrement`** (the down one) and
  **`onActivate`** (Space). A canvas naming them takes those keys
  itself; they reach no `onKey`.
- Keys ride the node's own **`onKey`** as every focusable node's do: all
  non-reserved keys with their modifiers. Tab, Enter and Escape stay
  with Pane.

The canvas's payload events, as the listeners read them:

- A pointer event: `{"event":"pointer-down"|"pointer-move"|
  "pointer-up"|"pointer-enter"|"pointer-leave"|"double-click"|
  "secondary","x":12,"y":34,"button":"left"|"right","clicks":1,
  "ctrl":false,"alt":false,"shift":false}` — the point in the canvas's
  own space, its origin the top-left corner. Enter and leave carry no
  point: the window learns of the hover before it sees a position.
- A wheel event: `{"event":"wheel","x":..,"y":..,"dx":0,"dy":-2,
  "unit":"pixel"|"line","ctrl":..,"alt":..,"shift":..}`.
- A resize: `{"event":"resize","width":376,"height":108}`.

Every payload's numbers are JSON numbers; the SDKs read them into typed
events (`pane_core`'s tests hold the three languages to the same trees).

The drawing operations:

- **Rectangles and circles**: `{"op":"rect","x":..,"y":..,"width":..,
  "height":..,"radius":..,"fill":<colour>,"stroke":<colour>,
  "strokeWidth":..}` (the `radius` rounding the corners) and
  `{"op":"circle","x":..,"y":..,"radius":..,"fill":..,"stroke":..}` —
  a circle's `x` and `y` its center.
- **Paths**, built operation by operation and painted by a fill or a
  stroke: `{"op":"move"|"line","x":..,"y":..}`, `{"op":"quad","cx":..,
  "cy":..,"x":..,"y":..}` and `{"op":"cubic","c1x":..,"c1y":..,"c2x":..,
  "c2y":..,"x":..,"y":..}`, `{"op":"arc","x":..,"y":..,"radius":..,
  "start":..,"end":..,"ccw":false}` (radians), `{"op":"close"}`, then
  `{"op":"fill","color":<colour>}` or `{"op":"stroke","color":<colour>,
  "width":..,"cap":"butt"|"round"|"square","join":"miter"|"round"|
  "bevel"}` — the stroke one pixel wide when the tree gives none.
- **Text**: `{"op":"text","x":..,"y":..,"text":"..","style":<text
  style>,"level":<text level>,"color":<colour>,"size":..,"weight":..}`
  — one line, its top-left corner at `x`, `y`, in a token style or a
  raw size, its colour corrected against the canvas's surface as a
  text's is.
- **Images**: `{"op":"image","image":<icon model>,"x":..,"y":..,
  "width":..,"height":..}` — any image source of the icon model, drawn
  while it loads as its fallback or nothing.
- **Clip**: `{"op":"clip","x":..,"y":..,"width":..,"height":..}` — what
  follows is clipped to this rectangle, intersected with the clips
  before it.
- **Transform**: `{"op":"translate"|"scale","x":..,"y":..}` and
  `{"op":"rotate","degrees":..}` — the space the operations that follow
  draw in, from the origin.

Colours are the tree's grammar everywhere: tokens, raw values, pairs or
exact ones. A **fill or stroke is a drawing**, drawn as resolved and
never corrected (an authored swatch shows its exact value); only the
text of a text operation is corrected, as any text is.

The drawing is painted through GPUI's `canvas` and path builder, never
one element per shape: a run of shape operations between text and image
ones is one GPUI canvas, so the order the tree paints them in is kept —
and text and image operations draw as the positioned elements GPUI's own
text and images are, above the shapes they follow. Rectangles expand
into path operations at parse time; a stroke's caps and joins are the
path builder's.

The samples' colour picker is one: `guests/sample-{rust,js,ts}` draw
their swatch grid, preview and measured hex code as canvas operations
inside a layout. The designed fixture's `canvas` command answers one by
hand, filling its space and drawing what it receives, which
`crates/pane/tests/designed_canvas.rs` drives.

### Forms

A **`form`** node (#241) is a form: a column of children — the author's
own layout, the fields anywhere in it — whose submission is an action.
It has `onSubmit` (the callback a submission runs) and `submitLabel`
(what its submit button says; "Submit" by default), and Pane draws the
button under the fields: Enter in a single-line field of the form
submits it, as Ctrl+Enter does in a text area (whose Enter inserts a
newline) and the button, the footer's primary action and the invoke
binding do. A submission collects the values of every field in the
form's subtree — keyed by each field's **`key`**; a field without one
carries no value — and runs `onSubmit` with `{"values": {"<key>":
<value>, …}}` as the event's payload. A form whose `onSubmit` is absent
is one **Pane itself answers**: the window collects the values and hands
them to the launcher, which is how Pane's own forms (the argument form,
the Setup screen, the alias form and the npm and Git install forms) are
the same components.

Validation is the extension's: a submission's answer is the tree it
answers with, so the field's **`error`** — drawn under it, in the danger
tone — is how a rejected value is reported. The remembered values are
Pane's: a field with **`remember`** has its last submitted value kept as
the package's *settings* (a record Pane stores for the installed
package, under the key `pane-form/<form key>/<field key>`) and prefilled
the next time a view opens holding that field — the value is a starting
point, never an instruction, and a re-render never fights the field's
live value.

Every field carries what a field says around its control: **`title`**
(over it, and naming it to assistive technology when the kind's own
label does not), **`info`** (the note under it), **`error`**, the
**`default`** value it starts from when the tree names none, and
**`remember`**. A field's **`id`** in the SDKs is its key: the identity
its value is submitted under and its state kept by. The fields:

- **`text-input`**, **`password-input`**, **`text-area`** — as above,
  wearing the field's chrome; `text-area`'s Enter inserts a newline and
  Ctrl+Enter submits the form.
- **`checkbox`**, **`toggle`** — `on`/`checked`, `label` (beside the
  control), `onChange`; a submission carries `true` or `false`.
- **`select`** — the searchable dropdown, with `options`' `section`
  (options of one section are drawn together under its label),
  `placeholder`, `default` (the option chosen when none is named) and
  `onInput` (the popup's query as the user types it, for a dropdown
  whose search the extension handles: `search: false` makes the popup
  show the options the extension answers with, filtering none of its
  own). A submission carries the chosen option's `value`.
- **`date-picker`** — a date field, typed or stepped with the arrow keys
  (Up and Down step a date by a day), its value `"YYYY-MM-DD"`.
- **`date-time-picker`** — alike, `"YYYY-MM-DD HH:MM"`, stepped by a
  minute.
- **`tag-picker`** — a multi-select of `options`, the chosen `tags`
  drawn as chips; Enter commits the highlighted matching option, a chip
  click removes it, and a change tells the extension
  `{"value": ["…", …]}`. A submission carries the chosen values.
- **`file-picker`**, **`folder-picker`** — a path typed or chosen with
  the system's dialog (the "Choose…" button opens it); `multiple` (the
  SDKs' `allowMultiple`) chooses several paths as chips. A submission
  carries the path, or the list of them.
- **`markdown`** — the description between a form's fields (the SDKs'
  `Form.Description`); **`divider`** — its separator; **`link`** — its
  link. They are the shared components they always were.

The SDKs name them Raycast-style: `Form`, `Form.TextField`,
`Form.PasswordField`, `Form.TextArea`, `Form.Checkbox`, `Form.Toggle`,
`Form.DatePicker`, `Form.DateTimePicker`, `Form.Dropdown`,
`Form.TagPicker`, `Form.FilePicker`, `Form.FolderPicker`,
`Form.Description`, `Form.Separator` and `Form.Link` in JSX, whose
`onSubmit` is told the values object; `pane_extension::form` in Rust
(`Form::new()`, `form::text_field("name")`, `cx.form_listener(|this,
values| …)`), the values read by key (`values.text("name")`,
`values.on("dark")`, `values.list("tags")`).

### Icons and images

`icon`, `icon-tile`, `image`, a button's `icon`, a rich row's and an
empty state's use the icon model the list tree uses
([list-tree.md](list-tree.md), ADR 0036): a reicon builtin by name, a
packaged PNG or SVG by path (with its `@dark` and `@light` variants, or a
light and dark pair), a web image by URL through the existing bounded
download and cache, a system file or application icon, or bounded inline
`data:` — each with `tint` (a colour, corrected), `mask`
(`circle`, `rounded-rectangle`), `fallback` (four deep) and `tooltip`,
which also names it to assistive technology; an icon without one is
decoration. Web images load as rows' icons do: the fallback shows until
one arrives, and on failure.

## The standard views

The List, the Grid and the Detail are UI components of the tree whose
behaviour the host owns (#240), so the common case needs no layout work at
all. A view names at most one of them — the first in its tree, in document
order; the screen presents it: the search header's field (with the
navigation title above it, replacing the footer title #239 drew), the
launcher's own rows list, virtualised and accessible as root search's is.

- **`list`** — the List document's item vocabulary on the tree
  ([list-tree.md](list-tree.md)): `list-section` and `list-item` children
  (a `list-item` may sit in the list directly, an implicit section), a
  `list-dropdown` child (below), and any other child the empty view drawn
  when no item is. The list's own properties: `searchPlaceholder`,
  `searchText` (the field's value, partially controlled as a field's is),
  `selectedKey`, `is-loading` (the loading bar, drawn once loading has run
  past 300 ms — `LOADING_MS`; the bar's look is the launcher polish's,
  #248), `is-showing-detail`, `hasMore`, `pageSize` (clamped 1–100), and
  the events `onSearchText` (the view handles the search itself: Pane then
  filters nothing, and the text is told to the view, throttled to 250 ms
  by default), `onSelectionChange` (told the selected item's key) and
  `onLoadMore` (raised as the selection nears the end of what is shown
  while `hasMore` says more is there — once for each growth of the list).
  Pane filters the items by the field's text with the root-search matcher
  ([root-search.md](root-search.md)), on the title, the subtitle and the
  item's `keywords`, unless the view handles the search itself.
- **`list-section`** — a group of items under its `title` and `subtitle`.
- **`list-item`** — a key (`key`), a `title`, a `subtitle`, `keywords`, an
  `icon`, `accessories` (the List document's: text, a date, a tag, each
  with an icon, a colour and a tooltip), `onPress` (the primary action),
  `actions` (`{ "title", "onPress" }`; the second is the secondary
  action, Ctrl+Enter — the Actions panel that lists them all is the
  action model's, #120), `detail` (the pane's content when the item is
  selected and the list `is-showing-detail` — built for the selected item
  alone, the render context's `selected` naming it), and children (the
  author's own row subtree, drawn in the place of the standard row while
  Pane still selects and activates it).
- **`grid`** — the List's behaviour with cells: `grid-item` children in
  `list-section`s, each section with its own `columns` (1–8, 5 by
  default), `aspectRatio`, `fit` and `inset`. A cell is an `image`, a
  `color`, or the author's own subtree in its children, with a `title`
  and a `subtitle` under it. The arrows move the selection by cell and by
  row, Ctrl+Up and Ctrl+Down by section, no wrap.
- **`list-dropdown`** — the search-bar dropdown, beside the search field:
  `items` (`{ "value", "title" }`), the chosen `value`, a `placeholder`,
  and `onChange` (told the chosen item's id). It is the searchable select
  (see `select`), keyed by its place in the tree.
- **`detail`** — a scrolled column of what a record or an article is:
  Markdown, a metadata panel, a loading state, actions, as children.

A Markdown image is the icon model's: `![alt](source =WxH "title")`, its
source a packaged image's path, a web image's URL or inline `data:`, its
size the `=WxH` its source named (either number may be left out). A
Markdown link is opened by Pane with the system's handler.

The event set grows with the views: `onSelectionChange` (a selection's
move, `{"value": "…"}` naming the selected item's key), `onLoadMore`
(`{}`), and the List's `onSearchText`, an input event (`{"value": "…"}`)
raised on the list's key. The render context grows with them too: it
names the selected item's key (`"selected"`) of the view's List or Grid,
which the detail pane's content is built for.

The command-search export (`pane:extension/command-search`,
`docs/command-search.md`) is retired: a command's search is the List's
search-text event, the view handling it itself (throttled) or Pane
filtering locally. Pane's registered Files command keeps its host-side
search of the file index (#177) — no command of a package opens a search
field of its own.

## Reading a document

Reading is strict in the document's shape and lenient exactly where
versioning asks for it, as the list tree's is:

- A property Pane does not know is ignored, at every level.
- A token this version does not know is left out, as a newer minor
  version may add one.
- A node whose `type` Pane does not know, or whose `requires` it does not
  meet, draws the `fallback` the tree gave, else its children if it has
  any, and nothing without either.
- An icon Pane cannot read is not drawn (its image node draws its
  placeholder).
- A document that is not JSON, lacks `version` or `root`, or a node's
  `type`, a text's `text` or `spans`, a button's or link's `label`, a
  markdown's `markdown`, a rich row's or section header's `title`, an
  empty state's `title`, a segmented's or select's `options`, a slider's
  or progress's `value`, or gives one of the wrong type, is
  **unreadable**: the command's failure, which Pane reports as its own
  reading of what the extension answered ("Pane could not read what the
  extension answered: ..."), never a crash.
- A document over one of the limits, or of another major version, is the
  **extension's error** ("The extension reported an error: ..."): the view
  keeps its last good tree, which an unreadable tree leaves too.

### Limits

One document may hold at most 10,000 nodes (counting `fallback`
subtrees), be at most 64 levels deep and at most 4 MiB of JSON; each text
node at most 64 KiB; each markdown source at most 1 MiB; each inline
`data:` image at most 1 MiB. Raw lengths are clamped to 0–4096 px, raw
opacity to 0–1. One command's navigation stack holds at most 32 views.
Provisional (#121), as the spec's proposed defaults.

The parse and reconcile cost of a 500- and a 5,000-node tree is measured
and recorded with the change (`crates/pane-core/tests/designed_cost.rs`
prints it), not gated.
