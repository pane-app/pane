# Extension guests

Extensions are WebAssembly components implementing the `pane:extension`
contract in [`wit/extension.wit`](../wit/extension.wit). Pane registers only
WASI 0.3 interfaces; a component that imports WASI 0.2 (for example through
Rust's standard library on `wasm32-wasip2`) is rejected with an explanation.

- `pane-guest`: Rust bindings for the contract. `no_std`, so only WASI 0.3 is
  imported; it supplies the allocator, a trapping panic handler,
  `cabi_realloc` and `memcmp`/`bcmp` (which string comparisons need).
- `sample-rust`, `sample-js`, `sample-ts`: the same sample command in Rust,
  JavaScript and TypeScript. All three show the same items, the same form and
  the same color picker, and give the same answers and errors; the contract
  tests in `crates/pane-core/tests/samples.rs` and `crates/pane/tests/window.rs`
  hold each of them to that.
- `sample-settings`, `sample-settings-js`, `sample-settings-ts`: the same
  command in Rust, JavaScript and TypeScript, which keeps a chosen greeting
  style in Pane's settings ([Keeping settings](#keeping-settings)); the
  fixtures for disabling and re-enabling a package, held alike by
  `crates/pane-core/tests/disable.rs`.
- `js`: `@pane/extension`, TypeScript declarations for the contract
  (`pane.d.ts`) and the WIT world JS/TS commands are built against.
- `prebuilt`: the JS and TS sample components (both samples in each
  language), committed so that tests and
  `cargo run -p pane` need no JavaScript toolchain, with `manifest.json`
  recording their hashes and build inputs.
- `packages`: the samples' package manifests (`pane.json`). `cargo xtask
  guests` puts each one with its built component in
  `target/guests/packages/<name>/`, a ready-to-install package.
- `fixtures/faulty`: test fixture whose actions, form and custom view return
  an error or trap.
- `fixtures/mixed-p2`: negative control that imports WASI 0.2 and must be rejected.
- `fixtures/old-api`: negative control built against extension API 0.1 as it
  was before `item` gained `platforms` and custom views, with its own copy of
  that WIT; Pane's type check refuses it at install and when it loads.
- `fixtures/mismatched-api`: negative control whose exports all have the
  names Pane looks for while `item` lacks one field, so only the type check
  can refuse it.

## Writing a Rust command

The [sample](sample-rust/src/lib.rs) is the complete example. A command is a
`cdylib` crate depending on `pane-guest` that implements four async
functions and names its custom view type (see [Forms](#forms) and
[Custom views](#custom-views) for the last two):

```rust
#![no_std]

use pane_guest::alloc::{string::String, vec, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View};

struct Hello;
pane_guest::export!(Hello);

impl Guest for Hello {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let item = Item {
            id: "hi".into(),
            title: "Say hi".into(),
            subtitle: None,
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View { title: "Hello".into(), items: vec![item] })
    }

    async fn run_action(_item_id: String) -> Result<String, String> {
        Ok("hi!".into())
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError { field: None, message: "this command has no forms".into() })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("this command has no custom views".into())
    }
}
```

Returning `Err` shows the message as an error; a panic traps the guest, which
Pane reports and recovers from by starting a fresh instance on the next call.
WASI 0.3 interfaces are available through the
[`wasip3`](https://docs.rs/wasip3/0.9.0/wasip3/) crate with
`default-features = false`; the sample awaits `wasi:clocks` this way.

Build with the pinned toolchain (`wasm32-wasip2` is the compiler target name;
the emitted component imports only WASI 0.3):

```sh
cargo xtask guests
# or, for the guests workspace only:
cd guests && cargo build --release --target wasm32-wasip2
```

To try a rebuilt sample in the launcher, `cargo run -p pane` from the root; set
`PANE_EXTENSIONS_DIR` to a directory containing `sample_rust.wasm`,
`sample_js.wasm` and `sample_ts.wasm` to use different builds. To run your
own command, make it a package and install it; see
[Packaging and installing a local extension](#packaging-and-installing-a-local-extension).

Toolchain used: Rust 1.98.1, `wit-bindgen` 0.62.0, `wasip3` 0.9.0+wasi-0.3.0;
host Wasmtime and wasmtime-wasi 49.0.1.

### Keeping settings

A command of an installed package can keep string values between runs with
the `pane:extension/settings` interface in
[`wit/settings.wit`](../wit/settings.wit). The settings sample, in
[Rust](sample-settings/src/lib.rs), [JavaScript](sample-settings-js/src/index.js)
and [TypeScript](sample-settings-ts/src/index.ts), saves the greeting style
the user picks. In Rust it is `pane_guest::settings`:

```rust
use pane_guest::settings;

