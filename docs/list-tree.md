# A command's list as a tree

A view command's list reaches Pane through the typed envelope of
[ADR 0036](adr/0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md)
(#135, part of #120). `pane:extension/command` (`wit/extension.wit`) has two
functions for it:

- **`render(launch) -> result<string, string>`** answers the command's
  screen as a JSON tree that names its version. `launch` is the command's
  launch record (`wit/commands.wit`, #138): how its screen was opened, the
  same each time Pane asks for the screen again while it is open.
- **`handle-event(callback, details) -> result<string, string>`** takes the
  id of a callback the tree named, and the event's details as a JSON object
  (`{}` for now), and answers a JSON object. Pane then calls `render` again.

A no-view command (`"mode": "no-view"`, #138) has no tree: Pane calls
**`run(command, launch) -> result<string, string>`** each time it is
launched, and it answers the same JSON object as `handle-event`.

Adding to the tree needs no WIT change: every later list feature (several
actions, icons, accessories) is a field here, and the "Extension UI you can
design" specification (#121) adds layout primitives, components, Detail,
Grid, Form, navigation and the canvas to the same envelope without
redefining it. Forms (#20) and custom views (#21) keep their own functions,
`submit-form` and `open-view`, which take the item's id.

Authors never see the JSON or the callback ids. In Rust (`pane-guest`) a
command implements `pane_guest::Command`, whose `render` returns a
`pane_guest::List` of `pane_guest::Item`s with closures as actions
(`Item::new(id, title).action(Action::new("Open", || async { Ok("Done".into()) }))`,
or an untitled `.on_action(..)`). In JavaScript and TypeScript
(`@pane/extension`) the exported `command`'s `render` resolves with
`{ title, items }`, each item with `actions` (objects with a `title` and an
`onAction` function) or an untitled `onAction` function; the SDK's adapter
(`guests/js/adapt.js`) writes the tree. Both SDKs name an item's first
action by the item's id and its later ones by the id and their place
(`<id>#1`, `<id>#2`, ...), so the same action has the same callback in every
drawing, and an instance that has not drawn the list yet draws it before it
runs a callback it does not know. An id the list does
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
        "actions": [
          { "title": "Open", "onAction": "today" },
          { "title": "Copy", "onAction": "today#1" },
          { "title": "Copy Link", "onAction": "today#2", "section": "Share",
            "shortcut": { "modifiers": ["ctrl", "shift"], "key": "c" } },
          { "title": "Reveal", "onAction": "today#3", "section": "Share",
            "shortcut": { "windows": { "modifiers": ["ctrl", "shift"], "key": "e" },
                          "macos": { "modifiers": ["cmd", "shift"], "key": "r" } } },
          { "title": "Delete", "onAction": "today#4", "section": "Danger",
            "style": "destructive", "shortcut": { "modifiers": ["ctrl"], "key": "x" } }
        ]
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
  - **`actions`**: what the item offers, in order (#137). The first is its
    **primary action**: Enter and the footer's button run it, and the
    footer names it. The second is its **secondary action** (Ctrl+Enter),
    and Ctrl+Shift+Enter runs the third; a missing one does nothing. Ctrl+K
    opens the Actions panel, which lists them all. An action has an
    **`onAction`** callback id and optionally:
    - **`title`**: what the footer and the panel call it ("Run item" when
      it has none);
    - **`section`**: the title of its section in the panel; consecutive
      actions with the same section are one section, and filtering the
      panel lists what matches as one list;
    - **`style`**: `default` or `destructive` (drawn in the destructive
      color); a style Pane does not know is the default;
    - **`shortcut`**: the keys that run it while the list has focus,
      without the panel: `{"modifiers": [...], "key": "..."}` for every
      system, or such an object per system under `windows`, `macos` and
      `linux` (a system left out binds none). Modifiers are `ctrl`, `alt`,
      `shift` and `cmd` (Command on macOS, the Windows key, Super); keys
      are named as Pane's key bindings name them (`c`, `,`, `enter`,
      `delete`, `f5`). Modifiers match exactly. Pane does not bind a
      shortcut that is one of its own keys (the Keyboard page's bindings as
      the user has them, Escape, Ctrl+K, Up and Down, Tab, Enter and the
      action chords, Ctrl and a digit, the pin keys), one with no Ctrl, Alt
      or Cmd (it would take a key a search field types or moves with; the
      function keys are allowed alone), one it cannot read, or one an
      earlier action of the item already has: the action then stays in the
      panel without it, and while its package is being developed the
      status line says which shortcuts were not bound and why;
  - **`form`**: choosing the item opens this form instead (fields of kind
    `text`, with an optional `placeholder`, or `choice`, with `choices`);
  - **`customView`**: choosing the item opens this custom view instead
    (ignored when `form` is set);
  - **`platforms`**: the systems (`windows`, `macos`, `linux`) the item's
    action works on; elsewhere the item is listed as unavailable.

  An item with no action, form or custom view cannot be activated: the
  footer's button says "No actions", and Enter says so in the status line.
  A held key's repeats and a double click's second click never run an
  action again. Optional fields may be omitted or `null`. A field whose
  name starts with `on` holds a callback id.

`handle-event` and `run` answer an object, `{}` for now. Pane shows nothing
of it (#141): an action, a no-view run or a search result says what happened
through the host functions every command has (`wit/feedback.wit`), with a
toast in the footer or a HUD, or by closing the window. The **`status`** text
the first version of the tree carried is ignored. An error a command answers
with is shown as a failure toast with a "Copy Error" action.

A toast's action is a callback id too: choosing it calls the command's
`handle-event` with it, as an item's action does. The SDKs name a toast's
actions `toast:<n>:primary` and `toast:<n>:secondary`, `<n>` counting the
toasts the instance showed, and run the newest toast's.

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
