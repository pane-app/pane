# A designed view as a tree

A designed view is a screen an extension describes as a tree of layout
nodes and UI components that Pane renders, in Pane's design, with Pane's
keyboard behaviour and accessibility. A command opens one when its
`pane.json` entry says `"mode": "designed"`.

This page is the reference for that tree: how Pane calls a view, the
JSON document and its components, the standard views, forms, the canvas
and the limits. Extension authors write views through the SDKs and never
see the JSON; [Writing a view with the SDKs](#writing-a-view-with-the-sdks)
shows how.

Tracking: #235, #237, part of #121.

## How Pane calls a view

The tree reaches Pane through the typed envelope of
[ADR 0036](https://github.com/hoangvu12/pane/blob/main/docs/adr/0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md).
It extends the envelope the list tree uses (`render` and `handle-event`,
[list-tree.md](list-tree.md)) without redefining it.
`pane:extension/command` (`wit/extension.wit`) has what a designed view
needs: `open-view`, `view.render` and `view.handle-event`.

The envelope is the only part an extension's component is type-checked
against. A UI component, a property or an event can be added with no WIT
change, and a component built against an older component set keeps
working.

### Opening a view

`open-view(command, launch) -> result<view, string>` opens the designed
view of the command `command`, passing its launch record, and answers a
`view` resource. `command` is the command's id in `pane.json`, so one
component can serve several commands.

Pane calls `open-view` at the command's launch, however it is launched:
Enter on its row, its alias, a fallback, its hotkey, its quick slot, or
another command. It never calls the command's `render`. A component that
serves no designed command answers with an error; Pane never calls it
then.

The custom view's own opener, `open-custom-view(item-id)`, is retired:
what a custom view drew is a canvas in the tree now (#242). The custom
views of #21 (described in the retired custom-views document, folded
into this document's [canvas](#the-canvas)) became the canvas. Their
rectangles and text are canvas operations, their keys and pointer input
the canvas's events, and their one accessibility node the canvas's one.

### Rendering a view

`view.render(context) -> result<rendered, string>` draws the view.
`rendered` carries:

- the tree as JSON, naming the version of the UI component set it uses;
- `refresh-after-ms`, how long Pane waits before asking again (#236, see
  [Refreshes](#refreshes)).

`context` is JSON too, and can grow without WIT changes. It holds:

- the sequence number of this render;
- the id of the view it draws;
- why Pane asks: `"open"` (the view's first drawing), `"event"` (the
  render an event's answer asks for), `"refresh"` (the drawing the view
  asked to wait for) or `"push"` (the drawing the view's own work asked
  for, #243);
- the version of the component set Pane supports;
- the sizes its canvases were laid out at (#242).

For example:

```json
{"render": 1, "view": 7, "why": "open", "ui": "1.3", "canvases": {"grid": {"width": 376, "height": 108}}}
```

While it renders, the view's canvases may ask Pane to measure text
(`pane:extension/view.measure-text`, wit/view.wit). Pane answers with the
width and height a text run occupies at the size and weight the tree's
text styles resolve to, so text drawn beside shapes fits what it says.
The SDKs call it `pane_extension::view::measure_text` in Rust and
`measureText` in JavaScript and TypeScript.

### Handling an event

`view.handle-event(event) -> result<outcome, string>` handles the user's
input. The `ui-event` carries:

- `callback`: the callback id the tree named;
- `render`: the sequence number of the render whose tree the user saw;
- `key`: the key of the node the event was raised on (`""` when the tree
  gave it none);
- `payload`: the event's details as JSON. That is `"{}"` for a press,
  `{"value": …}` for a change or an input (the value the user chose or
  typed: a boolean, a string or a number), and `{"key": "…"}` for a key.

Pane then calls `view.render` again and draws the answer. The `outcome`
(`push`, `replace`, `pop`) changes the view's
[navigation stack](#the-navigation-stack).

## Events

The event set:

- press: a button, a link, a row, a span;
- change: a committed value, a field's or a control's;
- input: a field's value as the user types it, only when the field asks
  for it;
- focus and blur: a focusable node taking and losing the keyboard;
- key: a key pressed while a node that asks for them is focused;
- the pop event of the navigation stack.

Tab, Enter and Escape stay with Pane, and so does any key that acted.

### Stale events

An event is raised on the tree the user saw: each control carries the
render it was drawn from. It is delivered even if the view has rendered
since, as long as both of these hold:

- the view holds no render more than one past it (the two the SDKs
  keep);
- the node with that key still names a handler of the event's kind.

Otherwise it is dropped, and reported in the extension's log while its
package is developed.

### Order and errors

Events of one view are delivered one at a time, in order. An answer that
arrives after its view left the screen is discarded. A tree is shown
only if no later event's tree is on screen yet.

An error the extension answers with, from `open-view`, `render` or
`handle-event`, is shown on the screen while the view stays open with its
last good tree. A crash closes it, as a crash closes a custom view.

## Writing a view with the SDKs

The SDKs write the tree and name the callbacks for the author.

In Rust (`pane-extension`), the command's `open_designed_view` answers
with its view's state: a type implementing `pane_extension::view::View`.
Its `render` builds the tree with `column()`, `row()`, `stack()`,
`text()`, `button()` and every other constructor of
`pane_extension::view`. Each control's closure is named with
`cx.listener(|this: &mut V| ...)`, and its style is set through the
same-named methods every builder carries.

In JavaScript and TypeScript (`@pane-app/extension`), the exported
command's `openView` resolves with `createView(Component)` from
`@pane-app/extension/view`. The component is written as JSX (`Column`,
`Row`, `Stack`, `Text`, `Button`, every property type-checked; the
`jsx-runtime` module is the JSX import source), or as the elements
`jsxs(...)` builds directly. `useState`, `useRef` and `useMemo` hold
state; the SDK's runtime writes the tree.

Both SDKs keep the callbacks of the last two renders. A press of what the
user could see is delivered even if the view has rendered since, and an
older event is dropped.

The samples are a counter and a components gallery, in
[Rust](../guests/sample-view/src/lib.rs),
[JavaScript](../guests/sample-view-js/src/index.js) and
[TypeScript](../guests/sample-view-ts/src/index.tsx), held alike by
`crates/pane-core/tests/designed_views.rs`.

## The navigation stack

A designed view's screen holds a stack of views (#239): the root view the
command opened, and each view a view's answer pushed above it. The top of
the stack is on screen.

The stack is Pane's; the extension's state lives in each view's
resource. A popped or replaced view's resource is dropped and never used
again.

One command's stack holds at most 32 views. A push that would go beyond
it is the extension's error ("the navigation stack already holds 32
views; a further push is refused"), and the view keeps its last good
tree, as it does for any error.

### What an answer does to the stack

What `handle-event` answers changes the stack. At most one field of the
`outcome` is acted on: a `pop` first, then a `replace`, then a `push`
(the SDKs answer exactly one).

- `push`: the view the answer names is pushed above the answering one,
  and drawn (its first render). The answering view stays below, keeping
  its state and its tree, which shows again when the pushed view pops.
- `replace`: the view the answer names takes the answering one's place,
  at its depth. The replaced view is dropped.
- `pop`: the answering view pops itself, dropped with its resource, and
  answers the string as the pop's result to the view below. Popping the
  root view drops the whole stack, leaving the command as the back key's
  own pop of the root does. A pop that carried no result answers the
  empty string.

### The back key

The back key (Escape, or whatever the Keyboard page binds back to) pops
the top view without asking. The view below's last tree shows at once
first, with no guest call. Then the view below is told the view above it
popped, and re-renders as after any event.

Before popping, the back key does what it does on every screen, in this
order:

1. clears a non-empty search field;
2. closes an open dropdown or menu;
3. cancels an input method's composition.

The "Launcher polish" specification (#123) owns that order. A designed
view shows no search field of its own today; the List's arrives with
#240.

Backspace pops the top view too, but not on key repeat. When the stack
holds only the root view, the back key leaves the command, as it leaves a
list command. `window.pop-to-root` (wit/feedback.wit, the host function)
and Shift+Esc drop the whole stack, leaving for root search at once.

### The pop event

The event that tells a view the one above it popped is the pop event.
Its callback id is 0, an id no tree's `onPress` names (the SDKs' ids
start at 1). Its payload is the pop's result:

- `{"pop": "…"}` for a pop that answered one;
- `{"pop": null}` for a pop that carried none (the back key's, so no
  view's result reached the one below).

An answer to it is an answer like any other: it may itself push, replace
or pop.

### Writing the stack in the SDKs

In Rust: `cx.push(open)` (with `cx.push_with`, whose `on_pop` answers the
pushed view's pop), `cx.replace(open)`, `cx.pop()` and
`cx.pop_with(result)`.

In JavaScript and TypeScript: `push(target, onPop)`, `replace(target)`,
`pop(result)` and the Raycast-style `Push(target, onPop)` as a button's
`onClick`. The `onPop` handler lives on the pushing side and is never
sent over the wire.

The sample is a navigation sample, in
[Rust](../guests/sample-nav/src/lib.rs),
[JavaScript](../guests/sample-nav-js/src/index.js) and
[TypeScript](../guests/sample-nav-ts/src/index.tsx), held alike by
`crates/pane-core/tests/navigation_stack.rs`.

## Refreshes

A render's answer may ask Pane to draw the view again after some
milliseconds (`refresh-after-ms`). That is how a screen changes by
itself (a timer, a clock, a progress bar, a poll of a service) with no
change to the extension runtime (#236).

The ask is clamped to a floor of 100 ms and a ceiling of 24 hours. Each
refresh is an ordinary guest call (`render` again, with no event), so it
holds other extensions' calls while it computes, exactly as any call
does. Pane sends it numbered with the view's events, with at most one
refresh in flight at a time, and a late answer never replaces a newer
tree. Pane wakes the window for each answer through the change channel
the continuing services use.

### When a refresh runs

- Each answer's own ask paces the view. The refresh happens the delay
  after the answer that asked for it, so the period is the delay plus the
  render's own time, as "draw me again after this" reads. Time that
  passes while a refresh runs, or in one jump of the clock, is served by
  the next refresh, not replayed.
- Refreshes run only while the view is on top of its stack (the view the
  screen shows), and while the launcher's window is shown expanded. A
  view below another, or one whose screen the user left, refreshes
  nothing. Neither does a window that is hidden or collapsed to its
  search field, where no view is drawn, so a hidden screen uses no CPU or
  battery.
- A refresh that fell due otherwise waits for the next showing, which
  runs it at once: one refresh from the clock's current time, not the
  ticks it missed.
- Leaving the view cancels its refresh; a view opened afresh asks anew.
- An answer that asks for no refresh (one that failed, or the view's
  done) ends any asked before it. The ask belongs to the answer, and
  every answer rules.

### Timers in the SDKs

The SDKs build their asynchronous helpers on this for timers and
polling. The JavaScript and TypeScript SDK has `useInterval`, which runs
once each time `ms` passes while the view is open. The Rust SDK has
`.refresh_after(Duration)` on the render's root, and `cx.refreshed()`,
true on the render that answers the view's own ask (the one whose context
says `"refresh"`), where an interval's work runs.

A view with pending data does not ask for a prompt refresh: its arrival
asks for a drawing itself ([Pushes](#pushes)).

The sample is a timer, in [Rust](../guests/sample-timer/src/lib.rs),
[JavaScript](../guests/sample-timer-js/src/index.js) and
[TypeScript](../guests/sample-timer-ts/src/index.tsx), held by
`crates/pane-core/tests/view_refresh.rs`.

## Keys and host-owned state

What must react within a frame is Pane's, kept by key (#238). Each new
tree is reconciled against the previous one by path key: a parent's key
composed with the child's own, or the child's position when it gives
none. Per key, the window keeps:

- the text, the caret, the selection and the input method's composition
  of a field (GPUI's editable text, with undo and the clipboard), and the
  select's open state, query and highlight;
- the focus of every focusable control, its hover and pressed, and each
  scroll region's position;
- stable accessibility ids: the paths themselves.

A key that disappears loses its state, and a node of a different kind
under the same key is new.

Static nodes may omit keys and are matched by position. Interactive and
stateful nodes (fields, selects, toggles, scroll regions, anything with a
callback) should take one. Keys shared by siblings, and stateful nodes
without one, are matched by position instead and reported in the
extension's log while the package is developed. This generalises the
form's reconciliation of its fields' ids and focus.

### Partially controlled inputs

A field edits at once. The extension hears `input` as the user types
(coalesced to the latest while one is in flight, throttled when the field
asks) and `change` when a value is committed (Enter, a blur).

A value the extension sets wins, but only when it differs from the
node's value in the extension's *previous* render. An unchanged value is
no instruction, and neither is one the field itself reported (its own
text, or a value an input or change event carried). Echoing a field's
value back therefore never fights fast typing. A value that counts as
set replaces the text and moves the caret to its end.

## Pushes

A view can ask Pane to draw it again itself (`pane:extension/view`,
#243). `ask-to-render(view)` names the view by the id its render context
carries (`"view"`), and can be called from inside a call or between
calls.

Between calls is what it is for. The runtime keeps a guest that still has
work running while one of its designed views is open: its store's event
loop is driven, as a call's is. Work a view's answer started (the load a
loading state waits for, a call the view has not awaited) lands in the
background, and the view asks for a drawing the moment it does.

The guest's computing between calls is bounded as any call's is. It is
metered against the compute limit and yields at every epoch tick. A guest
that computes for too long is stopped as unresponsive, its instance
dropped and its package reported. The operation calls it makes meanwhile
are served as a call's are, and its generation's end stops it as it
stops a call.

### How a push is served

- The drawing a push asks for is sent as a refresh's is: numbered with
  the view's events, with at most one drawing of a view in flight at a
  time. Asks that arrive meanwhile are coalesced into the drawing that
  follows its answer. A late answer never replaces a newer tree, and an
  answer arriving after the view left is dropped with it.
- A push is served only while the view is the top of its stack and the
  launcher's window is shown, as a refresh is. One that arrives otherwise
  waits for the next showing. An ask for a view no longer in the stack is
  dropped.
- A push is served ahead of a refresh the clock made due, since data
  landing is newer than a timer. The drawing it asks for rules the view's
  ask: its answer carries the next one, exactly as any answer's does.
- The render a push asks for says so in its context (`"why": "push"`), so
  an interval's work does not run in it as a refresh's does. The drawing
  shows what landed, without ticking.

### Loading helpers in the SDKs

The SDKs' loading helpers are built on pushes. The JavaScript and
TypeScript SDK's `usePending(load)` starts the work in the view's first
render, answers its loading state, and asks for a drawing when the work
answers. The Rust SDK's `Pending::loading(work)` does the same, running
the work beyond the render that started it. The view shows one cycle of
loading state, with no flicker and no timer, and the data is drawn the
moment it arrives.

The work must be a real future: one that awaits something the host
answers (a file, a service, the clock) or finishes on its own. Nothing
schedules a future that wakes no one.

The `loading` command of the designed view sample shows it, in
[Rust](../guests/sample-view/src/lib.rs),
[JavaScript](../guests/sample-view-js/src/index.js) and
[TypeScript](../guests/sample-view-ts/src/index.tsx).
`crates/pane-core/tests/view_push.rs` holds the pushes: ordered against
event answers, dropped after the view left, and drawn while another
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

### Versions

`version` is the version of the UI component set the document targets,
as `"<major>.<minor>"`. Pane reads a document of its own major and any
minor. One of another major is refused as the extension's error, naming
both versions.

Additive changes (a component, a property, an event) bump the minor:

- 1.1 added the layout primitives, the shared UI components, the tokens'
  colour grammar and Markdown (#237).
- 1.2 added the keyed state (fields that edit, the select's searchable
  state, scroll by key), with the inputs' partial control and the focus,
  blur and key events (#238).
- 1.3 added the standard views (#240): the List with its sections,
  keywords, host filtering, controlled search text and selection, the
  search-bar dropdown, pagination, the empty view and the detail pane;
  the Grid; the Detail, and Markdown images. It also added the canvas
  (#242), and the form (#241): the `form` node whose submission is an
  action, the fields' titles, notes, errors and remembered values, the
  date, tag, file and folder pickers, and a select's sections and
  extension-handled search.

### What every node may have

Every node has a `type`, and may have:

- `key`: the node's stable identity among its siblings, which Pane keeps
  node state under ([the keyed reconciler](#keys-and-host-owned-state)).
  That covers the focus of a control, a field's text and caret, a
  select's open state and a scroll region's position, each surviving a
  re-render that still draws the node, wherever it moved.
- `name`: what assistive technology reads the node by, when the node's
  own content does not name it (a column's or row's).
- `navigationTitle`: what names the view, read from the root node only
  and ignored on any other node. It is the screen's title, shown where a
  screen's title is: the footer's left names it as it names a custom
  view's, and the search header, once the List's search field owns it
  (#240).
- `focus`: `true` asks for the keyboard. An ask that is new (the tree the
  user saw did not name it) focuses the node, so a view's opening ask is
  an auto-focus and a later one a focus moved from code. An unchanged ask
  leaves the focus wherever the user moved it.
- `onFocus`, `onBlur`: callback ids run when a focusable node takes and
  loses the keyboard.
- `onKey`: a callback id run for a key pressed while the node is focused.
  Its payload `{"key": "…"}` names the key as a key sequence spells it
  (its modifiers, then its key). Tab, Enter and Escape stay with Pane,
  and so does any key that acted.
- `requires`: the minimum minor version of the component set the node
  needs. A node Pane cannot satisfy degrades as an unknown node does.
- `fallback`: the node drawn instead when Pane does not know the node.
- `children`: the node's children, in order.
- the [style every node carries](#the-style-every-node-carries).

### The style every node carries

Any node, whichever its type, may set how it takes space and the surface
it draws. Pane applies the `hover` and `pressed` variants of that surface
itself, with no call into the extension (Figma-style).

| Group | Properties |
| --- | --- |
| Sizing | `grow` and `shrink` (numbers); `basis`, `width`, `height`, `minWidth`, `maxWidth`, `minHeight` and `maxHeight` (a length); `aspectRatio` (a number, its width over its height; a non-positive one is left out) |
| Surface | `background` (a colour); `border` (`{ "width": <length>, "color": <colour> }`, either alone allowed); `radius` (a radius); `opacity` (0 to 1) |
| Variants | `hover` and `pressed`, each an object restating any of the surface's properties, drawn while the pointer is over the node or the node is pressed |
| In a stack | `place` (one of the nine places) and `offset` (a length, or `{ "x": ..., "y": ... }`), which move the node from its place |

A length is a space token (`"xs"` to `"xxl"`), a number of pixels, a
string of pixels (`"12px"`), or a fraction of the parent (`"1/2"` or
`"50%"`). A radius is a radius token (`"s"`, `"m"`, `"l"`, `"full"`) or
pixels. Raw lengths are clamped to 0 to 4096 px; raw opacity to 0 to 1.

### Tokens and raw values

A colour is accepted wherever a token is, in any of its forms:

- a tone token by its name: `neutral` (the primary ink), `accent`,
  `success`, `warning`, `danger`, `primary`, `secondary`, and the seven
  palette colours `red`, `orange`, `yellow`, `green`, `blue`, `purple`,
  `magenta`;
- a raw colour: `#RGB`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`,
  `hsl()` or `hsla()`, channels as numbers or percentages;
- a pair: `{ "light": <colour>, "dark": <colour> }`, one per appearance;
- an exact colour: `{ "raw": <colour or pair> }`, drawn as it is.

A colour used for text or an icon (a text's `color`, a span's, a link's,
a tag's, a tint) gets contrast correction, unless it is given as `raw`.
Pane corrects it against the surface it is drawn on, moving its
lightness until it reads at 2.5:1. The spec chose that ratio
deliberately below Pane's own icon 3.0 and text 4.5. A colour used for a
background is drawn as it is; the text and icons on it are corrected
against it, composited over what is behind.

The token set also names:

- the space tokens (`xs` to `xxl`);
- the text style (`heading`, `title`, `body`, `caption`, `mono`,
  `small-mono`) and text level (`primary`, `secondary`, `tertiary`,
  `quaternary`) of #235;
- the radius tokens (`s`, `m`, `l`, `full`);
- the icon size tokens (`s`, `m`, `l`, `xl`).

The tokens are a stable mapping layer over Pane's private theme,
resolving per appearance (and over the background image), so the theme
can change without renaming a token.

Buttons are focusable, in tree order: Tab and Shift+Tab move through
them, and Enter and Space press the focused one. Escape stays with Pane,
as it does for a custom view. It pops the navigation stack (above), and
leaves the screen when only the root view is on it.

### Layout primitives

- `column`, `row`: children below or beside each other, with `gap`,
  `padding` (as #235), `align`, `justify` and `wrap`.
- `stack`: children drawn over each other. Each is placed at one of the
  nine places (`place` on the child, the stack's `align` the default),
  with an optional `offset` from it.
- `scroll`: a scrolling region, `orientation` `"vertical"` (the default)
  or `"horizontal"`, whose position Pane keeps by key.
- `spacer`: space, growing to fill what it is given unless its style says
  otherwise.
- `divider`: a hairline rule, `orientation` as a scroll's.
- `card`: children on Pane's own card surface, with a column's layout.

Layout is computed by GPUI's flex layout; nothing is laid out by the
extension.

### Text

A text holds one text: `text` (a string), or `spans`, an array of
`{ "text", "style", "level", "color", "code", "onPress" }`. A span with
`onPress` is a link, its callback run by a press.

A text takes `style`, `level`, `color` (a colour), `size` (pixels),
`weight` (100 to 900) and `truncate` (one line with an ellipsis). In the
SDKs the children spell the `text`, and `Span` children make the spans.

### Shared UI components

These are Pane's own, drawn from Pane's own families. Each has one
accessibility mapping (role, name, value, state) owned by the host.

All are focusable where they name a callback, in tree order. Tab and
Shift+Tab move through them, Enter and Space press or change the focused
one, and the segmented control's and slider's arrows move them. Escape
stays with Pane, as it does for a custom view: it leaves the screen.

#### Buttons, links and display

- `button`: `label`, `tone` (`default`, `secondary`, `ghost`, `accent`,
  `destructive`), `icon`, `keys` (a key sequence drawn after its label),
  `enabled` and `onPress`. Without `onPress` it draws its label as a
  text.
- `link`: `label`, `color`, `onPress`. Its label is underlined in the
  accent.
- `icon`, `icon-tile`: the [icon model](#icons-and-images), `size`. The
  tile draws it on Pane's own tile.
- `image`: the icon model as `image`, `size`, `fit` (`contain`, the
  default, `cover`, `fill`). Its children are its placeholder, drawn
  while it loads or cannot be read.
- `rich-row`: `title`, `subtitle`, `icon`, `accessories`
  (`{ "text", "tag", "color" }`), `onPress`. The launcher's own result
  row as a component.
- `keycap`: `key`, the key its cap shows.
- `key-sequence`: `keys`, one cap per key, drawn as the launcher's
  shortcuts are.
- `tag`, `badge`: `text`, `color`. A short label in a chip, and a short
  count in a filled one.
- `progress`: `value` (0 to 1), `label`.
- `loading`: `label`. An indeterminate bar, its highlight sweeping it.

#### Controls

- `toggle`: `on`, `label`, `onChange`. The Settings board's switch. A
  change tells the extension `{"value": true|false}`.
- `checkbox`: `checked`, `label`, `onChange`. Alike, a box with a check.
- `segmented`: `options` (`{ "value", "label" }`), `value`, `label`,
  `onChange`. The Settings board's track; its arrows move the choice, a
  change telling the extension `{"value": "…"}`.
- `select`: `options`, `value`, `label`, `onChange`. Pane's searchable
  select, keyed, so its open state, query and highlight survive a
  re-render that still draws it. Enter, Down and a click open its popup:
  the choices as rows, narrowed by a local search, the arrows and Enter
  choosing. A choice commits through `onChange`; Escape and Tab close it
  without choosing. The trigger shows the choice the tree names, read
  live each frame.
- `slider`: `value`, `min` (0), `max` (1), `step` (0.1), `label`,
  `onChange`. Its arrows adjust it by its step, a click moves it to where
  its rail was clicked, and a change tells the extension
  `{"value": <number>}`.

#### Content

- `markdown`: `markdown` (the source; the children spell it). A
  CommonMark subset with GitHub's tables and task lists, drawn in the
  theme's own typography. LaTeX and images are out of scope here.
- `section-header`: `title` (the children spell it), `note`.
- `metadata-list`: `items`, each
  `{ "label", "value", "onPress" (making the value a link), "tags",
  "separator" }`.
- `empty-state`: `title`, `description`, `icon`, and its children its
  actions. The notice the launcher's own empty board draws.

#### Text fields

`text-input`, `password-input` and `text-area` take `value`,
`placeholder`, `label`, `onInput`, `throttleMs` and `onChange`. Each is a
well holding GPUI's own editable text: typing, the input method's
composition, the selection, undo and the clipboard never wait for the
extension. They are keyed, so a field's editing state survives a
re-render that still draws it.

The field is partially controlled. The value the tree names is the value
it starts from, and the one an echo of it never fights. `onInput` hears
its value as the user types (coalesced to the latest while one is in
flight, throttled to `throttleMs` when given). `onChange` runs on its
commits: Enter and a blur. A text area's Enter inserts a newline, so its
commits are blurs.

### The canvas

`canvas` is a leaf the extension draws into, for what the UI components
cannot show (#242, the custom view's successor): a chart, a colour
wheel, a game board, beside ordinary controls.

It is keyed like every stateful node: its focus, its drag and its bounds
survive a re-render that still draws it. It is sized by its style as any
node is, with a fixed `width` and `height` or the space the layout gives
it (`grow`). The render context names that size for it by its key, and a
change of it is the resize event.

#### Canvas properties

- `ops`: the drawing operations, painted in order (later ones over
  earlier ones), clipped to the canvas's size. At most 20,000; a canvas
  over it is the extension's error, and the view keeps its last good
  tree. Each text operation is at most 64 KiB.
- `role`, `label`, `value`: what the canvas is to assistive technology,
  which is one node whatever it draws. The role is one of the widened set
  `color-well`, `slider`, `image`, `figure`, `group`, `generic` (the
  default). The value is what it holds, as a colour well names its chosen
  colour. The drawing itself adds no nodes.

#### Canvas input

The input handlers each name a callback id:

- `onPointerDown`, `onPointerUp`;
- `onPointerMove`: a drag's, coalesced to the latest while one is in
  flight, as the custom view's were;
- `onPointerEnter` and `onPointerLeave`: the hover. Enter carries no
  point, and neither is a drag;
- `onWheel`, `onDoubleClick`, `onSecondary` (the right button) and
  `onResize`.

A press outside the drawing area is no press of the canvas. A drag
continues outside it, and the window's deactivation ends one the window
can no longer see the release of.

For a control-like canvas that parses no keys, there are semantic
handlers: `onIncrement` (the up arrow), `onDecrement` (the down one) and
`onActivate` (Space). A canvas naming them takes those keys itself; they
reach no `onKey`.

Other keys ride the node's own `onKey`, as every focusable node's do: all
non-reserved keys with their modifiers. Tab, Enter and Escape stay with
Pane.

#### Canvas event payloads

The canvas's payload events, as the listeners read them:

- A pointer event: `{"event":"pointer-down"|"pointer-move"|
  "pointer-up"|"pointer-enter"|"pointer-leave"|"double-click"|
  "secondary","x":12,"y":34,"button":"left"|"right","clicks":1,
  "ctrl":false,"alt":false,"shift":false}`. The point is in the canvas's
  own space, its origin the top-left corner. Enter and leave carry no
  point: the window learns of the hover before it sees a position.
- A wheel event: `{"event":"wheel","x":..,"y":..,"dx":0,"dy":-2,
  "unit":"pixel"|"line","ctrl":..,"alt":..,"shift":..}`.
- A resize: `{"event":"resize","width":376,"height":108}`.

Every payload's numbers are JSON numbers. The SDKs read them into typed
events (`pane_core`'s tests hold the three languages to the same trees).

#### Drawing operations

- Rectangles and circles: `{"op":"rect","x":..,"y":..,"width":..,
  "height":..,"radius":..,"fill":<colour>,"stroke":<colour>,
  "strokeWidth":..}` (the `radius` rounding the corners) and
  `{"op":"circle","x":..,"y":..,"radius":..,"fill":..,"stroke":..}`. A
  circle's `x` and `y` are its center.
- Paths, built operation by operation and painted by a fill or a stroke:
  `{"op":"move"|"line","x":..,"y":..}`, `{"op":"quad","cx":..,
  "cy":..,"x":..,"y":..}` and `{"op":"cubic","c1x":..,"c1y":..,"c2x":..,
  "c2y":..,"x":..,"y":..}`, `{"op":"arc","x":..,"y":..,"radius":..,
  "start":..,"end":..,"ccw":false}` (radians), `{"op":"close"}`, then
  `{"op":"fill","color":<colour>}` or `{"op":"stroke","color":<colour>,
  "width":..,"cap":"butt"|"round"|"square","join":"miter"|"round"|
  "bevel"}`. The stroke is one pixel wide when the tree gives none.
- Text: `{"op":"text","x":..,"y":..,"text":"..","style":<text
  style>,"level":<text level>,"color":<colour>,"size":..,"weight":..}`.
  One line, its top-left corner at `x`, `y`, in a token style or a raw
  size, its colour corrected against the canvas's surface as a text's
  is.
- Images: `{"op":"image","image":<icon model>,"x":..,"y":..,
  "width":..,"height":..}`. Any image source of the icon model, drawn
  while it loads as its fallback or nothing.
- Clip: `{"op":"clip","x":..,"y":..,"width":..,"height":..}`. What
  follows is clipped to this rectangle, intersected with the clips
  before it.
- Transform: `{"op":"translate"|"scale","x":..,"y":..}` and
  `{"op":"rotate","degrees":..}`. They set the space the operations that
  follow draw in, from the origin.

#### Colours and painting

Colours are the tree's grammar everywhere: tokens, raw values, pairs or
exact ones. A fill or stroke is a drawing: it is drawn as resolved and
never corrected, so an authored swatch shows its exact value. Only the
text of a text operation is corrected, as any text is.

The drawing is painted through GPUI's `canvas` and path builder, never
one element per shape. A run of shape operations between text and image
ones is one GPUI canvas, so the order the tree paints them in is kept.
Text and image operations draw as the positioned elements GPUI's own
text and images are, above the shapes they follow. Rectangles expand
into path operations at parse time; a stroke's caps and joins are the
path builder's.

The samples' colour picker is one: `guests/sample-{rust,js,ts}` draw
their swatch grid, preview and measured hex code as canvas operations
inside a layout. The designed fixture's `canvas` command answers one by
hand, filling its space and drawing what it receives, which
`crates/pane/tests/designed_canvas.rs` drives.

### Forms

A `form` node (#241) is a form: a column of children (the author's own
layout, with the fields anywhere in it) whose submission is an action.
It has `onSubmit`, the callback a submission runs, and `submitLabel`,
what its submit button says ("Submit" by default). Pane draws the button
under the fields.

The form is submitted by:

- Enter in a single-line field of the form;
- Ctrl+Enter in a text area (whose Enter inserts a newline);
- the button;
- the footer's primary action;
- the invoke binding.

A submission collects the values of every field in the form's subtree,
keyed by each field's `key` (a field without one carries no value). It
runs `onSubmit` with `{"values": {"<key>": <value>, …}}` as the event's
payload.

A form whose `onSubmit` is absent is one Pane itself answers: the window
collects the values and hands them to the launcher. That is how Pane's
own forms (the argument form, the Setup screen, the alias form and the
npm and Git install forms) are the same components.

#### Validation and remembered values

Validation is the extension's. A submission's answer is the tree it
answers with, so the field's `error` (drawn under it, in the danger tone)
is how a rejected value is reported.

Remembered values are Pane's. A field with `remember` has its last
submitted value kept as the package's *settings*: a record Pane stores
for the installed package, under the key
`pane-form/<form key>/<field key>`. It is prefilled the next time a view
opens holding that field. The value is a starting point, never an
instruction, and a re-render never fights the field's live value.

#### Fields

Every field carries what a field says around its control:

- `title`: over it, and naming it to assistive technology when the
  kind's own label does not;
- `info`: the note under it;
- `error`;
- `default`: the value it starts from when the tree names none;
- `remember`.

A field's `id` in the SDKs is its key: the identity its value is
submitted under and its state kept by.

The fields:

- `text-input`, `password-input`, `text-area`: as above, wearing the
  field's chrome. `text-area`'s Enter inserts a newline and Ctrl+Enter
  submits the form.
- `checkbox`, `toggle`: `on`/`checked`, `label` (beside the control),
  `onChange`. A submission carries `true` or `false`.
- `select`: the searchable dropdown, with `options`' `section` (options
  of one section are drawn together under its label), `placeholder`,
  `default` (the option chosen when none is named) and `onInput`. The
  `onInput` is the popup's query as the user types it, for a dropdown
  whose search the extension handles: `search: false` makes the popup
  show the options the extension answers with, filtering none of its
  own. A submission carries the chosen option's `value`.
- `date-picker`: a date field, typed or stepped with the arrow keys (Up
  and Down step a date by a day). Its value is `"YYYY-MM-DD"`.
- `date-time-picker`: alike, `"YYYY-MM-DD HH:MM"`, stepped by a minute.
- `tag-picker`: a multi-select of `options`, the chosen `tags` drawn as
  chips. Enter commits the highlighted matching option, a chip click
  removes it, and a change tells the extension `{"value": ["…", …]}`. A
  submission carries the chosen values.
- `file-picker`, `folder-picker`: a path typed or chosen with the
  system's dialog (the "Choose…" button opens it). `multiple` (the SDKs'
  `allowMultiple`) chooses several paths as chips. A submission carries
  the path, or the list of them.
- `markdown` is the description between a form's fields (the SDKs'
  `Form.Description`), `divider` its separator and `link` its link. They
  are the shared components they always were.

#### Forms in the SDKs

The SDKs name them Raycast-style. In JSX: `Form`, `Form.TextField`,
`Form.PasswordField`, `Form.TextArea`, `Form.Checkbox`, `Form.Toggle`,
`Form.DatePicker`, `Form.DateTimePicker`, `Form.Dropdown`,
`Form.TagPicker`, `Form.FilePicker`, `Form.FolderPicker`,
`Form.Description`, `Form.Separator` and `Form.Link`, whose `onSubmit`
is told the values object.

In Rust, `pane_extension::form` (`Form::new()`,
`form::text_field("name")`, `cx.form_listener(|this, values| …)`), the
values read by key (`values.text("name")`, `values.on("dark")`,
`values.list("tags")`).

### Icons and images

`icon`, `icon-tile`, `image`, a button's `icon`, a rich row's and an
empty state's use the icon model the list tree uses
([list-tree.md](list-tree.md), ADR 0036). An icon is one of:

- a reicon builtin by name;
- a packaged PNG or SVG by path (with its `@dark` and `@light` variants,
  or a light and dark pair);
- a web image by URL, through the existing bounded download and cache;
- a system file or application icon;
- bounded inline `data:`.

Each takes `tint` (a colour, corrected), `mask` (`circle`,
`rounded-rectangle`), `fallback` (four deep) and `tooltip`, which also
names it to assistive technology; an icon without one is decoration.
Web images load as rows' icons do: the fallback shows until one arrives,
and on failure.

## The standard views

The List, the Grid and the Detail are UI components of the tree whose
behaviour the host owns (#240), so the common case needs no layout work
at all.

A view names at most one of them: the first in its tree, in document
order. The screen presents it with the search header's field (with the
navigation title above it, replacing the footer title #239 drew) and the
launcher's own rows list, virtualised and accessible as root search's
is.

### The List

`list` puts the List document's item vocabulary on the tree
([list-tree.md](list-tree.md)). Its children are `list-section` and
`list-item` children (a `list-item` may sit in the list directly, an
implicit section), a `list-dropdown` child
([below](#the-search-bar-dropdown)), and any other child, which is the
empty view drawn when no item is.

The list's own properties:

- `searchPlaceholder`;
- `searchText`: the field's value, partially controlled as a field's is;
- `selectedKey`;
- `is-loading`: the loading bar, drawn once loading has run past 300 ms
  (`LOADING_MS`). The bar's look is the launcher polish's (#248);
- `is-showing-detail`;
- `hasMore`;
- `pageSize`: clamped 1 to 100.

Its events:

- `onSearchText`: the view handles the search itself. Pane then filters
  nothing, and the text is told to the view, throttled to 250 ms by
  default.
- `onSelectionChange`: told the selected item's key.
- `onLoadMore`: raised as the selection nears the end of what is shown
  while `hasMore` says more is there, once for each growth of the list.

Unless the view handles the search itself, Pane filters the items by the
field's text with the root-search matcher ([root-search.md](root-search.md)),
on the title, the subtitle and the item's `keywords`.

### Sections and items

- `list-section`: a group of items under its `title` and `subtitle`.
- `list-item`: a key (`key`), a `title`, a `subtitle`, `keywords`, an
  `icon`, `accessories` (the List document's: text, a date, a tag, each
  with an icon, a colour and a tooltip), `onPress` (the primary action),
  `actions`, `detail` and children.
  - `actions` are `{ "title", "onPress" }`. The second is the secondary
    action, Ctrl+Enter. The Actions panel that lists them all is the
    action model's (#120).
  - `detail` is the pane's content when the item is selected and the
    list `is-showing-detail`. It is built for the selected item alone,
    the render context's `selected` naming it.
  - The children are the author's own row subtree, drawn in the place of
    the standard row while Pane still selects and activates it.

### The Grid

`grid` has the List's behaviour with cells: `grid-item` children in
`list-section`s. Each section has its own `columns` (1 to 8, 5 by
default), `aspectRatio`, `fit` and `inset`.

A cell is an `image`, a `color`, or the author's own subtree in its
children, with a `title` and a `subtitle` under it. The arrows move the
selection by cell and by row, and Ctrl+Up and Ctrl+Down by section, with
no wrap.

### The search-bar dropdown

`list-dropdown` is the search-bar dropdown, beside the search field. It
has `items` (`{ "value", "title" }`), the chosen `value`, a
`placeholder`, and `onChange` (told the chosen item's id). It is the
searchable select (see `select`), keyed by its place in the tree.

### The Detail

`detail` is a scrolled column of what a record or an article is:
Markdown, a metadata panel, a loading state, actions, as children.

A Markdown image is the icon model's: `![alt](source =WxH "title")`. Its
source is a packaged image's path, a web image's URL or inline `data:`,
and its size the `=WxH` its source named (either number may be left
out). A Markdown link is opened by Pane with the system's handler.

### Events and the render context

The event set grows with the views:

- `onSelectionChange`: a selection's move, `{"value": "…"}` naming the
  selected item's key;
- `onLoadMore`: `{}`;
- the List's `onSearchText`: an input event (`{"value": "…"}`) raised on
  the list's key.

The render context grows with them too. It names the selected item's key
(`"selected"`) of the view's List or Grid, which the detail pane's
content is built for.

### Searching a command

The command-search export (`pane:extension/command-search`, described in
the retired command-search document) is retired. A command's search is
the List's search-text event: the view handles it itself (throttled), or
Pane filters locally.

Pane's registered Files command keeps its host-side search of the file
index (#177). No command of a package opens a search field of its own.

## Reading a document

Reading is strict in the document's shape, and lenient exactly where
versioning asks for it, as the list tree's is:

- A property Pane does not know is ignored, at every level.
- A token this version does not know is left out, as a newer minor
  version may add one.
- A node whose `type` Pane does not know, or whose `requires` it does not
  meet, draws the `fallback` the tree gave, else its children if it has
  any, and nothing without either.
- An icon Pane cannot read is not drawn (its image node draws its
  placeholder).
- A document is unreadable when it is not JSON, lacks a field Pane
  needs, or gives one of the wrong type. That is the command's failure,
  which Pane reports as its own reading of what the extension answered
  ("Pane could not read what the extension answered: ..."), never a
  crash. The fields Pane needs are `version` and `root`, a node's `type`,
  a text's `text` or `spans`, a button's or link's `label`, a markdown's
  `markdown`, a rich row's or section header's `title`, an empty state's
  `title`, a segmented's or select's `options`, and a slider's or
  progress's `value`.
- A document over one of the [limits](#limits), or of another major
  version, is the extension's error ("The extension reported an error:
  ..."). The view keeps its last good tree, which an unreadable tree
  leaves too.

### Limits

| What | Limit |
| --- | --- |
| Nodes in one document (counting `fallback` subtrees) | 10,000 |
| Depth of one document | 64 levels |
| JSON in one document | 4 MiB |
| One text node | 64 KiB |
| One markdown source | 1 MiB |
| One inline `data:` image | 1 MiB |
| Raw lengths | clamped to 0 to 4096 px |
| Raw opacity | clamped to 0 to 1 |
| One command's navigation stack | 32 views |

The limits are provisional (#121), as the spec's proposed defaults.

The parse and reconcile cost of a 500- and a 5,000-node tree is measured
and recorded with the change (`crates/pane-core/tests/designed_cost.rs`
prints it), not gated.