settings::set("greeting-style", "formal")?;          // Result<(), String>
let style: Option<String> = settings::get("greeting-style")?;
```

In JavaScript and TypeScript it is a module (typed in
[`js/settings.d.ts`](js/settings.d.ts)); an error is thrown as an `Error`
whose message is the reason, so rethrowing it shows the reason to the user:

```ts
import { get, set } from "pane:extension/settings@0.1.0";

set("greeting-style", "formal");
const style: string | null = get("greeting-style");
```

- Values belong to the installed package's source identity, not its title
  or managed copy: two installed copies of the same package have separate
  settings, and an update keeps them.
- They are kept while the package is disabled and while Pane is not running,
  and the command sees them again when the package is enabled. While it is
  disabled nothing of the package runs, and once it is disabled a call still
  finishing from before cannot save: `set` fails instead of writing.
- Pane keeps them in `extensions/settings.json` in its data folder. If that
  file cannot be read, `get` and `set` return the reason and Pane does not
  overwrite the file. Each `set` replaces the file whole (a crash leaves the
  old or the new file), but it is not locked: two Pane processes using the
  same data folder can lose each other's last write. Keeping to one running
  Pane is a later concern.
- A command built into Pane rather than installed from a package has no
  settings: `get` and `set` return an error.
- A component that does not import `settings` is unaffected; it is built for
  the `extension` world as before. `extension-with-settings` adds the import
  within extension API 0.1, so a component that uses settings needs a Pane
  with this change. JavaScript and TypeScript commands are built against a
  world that includes it, so the prebuilt JS/TS components list the import
  whether or not they use it.

## Writing a JavaScript or TypeScript command

The [JavaScript](sample-js/src/index.js) and
[TypeScript](sample-ts/src/index.ts) samples are complete examples. A command
is an npm package whose `main` module exports `command` with four async
functions (see [Forms](#forms) and [Custom views](#custom-views) for the last
two). Pane's types come from
`@pane/extension` (a `file:../js` development dependency); they describe plain
values, not engine objects:

```ts
import type { Command } from "@pane/extension";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

