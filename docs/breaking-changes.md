# Breaking changes

The record of the extension API's breaking changes, the committed source
each Pane release's notes carry: one **Breaking changes** section per
extension API version, each change shown with code before and after, for
Rust and for JS/TS, so updating an extension is mechanical
([ADR 0046](adr/0046-every-breaking-change-to-the-extension-api-gets-a-new-version.md)).
Each Pane release whose extension API moved carries the sections of the
versions it moved, as they are written here.

The shape is the rule (ADR 0046): breaking changes are gathered into
deliberate API versions, never shipped in a patch release, so the record
holds one section per version, not per change. A section is written when
its version moves — 0.2's was written with
[#320](https://github.com/pane-app/pane/issues/320)'s bump — and
`cargo xtask ci-lints` refuses a version without its section
([xtask/src/release_notes.rs](../xtask/src/release_notes.rs)): the WIT
guard's sibling ([#319](https://github.com/pane-app/pane/issues/319)
refuses a breaking change without a new version; this refuses a new
version without its announced changes).

A section's heading is a line of its own, exactly `## Extension API X.Y`
with the version's MAJOR.MINOR and nothing more: the level-two headings
of this file are its sections, one per version, and the check looks for
that line alone — a heading of another level, one with more on its line,
or the heading quoted inside a code block is not a section. Under the
heading, one entry per change: what changed and why, then the code before
and after — the WIT first, then the SDKs an extension author calls it
through, the Rust `pane-extension` crate and the `@pane-app/extension`
npm package — each snippet true to the code as it is in this repository.
A section holds only its version's breaking changes; everything else a
release's notes say is that release's own.

## Extension API 0.2

