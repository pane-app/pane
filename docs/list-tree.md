# A command's list as a tree

A view command's list reaches Pane through the typed envelope of
[ADR 0036](adr/0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md)
(#135, part of #120). `pane:extension/command` (`wit/extension.wit`) has two
functions for it:

- **`render() -> result<string, string>`** answers the command's screen as a
  JSON tree that names its version.
- **`handle-event(callback, details) -> result<string, string>`** takes the
  id of a callback the tree named, and the event's details as a JSON object
  (`{}` for now), and answers a JSON object. Pane then calls `render` again.

Adding to the tree needs no WIT change: every later list feature (several
actions, icons, accessories) is a field here, and the "Extension UI you can
design" specification (#121) adds layout primitives, components, Detail,
Grid, Form, navigation and the canvas to the same envelope without
redefining it. Forms (#20) and custom views (#21) keep their own functions,
`submit-form` and `open-view`, which take the item's id.

Authors never see the JSON or the callback ids. In Rust (`pane-guest`) a
command implements `pane_guest::Command`, whose `render` returns a
`pane_guest::List` of `pane_guest::Item`s with closures as actions
(`Item::new(id, title).on_action(|| async { Ok("Done".into()) })`). In
JavaScript and TypeScript (`@pane/extension`) the exported `command`'s
`render` resolves with `{ title, items }`, each item with an `onAction`
function; the SDK's adapter (`guests/js/adapt.js`) writes the tree. Both
SDKs name an item's action by the item's id, so the same item has the same
callback in every drawing, and an instance that has not drawn the list yet
draws it before it runs a callback it does not know. An id the list does
not name goes to the command's `run_search_result` (`runSearchResult`), which
is how a search result's id ([command search](command-search.md)) is run.

## Version 1

```json
{
  "version": 1,
  "view": {
    "type": "list",
    "title": "Notes",
    "items": [
      {
        "id": "today",
        "title": "Today",
        "subtitle": "3 notes",
        "actions": [{ "title": "Open", "onAction": "today" }]
      },
      {
        "id": "new",
        "title": "New note",
        "form": {
          "title": "New note",
          "submitLabel": "Save",
          "fields": [
            { "id": "text", "label": "Text", "kind": "text", "placeholder": "Write here" },
            { "id": "where", "label": "Where", "kind": "choice",
              "choices": [{ "id": "inbox", "label": "Inbox" }] }
          ]
        }
      },
      {
        "id": "color",
        "title": "Choose a color",
        "customView": { "title": "Choose a color", "label": "Color", "role": "color-well" },
        "platforms": ["windows", "macos"]
      }
    ]
  }
}
```

- **`version`** (number, at least 1): the version of the component set the
  tree uses. Pane knows version 1.
- **`view`**: the screen. Its **`type`** is `list`, the only view of
  version 1; a list has a **`title`** and **`items`**, in order.
- An item has an **`id`** (Pane keeps the selection on it when the list is
  drawn again, and passes it to `submit-form` and `open-view`), a
  **`title`**, and optionally:
  - **`subtitle`**: a second line;
  - **`actions`**: what choosing the item does. An action has an
    **`onAction`** callback id and an optional **`title`**. For now Pane
    runs the first and ignores the others (#137 brings the rest);
  - **`form`**: choosing the item opens this form instead (fields of kind
    `text`, with an optional `placeholder`, or `choice`, with `choices`);
  - **`customView`**: choosing the item opens this custom view instead
    (ignored when `form` is set);
  - **`platforms`**: the systems (`windows`, `macos`, `linux`) the item's
    action works on; elsewhere the item is listed as unavailable;
  - **`icon`**: drawn before the title ([Icons](#icons), #139);
  - **`titleTooltip`**, **`subtitleTooltip`**: text shown while the pointer
    rests on the title or the subtitle, such as all of a title the row
    cuts off;
  - **`accessories`**: shown on the right of the row, in order
    ([Accessories](#accessories), #139).

  An item with no action, form or custom view does nothing when chosen,
  and says so. Optional fields may be omitted or `null`. A field whose name
  starts with `on` holds a callback id.

`handle-event` answers an object. Version 1 knows one field, **`status`**:
text Pane shows as the action's result in the status line. That text is
transitional; the ticket that brings toasts removes it. An answer without
it shows nothing.

## Reading a tree

- A field Pane does not know is ignored, at every level, and so is a
  platform name it does not know.
- A tree naming a newer version than Pane knows is drawn for what Pane
  understands of it. A view whose `type` Pane cannot show is the view's
  failure.
- A tree or answer that is not JSON, lacks a field Pane needs (`version`,
  `view`, a list's `title` and `items`, an item's `id` and `title`, an
  action's `onAction`) or gives a field of the wrong type is the command's
  failure, shown as "Pane could not read what the extension answered: …" as a
  failed view is: never a crash, so it does not count towards pausing the
  package.
- After each event the command handled (with an answer or an error of its
  own), Pane asks for the tree again and keeps the selection on the same
  item, or at the same place when the item is gone. If that drawing fails,
  the list stays as it was, with the error. A scheduled run (#47) is the
  same: Pane draws the list, runs the scheduled item's action, and draws
  the list again when the command's screen is open.

## Icons

An icon (#139, ADR 0036) is an item's `icon`, an accessory's, an action's
(`icon` beside `onAction`; the Actions panel draws it) and a package's and
command's in `pane.json`. It is one of:

- **text**: a built-in icon's name, or, when it ends in `.png` or `.svg`
  (or names a folder), a packaged image's path: `"star"`,
  `"assets/logo.png"`;
- **`{"builtin": "star", "filled": true}`**: a built-in icon, from the
  whole [reicon](https://reicon.dev) set Pane vendors
  (`crates/pane-core/assets/reicon/`), by its reicon name in kebab case
  (`ArrowUpRight` is `arrow-up-right`; reicon's own spelling works too), in
  its Outline weight or, `filled`, its Filled one;
- **`{"path": "assets/logo.png"}`**: a PNG or SVG file inside the package.
  Beside `logo.png`, `logo@light.png` and `logo@dark.png` are drawn in the
  light and the dark theme when the package has them;
- **`{"light": "assets/sun.svg", "dark": "assets/moon.svg"}`**: one file
  for each theme;
- **`{"url": "data:image/svg+xml,…"}`**: an image by URL. A `data:` URL
  (an SVG or a PNG, percent-encoded or base64) is drawn as it is; this is
  how the SDKs' avatar and progress ring helpers draw. A web image
  (`http(s)`) comes with a later version (#142): until then its fallback
  shows.

Any icon may also have:

- **`tint`**: a colour, drawing a built-in icon in it, or an image as a mask
  in it: a tone (`primary`, `secondary`, `accent`, `red`, `orange`,
  `yellow`, `green`, `blue`, `purple`, `magenta`), a raw colour (`#rgb`,
  `#rgba`, `#rrggbb`, `#rrggbbaa`), or `{"light": …, "dark": …}`. Pane
  corrects the colour's contrast against the panel, so a raw colour cannot
  make an icon unreadable;
- **`mask`**: `circle` or `rounded-rectangle`, the shape an image is
  clipped to;
- **`fallback`**: another icon, drawn when this one cannot be (a name Pane
  does not have, an image the package does not ship or Pane cannot read),
  nested at most 4 deep;
- **`tooltip`**: shown while the pointer rests on the icon. It also makes
  the icon something assistive technology reads; an icon without one is
  decoration, which it skips.

A list's images are named relative to the package folder; they belong under
its `assets` folder, which Pane copies into its managed copy with the
package (with the files the package's and its commands' icons name). Pane
draws extensions' icons bare, without a tile behind them; Pane's own rows
keep their tiles (ADR 0035). A package without an icon of its own shows a
tile with its title's first letter.

In a tree, an icon Pane cannot read (an unknown source, say) is left out,
and so is a tint, mask, fallback or tooltip it cannot read: never the
tree's failure. In `pane.json`, an icon is checked at install: an unknown
built-in name, an image the package does not ship, or a source other than a
built-in icon or a packaged image refuses the package, with the reason.

## Accessories

An item's `accessories` are shown on the right of its row, in order; a row
draws the first three (Pane reports more while the package is developed).
Each is an object with one of:

- **`text`**: text, such as a count;
- **`date`**: a time in milliseconds since the Unix epoch, shown relative to
  now ("now", "5m", "2h", "3d", "2w", "4mo", "1y", "in 5m") and kept
  current while the list is open; its tooltip is the time in full ("Jan 1,
  2026, 14:02") unless it has one of its own;
- **`tag`**: text on a wash of its colour;

and optionally an **`icon`** (drawn before the text; an accessory may be an
icon alone), a **`color`** (a colour or a light and dark pair, as a tint is,
corrected for contrast as text) and a **`tooltip`**. Assistive technology
reads the accessories with their row. An accessory Pane cannot read is left
out.

```json
{
  "id": "ada",
  "title": "Ada Lovelace",
  "icon": { "builtin": "user", "tint": "blue" },
  "titleTooltip": "Ada Lovelace, the first programmer",
  "accessories": [
    { "text": "3", "tooltip": "Unread" },
    { "date": 1767225600000 },
    { "tag": "Open", "color": "green" }
  ]
}
```

In Rust, `pane_guest::{Icon, Accessory, Tone, Color, Tint, Mask}` build them
(`Item::new(..).icon(Icon::builtin("user").tint(Tone::Blue))
.accessory(Accessory::tag("Open").color(Tone::Green))`), and
`pane_guest::icon::{avatar, progress_ring}` build an avatar of initials and
a progress ring. In JavaScript and TypeScript, an item's `icon` and
`accessories` take these shapes as they are (a `date` may be a `Date`), and
`@pane/extension/icons` exports `avatar` and `progressRing`.