export const command: Command = {
  async getView() {
    return { title: "Hello", items: [{ id: "hi", title: "Say hi" }] };
  },
  async runAction(itemId) {
    if (itemId !== "hi") throw new Error(`unknown item: ${itemId}`);
    await waitFor(10_000_000); // 10 ms; the command suspends meanwhile
    return "hi!";
  },
  async submitForm() {
    throw { message: "this command has no forms" };
  },
  async openView() {
    throw new Error("this command has no custom views");
  },
};
```

Throwing (a rejected promise) shows the error's message, or a thrown string,
as an error. Returning a value of the wrong type, such as `undefined` from
`runAction`, traps the guest, which Pane reports and recovers from as for
Rust. npm dependencies are bundled into the component; the samples use
[Zod](https://zod.dev) 4.6.5 (`zod/mini`) and show its validation failure as a
normal error. Only ECMAScript built-ins are available, not Node.js or browser
APIs; WASI 0.3 imports declared by [the world](js/wit/world.wit) (currently
`wasi:clocks/monotonic-clock`) are imported by name and typed in
[`js/wasi.d.ts`](js/wasi.d.ts). `Math.random`, `Date.now()` and
`performance.now()` are fresh in each instance.

**Snapshot caveat.** A component is built by running the module once and
snapshotting the engine, so module top-level code runs at build time, on the
build machine, and every instance starts from its result. Keep top-level code
to pure setup such as schemas and constants: secrets, IDs, timestamps, random
values or anything else meant to differ per instance belong inside
`getView`/`runAction`. For the same reason, rebuilt components are never
byte-identical.

Build commands, from the repository root:

```sh
cargo xtask js-guests        # rebuild guests/prebuilt/ and target/guests/ from the samples
python3 tools/componentize-js/pane_js.py build <package dir> <out.wasm>   # any command package
python3 tools/componentize-js/pane_js.py check   # are the prebuilt samples current and their npm licenses permissive?
```

A build installs the package's locked dependencies into a staging copy,
type-checks it with TypeScript when it has a `tsconfig.json` (the JS sample is
checked through JSDoc), bundles it with esbuild and componentizes it. The TS
sample's `.ts` source is transpiled by esbuild and runs on the same runtime as
JS. The first run builds the toolchain (about five minutes), later runs take
seconds. Everything downloaded or built goes to `PANE_JS_TOOLCHAIN_DIR`,
default `~/.cache/pane/componentize-js` on Linux,
`~/Library/Caches/pane/componentize-js` on macOS and
`%LOCALAPPDATA%\pane\componentize-js` on Windows; the source tree is not
written except for the output. After editing a sample, run
`cargo xtask js-guests` and commit the updated `guests/prebuilt/`.

Prerequisites, in addition to the Rust ones in the [README](../README.md):

- Python 3.12 or later (`python3`; on Windows use `python` in the commands
  above; set `PYTHON` for `cargo xtask` if yours is named differently), git,
  and Node.js 22 or later with npm. `cargo xtask ci` also runs the `check`
  subcommand, so it needs Python too.
- The build installs Rust `nightly-2026-09-27` with `rust-src` through rustup,
  and downloads wasi-sdk 34 for the host (x86_64 or arm64, all three OSes).
- **Windows:** `python` from python.org or the Microsoft Store and Node.js
  from nodejs.org; run from a normal shell. **macOS:** Xcode Command Line
  Tools already provide git; install Python and Node.js from their installers
  or Homebrew. **Linux:** the distribution's `python3`, `git`, `nodejs` and
  `npm` (Node.js 22+, for example through nvm).

Toolchain used: upstream [componentize-qjs](https://github.com/andreiltd/componentize-qjs)
0.4.5 at `e563c6d6` with the two patches in
[`tools/componentize-js/patches`](../tools/componentize-js/patches), its
QuickJS runtime built with `nightly-2026-09-27` for `wasm32-wasip3` against
wasi-sdk 34, the componentizer built with Rust 1.98.1, esbuild 0.28.2 and
TypeScript 7.0.2. Every JS component imports the same 20 WASI 0.3 interfaces
through its libc, whatever the source uses, and is about 4.3 MB. Only Linux
x86_64 builds have been run; the scripts avoid OS-specific paths, but Windows
and macOS builds are unverified. See
[tools/componentize-js](../tools/componentize-js/README.md) for the patch queue.

## Forms

An item can open a form instead of running an action: a single-line text
field and a choice of one option per field, and a submit button. Pane renders
the controls, handles focus, typing and input methods, and calls
`submit-form` with every field's value; the command validates them and answers
with a result, or with an error about one field (shown under it, with focus
moved there) or about the whole form. The contract, keyboard behavior and
accessibility are described in [docs/forms.md](../docs/forms.md). The "Greet
someone" item of each sample is the complete example.

Rust:

```rust
use pane_guest::{Choice, Field, FieldKind, FieldValue, Form, FormError, Item, TextField};

let form = Form {
    title: "Greet someone".into(),
    fields: vec![
        Field {
            id: "name".into(),
            label: "Name".into(),
            kind: FieldKind::Text(TextField { placeholder: Some("Ada Lovelace".into()) }),
        },
        Field {
            id: "greeting".into(),
            label: "Greeting".into(),
            kind: FieldKind::Choice(vec![
                Choice { id: "hello".into(), label: "Hello".into() },
                Choice { id: "morning".into(), label: "Good morning".into() },
            ]),
        },
    ],
    submit_label: "Greet".into(),
};
let item = Item { id: "form".into(), title: "Greet someone".into(), subtitle: None, form: Some(form), platforms: None };

