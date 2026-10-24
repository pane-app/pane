# A command's list as a tree

A view command shows its screen as a list. This page describes that list
exactly: the JSON tree a command answers with, the calls Pane makes, what
an item can carry, and how the SDKs write it all. Authors never see the
JSON or the callback ids, so this is the reference for anyone who needs
the shape underneath the SDKs.

## How Pane calls the command

The list reaches Pane through the typed envelope of
[ADR 0036](https://github.com/pane-app/pane/blob/2a4f9c43c990656325297a5980f34fa4bddba76e/docs/adr/0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md)
(#135, part of #120). `pane:extension/command` (`wit/extension.wit`) has
two functions for it:

- `render(launch) -> result<string, string>` answers the command's screen
  as a JSON tree that names its version. `launch` is the command's launch
  record (`wit/commands.wit`, #138): how its screen was opened. It is the
  same each time Pane asks for the screen again while it is open.
- `handle-event(callback, details) -> result<string, string>` takes the id
  of a callback the tree named and the event's details as a JSON object
  (`{}` for now), and answers a JSON object. Pane then calls `render`
  again.

A no-view command (`"mode": "no-view"`, #138) has no tree. Each time it is
launched, Pane calls `run(command, launch) -> result<string, string>`,
which answers the same JSON object as `handle-event`.

### Growing the tree

Adding to the tree needs no WIT change. Every later list feature (several
actions, icons, accessories) is a field here. The "Extension UI you can
design" specification (#121) adds layout primitives, components, Detail,
Grid, Form, navigation and the canvas to the same envelope without
redefining it.

Two older parts of this interface now live in the designed tree:

- The form is a designed view now (#241): its fields are the tree's field
  components and its submission is an event. `submit-form` is gone from
  the interface, and a component built for the older shape is refused
  with a message naming the change.
- The custom view of #21 became the canvas of the designed tree (#242,
  [designed-tree.md](designed-tree.md)).

## Writing a list with the SDKs

The SDKs write the tree and name the callbacks for the author.

In Rust (`pane-extension`), a command implements `pane_extension::Command`.
Its `render` returns a `pane_extension::List` of `pane_extension::Item`s,
with closures as actions, or an untitled `.on_action(..)`:

```rust
Item::new(id, title).action(Action::new("Open", || async { Ok("Done".into()) }))
```

In JavaScript and TypeScript (`@pane-app/extension`), the exported
`command`'s `render` resolves with `{ title, items }`. Each item has
`actions` (objects with a `title` and an `onAction` function) or an
untitled `onAction` function. The SDK's adapter (`guests/js/adapt.js`)
writes the tree.

A form is a designed view now (#241, `docs/designed-tree.md`): the
`"mode": "designed"` command of the same package holds the field
components, and its submission is an event. `List::form` is gone.

`pane_extension::commands::current().command` says which of a component's
commands is opened (the launch record's `command`), so one component can
draw several view commands' screens.

### Callback ids

Both SDKs name an item's first action by the item's id, and its later
ones by the id and their place (`<id>#1`, `<id>#2`, ...). The same action
therefore has the same callback in every drawing. An instance that has
not drawn the list yet draws it before it runs a callback it does not
know.

An id the list does not name goes to the command's `run_search_result`
(`runSearchResult`). A designed List that handles its own search runs it
for its items' presses (#240). The `command-search` export it once
answered is retired; see [designed-tree.md](designed-tree.md), the
"standard views" section.

### Submenus in the SDKs

An action may open a submenu instead (#140):

- In Rust, `Action::submenu("Open With…", Submenu::new("Open With").entries([..]))`,
  or `Submenu::lazy("Move to List", || async { Ok(vec![..]) })` for
  entries given when it opens.
- In JavaScript and TypeScript, an action with
  `submenu: { title, entries }`, or `submenu: { title, onOpen }` with
  `onOpen` resolving with the entries.

Both SDKs name a submenu's entries after the action that opens it and
their place (`<callback>/0`, `<callback>/1`, ...). A lazy submenu's
`onOpen` is named as the action itself would be.

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
        "actions": [
          { "title": "Open", "onAction": "today" },
          { "title": "Copy", "onAction": "today#1" },
          { "title": "Copy Link", "onAction": "today#2", "section": "Share",
            "shortcut": { "modifiers": ["ctrl", "shift"], "key": "c" } },
          { "title": "Reveal", "onAction": "today#3", "section": "Share",
            "shortcut": { "windows": { "modifiers": ["ctrl", "shift"], "key": "e" },
                          "macos": { "modifiers": ["cmd", "shift"], "key": "r" } } },
          { "title": "Delete", "onAction": "today#4", "section": "Danger",
            "style": "destructive", "shortcut": { "modifiers": ["ctrl"], "key": "x" } },
          { "title": "Open With…", "submenu": { "title": "Open With", "entries": [
              { "title": "Notepad", "onAction": "today#5/0", "section": "Editors" },
              { "title": "Browser", "onAction": "today#5/1",
                "shortcut": { "modifiers": ["ctrl", "shift"], "key": "b" } }
          ] } },
          { "title": "Move to List…", "section": "Organize",
            "submenu": { "title": "Move to List", "onOpen": "today#6" } }
        ]
      },
      {
        "id": "new",
        "title": "New note",
        "subtitle": "Type a note in a form the command checks"
      },
      {
        "id": "color",
        "title": "Choose a color",
        "platforms": ["windows", "macos"]
      }
    ]
  }
}
```

### The tree and its view

- `version` (number, at least 1): the version of the component set the
  tree uses. Pane knows version 1.
- `view`: the screen. Its `type` is `list`. A list has a `title` and
  `items`, in order.

A `form` view is refused, with a message naming the change. That was the
typed form of the tree's first version, submitted through `submit-form`.
A form is a designed view now (#241, `docs/designed-tree.md`): its fields
are the tree's field components and its submission is an event.

### Items

An item has an `id` and a `title`. Pane keeps the selection on the `id`
when the list is drawn again, and passes it to `open-view`. Optionally,
an item also has:

- `subtitle`: a second line.
- `actions`: what the item offers, in order (#137). See
  [Actions](#actions).
- `platforms`: the systems (`windows`, `macos`, `linux`) the item's action
  works on. Elsewhere the item is listed as unavailable.
- `icon`: drawn before the title ([Icons](#icons), #139).
- `titleTooltip`, `subtitleTooltip`: text shown while the pointer rests on
  the title or the subtitle, such as all of a title the row cuts off.
- `accessories`: shown on the right of the row, in order
  ([Accessories](#accessories), #139).

An item with no action cannot be activated: the footer's button says
"No actions", and Enter says so in the status line. A held key's repeats
and a double click's second click never run an action again.

Optional fields may be omitted or `null`. A field whose name starts with
`on` holds a callback id.

### Actions

An item's actions come in order (#137), and the keys reach them by place:

| Key | What it does |
| --- | --- |
| Enter, or the footer's button | Runs the first action, the primary action. The footer names it. |
| Ctrl+Enter | Runs the second, the secondary action. |
| Ctrl+Shift+Enter | Runs the third. |
| Ctrl+K | Opens the Actions panel, which lists them all. |

A key whose action is missing does nothing.

An action has either an `onAction` callback id or a `submenu`
([Submenus](#submenus)), and optionally:

- `title`: what the footer and the panel call it ("Run item" when it has
  none).
- `section`: the title of its section in the panel. Consecutive actions
  with the same section are one section, and filtering the panel lists
  what matches as one list.
- `style`: `default` or `destructive` (drawn in the destructive color). A
  style Pane does not know is the default.
- `shortcut`: the keys that run it while the list has focus, without the
  panel ([Shortcuts](#shortcuts)).

#### Shortcuts

A shortcut is `{"modifiers": [...], "key": "..."}` for every system, or
such an object per system under `windows`, `macos` and `linux`. A system
left out binds none.

Modifiers are `ctrl`, `alt`, `shift` and `cmd` (Command on macOS, the
Windows key, Super). Keys are named as Pane's key bindings name them
(`c`, `,`, `enter`, `delete`, `f5`). Modifiers match exactly.

Pane does not bind a shortcut that:

- is one of its own keys: the Keyboard page's bindings as the user has
  them, Escape, Ctrl+K, Up and Down, Tab, Enter and the action chords,
  Ctrl and a digit, the pin keys;
- has no Ctrl, Alt or Cmd, because it would take a key a search field
  types or moves with (the function keys are allowed alone);
- it cannot read;
- an earlier action of the item already has.

The action then stays in the panel without it. While its package is
being developed, the status line says which shortcuts were not bound and
why.

#### Submenus

A `submenu` (#140), given instead of `onAction`, holds further choices
the Actions panel opens in place when the action is chosen ("Open
With…"). It is drawn with a chevron. It has a `title`, which the panel's
header shows while it is open, and either:

- `entries`: actions given with the tree, each with its own `onAction` or
  `submenu`, `title`, `section`, `style` and `shortcut`; or
- `onOpen`: a callback id Pane hands to `handle-event` each time the
  submenu opens, once per opening. The answer's `entries` are the
  submenu's.

Until an `onOpen` submenu's answer arrives, it shows a loading entry. An
error (the command's own, a crash, an answer without `entries` or one
Pane cannot read) shows as the submenu's one entry, and the panel stays
open. An answer that arrives after the submenu closed (Escape, the panel
closed) or after another item was selected is discarded. Asking for
entries draws nothing again.

In the panel:

- Typing filters the level shown, flattening its sections.
- Escape steps back one level, and from the item's actions closes the
  panel.
- An entry's shortcut is bound by the rules above within its own submenu,
  and works only while that submenu is shown.
- Choosing an entry calls the command back with its `onAction` and draws
  the list again, as any action does.
- Enter, an action chord or the shortcut of an item's action that opens a
  submenu open the panel at it.

### What a callback answers

`handle-event` and `run` answer an object. Pane shows nothing of it as
text (#141). An action, a no-view run or a search result says what
happened through the host functions every command has
(`wit/feedback.wit`): a toast in the footer, a HUD, or closing the window.
The `status` text the first version of the tree carried is ignored. An
error a command answers with is shown as a failure toast with a "Copy
Error" action.

The one field Pane reads is `entries`: the entries of a submenu whose
`onOpen` Pane handed over, written as a submenu's `entries` are:

```json
{"entries": [{"title": "Inbox", "onAction": "today#6/0"}]}
```

A toast's action is a callback id too. Choosing it calls the command's
`handle-event` with it, as an item's action does. The SDKs name a toast's
actions `toast:<n>:primary` and `toast:<n>:secondary`, where `<n>` counts
the toasts the instance showed, and run the newest toast's.

## Reading a tree

- A field Pane does not know is ignored, at every level, and so is a
  platform name it does not know.
- A tree naming a newer version than Pane knows is drawn for what Pane
  understands of it. A view whose `type` Pane cannot show is the view's
  failure.
- Some trees and answers are the command's failure: one that is not
  JSON, lacks a field Pane needs (`version`, `view`, a list's `title` and
  `items`, an item's `id` and `title`, an action's `onAction` or
  `submenu`, a submenu's `title` and its `entries` or `onOpen`), gives
  both of such a pair, or gives a field of the wrong type. It is shown as
  "Pane could not read what the extension answered: …", as a failed view
  is. It is never a crash, so it does not count towards pausing the
  package.
- After each event the command handled (with an answer or an error of its
  own), Pane asks for the tree again and keeps the selection on the same
  item, or at the same place when the item is gone. If that drawing
  fails, the list stays as it was, with the error.
- A scheduled run (#47) is the same: Pane draws the list, runs the
  scheduled item's action, and draws the list again when the command's
  screen is open.

## Icons

An icon (#139, ADR 0036) can be:

- an item's `icon`;
- an accessory's;
- an action's (`icon` beside `onAction`; the Actions panel draws it);
- a package's and a command's in `pane.json`.

It is one of these:

- Text: a built-in icon's name, or, when it ends in `.png` or `.svg` (or
  names a folder), a packaged image's path: `"star"`,
  `"assets/logo.png"`.
- `{"builtin": "star", "filled": true}`: a built-in icon from the whole
  [reicon](https://reicon.dev) set Pane vendors
  (`crates/pane-core/assets/reicon/`). It is named by its reicon name in
  kebab case (`ArrowUpRight` is `arrow-up-right`; reicon's own spelling
  works too), and drawn in its Outline weight, or its Filled one with
  `filled`.
- `{"path": "assets/logo.png"}`: a PNG or SVG file inside the package.
  When the package has `logo@light.png` and `logo@dark.png` beside
  `logo.png`, they are drawn in the light and the dark theme.
- `{"light": "assets/sun.svg", "dark": "assets/moon.svg"}`: one file for
  each theme.
- `{"url": "data:image/svg+xml,…"}`: an image by URL. A `data:` URL (an
  SVG or a PNG, percent-encoded or base64) is drawn as it is. This is how
  the SDKs' avatar and progress ring helpers draw.
- `{"url": "https://example.com/logo.png"}`: a web image (#142); see
  [Web images](#web-images).
- `{"file": "/Users/me/report.pdf"}`: a system icon (#142); see
  [System icons](#system-icons).
- `{"application": "<icon>"}`: an installed application's own icon
  (#172); see [Application icons](#application-icons).

### Web images

Pane downloads a web image itself, through its HTTP client and within the
ceilings of the extension's own web requests (ADR 0018):

- 10 s to connect;
- 20 s for the response's head;
- 10 s between two pieces of its body;
- 30 s in all;
- a 4 MiB body.

Pane keeps the image as the package's extension cache, so it is not
downloaded again after a restart, and "Clear cache" removes it.

A list never waits for a web image. A row's image is downloaded once its
row comes into view (#165). Its fallback (or, without one, a neutral
image glyph) shows until the image arrives. The fallback stays if the
download fails, is over the limits or is not an image (PNG, JPEG, GIF,
WebP, BMP, ICO or SVG). Rows naming the same URL share one download.

The SDKs' favicon helper names a site's `/favicon.ico` this way, with a
globe as its fallback.

### System icons

A system icon is the icon the system shows for the file, folder or
application at that path: a document's kind's, or an application's own,
drawn bare. The path is absolute, or from `~/`. On Windows a packaged
application can also be named by `shell:AppsFolder\<id>`.

Pane extracts the icon in the background and keeps it in its own folder.
On Windows, a file of a type whose files all show one icon is drawn with
the icon extracted for another file of its type that session. Search Files
lists many such files at once. Programs, shortcuts, internet shortcuts,
icon files, management consoles and folders each keep their own icon.
A path that does not exist shows the fallback. The SDKs' file icon helper
gives it a document as its fallback.

### Application icons

An application icon is named by the `icon` reference
`pane:extension/applications` returns with the application
([applications](applications.md#icons)), and drawn bare. The host
extracts it in the background and keeps it in its cache of the
applications' icons, with a packaged app's light and dark variants. Its
fallback (or a neutral placeholder) shows until it is there, and if it
cannot be extracted.

A package's or a command's icon in `pane.json` cannot be one.

### Tint, mask, fallback and tooltip

Any icon may also have:

- `tint`: a colour, drawing a built-in icon in it, or an image as a mask
  in it. It is a tone (`primary`, `secondary`, `accent`, `red`, `orange`,
  `yellow`, `green`, `blue`, `purple`, `magenta`), a raw colour (`#rgb`,
  `#rgba`, `#rrggbb`, `#rrggbbaa`), or `{"light": …, "dark": …}`. Pane
  corrects the colour's contrast against the panel, so a raw colour
  cannot make an icon unreadable.
- `mask`: `circle` or `rounded-rectangle`, the shape an image is clipped
  to.
- `fallback`: another icon, drawn when this one cannot be (a name Pane
  does not have, an image the package does not ship or Pane cannot read,
  a web image or system icon while it loads or if it fails). Fallbacks
  nest at most 4 deep.
- `tooltip`: shown while the pointer rests on the icon. It also makes the
  icon something assistive technology reads; an icon without one is
  decoration, which it skips.

### Packaged images and tiles

A list's images are named relative to the package folder. They belong
under its `assets` folder, which Pane copies into its managed copy with
the package (with the files the package's and its commands' icons name).

Pane draws extensions' icons bare, without a tile behind them. Pane's own
rows keep their tiles (ADR 0035). An extension that wants a tile draws it
into its image, as Pane's default extensions do (#163). Each ships an SVG
tile of the row tile's size, 28 by 28 with a corner radius of 7: a
coloured rounded square with a white symbol, so it lines up with Pane's
own tiles. One that ships a bare symbol is drawn bare.

A package without an icon of its own shows a tile with its title's first
letter.

### Icons Pane cannot read

In a tree, an icon Pane cannot read (an unknown source, say) is left out,
and so is a tint, mask, fallback or tooltip it cannot read. It is never
the tree's failure.

In `pane.json`, an icon is checked at install. An unknown built-in name,
an image the package does not ship, or a source other than a built-in
icon or a packaged image (a URL, a system icon) refuses the package, with
the reason.

## Accessories

An item's `accessories` are shown on the right of its row, in order. A
row draws the first three; Pane reports more while the package is
developed. Each accessory is an object with one of:

- `text`: text, such as a count;
- `date`: a time in milliseconds since the Unix epoch, shown relative to
  now ("now", "5m", "2h", "3d", "2w", "4mo", "1y", "in 5m") and kept
  current while the list is open. Its tooltip is the time in full ("Jan 1,
  2026, 14:02") unless it has one of its own;
- `tag`: text on a wash of its colour.

It may also have:

- an `icon`, drawn before the text (an accessory may be an icon alone);
- a `color`: a colour or a light and dark pair, as a tint is, corrected
  for contrast as text;
- a `tooltip`.

Assistive technology reads the accessories with their row. An accessory
Pane cannot read is left out.

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

### Icons and accessories in the SDKs

In Rust, `pane_extension::{Icon, Accessory, Tone, Color, Tint, Mask}`
build them:

```rust
Item::new(..).icon(Icon::builtin("user").tint(Tone::Blue))
.accessory(Accessory::tag("Open").color(Tone::Green))
```

`pane_extension::icon::{avatar, progress_ring}` build an avatar of
initials and a progress ring.

In JavaScript and TypeScript, an item's `icon` and `accessories` take
these shapes as they are (a `date` may be a `Date`), and
`@pane-app/extension/icons` exports `avatar` and `progressRing`.