0.2 carries two changes of shape: the one
[#320](https://github.com/pane-app/pane/issues/320) took deliberately
with the bump, and the screen redesign the Extension UI specification
([#121](https://github.com/pane-app/pane/issues/121), #235–#243) landed
on main while 0.1 was current — its shapes reach a release under the
version this bump names, and are recorded here with it. The 0.1 world is
[wit/previous/](../wit/previous); the 0.2 tree is [wit/](../wit) and
[docs/designed-tree.md](designed-tree.md) describes the view a command
answers with now.

`show-hud`'s `style` parameter gets its own `hud-style` type
([#320](https://github.com/pane-app/pane/issues/320)): a HUD was styled
by `toast-style`, whose semantics differ for a HUD. A success toast hides
after 3 seconds, pausing while the pointer is over it; a success HUD
hides after 1.2. An `animated` toast stays until it is updated or hidden;
an `animated` HUD hides like a success does. A toast can carry actions
and replaces the one shown; a HUD has no actions and never queues.
Toasts keep `toast-style`: nothing that shows a toast changes.

**WIT** ([wit/feedback.wit](../wit/feedback.wit); the 0.1 world is
[wit/previous/feedback.wit](../wit/previous/feedback.wit)) — the style of
`show-hud` names the new type:

```wit
// before (0.1)                        // after (0.2)
enum toast-style {                     enum toast-style {
  animated,                              animated,
  success,                               success,
  failure,                               failure,
}                                      }
                                       enum hud-style {
                                         animated,
                                         success,
                                         failure,
                                       }
show-hud: func(title: string,          show-hud: func(title: string,
  style: toast-style);                   style: hud-style);
```

**Rust** (the [pane-extension](../guests/pane-extension/src/feedback.rs)
SDK) — `feedback::show_hud`'s style parameter is a `HudStyle`:

```rust
// before                               // after
use pane_extension::feedback::{         use pane_extension::feedback::{
    ToastStyle, show_hud,                   HudStyle, show_hud,
};                                      };

show_hud("Saved", ToastStyle::Success); show_hud("Saved", HudStyle::Success);
```

**JS/TS** (the [@pane-app/extension](../guests/js/feedback.d.ts) SDK) —
`showHUD`'s style parameter is a `HudStyle`, and `ToastStyle` is a
toast's own:

```ts
// before                                // after
import { showHUD, type ToastStyle }      import { showHUD, type HudStyle }
  from "@pane-app/extension/feedback";     from "@pane-app/extension/feedback";

const style: ToastStyle = "success";     const style: HudStyle = "success";
showHUD("Saved", style);                 showHUD("Saved", style);
```

A JS/TS call that passes the style string itself —
`showHUD("Saved", "success")` — is unchanged, the values being the same
strings in both versions: what changed is which type the SDK's
declarations name for the parameter, so a style kept in a
`ToastStyle`-typed variable where a HUD's style is meant is kept in a
`HudStyle`-typed one.

The typed `form` of the command interface is retired; a form is a
designed view now ([#241](https://github.com/pane-app/pane/issues/241),
[designed-tree.md](designed-tree.md)): the tree's `form` node holds the
author's own layout with the field components anywhere in it —
`TextField`, `PasswordField`, `TextArea`, `Checkbox`, `Toggle`,
`DatePicker`, `DateTimePicker`, `Dropdown`, `TagPicker`, `FilePicker`,
`FolderPicker`, and the static `Description`, `Separator` and `Link` —
and submitting it is an event the view answers, so a validation error is
the field's own `error`, drawn in place beside it. A component that still
answers `submit-form` is refused naming the change, and a `form` view in
a tree is refused wherever the reader sees one: a form is JSON in the
view's tree now, no shape of the interface at all.

**WIT** — the records and the function the command interface no longer
has (the 0.1 world keeps them,
[wit/previous/extension.wit](../wit/previous/extension.wit)):

```wit
// before (0.1) — gone in 0.2: the form is the tree's `form` node
record form { title: string, fields: list<field>, submit-label: string }
record field { id: string, label: string, kind: field-kind }
variant field-kind { text(text-field), choice(list<choice>) }
record field-value { id: string, value: string }
record form-error { field: option<string>, message: string }
submit-form: async func(item-id: string, values: list<field-value>)
  -> result<string, form-error>;
```

**Rust** — `List::form` and the trait's `submit_form` are gone; the view
answers a `form::Form` tree, its submission a
[`Cx::form_listener`](../guests/pane-extension/src/form.rs) the values
are keyed by the fields' ids to:

```rust
// before                                 // after
item("form", "Greet someone",             let submit = cx.form_listener(
    "Fill in a form the guest checks")        |view, values| {
    .form(Form {                              let name = values.text("name")
        title: "Greet someone".into(),            .unwrap_or_default();
        fields: vec![                           // ... check it, answer a
            Field { id: "name".into(),          // tree with the field's
                label: "Name".into(),           // error drawn in place
                kind: FieldKind::Text(      };
                TextField {                form::Form::new()
                    placeholder: None, }) }    .key("form")
        ],                                     .navigation_title("Greet someone")
        submit_label: "Greet".into(),          .submit_title("Greet")
    });                                        .on_submit(submit)
                                               .child(form::TextField::new("name")
async fn submit_form(                              .title("Name")
    item_id: String,                               .error(error))
    values: Vec<FieldValue>,           )   // the view's render answer
) -> Result<String, FormError> { ... }
```

**JS/TS** — an item's `form` property and the exported command's
`submitForm` are gone; the designed view answers a `Form` element whose
fields are elements:

```ts
// before                                 // after
item.form = {                             import { Form, TextInput }
  title: "Greet someone",                   from "@pane-app/extension/view";
  fields: [{
    id: "name",                           jsxs(Form, {
    label: "Name",                          key: "form",
    kind: { tag: "text",                    navigationTitle: "Greet someone",
             val: { placeholder: null } },    submitTitle: "Greet",
  }],                                         onSubmit: (values) => {
  submitLabel: "Greet",                        const name = values.name;
};                                             // ... answer a new tree
                                             },
export const command = {                   children: [
  async submitForm(id, values) {              jsxs(TextInput, {
    /* ... */                                   key: "name",
  },                                            label: "Name",
};                                              error,
                                              }),
                                            ],
                                          });
```

The custom view became the designed tree
([#235](https://github.com/pane-app/pane/issues/235),
[#242](https://github.com/pane-app/pane/issues/242)): a screen is a tree
of layout nodes and UI components Pane renders, and the custom view the
extension drew itself from rectangles and text is one leaf of it, the
`canvas` node — free drawing with the accessibility the tree's own
components have (`role`, `label`, `value`) and the same events (pointer,
keys, increments), its drawing helped by
[wit/view.wit](../wit/view.wit)'s `ask-to-render` and `measure-text`. The
`command` interface's `custom-view` resource and its shapes are gone; the
resource is `view`, whose `render` answers the tree as JSON and whose
`handle-event` answers the tree's callback ids, and `open-view` opens a
command's designed view by its command id and launch record, not an
item's id.

**WIT** — the custom view of 0.1 (a resource whose `render` answered a
`frame` of shapes, `handle-event` a `view-event`) beside what the
interface answers now:

```wit
// before (0.1)                          // after (0.2)
record frame {                           resource view {
  width: u32,                              render: async func(context: string)
  height: u32,                              -> result<rendered, string>;
  shapes: list<shape>,                    handle-event: async func(
  value: string,                            event: ui-event)
}                                            -> result<outcome, string>;
variant shape {                          }
  rect(rect),                            record rendered {
  text(text),                              tree: string,
}                                          refresh-after-ms: option<u32>,
resource custom-view {                   }
  render: async func() -> frame;         open-view: async func(command: string,
  handle-event: async func(                launch: launch-record)
    event: view-event)                    -> result<view, string>;
    -> result<_, string>;
}
open-view: async func(item-id: string)
  -> result<custom-view, string>;
```

**Rust** — the command's `CustomView` type (a `GuestCustomView` whose
`render` answered a `Frame`) is gone; the command's view implements
[`View`](../guests/pane-extension/src/view.rs), its `render` answering a
tree whose canvas is a node:

```rust
// before                                 // after
type CustomView = ColorView;              impl View for ColorView {
                                            fn render(
async fn open_view(                         &mut self,
    item_id: String,                        cx: &mut Cx<Self>,
) -> Result<CustomView, String> {         ) -> impl IntoAnswer {
    Ok(ColorView { ... })                   canvas()
}                                             .key("grid")
                                              .role(CanvasRole::ColorWell)
impl GuestCustomView for ColorView {           .label("Color")
    async fn render(&self) -> Frame {           .value(hex(chosen))
        Frame {                                 .on_pointer_down(
            width, height,                         cx.canvas_listener(|view, _| {
            shapes: vec![                            view.pointed();
                Shape::Rect(Rect {                }))
                    x, y, width,                   .on_pointer_move(
                    height,                         cx.canvas_listener(|view, e| {
                    fill })],                        view.pointed();
            },                                    }))
            value: name,                      }
        }                                   }
    }                                     }
```

**JS/TS** — an item's `customView` property (opening a view that answered
frames of shapes) is gone; the canvas is an element of the designed
view's tree:

```ts
// before                                 // after
item.customView = {                       import { Canvas }
  title: "Choose a color",                  from "@pane-app/extension/view";
  label: "Color",
  role: "color-well",                     jsxs(Canvas, {
};                                          key: "grid",
                                            role: "color-well",
export const command = {                    label: "Color",
  async openView(id) {                      value: chosen,
    return new ColorView();                 onPointerDown: (event) => {
  },                                          pointed(event);
};                                          },
                                            onPointerMove: (event) => {
class ColorView {                             pointed(event);
  async render() {                         },
    return {                              });
      width, height,
      shapes: [
        { tag: "rect",
          val: { x, y, width, height, fill } },
      ],
      value: name,
    };
  }
  async handleEvent(event) { /* ... */ }
}
```

The `command-search` export is gone
([#240](https://github.com/pane-app/pane/issues/240)): a command's own
search field is the designed tree's `List` — Pane owns the field, filters
the items the tree names with the root-search matcher, and a view that
searches as the user types handles the text itself, its List's search
events asking it — so the interface a command exported beside `command`
to be asked with the field's text is no longer called, and
`wit/search.wit` is deleted. A command that searched as the user typed
moves its search into its designed view: the package search sample
(`guests/sample-search-js`) is the example, its service asked from the
view's own search handler now.

**WIT** — the interface and the provider world a command with
`"search": true` exported beside `command` (the 0.1 world keeps them,
[wit/previous/search.wit](../wit/previous/search.wit)); 0.2 has none of
it:

```wit
// before (0.1) — gone in 0.2: the List's own search field is the tree's
interface command-search {
  search: async func(command: string, query: string)
    -> result<list<search-result>, string>;
}
world command-search-provider {
  export command-search;
}
```

**Rust** — the `search` module's export beside `export!`, and the `Guest`
implementation, are gone; the view's List hears the text itself:

```rust
// before                                 // after
pane_extension::export!(Packages);        pane_extension::export!(Packages);
pane_extension::search::export!(Packages);
                                          // The search view: the List
impl pane_extension::search::Guest        // that handles its search
    for Packages {                        // itself (#240).
{                                         List::new("Package search")
    async fn search(                          .on_search_text(
        command: String,                          cx.value_listener(
        query: String,                                |this, text| {
    ) -> Result<Vec<SearchResult>, String>              this.search(text);
    ) { ... }                                       }))
}                                             .items(...)
```

**JS/TS** — the exported `commandSearch` object and `"search": true` in
`pane.json` are gone; the designed view's List hears the text:

```ts
// before                                 // after
export const commandSearch = {            import { List }
  async search(command, query) {            from "@pane-app/extension/view";
    const found = await fetchJson(
      `/search?q=${encodeURIComponent(    jsxs(List, {
        query)}`);                           key: "results",
    return found.results.map(                searchPlaceholder: "Search",
      (pkg) => ({                            onSearchText: (text) =>
        id: `package:${pkg.name}`,             searchView(text),
        title: pkg.name,                   }),
        subtitle: pkg.summary,           });
      }));                               // pane.json: no "search": true
  },
};
// pane.json: "search": true
```

Extensions built for 0.1 keep running: this Pane runs them through the
adapter until the next Pane release — the window ADR 0046 gives their
authors — and their page names the version they were built for. The
window carries what 0.2 kept: a 0.1 command's host imports are served,
converted where the version retyped one (`show-hud`'s style is), and its
list screen and its items' events run through the previous world's
bindings. It does not carry what 0.2 replaced: a 0.1 command's typed
forms, custom views and `command-search` export are of the shapes the
tree and the launcher no longer take, and are refused of a 0.1 build as
of any other — `open-view` says so, naming the change — so an extension
whose screen is a form, a custom view or a search-as-you-type field
updates to 0.2's tree with this release, as every extension does.
Official extensions need 0.2 releases before the Pane release that pins
them.