// In `impl Guest`:
async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
    let name = values.iter().find(|v| v.id == "name").map_or("", |v| v.value.trim());
    if name.is_empty() {
        return Err(FormError { field: Some("name".into()), message: "Enter a name".into() });
    }
    Ok(format!("Hello, {name}"))
}
```

JavaScript or TypeScript (fields are camelCase; a field's `kind` is a tagged
value, `{ tag: "text", val: {...} }` or `{ tag: "choice", val: [...] }`):

```ts
const form: Form = {
  title: "Greet someone",
  fields: [
    { id: "name", label: "Name", kind: { tag: "text", val: { placeholder: "Ada Lovelace" } } },
    { id: "greeting", label: "Greeting", kind: { tag: "choice", val: [
      { id: "hello", label: "Hello" }, { id: "morning", label: "Good morning" },
    ] } },
  ],
  submitLabel: "Greet",
};
// items: [{ id: "form", title: "Greet someone", form }]

async submitForm(itemId, values) {
  const name = values.find((v) => v.id === "name")?.value.trim() ?? "";
  if (!name) throw { field: "name", message: "Enter a name" } satisfies FormError;
  return `Hello, ${name}`;
},
```

In JS/TS, reject a submission by throwing a plain `FormError` object as above.
Throwing an `Error` from `submitForm` is treated as a crash, not as a
validation message. The samples validate with Zod and turn its first issue
into a `FormError`.

## Actions for some operating systems only

An item can list the operating systems its action (or form) works on. On any
other system Pane still lists it, shows why it is unavailable ("Not available
on Linux: this action supports only Windows") and never calls the command for
it, so the rest of the command keeps working:

```rust
Item {
    platforms: Some(vec![Platform::Windows]), // pane_guest::Platform
    ..item("windows-only", "Windows-only action", "Declared to work on Windows only")
}
```

```js
{ id: "windows-only", title: "Windows-only action", platforms: ["windows"] }
```

`platforms` is `None` / omitted for every system. The command cannot tell
which system it runs on; Pane applies the declaration. The samples' last two
items are the runnable example, and
[platform availability](../docs/platform-availability.md) has the details.

## Custom views

An item can open a custom view that the command draws itself: filled
rectangles and one-line text in a fixed-size area, redrawn after each key
(arrows, Home, End) or pointer event (press over the view, drag, release).
Pane keeps focus and the focus ring, and exposes the view to assistive
technology as one control with the item's label and role and the value the
view reports. The command keeps each open view's state in a `custom-view`
resource that `open-view` returns; Pane drops it when the view closes. The
contract, input, lifecycle and accessibility are described in
[docs/custom-views.md](../docs/custom-views.md). The "Choose a color" item of
each sample is the complete example.

Rust (the view is a type implementing `GuestCustomView`; its methods take
`&self`, so state goes in `Cell`s or `RefCell`s):

```rust
use core::cell::Cell;
use pane_guest::alloc::{format, string::String, vec};
use pane_guest::{
    CustomView, CustomViewInfo, CustomViewRole, Frame, GuestCustomView, Key, Rect, Shape, ViewEvent,
};

struct Picker { column: Cell<i32> }

impl GuestCustomView for Picker {
    async fn render(&self) -> Frame {
        let x = self.column.get() * 36;
        Frame {
            width: 288,
            height: 36,
            shapes: vec![Shape::Rect(Rect { x, y: 0, width: 36, height: 36, fill: 0x1e88e5 })],
            value: format!("Column {}", self.column.get() + 1),
        }
    }

    async fn handle_event(&self, event: ViewEvent) -> Result<(), String> {
        match event {
            ViewEvent::Key(Key::Right) => self.column.set((self.column.get() + 1).min(7)),
            ViewEvent::Key(Key::Left) => self.column.set((self.column.get() - 1).max(0)),
            ViewEvent::PointerDown(at) => self.column.set((at.x / 36).clamp(0, 7)),
            _ => {}
        }
        Ok(())
    }
}

