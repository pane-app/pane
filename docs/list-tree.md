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
    action works on; elsewhere the item is listed as unavailable.

  An item with no action, form or custom view does nothing when chosen,
  and says so. Optional fields may be omitted or `null`. A field whose name
  starts with `on` holds a callback id.

`handle-event` and `run` answer an object. Version 1 knows one field, **`status`**:
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