// The item: `custom_view: Some(CustomViewInfo { title: "Pick".into(), label:
// "Column".into(), role: CustomViewRole::ColorWell })`. In `impl Guest`:
type CustomView = Picker;

async fn open_view(_item_id: String) -> Result<CustomView, String> {
    Ok(CustomView::new(Picker { column: Cell::new(0) }))
}
```

JavaScript or TypeScript (a view is any object with `async render()` and
`async handleEvent(event)`; shapes and events are tagged values):

```ts
class Picker implements CustomView {
  column = 0;
  async render(): Promise<Frame> {
    return {
      width: 288,
      height: 36,
      shapes: [{ tag: "rect", val: { x: this.column * 36, y: 0, width: 36, height: 36, fill: 0x1e88e5 } }],
      value: `Column ${this.column + 1}`,
    };
  }
  async handleEvent(event: ViewEvent) {
    if (event.tag === "key" && event.val === "right") this.column = Math.min(this.column + 1, 7);
    if (event.tag === "key" && event.val === "left") this.column = Math.max(this.column - 1, 0);
    if (event.tag === "pointer-down") this.column = Math.min(Math.max(Math.floor(event.val.x / 36), 0), 7);
  }
}
// items: [{ id: "pick", title: "Pick", customView: { title: "Pick", label: "Column", role: "color-well" } }]

async openView(itemId) {
  return new Picker();
},
```

Both methods must be `async` in JS/TS (see the
[contract notes](../docs/custom-views.md#contract)); `@pane/extension` types
them as returning a `Promise`, so the build's type check rejects a
synchronous one. A frame may have at most 4096 shapes, 256 characters per
text and 4096 x 4096 pixels; Pane shows a larger one as your error. Throwing from
`handleEvent` shows the error and keeps the view; a crash closes it. A
command without custom views uses `type CustomView = NoCustomView;` in Rust
and makes `open_view`/`openView` fail.

## Packaging and installing a local extension

A package is a folder with a `pane.json` manifest at its root and the built
component of each command it lists. The same format serves Rust, JavaScript
and TypeScript: Pane sees only components.

```json
{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [
    {
      "id": "hello",
      "title": "Say hi",
      "subtitle": "Optional second line in root search",
      "component": "target/wasm32-wasip2/release/hello.wasm"
    }
  ]
}
```

- `manifestVersion` (required): the manifest format, currently `1`. A newer
  number is refused with "a newer Pane is needed".
- `title` (required): the display title. It is not the package's identity.
- `version` (optional): shown before installing and after an update.
- `apiVersion` (required): the `pane:extension` contract the components are
  built against, `MAJOR.MINOR` (this Pane provides `0.1`, from
  [`wit/extension.wit`](../wit/extension.wit)). Before 1.0 the minor version
  must match; from 1.0, any minor version up to Pane's in the same major.
- `platforms` (optional): the operating systems the package supports, from
  `windows`, `macos` and `linux`; omitted means all of them, and `[]` means
  none. On a system it does not list, Pane explains the package ("Not
  available on Linux: this package supports only Windows") instead of
  installing it. See
  [platform availability](../docs/platform-availability.md).
- `commands` (required, at least one): `id` unique in the package, `title`,
  optional `subtitle`, optional `platforms` (the same list, for this command
  alone: elsewhere its root row is listed with the reason and does not
  open), and `component`, a relative path inside the package folder (no
  `..`, no absolute path) to a built component.

Unknown fields are ignored. The component must exist when you install: a
package whose component is not built is refused as source-only, with the
missing path. Pane then checks each component without running it: it must
compile, import only WASI 0.3 and export the extension interface, each
function Pane calls with the types it calls it with.
A component built against an older shape of the same `apiVersion` (the
pre-release API 0.1 changes between slices) is therefore refused at install,
naming the first mismatch ("it was built for an older extension API shape:
rebuild it against Pane's current extension API 0.1 (`get-view`: type
mismatch for field items: expected record of 6 fields, found 4 fields)");
rebuild it against the current [`wit/extension.wit`](../wit/extension.wit).

Where the component comes from is up to your build. A standalone Rust crate
can point `component` at `target/wasm32-wasip2/release/<name>.wasm` inside
the crate folder after `cargo build --release --target wasm32-wasip2`. A
JavaScript or TypeScript package can build into its own folder with
`python3 tools/componentize-js/pane_js.py build <package dir> <package dir>/dist/<name>.wasm`
and point at `dist/<name>.wasm`. The repository's samples live in one Cargo
workspace and a prebuilt folder, so their manifests are in
[`packages/`](packages) and `cargo xtask guests` assembles each with its
component into `target/guests/packages/<name>/`.

To install, choose **Install extension from folder…** at the end of root
search, pick the package folder, check the source, version, commands and
compatibility Pane shows, and press Enter on **Install**. The package's
commands appear in root search, the first one selected. From the command line,
`cargo run -p pane -- --install target/guests/packages/sample-rust` (or
`pane --install <folder>`) opens the same screen, which also helps where no
folder picker is available: on Linux the picker is the desktop portal
(`xdg-desktop-portal`), and without one Pane shows why it could not open it.

What installing does:

- **Identity.** The package is identified by its folder's absolute path as
  the operating system resolves it (`std::fs::canonicalize`): symbolic links
  and `..` are followed, and on file systems that ignore letter case or
  Unicode normalization (the defaults on Windows and macOS) the stored
  spelling is used, so two spellings of one folder are one package. Pane does
  no case folding or normalization of its own, so on a case-sensitive Linux
  file system `Hello` and `hello` are two packages. On Windows the `\\?\`
  prefix is dropped. A folder path that is not valid Unicode is refused.
  Moving or renaming the folder makes it a different package.
- **Copy.** `pane.json` and the listed components (nothing else) are copied
  into Pane's data folder, under `extensions/packages/<n>/`, and recorded in
  `extensions/installed.json`. Your folder is never written, and the
  installed copy keeps working if the folder changes or is deleted. The data
  folder is `%LOCALAPPDATA%\Pane\data` on Windows,
  `~/Library/Application Support/Pane` on macOS and `$XDG_DATA_HOME/pane`
  (default `~/.local/share/pane`) on Linux; `PANE_DATA_DIR` overrides it.
- **Duplicates and updates.** Installing a folder that is already installed is
  refused. Choosing it again shows **Update** instead, which replaces the
  installed copy with the folder's current contents under the same identity,
  whatever its new title or version. Two different folders are two packages,
  even with identical contents, and nothing is merged or switched between
  them.
- **Listing.** Installed commands are listed from the manifests alone; no
  guest runs until you open a command. A damaged installed copy stays listed
  with its problem.
- **Disabling.** **Manage extensions…**, the last row of root search once a
  package is installed, lists every installed package with whether it is
  enabled and its source, so copies with the same title can be told apart.
  Enter disables or enables the selected one; only that installation
  changes. A disabled package's commands leave root search (they are not
  shown greyed out), an open command of it closes, its running instances are
  dropped and it can no longer save settings, so none of its code runs. This
  happens as soon as you press Enter, before the choice is written; if it
  cannot be written, the package is enabled again with the reason. Pressing
  Enter again while the choice is being written does nothing. The choice is
  recorded in
  `installed.json` (`"disabled": true`) and holds after restarting Pane and
  after an Update. Its settings are kept, and enabling it brings its
  commands back with them. The package stays installed at the same identity;
  choosing its folder again shows it as disabled.

Uninstalling and rebuilding on save are not implemented yet; to pick up a
rebuilt component, choose the folder again and Update.

Known limits of local packages so far:

- An update is not coordinated with a command that is running or open: the
  replaced copy's code is dropped, so an open command of the package loses
  its state and may fail until you open it again from root search. Staged
  activation that waits for running commands comes with reload (#11, #14).
- Disabling does not cancel a call already running in the package: it
  finishes (its answer is not shown, and it cannot save settings), then its
  instance is dropped; a call that had not started is refused. Cancelling async work,
  background services, timers and hotkeys are not part of the extension API
  yet and come with their own tickets. Disabling does not yet consider
  packages that depend on the disabled one (#43).
