# Extension guests

This folder holds Pane's extension SDKs for Rust, JavaScript and
TypeScript, along with the sample extensions and test fixtures built on
them. This guide is for anyone writing an extension for Pane.

An extension is a WebAssembly component that implements the
`pane:extension` contract in [`wit/extension.wit`](../wit/extension.wit).
Pane registers only WASI 0.3 interfaces. A component that imports WASI 0.2
(for example through Rust's standard library on `wasm32-wasip2`) is
rejected with an explanation.

Pane's default extensions are not in this folder. Each one lives in its own
repository ([see below](#default-extensions-live-in-their-own-repositories)).

## Where to start

1. Write a first command, [in Rust](#writing-a-rust-command) or
   [in JavaScript or TypeScript](#writing-a-javascript-or-typescript-command).
   The samples are complete examples.
2. Make it a package and install it:
   [Packaging and installing a local extension](#packaging-and-installing-a-local-extension).
3. Change it without restarting Pane:
   [reload the package](#reloading-a-package-while-pane-stays-open), or let
   Pane [build and reload it on every save](#developing-a-package-build-and-reload-on-save).
4. Share it by [publishing to npm](#publishing-a-package-to-npm) or
   [from a Git repository](#publishing-a-package-from-a-git-repository).

The rest of this guide is reference, one section per thing a command can
do, each with Rust and JavaScript or TypeScript examples:

- Data and failures: [settings](#keeping-settings),
  [content, cache and credentials](#keeping-content-cache-and-credentials),
  [errors and crashes](#errors-and-crashes),
  [printing and logging](#printing-and-logging),
  [actions for some operating systems only](#actions-for-some-operating-systems-only).
- Views: [forms](#forms), [searching inside a command](#searching-inside-a-command)
  (with [web requests](#web-requests)), [the canvas](#the-canvas-drawing-inside-a-designed-view).
- Root search: [results computed from the query](#root-results-computed-from-the-query)
  (including [Pane's file index](#panes-file-index)),
  [results supplied ahead of the query](#root-results-supplied-ahead-of-the-query),
  [root providers](#root-providers), [arguments](#arguments),
  [a command that takes a query](#a-command-that-takes-a-query),
  [keywords](#a-commands-keywords),
  [typed addresses and paths](#commands-for-typed-addresses-and-paths).
- Running without the user: [no-view commands](#no-view-commands-and-the-launch-record),
  [scheduled work](#scheduled-work), [continuing services](#continuing-services).
- Telling the user and reaching the system:
  [what a command does after it acts](#what-a-command-does-after-it-acts),
  [preferences](#preferences-and-the-setup-screen),
  [clipboard history](#clipboard-history), [native helpers](#native-helpers),
  [system programs](#system-programs),
  [the Run dialog](#running-what-the-run-dialog-runs),
  [session and power commands](#session-and-power-commands),
  [the open windows](#listing-the-open-windows).
- Other extensions: [operations](#operations), [capabilities](#capabilities),
  [owned registrations](#owned-registrations),
  [the activation entry point](#the-activation-entry-point),
  [the state handoff](#the-state-handoff),
  [dependencies](#dependencies-on-other-extensions).

[What this folder holds](#what-this-folder-holds), at the end, lists the
SDKs, samples and fixtures.

## Writing a Rust command

The [sample](sample-rust/src/lib.rs) is the complete example. A command is
a `cdylib` crate that depends on `pane-extension` and implements
`pane_extension::Command`:

- `render` returns its list. Each item's action is a closure.
- Its entry point for designed views (see [Forms](#forms)) names its type.

The SDK hands Pane the list as a versioned JSON tree. When the user
chooses an item, the SDK runs its closure, and Pane then asks for the list
again ([list-tree.md](../docs/list-tree.md)):

```rust
#![no_std]

use pane_extension::alloc::{string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, Item, List};
use pane_extension::view::NoDesignedView;

struct Hello;
pane_extension::export!(Hello);

impl Command for Hello {
    type DesignedView = NoDesignedView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Hello").item(Item::new("hi", "Say hi").on_action(|| async {
            show_toast(Toast::success("hi!"));
            Ok(())
        })))
    }

    async fn open_designed_view(
        _command: String,
        _launch: pane_extension::LaunchRecord,
    ) -> Result<NoDesignedView, String> {
        Err("this command opens no designed view".into())
    }
}
```

Pane shows nothing when an action succeeds. The action tells the user what
happened itself, with a toast in the launcher's footer or a HUD over other
applications once the launcher closes. Both come from
`pane_extension::feedback`: `show_toast` with `Toast::success`,
`Toast::failure` or `Toast::animated` (which it can update or hide), and
`show_hud`. An action may also close the window or return to root search
(`pane_extension::window`).

Returning `Err` shows the message as a failure toast. A panic traps the
guest; Pane reports it and recovers by starting a fresh instance on the
next call.

WASI 0.3 interfaces are available through the
[`wasip3`](https://docs.rs/wasip3/0.9.0/wasip3/) crate with
`default-features = false`. The sample awaits `wasi:clocks` this way.

### Building and trying it

Build with the pinned toolchain. `wasm32-wasip2` is the compiler target
name; the emitted component imports only WASI 0.3.

```sh
cargo xtask guests
# or, for the guests workspace only:
cd guests && cargo build --release --target wasm32-wasip2
```

To try a rebuilt sample in the launcher, install its assembled package
from the root: `cargo run -p pane -- --install target/guests/packages/sample-rust`
(or `sample-js`, `sample-ts`), then press Enter. Pane builds no sample in.
To run your own command, make it a package and install it the same way;
see [Packaging and installing a local extension](#packaging-and-installing-a-local-extension).

Toolchain used: Rust 1.98.1, `wit-bindgen` 0.62.0, `wasip3`
0.9.0+wasi-0.3.0; host Wasmtime and wasmtime-wasi 49.0.1.

### Keeping settings

A command of an installed package can keep string values between runs
with the `pane:extension/settings` interface in
[`wit/data.wit`](../wit/data.wit). The settings sample, in
[Rust](sample-settings/src/lib.rs), [JavaScript](sample-settings-js/src/index.js)
and [TypeScript](sample-settings-ts/src/index.ts), saves the greeting
style the user picks. In Rust it is `pane_extension::settings`:

```rust
use pane_extension::settings;

settings::set("greeting-style", "formal")?;          // Result<(), String>
let style: Option<String> = settings::get("greeting-style")?;
```

In JavaScript and TypeScript it is a module (typed in
[`js/data.d.ts`](js/data.d.ts)). An error is thrown as an `Error` whose
message is the reason, so rethrowing it shows the reason to the user:

```ts
import { get, set } from "pane:extension/settings@0.2.0";

set("greeting-style", "formal");
const style: string | null = get("greeting-style");
```

How settings behave:

- Values belong to the installed package's source identity, not its title
  or managed copy. Two installed copies of the same package have separate
  settings, and an update keeps them.
- They are kept while the package is disabled and while Pane is not
  running, and the command sees them again when the package is enabled.
  While the package is disabled nothing of it runs. A call still finishing
  from before the disable cannot save: `set` fails instead of writing.
- Pane keeps them in `extensions/settings.json` in its data folder. If
  that file cannot be read, `get` and `set` return the reason and Pane
  does not overwrite the file. Each `set` replaces the file whole, so a
  crash leaves the old or the new file. The file is not locked, though:
  two Pane processes using the same data folder can lose each other's last
  write. Keeping to one running Pane is a later concern.
- A command built into Pane rather than installed from a package has no
  settings: `get` and `set` return an error.
- A component that does not import `settings` is unaffected; it is built
  for the `extension` world as before. `extension-with-data` adds the
  imports within extension API 0.2, so a component that uses settings
  needs a Pane with this change. JavaScript and TypeScript commands are
  built against a world that includes it, so the prebuilt JS/TS components
  list the import whether or not they use it.

### Keeping content, cache and credentials

Next to `settings`, [`wit/data.wit`](../wit/data.wit) has three
interfaces with the same `get` and `set`, one for each other kind of
[extension data](../docs/extension-data.md):

- `content` for the extension's own durable records
- `cache` for values it can make again
- `credentials` for secrets kept on this computer

The settings sample uses all three. In Rust they are
`pane_extension::{content, cache, credentials}`. In JavaScript and
TypeScript they are the modules `pane:extension/content@0.2.0`,
`pane:extension/cache@0.2.0` and `pane:extension/credentials@0.2.0`:

```ts
import * as cache from "pane:extension/cache@0.2.0";

const greeting = cache.get("last-greeting") ?? makeGreeting();
cache.set("last-greeting", greeting);
```

- They behave like settings: owned by the source identity, kept while the
  package is disabled or updated, and refused while it is disabled. Each
  is kept in its own file (`content.json`, `cache.json`,
  `credentials.json`).
- The user can clear an extension's cache in Settings › Extensions at any
  time, without the extension running, so expect any cache value to be
  missing. Its settings, content and credentials are kept.
- Uninstalling removes the cache and credentials. The user chooses whether
  the settings and content are kept for a later install of the same
  source, so an extension installed again may find settings and content
  without a cache or a credential.
- Credentials are plain text in Pane's data folder, not in the system's
  keychain. On macOS and Linux only the user can read their file, but
  other extensions and programs running as the user can
  ([limits](../docs/extension-data.md#limits)).
- The extension migrates its own values between its versions; Pane keeps
  them unchanged across an update.

## Writing a JavaScript or TypeScript command

The [JavaScript](sample-js/src/index.js) and
[TypeScript](sample-ts/src/index.tsx) samples are complete examples. A
command is an npm package whose `main` module exports `command`. It has
`render`, which returns its list (each item's action is a function,
`onAction`), and it opens its designed views (see [Forms](#forms) and
[The canvas](#the-canvas-drawing-inside-a-designed-view)).

The SDK hands Pane the list as a versioned JSON tree and runs an item's
`onAction` when the user chooses it; Pane then asks for the list again
([list-tree.md](../docs/list-tree.md)). Pane's types come from
`@pane-app/extension` (a `file:../js` development dependency). They
describe plain values, not engine objects:

```ts
import type { Command } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

export const command: Command = {
  async render() {
    return {
      title: "Hello",
      items: [
        {
          id: "hi",
          title: "Say hi",
          async onAction() {
            await waitFor(10_000_000); // 10 ms; the command suspends meanwhile
            showToast({ title: "hi!" });
          },
        },
      ],
    };
  },
  async openView() {
    throw new Error("this command opens no designed view");
  },
};
```

Pane shows nothing of what an `onAction` resolves with. The command tells
the user what happened with `@pane-app/extension/feedback` (`showToast`,
`showHUD`), or closes the window (`closeMainWindow`, `popToRoot`,
`clearSearchBar`); see
[What a command does after it acts](#what-a-command-does-after-it-acts).

When something goes wrong:

- Throwing (a rejected promise) shows the error's message, or a thrown
  string, as a failure toast.
- Resolving with a value, such as `null` from an `onAction`, traps the
  guest. Pane reports it and recovers as it does for Rust.
- A list Pane cannot read (a title that is not text, say) is the command's
  failure, not a crash.

### What a command can use

npm dependencies are bundled into the component. The samples use
[Zod](https://zod.dev) 4.6.5 (`zod/mini`) and show its validation failure
as a normal error.

Only ECMAScript built-ins and `console`
([printing and logging](#printing-and-logging)) are available, not Node.js
or browser APIs. WASI 0.3 imports declared by [the world](js/wit/world.wit)
are imported by name. Currently those are `wasi:clocks/monotonic-clock`,
and `wasi:http/client` for a command whose bundle imports it. The clock is
typed in [`js/wasi.d.ts`](js/wasi.d.ts), and web requests have a typed
helper, [`@pane-app/extension/http`](#web-requests). `Math.random`,
`Date.now()` and `performance.now()` are fresh in each instance.

### The snapshot caveat

A component is built by running the module once and snapshotting the
engine. Module top-level code therefore runs at build time, on the build
machine, and every instance starts from its result.

Keep top-level code to pure setup such as schemas and constants. Secrets,
IDs, timestamps, random values and anything else meant to differ per
instance belong inside `render` and the actions. For the same reason,
rebuilt components are never byte-identical.

### Building

Build commands, from the repository root:

```sh
cargo xtask js-guests        # rebuild guests/prebuilt/ and target/guests/ from the samples
cargo run -p pane-ext -- dev <package dir>   # any command package: build it and develop it in Pane
cargo xtask js-guests --check # are the prebuilt samples current and their npm licenses permissive?
```

A build is pane-build's JavaScript build (`crates/pane-build`, #218). It:

1. installs the package's locked dependencies into a staging copy;
2. type-checks it with the package's TypeScript when it has a
   `tsconfig.json` (the JS sample is checked through JSDoc);
3. bundles it with esbuild;
4. componentizes it with the componentizer the repository links (the
   committed `runtime.wasm` and `libc.so` of
   `tools/componentize-js/wasm-parts`).

TypeScript and esbuild both come from the package's own `node_modules`,
so its lockfile pins their versions. The TS sample's `.ts` source is
transpiled by esbuild and runs on the same runtime as JS.

A build needs Node.js and npm and the repository's Rust, and nothing else.
The staging copy is kept in the user's cache
(`pane/js-build/<package>-<hash>`, under `~/.cache` on Linux,
`~/Library/Caches` on macOS and `%LOCALAPPDATA%` on Windows), so a save
whose lockfile did not change installs nothing. After editing a sample,
run `cargo xtask js-guests` and commit the updated `guests/prebuilt/`.

### Prerequisites

In addition to the Rust ones in the [README](../README.md), you need
Node.js 22 or later with npm, and nothing else: no Python, no nightly
Rust, no wasi-sdk. The componentizer is linked into the build
(`crates/pane-build`'s `componentizer` feature), with the wasm parts the
repository commits. The published CLI (`npm install @pane-app/cli`, whose
JavaScript shim picks the platform package for the system) is the same
build. Rebuilding the wasm parts themselves is a maintainer's step
(`cargo xtask wasm-parts`), never part of building a package; see
[tools/componentize-js](../tools/componentize-js/README.md).

Where to get Node.js:

- Windows: from nodejs.org. Run from a normal shell.
- macOS: from its installer or Homebrew.
- Linux: the distribution's `nodejs` and `npm` (Node.js 22+, for example
  through nvm).

### Toolchain

The toolchain used:

- the vendored [componentize-qjs](https://github.com/andreiltd/componentize-qjs)
  0.4.5 at `e563c6d6`
  ([`tools/componentize-js/componentize-qjs`](../tools/componentize-js/componentize-qjs),
  the patch queue applied in its source);
- its QuickJS runtime, built with `nightly-2026-09-27` for `wasm32-wasip3`
  against wasi-sdk 34 (the committed `runtime.wasm`);
- wasi-sdk 34's `libc.so` (also committed);
- the componentizer, built with the repository's Rust 1.98.1;
- esbuild 0.28.2 and TypeScript 7.0.2.

Every JS component imports the same 20 WASI 0.3 interfaces through its
libc, whatever the source uses. If its bundle imports `wasi:http` (itself
or through `@pane-app/extension/http`), it also imports `wasi:http`'s
`types` and `client`, and Pane then lists it as using the network. A
component is about 4.4 MB.

The componentizer and the `pane-ext` that runs it ship as npm packages:
`@pane-app/cli`, whose JavaScript shim picks the `@pane-app/cli-<target>`
platform package for the system. CI builds them for Windows, macOS and
Linux on x64 and arm64 from the committed wasm parts. The workflow that
packs them (`cli-packages.yml`) also installs them into a scratch project
with only Node.js and npm and builds this sample with them. See
[tools/componentize-js](../tools/componentize-js/README.md) for the patch
queue and the wasm parts.

## Forms

A form is a designed view (#241): a `"mode": "designed"` command whose
view answers a `form` node with the fields in the author's own layout.

The fields are the tree's field components: text, password, text area,
checkbox, toggle, date and date-time pickers, dropdowns with sections and
search, tag pickers, and file and folder pickers, with descriptions,
separators and links between them. Each has a title, a note, an error, a
default, an auto-focus and a remembered value.

Submitting is an action. Enter in a single-line field submits, Ctrl+Enter
does in a text area, and so does the form's submit button. The values
arrive keyed by the fields' `id`s. When the view refuses a value, the tree
it answers with sets that field's `error`.

The contract, keyboard behavior and accessibility are described in
[docs/designed-tree.md](../docs/designed-tree.md). The "Greet someone"
command of each sample is the complete example.

Rust ([`pane_extension::form`](../guests/pane-extension/src/form.rs)):

```rust
use pane_extension::form;

struct Greeting { name: core::cell::RefCell<String> }

impl pane_extension::view::View for Greeting {
    fn render(&mut self, cx: &mut pane_extension::view::Cx<Self>) -> impl pane_extension::view::IntoAnswer {
        let submit = cx.form_listener(|this, values| {
            let name = values.text("name").unwrap_or_default().trim().to_owned();
            if name.is_empty() {
                *this.name.borrow_mut() = String::new();
                return;
            }
            *this.name.borrow_mut() = name;
        });
        let error = if self.name.borrow().is_empty() { "Enter a name" } else { "" };
        form::Form::new()
            .key("form")
            .submit_title("Greet")
            .on_submit(submit)
            .child(form::text_field("name").title("Name")
                .placeholder("Ada Lovelace").remember().error(error))
            .into_answer()
    }
}

// In `impl Command`: `type DesignedView = Greeting;` and `open_designed_view`
// answers it for the form command's id.
```

JavaScript or TypeScript (`Form` and `Form.*` from
`@pane-app/extension/view`):

```tsx
import { Form, createView, useState } from "@pane-app/extension/view";

function GreetingForm() {
  const [answer, setAnswer] = useState("");
  return (
    <Form
      submitTitle="Greet"
      onSubmit={(values) => setAnswer(`Hello, ${values.name}`)}
    >
      <Form.TextField id="name" title="Name" placeholder="Ada Lovelace" remember />
      <Form.PasswordField id="secret" title="Secret" />
      <Form.TextArea id="notes" title="Notes" />
      <Form.Checkbox id="updates" label="Send updates" />
      <Form.DatePicker id="day" title="Day" />
      <Form.Dropdown id="greeting" title="Greeting"
        options={[{ value: "hello", label: "Hello" }]} />
      <Form.TagPicker id="tags" title="Tags" options={[{ value: "friend", label: "Friend" }]} />
      <Form.FilePicker id="file" title="File" />
      <Form.Description>{"What the form asks for."}</Form.Description>
    </Form>
  );
}

export const command = { async openView(id) { return createView(GreetingForm); } };
```

A field's `remember` keeps its last submitted value as the package's
settings and prefills it the next time the form opens.

## Errors and crashes

An error a command returns (Rust `Err`; in JS/TS, anything a handler
throws) is its message to the user. Pane shows it and the command keeps
running, however often it happens. The JS/TS build wraps the exported
handlers ([`guests/js/adapt.js`](js/adapt.js)) so that a thrown `Error`,
string or `{ message }` object is always such an error.

A crash is different. A Rust panic, or in JS/TS resolving with a value of
the wrong type, traps the guest. Pane reports it and starts a fresh
instance for the next call. After three crashes within five minutes, or a
component that cannot start, Pane pauses the whole package until the user
chooses Retry in **Settings › Extensions**, where "Why <title> is paused"
shows the details. The package keeps its data ([pausing](../docs/pausing.md)).

So report expected failures, such as a missing sign-in, as errors, never
by crashing. The settings samples' **Crash** item shows a crash in each
language.

### Memory

Each instance's memory may grow to 128 MiB. Pane refuses it more: the
allocation fails, which traps the guest, and the user sees "The extension
crashed: it ran out of memory: an extension may use at most 128 MiB". It
counts towards pausing the package as any crash does.

A web response's body (at most 4 MiB) fits many times over. Keep large
data in files or the cache rather than in memory.

### When the extension runtime crashes

Pane's extension runtime itself can crash too, from a fault in Pane rather
than in any extension. Pane then stops every call in progress and never
runs one again by itself. An action that did its work (saving, sending a
request) may have lost only its answer: the user sees that the runtime
stopped and runs it again only if they want it done again
([runtime crashes](../docs/pausing.md#when-the-extension-runtime-itself-crashes)).

Write an action whose repetition matters so the user can tell whether it
ran, as the Rust settings sample's **Count** does by showing the count it
saved in a toast.

### The compute limit

Every call Pane makes into a command (opening it, an action, a form, a
search, a view event) may compute for 5 seconds in all. Only your own
code's computing counts. Time spent waiting (a clock, a save, a helper, a
web request, another extension's operation) does not, nor does time inside
Pane's own host calls or starting your component.

The count is per call and cumulative. Awaiting between parts of a
computation does not reset it; only the call finishing does. A call past
the limit holds every other extension's calls behind it, so Pane stops it
as **not responding**. Its instance goes, the user sees "The extension
stopped responding", and it counts towards pausing the package as a crash
does
([extensions that stop responding](../docs/pausing.md#when-an-extension-stops-responding)).

So keep each call's own computing well under 5 seconds. Split long work
into several calls (an action that does one part and saves where it got
to), or run it in a [native helper](#native-helpers), which runs for as
long as its work takes while other extensions' calls are served. The
settings samples' **Stop responding** item shows the limit in each
language.

## Printing and logging

What a command writes to standard output and standard error goes to its
package's **extension log**. Pane's own messages about the package appear
between its lines: its crashes with their backtraces, calls that stopped
responding, pauses, and, while it is developed, each build and reload.

In Rust, `pane_extension::debug!`, `info!`, `warn!` and `error!` log a
line with its level, formatted as `format!` formats.
`pane_extension::println!` and `eprintln!` (and `print!`, `eprint!`) print
one without a level. A panic's message and location are logged as an
error before the guest traps
([`pane_extension::log`](pane-extension/src/log.rs)). Logging waits for
Pane to take the line, which it does at once. It works in a command's
calls, not in a view's destructor.

In JavaScript and TypeScript, `console.debug`, `log`, `info`, `warn`,
`error`, `trace` and `assert` format their arguments roughly as Node does
(an `Error` with its stack). What a handler throws is logged too, with its
stack, as well as being the error the command answers with
([`guests/js/console.js`](js/console.js)).

### Levels

Debug and info lines go to standard output, warnings and errors to
standard error. A line may start with its level as `<7>` (debug), `<6>`
(info), `<4>` (warning) or `<3>` (error), which the SDKs write. An
untagged line is info on standard output and an error on standard error.

### Limits and where the log is kept

Logging is never a hazard to Pane or the disk:

- A line longer than 4 KiB is cut, with a note of how much was.
- A package writing more than 1,000 lines in a second loses the rest of
  that second, and Pane notes how many it dropped.

While a package is developed, Pane keeps its most recent 5,000 lines and
appends every line to `extension.log` in its development folder under
Pane's data folder (rotated at 5 MiB, keeping one earlier file as
`extension.log.1`). The log starts afresh with each development session.

A package that is not developed keeps only its most recent 500 lines (or
256 KiB) in memory, for diagnostics. They are never written to disk or
sent anywhere, and they go when Pane quits or the package is uninstalled.

The settings samples' **Write to the log** and **Flood the log** items
show both in each language.

## Actions for some operating systems only

An item can list the operating systems its action (or form) works on. On
any other system Pane still lists it, shows why it is unavailable ("Not
available on Linux: this action supports only Windows") and never calls
the command for it, so the rest of the command keeps working:

```rust
Item {
    platforms: Some(vec![Platform::Windows]), // pane_extension::Platform
    ..item("windows-only", "Windows-only action", "Declared to work on Windows only")
}
```

```js
{ id: "windows-only", title: "Windows-only action", platforms: ["windows"] }
```

`platforms` is `None` or omitted for every system. The command cannot
tell which system it runs on; Pane applies the declaration. The samples'
last two items are the runnable example, and
[platform availability](../docs/platform-availability.md) has the details.

## Root results computed from the query

A command can answer what the user types into root search. The Rust
[sample](sample-rust) does this with "reverse <text>", and the
[Calculator](https://github.com/pane-app/calculator) default extension
does it with arithmetic. Its results are listed above the results root
search finds by title, and Enter on one performs its action:

- `copy` copies a text to the clipboard.
- `open-url` opens an `http://` or `https://` address with the system's
  handler for web links, normally the default browser. It opens an
  address of any scheme.

Set `"rootResults": true` on the command in `pane.json` and export
`pane:extension/root-results`
([`wit/root-results.wit`](../wit/root-results.wit)) beside the command.

Pane asks the command on every change of a query that is not blank, so
its instance starts with the first query typed. It discards an answer
once the query has changed. A query the command has no answer for returns
no results (an incomplete expression is not an error). Returning an error
is the extension failing, and Pane lists a result explaining it. A
disabled package is not asked. See
[root search](../docs/root-search.md#results-computed-from-the-query).

Pane asks with the query and when it was typed, as `at` (`WallTime` in
Rust, `WallTime` in TypeScript): milliseconds since the Unix epoch and the
local time's offset from UTC, by the clock root search's own dates are
shown by. A command can therefore answer about the current date or time
without reading a clock of its own.

A result that is an answer's card can say more than its title and action
in `answer` (`AnswerDetail`): the section it sits under in place of the
command's title ("Color", "Date & Time"), a swatch, and further ways to
copy it the Actions panel offers. The calculator's colours and dates use
all of it.

Rust (`pane_extension::root`; the component then exports both interfaces):

```rust
use pane_extension::alloc::{string::String, vec, vec::Vec};
use pane_extension::root::{RootAction, RootResult, WallTime};

pane_extension::export!(Sample);
pane_extension::root::export!(Sample);

impl pane_extension::root::Guest for Sample {
    async fn results_for(query: String, at: WallTime) -> Result<Vec<RootResult>, String> {
        let Some(text) = query.strip_prefix("reverse ") else {
            return Ok(Vec::new());
        };
        let reversed: String = text.chars().rev().collect();
        Ok(vec![RootResult {
            id: "reversed".into(),
            title: reversed.clone(),
            subtitle: None,
            action: RootAction::Copy(reversed),
            answer: None,
        }])
    }
}
```

JavaScript or TypeScript: add `"pane": { "rootResults": true }` to
`package.json`, so the build exports the interface, and export
`rootResults` from the module:

```ts
import type { RootResult, RootResults } from "@pane-app/extension";

export const rootResults: RootResults = {
  async resultsFor(query): Promise<RootResult[]> {
    if (!query.startsWith("reverse ")) return [];
    const reversed = [...query.slice(8)].reverse().join("");
    return [{ id: "reversed", title: reversed, action: { tag: "copy", val: reversed } }];
  },
};
```

The three samples answer "reverse <text>" this way, and "pane website"
with a result whose action opens a link (`RootAction::OpenUrl(url)` in
Rust, `{ tag: "open-url", val: url }` in JavaScript and TypeScript).
Their packages in [`packages/`](packages) set `rootResults`.

Once the query changes or root search is left, Pane cancels a call still
pending. One not started is never started. One waiting inside the command
(on an async import) is dropped with the command's instance, so module or
struct state kept between queries is lost and the next query starts a
fresh instance. Keep what must last in [settings](#keeping-settings) or
the [cache](#keeping-content-cache-and-credentials).

### Pane's file index

Pane keeps an index of the names of the files and folders under the
user's home folder (and the folders the user adds in Settings), which a
WASI guest could not walk itself. A command can search it through
`pane:extension/file-index` ([`wit/file-index.wit`](../wit/file-index.wit))
and answer results that open an entry (`open-file`).

The package's `pane.json` sets `"fileIndex": true`. Pane keeps the index
open, caught up and watched exactly while such a package is enabled and
not paused, and only such a package may search it.

`search(query, options)` answers at once from what is indexed; it never
waits for a walk. It answers the entries whose name, or the folders they
are in, match the query's words, best first, or for a blank query the
most recently modified. The `options`:

- filter by kind (file, folder, link);
- filter by category (documents, images, audio, video, archives,
  applications, text, and other for a file of none of these);
- sort by relevance or by modified time;
- page with `limit` (at most 200 per call) and `offset`.

Each entry carries the `id` Pane gave it for this package, its absolute
`path`, `name`, `folder` (below the home folder as `~/…`), `kind`, whether
opening it would run a `program`, `size`, `modified` (seconds since 1970)
and `volume`. `status()` says whether the index is off, being built (with
how many entries it has `found`), current or stopped, and why.

The path is the extension's to show and copy. Pane opens only by the id,
which it checks again first: the entry still exists, is the kind indexed,
is not a link, and is still in the folders file search covers. An
`open-file` result gives the id. Pane shows the entry's own name, its
folder and the system's icon in the row, and gives it its
[file actions](../docs/files.md#the-file-actions): Enter opens a document
or a folder, and shows a program in the file manager (never runs it).
Only the explicit Run runs one.

```rust
use pane_extension::file_index::{self, SearchOptions};
use pane_extension::root::{RootAction, RootResult, WallTime};

async fn results_for(query: String, at: WallTime) -> Result<Vec<RootResult>, String> {
    Ok(file_index::search(&query, SearchOptions::first(5))?
        .into_iter()
        .map(|entry| RootResult {
            id: entry.path,
            title: entry.name,
            subtitle: None,
            action: RootAction::OpenFile(entry.id),
            answer: None,
        })
        .collect())
}
```

JavaScript or TypeScript: add `"fileIndex": true` to the `"pane"` options
of `package.json`, so the build imports the interface (a command without
it does not), and import it. `search` throws an object whose `payload` is
the reason, and WIT's `u64` numbers are `bigint`. The declarations are in
[`js/file-index.d.ts`](js/file-index.d.ts).

```ts
import { search } from "pane:extension/file-index@0.2.0";

return search(query, { sort: "relevance", limit: 20, offset: 0 }).map((entry) => ({
  id: entry.path,
  title: entry.name,
  action: { tag: "open-file", val: entry.id },
}));
```

The [Files](https://github.com/pane-app/files) default extension works
this way: its command, Search Files, answers both root search and its own
field. [`sample-files`](sample-files), [`sample-files-js`](sample-files-js)
and [`sample-files-ts`](sample-files-ts) do the same in Rust, JavaScript
and TypeScript.

### Files of a granted folder

A command can find the files of the one folder the user granted its
package, which a WASI guest cannot read itself, through
`pane:extension/files` ([`wit/files.wit`](../wit/files.wit)), and answer
results that open one (`open-file`).

The package's `pane.json` sets `"folderAccess": true`. Pane then shows its
own "Choose folder…" row at the top of the package's commands and records
the folder the user picks.

The command never names or sees a path. `list-folder()` answers one of
three things:

- no folder is granted;
- Pane is listing it (Pane asks the command again once it is done, so
  answer no files for now);
- the listing Pane keeps for this visit of root search, whose files have
  an `id` and a `relative` path.

An `open-file` result gives the `id`. Pane shows the file's own name and
folder in the row, whatever the result's title says, and drops an id it
did not give. It gives the file its own
[file actions](../docs/files.md#the-file-actions): Open (Enter), Show in
Explorer (Ctrl+Enter), Open With…, Copy Path, Copy Name, Copy File and
Move to Recycle Bin (confirmed). Each checks the file again first, then
closes the window and says what it did in a HUD. For a program or script,
Enter shows it in Explorer and only Run runs it. The command is never
called for them.

Pane lists the folder under its
[scan policy](../docs/files.md#the-scan-policy) (`files.limits()` gives
its limits). File results are listed after the results root search finds
by title.

File search itself has used [Pane's file index](#panes-file-index) since
#175. The granted folder stays for a package that wants an exhaustive
listing of one folder the user chooses. The test fixture
[`fixtures/folder-files`](fixtures/folder-files) is what Files was before.

Rust (`pane_extension::files`):

```rust
use pane_extension::files::{self, FolderState};
use pane_extension::root::{RootAction, RootResult, WallTime};

async fn results_for(query: String, at: WallTime) -> Result<Vec<RootResult>, String> {
    let FolderState::Ready(listing) = files::list_folder()? else {
        return Ok(Vec::new());
    };
    Ok(listing
        .files
        .into_iter()
        .filter(|file| file.relative.contains(query.as_str()))
        .map(|file| RootResult {
            id: file.relative.clone(),
            title: file.relative,
            subtitle: None,
            action: RootAction::OpenFile(file.id),
            answer: None,
        })
        .collect())
}
```

JavaScript or TypeScript: add `"files": true` to the `"pane"` options of
`package.json`, so the build imports the interface (a command without it
does not), and import it. `listFolder` throws an object whose `payload` is
the reason; the declarations are in [`js/files.d.ts`](js/files.d.ts).

```ts
import { listFolder } from "pane:extension/files@0.2.0";

const state = listFolder();
if (state.tag !== "ready") return [];
return state.val.files
  .filter((file) => file.relative.includes(query))
  .map((file) => ({ id: file.relative, title: file.relative, action: { tag: "open-file", val: file.id } }));
```

### The entries of a folder the user typed

A command can list the entries of a folder the user typed into root
search (a path-like query ending in a separator), which a WASI guest
cannot read itself. It does so through `pane:extension/typed-folder`
([`wit/typed-folder.wit`](../wit/typed-folder.wit)) and answers results
that open one (`open-file`).

No folder is granted and the `pane.json` declares nothing, because the
user named the folder. `list-entries(folder)` takes what they typed, which
Pane resolves (`~` to the home folder, `file://` taken off). It answers
the direct entries: folders first and each in name order, at most 500,
with `truncated` set when the folder holds more. Each entry has the `id`
Pane gave it, its `name`, whether it is a `folder` and whether opening it
would run a `program`.

An error explains a folder that cannot be listed (not a path, a network
location, a file, or one Pane cannot read). Answer no results for it, as
for a missing folder.

An `open-file` result gives the id. Pane shows the entry's own name and
folder in the row, whatever the result's title says, and gives the entry
its own [file actions](../docs/files.md#the-file-actions), checking it
again first. The bounds are Pane's, not the command's. Root search lists
the entries under "Files", below the rows declared for the path, and adds
a row saying so when the listing is partial (see
[docs](../docs/files.md#the-typed-folder)).

Rust (`pane_extension::typed_folder`):

```rust
use pane_extension::root::{RootAction, RootResult, WallTime};
use pane_extension::typed_folder::{self, FolderEntry};

async fn results_for(query: String, at: WallTime) -> Result<Vec<RootResult>, String> {
    let Some(entries) = typed(&query) else {
        return Ok(Vec::new());
    };
    Ok(entries
        .into_iter()
        .map(|entry| RootResult {
            id: entry.id.clone(),
            title: entry.name,
            subtitle: None,
            action: RootAction::OpenFile(entry.id),
            answer: None,
        })
        .collect())
}

/// The entries of the folder the query names, when it ends in a separator.
fn typed(query: &str) -> Option<Vec<FolderEntry>> {
    let query = query.trim();
    query
        .ends_with(['/', '\\'])
        .then(|| typed_folder::list_entries(query).ok())
        .flatten()
        .map(|listing| listing.entries)
}
```

JavaScript or TypeScript: add `"typedFolder": true` to the `"pane"`
options of `package.json`, so the build imports the interface (a command
without it does not), and import it. `listEntries` throws an object whose
`payload` is the reason; the declarations are in
[`js/typed-folder.d.ts`](js/typed-folder.d.ts).

```ts
import { listEntries } from "pane:extension/typed-folder@0.2.0";

if (!query.endsWith("/") && !query.endsWith("\\")) return [];
try {
  return listEntries(query.trim()).entries.map((entry) => ({
    id: entry.id,
    title: entry.name,
    action: { tag: "open-file", val: entry.id },
  }));
} catch {
  return [];
}
```

The [Files](https://github.com/pane-app/files) default extension lists a
typed folder this way, and so do [`sample-files`](sample-files),
[`sample-files-js`](sample-files-js) and
[`sample-files-ts`](sample-files-ts) in Rust, JavaScript and TypeScript.

## Root results supplied ahead of the query

A command can also give root search results that do not depend on the
query, as the [Applications](https://github.com/pane-app/applications)
default extension gives the installed applications. Pane asks once root
search is used and keeps them. For a query that is not blank, it matches
and ranks them by title like commands.

Set `"indexedResults": true` on the command in `pane.json` and export
`pane:extension/indexed-results`
([`wit/applications.wit`](../wit/applications.wit)) beside the command.
Pane asks again after each return to root search. An error is listed as a
row explaining it.

A result's action opens an installed application
(`IndexedAction::OpenApplication(id)`), or a target of any kind, with an
application if one is named, as a quicklink does
(`IndexedAction::Open(OpenTarget { target, application })`). See
[root search](../docs/root-search.md#results-supplied-ahead-of-the-query)
and [applications](../docs/applications.md).

Any Rust command can also find and open the installed applications
through Pane (`pane_extension::applications`, the
`pane:extension/applications` import), since a WASI guest cannot:

```rust
use pane_extension::alloc::{string::String, vec::Vec};
use pane_extension::applications;
use pane_extension::indexed::{IndexedAction, IndexedResult};

pane_extension::export!(Apps);
pane_extension::indexed::export!(Apps);

impl pane_extension::indexed::Guest for Apps {
    async fn results() -> Result<Vec<IndexedResult>, String> {
        Ok(applications::installed()?
            .into_iter()
            .map(|app| IndexedResult {
                action: IndexedAction::OpenApplication(app.id.clone()),
                id: app.id,
                title: app.name,
                subtitle: Some(app.distinction.unwrap_or_else(|| "Application".into())),
                alternate_titles: app.alternate_titles,
                keywords: app.keywords,
            })
            .collect())
    }
}
```

An indexed result's `alternate_titles` find it as its title does, and its
`keywords` as its subtitle does (empty lists for none). The row shows its
title whichever matched.

### The installed applications

`applications::open(&id)` opens an application from a command's own
action. What Pane tells a command about each application:

- `id` is opaque and stable across its updates and Pane's restarts
  (several shortcuts to one program are one application). A command may
  keep it in its data and open the application later. Do not parse it.
- `name` is the one the system shows in the user's language.
- `alternate_titles` holds its other names (untranslated, its program's,
  such as `code`).
- `keywords` holds the words that find it (a desktop entry's `Keywords`).
- `distinction`, when another application has its name, is what tells it
  apart (its program's name, folder or path).
- `icon` is a reference to the application's own icon, which Pane
  extracts and keeps. A command's list shows it with
  `Item::new(..).icon(Icon::application(app.icon))`
  (`icon: { application: app.icon }` in JavaScript and TypeScript), and
  root search draws it for every result that opens an application
  ([applications](../docs/applications.md#icons)).

JavaScript or TypeScript: add `"pane": { "indexedResults": true }` to
`package.json`, so the build exports the interface. Import the host's
functions from `"pane:extension/applications@0.2.0"` (they throw an object
whose `payload` is the reason) and export `indexedResults`:

```ts
import type { IndexedResults } from "@pane-app/extension";
import { installed } from "pane:extension/applications@0.2.0";

export const indexedResults: IndexedResults = {
  async results() {
    return installed().map((app) => ({
      id: app.id,
      title: `Launch ${app.name}`,
      // Optional: other titles and words that find the result.
      alternateTitles: app.alternateTitles.map((name) => `Launch ${name}`),
      keywords: app.keywords,
      action: { tag: "open-application", val: app.id },
    }));
  },
};
```

The [JavaScript](sample-applications-js) and
[TypeScript](sample-applications-ts) applications samples do this, and
their commands list the applications and open one with `open(id)`. Their
packages in [`packages/`](packages) set `indexedResults`.

## Root providers

Some commands exist only to answer root search, as the
[Calculator](https://github.com/pane-app/calculator) and
[Applications](https://github.com/pane-app/applications) default
extensions' do. Such a command declares `"mode": "provider"` in
`pane.json` (#164) and is a **root provider**.

A root provider has no row of its own. It is never launched, so it cannot
be pinned and has no alias, fallback or hotkey. Neither the Actions panel
nor the Shortcuts page offers it, and root search learns nothing about it.
Its results still answer as a command's would, computed from the query
(`"rootResults": true`) or supplied ahead of it (`"indexedResults": true`),
each a root result of its own. Its extension's card in Settings lists it
under the extension's switch, which turns its results off and on.

```json
{ "id": "calculator", "title": "Calculator", "component": "calculator.wasm",
  "mode": "provider", "rootResults": true }
```

The rules for a provider:

- It must declare `rootResults` or `indexedResults`.
- It may not declare what only a launched command uses (`search`,
  `takesQuery`, `arguments` or a `schedule`). Otherwise the install is
  refused with the reason.
- It may run a continuing `service` and declare preferences.
- Its component exports the root-results or indexed-results interface
  beside `command` as any other does. Pane never opens or runs it, so it
  needs no `render` or `run` of its own. In Rust that is
  `impl pane_extension::Command` with only
  `type DesignedView = pane_extension::view::NoDesignedView`; in
  JavaScript or TypeScript, `export const command: Command = {}`.

Pins, aliases, fallbacks and hotkeys recorded for a command before it
became a provider (an update that changes its mode) are dropped at the
next start, with a toast naming them.

## Scheduled work

A command can declare that Pane runs one of its items on a schedule, so
it does work without the user asking. Add a `schedule` to its `pane.json`
entry naming how often to run and which item's action to run:

```json
{
  "id": "check",
  "title": "Check something",
  "component": "check.wasm",
  "schedule": { "everySeconds": 900, "item": "check-now" }
}
```

- `everySeconds` (required): the interval, from 1 second to 30 days
  (provisional bounds, pending the user's decision). Anything outside
  them, or any other field in `schedule`, is refused when the package is
  previewed or installed, before anything is installed.
- `item` (required): the id of the item whose action runs, the same
  action Enter runs from the command's list, at most 256 characters. The
  command usually lists the item, so the user can run it too.

Pane shows nothing of the run's success. A toast or HUD the action shows
itself (`pane_extension::feedback`) is shown as when the user runs it, as
a HUD while the launcher is hidden. An error the action answers with is
shown as a failure toast while the command's screen is open. A trap counts
as a crash of the package (three within five minutes pause it, as for any
action).

### When a schedule runs

The schedule runs only while the package's code may run, meaning it is
enabled and not paused. Installation alone schedules nothing that is not
enabled, and Pane activates the command (starting its instance) only when
a run is due, never another package.

- Disabling the package, uninstalling it, Pane pausing it after failures,
  or replacing its code by a reload or an update ends the schedule. A run
  still pending is stopped and its late answer discarded.
- Enabling the package, replacing its code or restarting Pane starts the
  schedule again, from a full interval, without replaying work that fell
  due meanwhile.
- At most one run of a command is asked for at a time. Ticks that fall
  due while one runs are coalesced into the next run after it answers.

The scheduled run is the item's ordinary action: Pane asks for the
command's list, then runs the item's action, as choosing it would. The
guest needs no new interface, and everything an action may do (read and
save data, call helpers, make requests, wait) works the same. See
[scheduled work](../docs/schedules.md) for the full contract, and the
samples ([`sample-schedule`](sample-schedule) in Rust,
[JavaScript](sample-schedule-js) and [TypeScript](sample-schedule-ts))
for complete examples.

## Continuing services

A command can run a continuing service, so it works without the user
asking and at no interval the manifest declares. Set `"service": true` on
its entry in `pane.json`, and export `pane:extension/service`
(`run-cycle`) beside `command`.

Pane calls `run-cycle` in a cycle while the package's code may run (it is
enabled and not paused). The cycle starts the moment it may: installing
an enabled package, enabling, replacing the code, or Pane starting. It
ends when it may not: disabled, uninstalled, paused after failures,
replaced. The service paces itself: each cycle answers the status to show
and how long to wait before the next.

```rust
use pane_extension::service::Cycle;

pane_extension::export!(Watching);
pane_extension::service::export!(Watching);

impl pane_extension::service::Guest for Watching {
    async fn run_cycle(command: String) -> Result<Cycle, String> {
        // One slice of the service's work: check what it watches, then
        // say what to show and when to run it again.
        let seen = content::get("events")?;
        Ok(Cycle {
            status: format!("Watching: {seen:?}"),
            next_seconds: 2,
        })
    }
}
```

- Each cycle is an ordinary guest call, stopped when the package's
  [generation](../docs/generations.md) ends. A disable, reload, update,
  uninstall or pause while it runs stops it, and its late answer is
  discarded. The instance goes with it, along with whatever the service
  keeps in it (its task's state), so enabling the package starts a fresh
  task. Keep each cycle short: waiting inside one holds every other
  extension's calls for as long as it waits, as any call does.
- A trap, or a cycle that computes for too long and Pane stops, is a
  crash of the package like any call's (three within five minutes pause
  it). An error the cycle answers with never pauses it, however often.
  The next cycle runs a second later, so one broken cycle never ends the
  service.
- `next_seconds` is at least 1 and at most 2592000 (30 days); anything
  outside is clamped (provisional bounds, as scheduled work's). Time that
  passes while a cycle runs is not replayed: the next cycle runs one
  cadence after its answer lands.
- The status shows on the command's screen while it is open, and the
  cycle runs whether or not it is.

A JavaScript or TypeScript command sets `"pane": { "service": true }` in
its `package.json` so that it is built with the interface, and exports
`runCycle` as `service` (typed `Service` and `Cycle` in
[`js/pane.d.ts`](js/pane.d.ts)):

```ts
import type { Cycle, Service } from "@pane-app/extension";

export const service: Service = {
  async runCycle(command: string): Promise<Cycle> {
    return { status: "Watching", nextSeconds: 2 };
  },
};
```

See [continuing services](../docs/services.md) for the full contract, and
the samples ([`sample-service`](sample-service) in Rust,
[JavaScript](sample-service-js) and [TypeScript](sample-service-ts)) for
complete examples. They include the ways a cycle can end (wait, fail,
crash, stop responding) and cadences beyond the bounds that Pane clamps
(0 seconds, 31 days).

## Clipboard history

A Rust command can keep clipboard history through Pane
(`pane_extension::clipboard_history`, the
`pane:extension/clipboard-history` import,
[`wit/clipboard.wit`](../wit/clipboard.wit)). Pane itself watches the
clipboard and keeps the text the user copies for the command's package,
once the command has turned it on, and only while the package runs and the
history is not paused. The package does not run while text is copied; it
reads what Pane kept:

```rust
use pane_extension::clipboard_history::{self as history, Capture};

// From an action the user chose, never on its own: history starts off.
history::set_capture(Capture::On)?;
for entry in history::entries()? {
    // entry.text, entry.age_seconds, entry.source ("notepad.exe")
}
```

The other functions:

- `status()` says whether it is on, why Pane cannot watch the clipboard
  (such as on a system without an adapter), the excluded programs, the
  count and the retention.
- `set-excluded` replaces the excluded programs.
- `copy(id)` puts an item on the clipboard again.
- `clear()` deletes every item and keeps history on.
- `delete-items(ids)` deletes some.
- `turn-off-and-clear()` turns history off and deletes every item at once.

Pane keeps plain text only, never text marked by its application as not to
be kept, and nothing while the package is disabled. Each item is kept for
the retention after it was copied: 7 days unless `set-retention(seconds)`
chose 1 minute to 365 days. Pane then deletes it itself, whether the
command runs or not. `entries()` never lists an expired item, so a command
needs no expiry of its own.

The [Clipboard History](https://github.com/pane-app/clipboard-history)
default extension is the example; see
[clipboard history](../docs/clipboard-history.md). It is the one package
whose history records from the first start, without calling `set-capture`
(ADR 0042). Pane draws its command in a view of its own, whose controls
(pause, retention, disabled applications, clearing) are Pane's. Its own
list is only Pause or Resume Recording and the items. The JavaScript and
TypeScript samples show the whole contract's controls. Only Windows has a
clipboard adapter so far, so its package declares
`"platforms": ["windows"]`.

A JavaScript or TypeScript command imports the interface when its
package.json sets `"pane": { "clipboardHistory": true }` (declared in
[`js/clipboard.d.ts`](js/clipboard.d.ts)); a command that does not set it
does not import it. On failure, each function throws an object whose
`payload` is the reason:

```ts
import * as history from "pane:extension/clipboard-history@0.2.0";

history.setCapture("on");
history.setRetention(86400); // keep each item for a day
for (const entry of history.entries()) {
  // entry.text, entry.ageSeconds, entry.source ("notepad.exe")
}
history.deleteItems(["7"]); // `delete-items`: `delete` is a JavaScript keyword
```

[`sample-clipboard-js`](sample-clipboard-js) and
[`sample-clipboard-ts`](sample-clipboard-ts) implement the Clipboard
History command in JavaScript and TypeScript. Each kept item has the
actions Paste, Copy and Delete (destructive, last). Paste is Enter and
uses [`system::paste`](#paste-the-front-application-and-selected-text).
Where Pane cannot paste, it copies the item with `copy(id)` and shows the
HUD "Copied — paste is not available here yet".

## No-view commands and the launch record

A command's entry in `pane.json` declares its **mode** (ADR 0037).
`"mode": "view"`, the default when it says nothing, opens a screen, its
list. `"mode": "no-view"` runs and opens none. Any other value is refused
at install with the reason. Pane reads the mode from the manifest, so it
knows at Enter what to do without running the command.

A **no-view command** runs each time it is launched: Enter on its row in
root search, its alias, a fallback, its global hotkey (which runs it
without showing Pane's window), its quick slot, another command, or its
own schedule. Root search, or whatever Pane shows, stays as it is.

Pane shows nothing of a success. The command tells the user what happened
itself, with a toast or a HUD (`pane_extension::feedback` in Rust), and
may close the window (`pane_extension::window`). An error it answers is
shown as a failure toast with a "Copy Error" action, and a toast it left
in progress (the animated style) is hidden once the run ends. A run
launched in the background has no one watching, so a command usually
shows nothing then, as the samples do. An error it answers never counts
towards [pausing](../docs/pausing.md); a crash does, as any call's.

A `schedule` without an `item` makes Pane run the command itself every
interval, in the background, showing nothing:

```json
{ "id": "tick", "title": "Tick", "component": "tick.wasm",
  "mode": "no-view", "schedule": { "everySeconds": 60 } }
```

### The launch record

Every command receives its **launch record** on every way in. It holds:

- whether the user launched it or Pane did in the background;
- from where: root search, an alias, a fallback, a hotkey, a quick slot,
  another command or a schedule;
- its [arguments](#arguments);
- the text sent through its alias or as a fallback;
- the JSON context another command passed
  ([`wit/commands.wit`](../wit/commands.wit)).

In Rust, implement `run` in `pane_extension::Command`. One component may
serve several commands, told apart by their id in `pane.json`. A view
command's `render` reads its record with
`pane_extension::commands::current()`. `render` and `open_designed_view`
have defaults, so a no-view command needs neither:

```rust
use pane_extension::alloc::{format, string::String};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::view::NoDesignedView;
use pane_extension::{Command, LaunchRecord, LaunchType};

struct Toggle;
pane_extension::export!(Toggle);

impl Command for Toggle {
    type DesignedView = NoDesignedView;

    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        // A background launch, such as a schedule's, shows nothing.
        if launch.launch_type != LaunchType::Background {
            let source = pane_extension::commands::source_name(launch.source);
            show_toast(Toast::success(format!("{command} ran from {source}")));
        }
        Ok(())
    }
}
```

In JavaScript or TypeScript, give the exported `command` a
`run(id, launch)`. A view command's `render(launch)` receives the record
too:

```ts
import type { Command } from "@pane-app/extension";
import { showHUD } from "@pane-app/extension/feedback";

export const command: Command = {
  async run(id, launch) {
    showHUD(`${id} ran from ${launch.source}`);
  },
};
```

### Launching another command

A command launches another with `pane:extension/commands`'s `launch`
(`pane_extension::commands::launch` in Rust, an import of
`pane:extension/commands@0.2.0` in JavaScript and TypeScript). The target
is one of its own package's commands, by its id in `pane.json`, or one of
another installed package, by that package's identity (`local:` and the
absolute folder Pane shows, `npm:` and its name, `git:` and its
repository). The launch passes JSON context and asks nothing.

- `user-initiated` opens the target as if the user had invoked it.
- `background` runs a no-view command without a window, and is refused
  for a view command.

A target that is not installed, has no such command, or is disabled,
paused or unavailable on this system is refused with the reason, which the
caller receives as an error. `launch` answers once the launch has started,
not when the target has run.

A `user-initiated` launch from a call no window was shown for (a timer
firing, a schedule, a service) is an **attention launch** (ADR 0053): it
shows Pane's launcher on the command, with the keyboard focus, so an
alarm can put its "Snooze / Dismiss" screen in front of the user, as
Raycast's `launchCommand` with `LaunchType.UserInitiated` does from a
background command. It must name a view command of the caller's own
package: another package's command, a no-view command, and one that
waits are refused too. `launch` answers once it is queued. Pane then
shows it when the user can take it:

- while the user is typing, in another application or in Pane, it waits
  for a second of no input, at most 10 seconds;
- while the launcher shows an Alert (a confirmation), it waits until the
  Alert is answered; any other screen makes way, and Settings stays
  open;
- while Game mode holds Pane's hotkeys, it waits until the game leaves
  the front;
- one is shown at a time, in the order asked: the next waits until the
  user leaves the screen the first opened. Each extension has at most one
  waiting; a newer one replaces it. A waiting one is dropped if its
  extension is disabled, paused, reloaded, updated or uninstalled.

The [no-view sample](sample-no-view) does all of this in Rust, and its
[JavaScript](sample-no-view-js) and [TypeScript](sample-no-view-ts)
copies do the same.

## What a command does after it acts

Pane shows nothing of what an action, a no-view `run` or a search result
answers. The command says what happened itself, through host functions
every command has, whatever its mode
([wit/feedback.wit](../wit/feedback.wit), ADR 0037):

- A toast in the launcher's footer, where the status line is: animated
  (work in progress), success or failure, with an optional message and up
  to two actions with shortcuts. One shows at a time. A new toast replaces
  the one shown, whose handle then does nothing; the command can update or
  hide its own. Success and failure leave after 3 seconds (paused while
  the pointer is over the toast). An animated one stays until updated or
  hidden, or the window deactivates, and Pane hides one a no-view run left
  once the run ends. Ctrl+T (Command+T on macOS) moves the focus to its
  actions; choosing one calls the command back, as an item's action does.
  While the launcher is hidden or collapsed, a toast is shown as a HUD.
- A HUD: Pane closes the window, then shows a short message near the
  bottom of the screen, over other applications, for 1.2 seconds (3 for a
  failure).
- The window. `close` hides it, choosing what its next showing shows
  (`default`: the user's Launcher setting; `immediate`: root search now;
  `suspended`: the screen left on display) and whether root search's query
  is emptied. `pop-to-root` returns to root search with the window open.
  `clear-search` empties the search field. In a call no window was shown
  for (a background launch, a schedule, a service) they do nothing and
  answer false.
- The row's subtitle. `set-subtitle` replaces the subtitle the command's
  root search row shows (and matches), such as "3 unread", until set
  again. Pane keeps it across restarts and forgets it on uninstall.
- A confirmation (#146), before something that cannot be undone,
  described below.

An error an action or a run answers is shown as a failure toast with a
"Copy Error" action.

### Confirmations

`confirm` shows a title, an optional message, the primary button (Enter;
drawn destructive when asked) and the dismiss button (Escape; "Cancel"
unless named) over the launcher's screen, showing the launcher first if it
is hidden. It answers true only for the primary button. Escape, a click
outside it or the window losing the focus answer false. Other packages'
calls are served while it waits.

Given a `remember` key, it offers "Don't ask again" (Space or a click
ticks it). Confirmed with the primary button while it is ticked, the
answer is remembered per package and key, across restarts, disabling and
updates, and later confirmations with that key answer true at once. A
dismissal is never remembered, so the user is asked again. "Reset
confirmations" on the extension's card in Settings › Extensions forgets
them, as uninstalling does.

In a call no window was shown for (a background launch, a schedule, a
service), `confirm` answers an error saying a confirmation is not
available there.

### Examples

Rust (`pane_extension::feedback`, `pane_extension::window`,
`pane_extension::commands::set_subtitle`):

```rust
use pane_extension::feedback::{Toast, ToastAction, ToastStyle, show_hud, show_toast};
use pane_extension::window::{PopToRootType, close};

let shown = show_toast(Toast::animated("Uploading…"));
// ... the work ...
shown.update(Toast::success("Uploaded").primary(ToastAction::new("Open", || async {
    show_toast(Toast::success("Opened"));
    Ok(())
})));
show_hud("Copied to Clipboard", ToastStyle::Success); // closes the window first
close(true, PopToRootType::Immediate);

// In an async action or run:
use pane_extension::feedback::{Confirmation, confirm};
let asked = Confirmation::new("Delete the note?")
    .primary("Delete")
    .destructive()
    .remember("delete-note");
if confirm(asked).await? {
    // ... delete it ...
}
```

JavaScript or TypeScript (`@pane-app/extension/feedback`):

```ts
import { closeMainWindow, setSubtitle, showHUD, showToast } from "@pane-app/extension/feedback";

const toast = showToast({ style: "animated", title: "Uploading…" });
// ... the work ...
toast.update({
  style: "success",
  title: "Uploaded",
  primaryAction: { title: "Open", onAction: async () => showToast({ title: "Opened" }) },
});
showHUD("Copied to Clipboard");
closeMainWindow({ clearRootSearch: true, popToRootType: "immediate" });
setSubtitle("3 unread");

// import { confirmAlert } from "@pane-app/extension/feedback";
const deleting = await confirmAlert({
  title: "Delete the note?",
  primaryAction: { title: "Delete", style: "destructive" },
  remember: "delete-note",
});
```

The [actions sample](sample-actions) and its
[JavaScript](sample-actions-js) and [TypeScript](sample-actions-ts) copies
use every one of them: its "Window", "Feedback" and "Confirm" items, and
its no-view commands "Window functions", "Spin", "Stumble" and "Confirm
Run".

### The clipboard, opening, revealing and recycling

Every command also reaches the system through Pane
([wit/system.wit](../wit/system.wit), ADR 0037). Each function does only
what it names; none closes the window or says anything.

- `copy` puts text or a file (by its absolute path) on the clipboard. A
  concealed copy carries the system's "do not record" marker, so
  clipboard managers, Pane's own clipboard history among them, do not
  keep it. On Windows the marker is
  `ExcludeClipboardContentFromMonitorProcessing`, with
  `CanIncludeInClipboardHistory` and `CanUploadToCloudClipboard` 0; on
  macOS it is `org.nspasteboard.ConcealedType`. Linux (X11) has no such
  marker, and copies text only for now.
- `read-clipboard` answers the clipboard's text, the file copied in the
  file manager, or nothing (not available on Linux yet).
- `open` opens anything, unfiltered: a URL of any scheme (`https:`,
  `mailto:`, `ms-settings:`, an application's own), a file, a folder or an
  application. It uses the system's handler, or an application named by
  its path or its installed-application id.
- `reveal` shows a path selected in File Explorer (Finder, or the file
  manager elsewhere).
- `trash` moves paths to the Recycle Bin (the trash elsewhere) and answers
  those it could not move, each with why.

A refusal is an answer, never a reason to pause the package.

The SDKs' **standard actions** compose these functions the way Raycast's
built-in actions behave:

- Copy ("Copy to Clipboard", then the window closes and a "Copied to
  Clipboard" HUD shows)
- Open
- Open With… (a submenu of the installed applications)
- Show in Explorer (named for the system)
- Move to Recycle Bin (destructive, with a HUD)

Each closes the window after it acts. Asked to keep it open, it says what
it did in a toast instead.

Rust (`pane_extension::system`, `pane_extension::actions`):

```rust
use pane_extension::actions;
use pane_extension::system::{self, Clip};

system::copy(&Clip::Text("hunter2".into()), true)?; // concealed
system::open("mailto:someone@example.com", None)?;
Item::new("note", "Note").actions([
    actions::copy(Clip::Text("Some text".into())).into(),
    actions::copy(Clip::Text("Some text".into())).keep_window_open().into(),
    actions::open_with(r"C:\Notes\todo.txt").into(),
    actions::show_in_file_manager(r"C:\Notes\todo.txt").into(),
    actions::move_to_trash([r"C:\Notes\todo.txt"]).into(),
]);
```

JavaScript or TypeScript (`@pane-app/extension/system`):

```ts
import { copy, copyAction, moveToTrashAction, open, openWithAction } from "@pane-app/extension/system";

copy("hunter2", { concealed: true });
open("C:\\Notes\\todo.txt", "C:\\Windows\\System32\\notepad.exe");
const actions = [
  copyAction("Some text"),
  copyAction("Some text", { keepWindowOpen: true }),
  openWithAction("C:\\Notes\\todo.txt"),
  moveToTrashAction(["C:\\Notes\\todo.txt"]),
];
```

The actions sample's "System" item calls each function on its own, and
its "Standard actions" item has every standard action.

### Paste, the front application and selected text

Three more system functions reach the application that was in front
before Pane:

- `paste` closes the window, brings that application back to the front,
  pastes text or a file into it through the clipboard, then puts back what
  the clipboard held, unless something else was copied meanwhile. The
  pasted content and what is put back are both copied concealed, so
  clipboard managers keep neither.
- `front-application` answers that application's name and icon (the path
  or `shell:` name whose system icon it is), or none. In Rust
  `front.icon()` is that system icon as an `Icon` for an item or an
  action; in JavaScript and TypeScript `front.icon` already is one
  (`{ file }`).
- `selected-text` answers the text selected in it, or none when nothing is
  selected, which is not a failure.

Each can answer "not available on this system yet" or a failure, and the
two are different. Until Pane's Windows power features land, and on macOS
and Linux (X11 included) for now, all three answer "not available". That
answer is never a reason to pause the package.

Where paste is not available, the window stays open and the clipboard is
left alone. The standard **Paste** action then copies the content instead,
closes the window and shows "Copied — paste is not available here yet" in
a HUD.

Rust (`SystemError::NotAvailable` / `SystemError::Failed`):

```rust
use pane_extension::actions;
use pane_extension::system::{self, Clip, SystemError};

let title = match system::front_application() {
    Ok(Some(front)) => format!("Paste to {}", front.name),
    _ => "Paste to Active App".into(),
};
Item::new("snippet", "Snippet").actions([
    actions::paste(Clip::Text("Kind regards".into())).into(),
    actions::paste(Clip::Text("Kind regards".into())).title(title).into(),
]);
match system::selected_text() {
    Ok(Some(text)) => { /* use it */ }
    Ok(None) => { /* nothing is selected */ }
    Err(SystemError::NotAvailable(why)) => { /* do something else */ }
    Err(SystemError::Failed(why)) => return Err(why),
}
```

JavaScript or TypeScript (a `NotAvailableError`, or an `Error` for a
failure):

```ts
import { frontApplication, NotAvailableError, pasteAction, selectedText } from "@pane-app/extension/system";

const front = frontApplication(); // { name, icon: { file } | null } or null
const actions = [pasteAction("Kind regards"), pasteAction("Kind regards", { title: `Paste to ${front?.name}` })];
try {
  const text = selectedText(); // null when nothing is selected
} catch (error) {
  if (!(error instanceof NotAvailableError)) throw error;
}
```

The actions sample's "Paste" item has the standard Paste, a "Paste to …"
titled from the front application, a paste that reports each answer, the
front application's name and icon, and "Search Selection", which handles
each answer of selected-text.

## Preferences and the Setup screen

A package that needs something from the user, such as an API key or a
folder, does not build a settings screen. It declares **preferences** in
`pane.json`, for the whole extension under the package's `preferences`,
or for one command under that command's. Pane draws, stores and checks
them, and hands each command its values.

```json
"preferences": [
  { "name": "apiKey", "type": "password", "title": "API key",
    "description": "Where to find it: see the help.", "required": true },
  { "name": "units", "type": "dropdown", "title": "Units", "required": true,
    "default": "metric",
    "options": [{ "value": "metric", "title": "Metric" },
                { "value": "imperial", "title": "Imperial" }] },
  { "name": "verbose", "type": "checkbox", "title": "Verbose",
    "label": "Say more", "default": false }
]
```

### Declaring a preference

A preference has:

- a `name`, unique among the package's preferences and each command's
  together, with no `#`;
- a `type`: `text`, `password`, `checkbox`, `dropdown`, `file`, `folder`
  or `application`;
- a `title`;
- optionally a `description`, a `placeholder`, `required` and a
  `default`. The default is a value, or one per system
  (`{ "windows": …, "macos": …, "linux": … }`).

A checkbox may have a `label`, and its default is `true` or `false`. A
dropdown needs its `options`, each a value, or a `value` with a `title`
(as an argument's are), and its default must be one of them. A duplicate
name, an unknown type or a dropdown default not among its options refuses
the package at install, with the reason.

### Reading the values

A command reads its **effective values**: its package's preferences, then
its own, each the value the user set or else its default. A checkbox's
value is a boolean and every other kind's is text. A preference with
neither is absent.

- Rust deserializes them into a type of its own with serde
  (`pane_extension::preferences::values::<T>()`).
- JavaScript and TypeScript call `getPreferenceValues()` from
  `@pane-app/extension/preferences`, TypeScript naming its interface
  (`getPreferenceValues<Preferences>()`).

Underneath is `pane:extension/preferences`
([`wit/preferences.wit`](../wit/preferences.wit)), which answers the
values as JSON.

### The Setup screen

A **required** preference with no value and no default is unset. Before a
launch by the user, Pane checks. With any unset, it shows the **Setup
screen** instead:

- the extension's title and "Set these up before using <command>";
- only the unset fields, with their descriptions, each with the control
  its extension's card in Settings has: a checkbox and a dropdown a
  choice, a password hidden as it is typed, and a file, folder or
  application a path with "Choose…", which opens the system's picker;
- the package's `HELP.md` beside them.

Submitting saves the values and launches the command as it was launched;
Escape launches nothing. A stored value that no longer fits counts as
unset: a dropdown value no longer among the options, or a file or folder
that no longer exists.

Every other way in (a background launch, root results, a schedule, a
service) does not run a command that needs setup. Its row in root search
says "Needs setup", and none of this counts as a failure.

### Changing the values later

The user changes the values later on the extension's card in Settings ›
Extensions. They are saved as they change and applied without a restart:

- a text or password in a field;
- a checkbox as a switch;
- a dropdown as a select (its value on a button that opens the options
  under a "Search choices" field, as Settings' own choices are);
- a file, folder or application in a field with "Choose…".

"Configure Command…" and "Configure Extension…" in root search's Actions
panel open it there.

Values are the package's [extension data](../docs/extension-data.md): a
password is a local credential, and every other value an extension
setting. So disabling keeps them. An update keeps those whose names are
still declared (and drops one whose type changed so that it no longer
fits). Uninstalling keeps the settings when asked, while always removing
the credentials.

The [preferences sample](sample-preferences) declares every type, and its
[JavaScript](sample-preferences-js) and [TypeScript](sample-preferences-ts)
copies answer the same.

## Arguments

A command may ask for up to three typed values before each run. Its
`pane.json` entry declares `arguments`, each with:

- a `name`;
- a `type`: `text`, `password` or `dropdown`;
- an optional `placeholder`;
- `required` (false unless it says);
- for a dropdown, its `options` (a value, or a `value` with a `title`).

```json
{ "id": "greet", "title": "Greet", "component": "greet.wasm", "mode": "no-view",
  "arguments": [
    { "name": "name", "type": "text", "placeholder": "Name", "required": true },
    { "name": "secret", "type": "password", "placeholder": "Secret" },
    { "name": "tone", "type": "dropdown", "placeholder": "Tone",
      "options": [{ "value": "warm", "title": "Warm" }, "brief"] }
  ] }
```

A fourth argument, a repeated name, an unknown type, a dropdown without
options, and a required argument on a command with a `schedule` are
refused at install with the reason.

The values reach the command in its launch record's `arguments`, by name,
in the order it declares them. An optional argument left empty is absent
(`launch.argument("name")` in Rust, `launch.arguments.find(...)` in
JavaScript and TypeScript).

### Arguments in root search

When the command's row is selected,
[root search](../docs/root-search.md#inline-argument-fields) shows the
fields inline after the query (#205): one field per argument, a password
masked, a dropdown's choices on a trigger that opens them, an optional
field marked so.

- Tab, Shift+Tab and the arrows move between the fields and the query.
- Enter with every required argument filled runs the command with the
  values. With one blank, Enter moves focus to it instead, marking it once
  it has been left blank.
- Typing the command's alias and a space enters the fields, carrying what
  is typed next into the first text or password one. A command without
  arguments opens at once instead, and one that takes a query keeps the
  row that sends the text after its alias.

The values are the search's own state, never recorded. They survive the
list being re-ranked and go when the query does, and a password's value is
recorded nowhere.

### Other launches

Every launch that does not come from root search (a global hotkey, a
quick slot, another command's) asks through Pane's argument form when a
required argument has no value. The form shows the command's title and
one field per argument, with focus on the first empty required one. Enter
with a required field still empty takes focus to it, and Escape launches
nothing. A background launch with a required argument missing is refused.

### Fallback text and remembered values

Text sent to the command as a fallback fills its first text or password
argument unless that has a value, and stays the launch record's fallback
text. A command may be a fallback when it takes a query, or when its first
argument is text and every other is optional.

Pane remembers the last value of each dropdown per command and offers it
again in the fields and the form. It never records a password's value
anywhere.

The [arguments sample](sample-arguments) does all of this in Rust, and
its [JavaScript](sample-arguments-js) and
[TypeScript](sample-arguments-ts) copies answer the same.

## A command that takes a query

The user can give any installed command an alias in Settings ›
Extensions, and typing it in root search lists the command first. The
command needs nothing for that.

A command that **takes a query** can also be sent text from root search.
The user types its alias, a space and the text ("ec hello"), or makes it a
fallback, which is listed below the results for any text typed, and
invokes that row. Pane launches the command only then, never while the
user types. The text, trimmed and never empty, arrives as its launch
record's **fallback text**.

Set `"takesQuery": true` on the command in `pane.json`. A no-view command
runs with the text and tells the user what it did with a toast (an error
it answers is shown as a failure toast) while root search stays as it
was. A view command opens its screen with it. See
[aliases and fallbacks](../docs/aliases.md).

Rust, as [`sample-query`](sample-query) does, Echo being a no-view
command:

```rust
async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
    let heard = match launch.fallback_text {
        Some(text) => format!("Echo heard “{text}”"),
        None => "Echo heard nothing".into(),
    };
    show_toast(Toast::success(heard));
    Ok(())
}
```

JavaScript or TypeScript, as the [JavaScript](sample-query-js) and
[TypeScript](sample-query-ts) query samples do:

```ts
export const command: Command = {
  async run(id, launch) {
    showToast({
      title: launch.fallbackText == null ? "Echo heard nothing" : `Echo heard “${launch.fallbackText}”`,
    });
  },
};
```

## A command's keywords

A command may declare `keywords` in `pane.json`: an author's search
terms, distinct from the [aliases](../docs/aliases.md) a user gives a
command. They find its row in root search exactly as its subtitle finds
it, ranked below a title match and above a scattered one. Keywords change
nothing else: the row keeps its title and subtitle, and no text is ever
sent for one.

```json
{ "id": "bin", "title": "Empty the Bin", "component": "sample_keywords.wasm",
  "mode": "no-view", "keywords": ["trash", "rubbish"] }
```

[`sample-keywords`](sample-keywords) and its `-js`/`-ts` copies do this
in Rust, JavaScript and TypeScript. The same sample's "Moons" command is a
root provider whose indexed result carries `alternateTitles` and
`keywords` of its own
([indexed results](#root-results-supplied-ahead-of-the-query)):

```ts
export const indexedResults: IndexedResults = {
  async results() {
    return [
      {
        id: "moon",
        title: "The Moon",
        subtitle: "What the keywords sample supplies",
        alternateTitles: ["Luna"],
        keywords: ["satellite", "rock"],
        action: { tag: "open", val: { target: "https://example.com/moon" } },
      },
    ];
  },
};
```

## Commands for typed addresses and paths

Root search understands a query that is a **web address** (an absolute
URL with a scheme, or a bare domain such as `github.com`, with `https://`
inferred) or a **path** (a drive letter and a separator, `\\`, `~`
resolved to the home folder, `/`, or `file://`). A command can be listed
only for such a query. Two fields say so:

- `"matches"` says what the command is matched by: `"title"` (the
  default), `"url"` or `"file-path"`.
- `"when"` says when it is listed at all: `"always"` (the default),
  `"blank"` (only while nothing is typed) or `"searching"` (only while
  something is).

A command declared for URL-like or path-like queries is never matched by
its title. It is listed only for a query that is one, in install order
below the results found by title. When the user invokes its row, Pane
sends it the parsed address or resolved path as its launch record's
[fallback text](#a-command-that-takes-a-query). See
[docs/root-search.md](../docs/root-search.md#understanding-the-typed-query)
for the grammar and where the rows sit.

Rust, as [`sample-matches`](sample-matches) does, "Hear an Address" being
a no-view command listed only for a typed web address:

```rust
async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
    match command.as_str() {
        "url" => {}
        other => return Err(format!("unknown command: {other}")),
    }
    let heard = match launch.fallback_text {
        Some(address) => format!("Heard “{address}”"),
        None => "Heard nothing".into(),
    };
    show_toast(Toast::success(heard));
    Ok(())
}
```

```json
{ "id": "url", "title": "Hear an Address", "component": "sample_matches.wasm",
  "mode": "no-view", "when": "searching", "matches": "url" }
```

JavaScript or TypeScript, as the [JavaScript](sample-matches-js) and
[TypeScript](sample-matches-ts) matches samples do:

```ts
export const command: Command = {
  async run(id, launch) {
    showToast({ title: launch.fallbackText ?? "Heard nothing" });
  },
};
```

The default extensions use it. Quicklinks' **Open in Browser** and
**Create Quicklink** are declared `"matches": "url"` (the form's link
prefilled with the address, which is what the fallback text carries), and
Files' **Open** and **Reveal in File Explorer** `"matches": "file-path"`.
An unknown `when` or `matches` is refused at install, with the reason.

## Searching inside a command

A command's own search is the designed List's search-text event (#240;
`command-search` is retired). A view whose List sets `onSearchText`
handles the search itself. Pane draws the header's search field and tells
the view its text, throttled (250 ms by default), and the view answers the
items it found ([docs/designed-tree.md](../docs/designed-tree.md), the
"standard views" section). Without `onSearchText`, Pane filters the
list's items locally with the root-search matcher.

Root search never asks the command, so nothing typed there reaches it or
its service. The search sample (sample-search, in Rust, JavaScript and
TypeScript) searches the fixture service this way. Pane's registered Files
command keeps its host-side search of the file index (#177); no command of
a package opens a search field of its own.

### Web requests

Web requests go through `wasi:http@0.3.0`'s client, which Pane links for
every command and sends from the host: `http` and `https` over HTTP/1.1,
trusting the system's certificates, with no redirects followed.

Pane bounds each request, whatever its options ask:

- 10 s to connect
- 20 s for the response head
- 10 s between two pieces of the body
- 30 s in all
- a body of at most 4 MiB
- four connections open at once per package

Past a limit the request fails with an error saying so. Any address is
allowed, this computer's and the local network's too. Settings ›
Extensions shows which packages use the network and the addresses each
tried to reach this session. The SDKs wrap it:

```rust
// Rust (no_std): pane_extension::http, the generated wasi:http bindings beside it.
let response = pane_extension::http::get(&url, &[("accept", "application/json")]).await?;
if response.status != 200 { return Err(format!("the service answered {}", response.status)); }
let found: Found = serde_json::from_slice(&response.body).map_err(|e| e.to_string())?;
```

```ts
// JS/TS: bundled into the component like any npm module.
import { get } from "@pane-app/extension/http";
const response = await get(url, { accept: "application/json" }); // throws Error("connection refused")...
const found = response.json();
```

A failure to get a response is an error whose message says why
("connection refused", "the address could not be resolved", "the host's
certificate is not trusted", "the service did not answer in time", "the
answer is larger than the 4194304 bytes Pane accepts"). Any status is a
response.

For other methods, request bodies or streaming, use the standard bindings
(`pane_extension::http::wasi::http`, or `wasi:http/types@0.3.0` and
`wasi:http/client@0.3.0` in JS, untyped). Libraries built on `wasi:http`
work; ones opening sockets themselves, or needing Node.js or browser
`fetch`, do not. Rust crates must build for `no_std` + `alloc` (the sample
uses `serde` and `serde_json` that way).

### Trying the search sample

The samples ([Rust](sample-search/src/lib.rs),
[JavaScript](sample-search-js/src/index.js),
[TypeScript](sample-search-ts/src/index.tsx)) search the fixture service,
a made-up package registry on this computer:

1. Run `cargo run -p pane-core --example fixture_service`. It uses port
   8740, the samples' default address; pass `-- --port N` for another,
   which their item "Service address" then sets.
2. Install `target/guests/packages/sample-search`.
3. Open Package search and type.

## The canvas: drawing inside a designed view

A command whose `pane.json` entry says `"mode": "designed"` opens a
designed view: a screen it describes as a tree of layout nodes and UI
components that Pane renders ([designed-tree.md](../docs/designed-tree.md)).
What a custom view drew in #21 (filled rectangles and one-line text) is
now one leaf of that tree, the **canvas**. It is a drawing surface for
what the UI components cannot show (a chart, a colour wheel, a game
board), beside ordinary controls. The "Choose a color" item of each sample
launches its package's `color` command, whose designed view draws the
picker as a canvas.

The canvas is sized by its style as any node is: a fixed `width` and
`height`, or the space the layout gives it, which the render context names
for it (`"canvases"`, by its key; a change of it is the resize event).

It takes input: the pointer's press, drag, release, enter and leave, the
wheel, a double click, the secondary button, and keys with their
modifiers (Tab, Enter and Escape stay with Pane). To assistive technology
it is one node, with a role from the widened set, a label and a value. A
control-like canvas can name the semantic handlers instead of parsing
keys: `onIncrement` (the up arrow), `onDecrement` (the down one) and
`onActivate` (Space).

Rust (the view is a type implementing
[`pane_extension::view::View`](../docs/designed-tree.md#the-canvas);
drawing operations are `Draw` values, events `CanvasEvent`s):

```rust
use core::cell::Cell;
use pane_extension::view::{CanvasEvent, Cx, Draw, Paint, View, canvas, column};
use pane_extension::Color;

struct Picker { column: Cell<i32> }

impl View for Picker {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        let x = self.column.get() as f32 * 36.;
        column().child(
            canvas()
                .key("grid")
                .width(Length::Px(288.))
                .height(Length::Px(36.))
                .role(CanvasRole::ColorWell)
                .label("Column")
                .value(format!("Column {}", self.column.get() + 1))
                .on_key(cx.value_listener(|this, key| {
                    if key == "right" { this.column.set((this.column.get() + 1).min(7)) }
                }))
                .on_pointer_down(cx.canvas_listener(|this, event| {
                    if let CanvasEvent::PointerDown { x, .. } = event {
                        this.column.set((x as i32 / 36).clamp(0, 7));
                    }
                }))
                .ops([
                    Draw::rect(x, 0., 36., 36.).filled(Paint::Exact(Color::hex("#1e88e5"))),
                ]),
        )
    }
}

// In `impl Command`: `type DesignedView = Picker;`
async fn open_designed_view(_command: String, _launch: LaunchRecord) -> Result<Picker, String> {
    Ok(Picker { column: Cell::new(0) })
}
```

JavaScript or TypeScript (a view is what
[`createView`](../docs/designed-tree.md) makes of a component; operations
and events are plain objects, every property typed):

```tsx
function Picker() {
  const [column, setColumn] = useState(0);
  return (
    <Column>
      <Canvas
        key="grid"
        width={288}
        height={36}
        role="color-well"
        label="Column"
        value={`Column ${column + 1}`}
        onKey={(key) => key === "right" && setColumn(Math.min(column + 1, 7))}
        onPointerDown={(event) => setColumn(Math.min(Math.max(Math.floor(event.x / 36), 0), 7))}
        ops={[{ op: "rect", x: column * 36, y: 0, width: 36, height: 36, fill: { raw: "#1e88e5" } }]}
      />
    </Column>
  );
}

// In the command: `openView(commandId) { return createView(Picker); }`.
```

A canvas may hold at most 20,000 drawing operations (64 KiB per text
operation). Pane shows a larger one as your error, and the view keeps its
last good tree. While the view renders, its canvases can ask Pane to
measure text (`measureText` in JS/TS, `pane_extension::view::measure_text`
in Rust), so text drawn beside shapes fits what it says. Throwing from a
listener shows the error and keeps the view; a crash closes it.

## Operations

A package can publish operations: named and versioned functions that
other extensions call through Pane, with JSON input and results. Any
command can call another package's operations. The full contract, errors
and limits are in [docs/operations.md](../docs/operations.md). The
operations samples show both sides in [Rust](sample-operations/src/lib.rs),
[JavaScript](sample-operations-js/src/index.js) and
[TypeScript](sample-operations-ts/src/index.tsx).

### Publishing an operation

Publish in `pane.json`; only listed operations are callable:

```json
"operations": [{ "id": "greet", "version": 1, "component": "sample_operations.wasm" }]
```

The component named there serves them, beside its command, like a command
computing [root results](#root-results-computed-from-the-query). In Rust
it implements `pane_extension::publish::Guest` and calls
`pane_extension::publish::export!`:

```rust
pane_extension::export!(Greeter);
pane_extension::publish::export!(Greeter);

impl pane_extension::publish::Guest for Greeter {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        // `input` and the returned text are JSON; `Err` is the operation's own error.
    }
}
```

A JavaScript or TypeScript package sets `"pane": { "operations": true }`
in its `package.json` and exports `publishedOperations` (typed as
`PublishedOperations`) with `async runOperation(operation, input)`.
Throwing is the operation's own error.

### Calling an operation

Call another package's operation with its source, the operation, the
version you were written for and JSON input:

```rust
use pane_extension::operations::call;

let result = call(source.into(), "greet".into(), 1, input) // "local:/…/sample-operations-js"
    .await
    .map_err(|error| error.explain())?; // "not-found: …", "failed: …"
```

```ts
import { call, type CallError } from "pane:extension/operations@0.2.0";

try {
  const result = await call(source, "greet", 1, JSON.stringify({ name })); // "local:/…"
} catch (error) {
  const { kind, message } = (error as { payload: CallError }).payload;
}
```

`source` is the target's identity exactly as installed: `local:` and the
absolute folder path it was installed from, which is the path Settings ›
Extensions shows after "local folder" (the samples ask for it in their
form). It can also be the id of a dependency your `pane.json` declares
([below](#dependencies-on-other-extensions)), which is how a package names
the extensions it is written for.

Pane starts the target only when it is called and never enables a
disabled one. It keeps each package's settings apart, and it refuses a
call back into a package already waiting in the same chain instead of
deadlocking.

### Capabilities

A capability is a second way to address the same calls: a named,
versioned set of operations, such as `acme:translate@1`, that any
installed package may provide and yours calls by that name, without naming
the package that serves it (ADR 0041). The full contract, routing and
errors are in [docs/operations.md](../docs/operations.md#capabilities).
The samples are a provider of `pane-samples:greet@1` in
[Rust](sample-greet/src/lib.rs), [JavaScript](sample-greet-js/src/index.js)
and [TypeScript](sample-greet-ts/src/index.ts), beside their consumers
([sample-capabilities](sample-capabilities/src/lib.rs)).

Provide a capability in `pane.json`. The component named there serves its
operations through the same export that serves published operations. Pane
passes each operation qualified by its capability
(`pane-samples:greet@1/greet`), so one component can tell the two apart:

```json
"provides": [
  { "capability": "pane-samples:greet@1", "component": "sample_greet.wasm",
    "operations": ["greet"] }
]
```

Call a capability by its name, with the operations your `pane.json`
declares under `uses`. A use may be:

- `optional`, so it is called only when some package provides the
  capability;
- narrowed with `commands` to the commands that need it;
- `"use": "all"`, to call every provider at once.

```rust
use pane_extension::capabilities::{available, call};

let answer = call("pane-samples:greet@1", "greet", input) // routed by Pane
    .await
    .map_err(|error| error.explain())?; // "not-found: …", "failed: …"
if available("pane-samples:farewell@1").is_none() {
    // No provider can serve it now; an optional use degrades gracefully.
}
```

```ts
import { available, call } from "@pane-app/extension/capabilities";

const answer = await call("pane-samples:greet@1", "greet", input);
if (available("pane-samples:farewell@1") === undefined) {
  // No provider can serve it now.
}
```

A use declared `"use": "all"` can call every provider at once, each answer
labelled with its provider's title:

```rust
use pane_extension::capabilities::call_every;

let answers = call_every("pane-samples:greet@1", "greet", input)
    .await
    .map_err(|error| error.explain())?; // one entry per provider that served
for answer in answers {
    // answer.provider, answer.title, answer.answer: Result<String, CallError>
}
```

Pane routes each call to the provider that can serve it (the first one
installed), never your own package. It refuses a call to a capability or
operation your `pane.json` does not declare, so its view of what you need
stays complete. A required use of one provider makes your commands wait
while no provider can serve it
([dependencies](../docs/dependencies.md#waiting-for-a-required-dependency));
optional uses and uses of every provider never do.

#### Providing a capability at run time

Mark a `provides` entry `"atRunTime": true` and your package provides the
capability only while its code holds a run-time provision for it. That
provision is an [owned registration](#owned-registrations) the component
makes, typically once the user has signed in. The manifest still names the
capability, so install plans and Settings work from it alone. A provision
the manifest does not declare and mark is refused.

Dropping the provision, its instance going or the package's code being
replaced withdraws the provider at once. The capability's consumers fall
back to another provider or wait for one. The registrations sample
provides `pane-samples:greet@1` this way after its "Sign in" action.

```rust
use pane_extension::registrations;

let provision = registrations::provide("acme:translate@1")?; // held until dropped
```

```ts
import { provide } from "@pane-app/extension/registrations";

const provision = provide("acme:translate@1"); // `dispose()` withdraws it
```

### Owned registrations

Anything a command registers at run time, it owns:

- a **dynamic root item** (a row of root search under one of the
  package's commands);
- a **timer**;
- a **folder watcher**;
- a **run-time provision** of a capability marked `atRunTime` (above).

Each is a handle the guest holds (`pane_extension::registrations` in Rust,
`@pane-app/extension/registrations` in JavaScript and TypeScript).
Dropping the handle undoes the registration. So does the instance that
made it going away, or the package's code being replaced by a disable, a
reload, an update, an uninstall or a pause. Nothing an extension registers
outlives its code: an author never writes cleanup code, and using a handle
of ended code is refused.

The full contract is in
[docs/generations.md](../docs/generations.md#owned-registrations). The
registrations sample, in [Rust](sample-registrations/src/lib.rs),
[JavaScript](sample-registrations-js/src/index.js) and
[TypeScript](sample-registrations-ts/src/index.ts), registers one of each.

```rust
use pane_extension::registrations::{self, Item};

let item = registrations::root_item("focus", Item::new("focus", "Focus: counting")
    .subtitle("registered at run time")
    .action(registrations::Action::new("Add one", || async {
        // repeatable: as often as the user chooses the row
        Ok(())
    })))?;
// A timer updates it; a `watcher` does the same for a folder's changes.
registrations::every(1, || async { item.update(...).ok(); Ok(()) })?;
```

```ts
import * as registrations from "@pane-app/extension/registrations";

const item = registrations.rootItem("focus", {
  id: "focus",
  title: "Focus: counting",
  actions: [{ title: "Add one", onAction: async () => {} }], // repeatable
});
registrations.every(1, async () => item.update({ ... }));
```

A dynamic item has the [item shape](../docs/list-tree.md) a list's items
have (id, title, subtitle, icon, accessories, actions) and is matched and
ranked like an indexed result. One that declares a `mode` (`"view"` or
`"no-view"`) is a **dynamic command**. Invoking it launches its command
with a launch record naming the item's id, so one component can offer a
row per workspace, or whatever it registered. Without a mode, invoking the
row runs its first action. A quick slot, an alias or a global hotkey holds
a dynamic command by its command and item id, and says so while it is not
registered.

Timers are `after` (one firing) or `every` (one every interval), from 1
second to 30 days. Each firing is a call into the component's `events`
export with the timer's tag: a guest call like any other, stopped with the
generation, with its traps counted towards pausing. Firings that fall due
while one is pending, or while the package
[waits](../docs/dependencies.md#waiting-for-a-required-dependency) for
what it needs, are coalesced into one.

A component that registers timers or watchers exports the events entry
point, and the SDK keeps the callback table, as for actions:

- in Rust, `pane_extension::registrations::export_events!(Events)`,
  importing `pane_extension::registrations::Events` beside it;
- in JavaScript and TypeScript, `"pane": { "events": true }` and
  `export const events = { handleEvent: registrations.handleEvent }`.

A watcher reports a folder's changes, coalesced for half a second, as the
paths that changed relative to the watched folder. An overflow is one
"rescan" event.

For each package Pane allows 1000 dynamic root items, 64 timers, 16
watchers and 16 provisions. One beyond is refused with the limit named.

### The activation entry point

A package whose `pane.json` declares `"activate"` has that component's
`activate` export called when its code may run and it is not waiting: at
install, enable, start, reload, update, Retry and on coming back from
waiting. It is called again when the instance that ran it is dropped while
the generation continues. Its registrations therefore exist without
waiting for the user. A trap in it is a crash, counted towards pausing.
Without it, a package's code first runs when the user asks for one of its
commands.

```json
"activate": "sample_registrations.wasm"
```

```rust
pane_extension::export!(Registrations);
pane_extension::lifecycle::export!(Registrations);

impl pane_extension::lifecycle::Guest for Registrations {
    async fn activate() {
        // register what the package registers
    }
}
```

```ts
// package.json: "pane": { "activate": true }
export const lifecycle = {
  async activate() {
    // register what the package registers
  },
};
```

### The state handoff

A component whose code is replaced, by Reload, by an update or by a
development-mode reload, can hand what it kept in memory to its new code.
It opts in by exporting `snapshot` and `restore` in the lifecycle
interface above (`pane_extension::lifecycle` in Rust,
`"pane": { "snapshot": true }` and `export const lifecycle = { … }` in
JavaScript and TypeScript), whatever its `pane.json` says.

Pane asks each idle instance for a snapshot before the old generation
ends, within one second and one megabyte, kept in memory only. The new
code's first instance restores it before anything else is asked of it, so
a user's counter, draft or running task carries on where it was.

The handoff never happens after a crash, a pause, a failure to start,
Retry, a disable followed by an enable, or a restart of Pane. The screen
that was open opens again for every package, opted in or not, with its
original launch record (only the command's root view).

The bytes are opaque and yours to version. The SDK's helpers serialise a
value (`pane_extension::state` in Rust, `@pane-app/extension/state` in
JavaScript and TypeScript; serde and JSON). A `restore` that cannot read
what an older release wrote should answer an error: the state is
discarded and the extension starts fresh, which is not a failure. In
development mode, a snapshot that was dropped, late, oversized or rejected
is reported in the package's log.

The full contract is in
[docs/generations.md](../docs/generations.md#the-state-handoff). The
handoff sample, in [Rust](sample-handoff/src/lib.rs),
[JavaScript](sample-handoff-js/src/index.js) and
[TypeScript](sample-handoff-ts/src/index.ts), keeps a counter and a draft:

```rust
impl pane_extension::lifecycle::Guest for Handoff {
    async fn activate() {} // this package declares no activation entry point

    async fn snapshot() -> Option<Vec<u8>> {
        pane_extension::state::save(&KEPT)
    }

    async fn restore(bytes: Vec<u8>) -> Result<(), String> {
        KEPT = pane_extension::state::load(&bytes)?;
        Ok(())
    }
}
```

```ts
// package.json: "pane": { "snapshot": true }
import { load, save } from "@pane-app/extension/state";

export const lifecycle = {
  async activate() {},
  async snapshot() {
    return save(kept);
  },
  async restore(bytes) {
    kept = load(bytes);
  },
};
```

### Dependencies on other extensions

A package that calls other packages' operations declares them, so that
installing it installs what it needs ([details](../docs/dependencies.md)):

```json
"dependencies": [
  {
    "id": "greeter",
    "source": "local:../sample-operations-js",
    "operations": [{ "id": "greet", "version": 1 }]
  },
  {
    "id": "rust-greeter",
    "source": "local:../sample-operations",
    "optional": true,
    "operations": [{ "id": "greet", "version": 1 }]
  }
]
```

- `id`: the name your code calls it by, `call("greeter", "greet", 1, input)`,
  in place of its identity; lowercase letters, digits and `-`.
- `source`: where it comes from, one of:
  - `local:` and its folder, relative to your package's folder (the folder
    a link to it points to) or absolute, with `/` between folders on every
    system. `\`, drive letters and `//server` shares are refused. Pane
    resolves it as it resolves an installed folder and keeps what it
    resolved to, so moving your source folder later does not change it.
  - `npm:` and an npm package name, optionally with an exact version
    (`npm:@pane-samples/greeter@0.1.0`), which Pane downloads when it is
    missing ([npm](../docs/npm.md#dependencies-from-npm)). A version pins
    it, so an installed copy of another version is a conflict Pane
    explains rather than a version your package did not ask for.
  - `git:` and a Git repository, optionally with `@` and a branch, tag or
    commit (`git:https://github.com/owner/repo@v1.0.0`), which Pane
    fetches when it is missing ([Git](../docs/git.md#dependencies-from-git)).

  A package published to npm or Git can use only `npm:` and `git:`
  sources.
- `optional` (default `false`): a required dependency is installed with
  your package when it is missing; an optional one never is. A call to an
  optional one that is not installed is `not-found` (the
  [sample](sample-dependencies/src/lib.rs) answers how to get it instead).
- `operations`: every operation you call, at the version you call. Pane
  checks them before installing anything, and a call through the id
  reaches only these.
- `platforms` (optional): the systems you need it on. Elsewhere it is
  neither installed nor checked.

The preview lists each dependency ("Requires: <title>, installed with it
from <source>", "already installed", or disabled, in which case it stays
disabled) and the optional ones. A required dependency that cannot be
installed is explained, and nothing is installed. That covers a missing
folder, a source-only package, another system, an operation it does not
publish at that version, and two packages needing different versions of
one operation. An installed dependency is never replaced by installing
another package: update it yourself.

## Native helpers

For what a WASI guest cannot do (an operating-system API, a native
library), a package can ship a **native helper**: an ordinary program
built for each operating system and processor it supports, which its
commands run through Pane. The command stays a WASI 0.3 component. Pane
runs the helper's file for the system it runs on and compiles nothing.

The contract, errors and limits are in [docs/helpers.md](../docs/helpers.md).
The helper samples are a [Rust](sample-helper/src/lib.rs), a
[JavaScript](sample-helper-js/src/index.js) and a
[TypeScript](sample-helper-ts/src/index.ts) command with their packages
([`packages/sample-helper`](packages/sample-helper/pane.json) and the
`-js` and `-ts` ones) and one helper,
[`helpers/echo`](helpers/echo/src/main.rs).

1. Write the helper as a plain program. It reads its input from standard
   input (closed after the input), writes its answer as UTF-8 text to
   standard output and exits with code 0. On failure it exits with another
   code and explains on standard error, which Pane shows. It may take
   arguments.

   It runs in its own folder of the installed copy, with Pane's
   environment, and must not leave processes behind: Pane ends the
   helper's own process when the run is cancelled or the package stops,
   not processes it started. It must be a native program. Pane refuses
   scripts (`#!`) on every system, and a Windows helper must be an `.exe`.
2. Build it for each target you support, on that system or with a cross
   toolchain, for example with Cargo:
   `cargo build --release --target aarch64-apple-darwin`. A target is
   `<os>-<arch>`: `windows`, `macos` or `linux`, then `x86_64` or
   `aarch64`. Link what it needs statically where you can, because Pane
   does not check a helper's library dependencies or minimum OS version.
3. Put the files in the package (regular files, not symbolic links) and
   declare them in `pane.json`. An `id` is lowercase letters, digits and
   dashes:

   ```json
   "helpers": [
     {
       "id": "echo",
       "targets": {
         "linux-x86_64": "helpers/linux-x86_64/pane-echo",
         "macos-aarch64": "helpers/macos-aarch64/pane-echo",
         "windows-x86_64": "helpers/windows-x86_64/pane-echo.exe"
       }
     }
   ]
   ```

   Installing checks this system's file: it must exist and be a program
   for this system (64-bit ELF on Linux, Mach-O on macOS, PE on Windows,
   for the named processor). Pane copies only that file, with mode 0755. A
   package without a file for this system still installs, and running that
   helper explains the targets it has. The preview lists each helper's
   targets. For the sample, `cargo xtask guests` builds `pane-echo` for the
   system it runs on and puts it in
   `target/guests/packages/sample-helper/helpers/<target>/`.
4. Run it from a command by its `id`, with arguments and input:

   ```rust
   use pane_extension::helpers;

   let answer = helpers::run("echo".into(), vec![], "hello".into())
       .await
       .map_err(|error| format!("{}: {}", error.kind.name(), error.message))?;
   // "not-found: …", "unavailable: Not available on Linux arm64: …",
   // "failed: helper `echo` failed (exit code 3): …", "refused: …"
   ```

   Dropping the future before it resolves cancels the run, and Pane ends
   the process. The sample's "Echo within a second" races it against
   `wasip3::clocks::monotonic_clock::wait_for`: that is the command's own
   timeout, as Pane sets none. A helper also ends when the call that
   started it returns, when the package is disabled, reloaded, updated,
   paused or uninstalled, and when Pane quits. Otherwise it runs for as
   long as its work takes (the sample's "Echo after a long wait" runs for
   40 seconds), and other extensions' calls are served meanwhile.

   In JavaScript or TypeScript, import `run` from
   `pane:extension/helpers@0.2.0` (declared in
   [`js/helpers.d.ts`](js/helpers.d.ts)). A failed run rejects with the
   error as `payload`:

   ```ts
   import { run, type HelperError } from "pane:extension/helpers@0.2.0";

   try {
     return await run("echo", [], "hello");
   } catch (error) {
     const { kind, message } = (error as { payload: HelperError }).payload;
     throw new Error(`${kind}: ${message}`);
   }
   ```

   A promise cannot be cancelled. A run the command stops awaiting (the
   samples' `Promise.race` against `waitFor`) keeps its helper until the
   Pane call returns, which ends it.

## System programs

A command may also run programs installed on the system, such as
PowerShell, winget or git
([ADR 0033](../docs/adr/0033-extensions-may-run-system-programs.md),
`pane:extension/programs` in [wit/programs.wit](../wit/programs.wit)).

Name a program by an absolute path, or by a bare name Pane finds on the
user's search path at the time of the call. On Windows the search path is
read from the registry, so a tool installed after Pane started is found.
Arguments are a list no shell reads.

- `run` waits for the program and answers its exit code, standard output
  and standard error (bytes, with text helpers).
- `spawn` answers a process to write to, read from as it writes, wait for
  and kill.

The options:

- the working folder (the user's home folder by default);
- environment changes;
- a timeout;
- a console window on Windows (none by default);
- elevated, through Windows' elevation prompt. The run then answers only
  the exit code, or `declined`; elsewhere it answers `unavailable` for
  now.

```rust
use pane_extension::programs::{self, Options};

let output = programs::run("git", &["status", "--short"], b"", Options::default().timeout(10_000))
    .await
    .map_err(|error| error.explain())?;
let changes = output.stdout_text();
```

```ts
import { run, spawn, powershell } from "@pane-app/extension/programs";

const output = await run("git", ["status", "--short"], { timeoutMs: 10_000 });
const process = await spawn("winget", ["upgrade", "--all"]);
for await (const line of process.lines()) toast.update({ style: "animated", title: line });
```

A program belongs to the call that started it. Pane ends it, and every
process it started (a Job Object on Windows, a process group elsewhere),
when the call returns or is dropped, when the package is disabled,
reloaded, updated, paused or uninstalled, and when Pane quits. What a
program leaves running when it exits runs on, in its tree, until then,
not just until the program's exit. Pane stops reading the program's output
once it exited (after two seconds for what is still in its pipes), so a
process it left holding its output does not hold up the answer. To have a
program outlive the command, open it with the system instead.

A `run` keeps at most 16 MiB of each stream; a program writing more is
ended and the run fails (`too-much-output`). Read a spawned program's
streams as it writes: one that writes much more than the command reads
waits.

A component that imports the interface is noted at install, update and
reload, and the extension list says "Runs system programs" with a row
listing the programs it ran this session. A JavaScript or TypeScript
command imports it only if its bundle uses it. The programs samples are a
[Rust](sample-programs/src/lib.rs), a
[JavaScript](sample-programs-js/src/index.js) and a
[TypeScript](sample-programs-ts/src/index.ts) command running `pane-echo`
by its bare name, which must be on the search path.

## Running what the Run dialog runs

`pane_extension::run` (`pane:extension/run`, wit/run.wit) runs a command
line as Windows' Run dialog (Win+R) reads it, with environment variables
expanded. The command line can name:

- a program by its bare name, found on the search path the registry names
  at the time of the call and in App Paths, or by its path, with
  arguments;
- a Control Panel applet, through the Control Panel program;
- a document, a folder, a network path, or a `shell:` or `ms-settings:`
  address.

An elevated run goes through Windows' own elevation prompt, which the user
may decline. The history is the Run dialog's own, in the registry, shared
in both directions: `run` records the command line that ran, `history`
lists it newest first and `delete-from-history` removes an entry from
both. This is Windows only; elsewhere every call answers `not-available`,
which is not a failure.

```rust
use pane_extension::run::{self, RunError};

run::run("notepad.exe C:\Notes\todo.txt", false)?;
run::run("regedit", true)?; // Windows asks first
let history = run::history()?; // newest first, as typed
```

A command that takes a query is the natural shape for it: the text typed
in root search is the command line, sent when the user invokes it through
its alias or as a fallback. The [Run](https://github.com/pane-app/run)
default extension does exactly that (ADR 0040), with "Run as
Administrator" and a "Run History" list beside it. The run samples are a
[Rust](sample-run/src/lib.rs), a [JavaScript](sample-run-js/src/index.js)
and a [TypeScript](sample-run-ts/src/index.ts) command answering the
same. A JavaScript or TypeScript command's package.json sets
`"pane": { "run": true }` to import the interface.

## Session and power commands

A command may also control the session, the power state and some system
settings (#255, #265, #266, ADR 0040, `pane:extension/system-commands` in
[wit/system-commands.wit](../wit/system-commands.wit)). It can:

- lock the screen, log the user out, restart, shut down, sleep or
  hibernate;
- turn the displays off or start the screen saver;
- change the volume of the default output device, or mute the
  microphones;
- open or empty the Recycle Bin;
- switch the system's appearance between light and dark, or toggle HDR;
- show the desktop, or toggle hidden files in File Explorer;
- eject the removable drives, or toggle Bluetooth.

Each function answers what it ended in, never an error: the state the
system is in now, or why nothing changed ("Restarting", "Volume 52%",
"Microphones muted", "Dark mode", "HDR on", "Ejected E:", "Hibernation is
not available on this computer: there is no hibernation file"). Show the
text in a HUD, as the System Commands default extension does. That
extension confirms the destructive ones first, with "Don't ask again",
emptying the Recycle Bin among them.

The decisions are Pane's:

- Restart and shut down force applications closed; a log out does not.
- Sleep turns the displays off on a computer that enters Modern Standby
  when they turn off, and suspends any other.
- A volume step moves the level as Windows' own volume keys do.
- The microphone toggle mutes every microphone when any is unmuted, and
  unmutes them all otherwise.
- The HDR and Bluetooth toggles turn every capable display or radio on
  when any is off, and all off when they are all on.
- An already empty Recycle Bin is a success, and ejection names each drive
  that refused, with why.

Set Volume takes its level, 0 to 100, as the command's argument. Windows
implements these functions today; other systems answer that the commands
are not available there yet.

```rust
use pane_extension::feedback::{show_hud, ToastStyle};
use pane_extension::system_commands::{self, Outcome};

match system_commands::lock_screen() {
    Outcome::Done(text) => show_hud(&text, ToastStyle::Success),
    Outcome::Explained(text) => show_hud(&text, ToastStyle::Failure),
}
```

```ts
import { lockScreen } from "@pane-app/extension/system-commands";
import { showHUD } from "@pane-app/extension/feedback";

const outcome = lockScreen();
showHUD(outcome.text, outcome.state === "done" ? "success" : "failure");
```

A JavaScript or TypeScript command imports the interface only if its
bundle uses it. The samples are a [Rust](sample-system-commands/src/lib.rs),
a [JavaScript](sample-system-commands-js/src/index.js) and a
[TypeScript](sample-system-commands-ts/src/index.ts) command, one item per
function, each saying what it answered.

## Listing the open windows

A command may also list the open windows and bring one of them to the
front, restoring it first if it is minimized (#263, ADR 0040,
`pane:extension/windows` in [wit/windows.wit](../wit/windows.wit)). The
list holds the windows Alt+Tab would show, in z-order with the front
application's window first. Each has its title, its application's name and
icon, and whether it is minimized, maximized, on another virtual desktop
or elevated.

A window's `id` is opaque and valid for the session alone. Give it back to
`activate` to switch to that window. One that closed is gone, and another
may have taken its place. This is Windows only; elsewhere every call
answers `not-available`, which is not a failure.

```rust
use pane_extension::windows;

for window in windows::list_windows()? {
    if window.title.contains("todo") {
        windows::activate(&window.id)?;
    }
}
```

```ts
import { activate, listWindows } from "pane:extension/windows@0.2.0";

for (const window of listWindows()) {
  if (window.title.includes("todo")) activate(window.id);
}
```

A JavaScript or TypeScript command's package.json sets
`"pane": { "windows": true }` to import the interface. The
[Switch Windows](https://github.com/pane-app/switch-windows) default
extension is the one that lists them (ADR 0040). The windows samples are a
[Rust](sample-switch-windows/src/lib.rs), a
[JavaScript](sample-switch-windows-js/src/index.js) and a
[TypeScript](sample-switch-windows-ts/src/index.ts) command answering the
same, one item per window, each switching to it.

## Packaging and installing a local extension

A package is a folder with a `pane.json` manifest at its root and the
built component of each command it lists. The same format serves Rust,
JavaScript and TypeScript: Pane sees only components.

### The manifest

```json
{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.2",
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

- `manifestVersion` (required): the manifest format, currently `1`. A
  newer number is refused with "a newer Pane is needed".
- `title` (required): the display title. It is not the package's
  identity.
- `version` (optional): shown before installing and after an update.
- `icon` (optional): the package's icon, shown in root search, quick
  slots, Settings and Shortcuts. It is one of:
  - a PNG or SVG image in the package, by its path (`"icon.png"`;
    `icon@dark.png` and `icon@light.png` beside it are drawn in the dark
    and light themes);
  - a light and dark pair
    (`{"light": "icon-light.png", "dark": "icon-dark.png"}`);
  - a built-in icon by name (`"star"`, reicon's names in kebab case).

  It takes an optional `tint`, `mask` and `fallback` as in
  [a list's icons](../docs/list-tree.md#icons). Without one Pane shows a
  tile with the title's first letter. An unknown built-in name or an image
  the package does not ship is refused at install, with the reason. A
  package from npm or Git without its own 512×512 icon is installed with a
  caution. Each command may have an `icon` of its own; one without shows
  its package's. A list's own images live under the package's `assets`
  folder, which Pane copies with the package.
- `apiVersion` (required): the `pane:extension` contract the components
  are built against, `MAJOR.MINOR`. This Pane provides `0.2`, and runs
  `0.1` through the host's adapter until its next release, from
  [`wit/extension.wit`](../wit/extension.wit). Before 1.0 the minor
  version must match; from 1.0, any minor version up to Pane's in the
  same major.
- `platforms` (optional): the operating systems the package supports,
  from `windows`, `macos` and `linux`. Omitted means all of them, and `[]`
  means none. On a system it does not list, Pane explains the package
  ("Not available on Linux: this package supports only Windows") instead
  of installing it. See
  [platform availability](../docs/platform-availability.md).
- `commands` (required, at least one, unless the package publishes
  `operations` or provides `provides`). Each command has:
  - `id`, unique in the package, without `#` (which Pane's records use to
    join it to the package identity);
  - `title`, and an optional `subtitle`;
  - optional `platforms`: the same list, for this command alone.
    Elsewhere its root row is listed with the reason and does not open;
  - `component`: a relative path inside the package folder (no `..`, no
    absolute path) to a built component;
  - optional `keywords`: search terms that find the command as its
    subtitle does ([below](#a-commands-keywords));
  - optional `when` and `matches`: when the command's row is listed and
    what it is matched by
    ([below](#commands-for-typed-addresses-and-paths));
  - optional `rootResults: true`: the command also computes
    [root results from the query](#root-results-computed-from-the-query);
  - optional `schedule`: [scheduled work](#scheduled-work), an interval
    and the item whose action Pane runs while the package is enabled.

  Users find a command in root search by its `title` and its `subtitle`
  (the package `title` when it has none), so put the words people will
  type there. Pane searches this metadata without running the command
  ([root search](../docs/root-search.md#matching-and-ranking)).
- `operations` (optional): the [operations](#operations) the package
  publishes; `commands` may then be empty.
- `provides` (optional): the [capabilities](#capabilities) the package
  provides, each a `capability` name, the `component` serving it and its
  `operations`, with an optional `platforms`; `commands` may then be empty
  too.
- `uses` (optional): the [capabilities](#capabilities) the package calls,
  each a `capability` name and the `operations` it calls, with an optional
  `optional`, `use`, `default` and `commands`.
- `dependencies` (optional): the other packages whose operations it calls,
  required or optional ([dependencies](#dependencies-on-other-extensions)).
- `helpers` (optional): the [native helpers](#native-helpers) the package
  ships, each an `id` and its file for each target (`"linux-x86_64":
  "helpers/linux-x86_64/tool"`).

### What Pane checks

Unknown fields are ignored. The component must exist when you install: a
package whose component is not built is refused as source-only, with the
missing path.

Pane then checks each component without running it. It must compile,
import only WASI 0.3 and export the extension interface, each function
Pane calls with the types it calls it with. For a command with
`rootResults` it must export the root results interface too.

A component built against an older shape of the version it names is
therefore refused at install, naming the first mismatch ("it was built for
an older extension API shape: rebuild it against Pane's extension API 0.1
(it has no function `render`)"). Rebuild it against the current
[`wit/extension.wit`](../wit/extension.wit) (or, for a component built
for `0.1`, the version this Pane still runs).

### Where the component comes from

That is up to your build. A standalone Rust crate can point `component`
at `target/wasm32-wasip2/release/<name>.wasm` inside the crate folder
after `cargo build --release --target wasm32-wasip2`.

A JavaScript or TypeScript package builds its component wherever its
build puts it. Development mode copies each built component into the
folder, so `pane.json` can point at `dist/<name>.wasm`.
`pane-ext dev <package dir>` builds it and hands it to Pane.

The repository's samples live in one Cargo workspace and a prebuilt
folder, so their manifests are in [`packages/`](packages), and
`cargo xtask guests` assembles each with its component into
`target/guests/packages/<name>/`.

### Installing

1. Choose **Install extension from folder…** at the end of root search.
2. Pick the package folder.
3. Check the source, version, commands and compatibility Pane shows.
4. Press Enter on **Install**.

The package's commands appear in root search, the first one selected.

From the command line,
`cargo run -p pane -- --install target/guests/packages/sample-rust` (or
`pane --install <folder>`) opens the same screen. That also helps where no
folder picker is available. On Linux the picker is the desktop portal
(`xdg-desktop-portal`), and without one Pane shows why it could not open
it.

### What installing does

- The package is identified by its folder's absolute path as the
  operating system resolves it (`std::fs::canonicalize`). Symbolic links
  and `..` are followed. On file systems that ignore letter case or
  Unicode normalization (the defaults on Windows and macOS) the stored
  spelling is used, so two spellings of one folder are one package. Pane
  does no case folding or normalization of its own, so on a
  case-sensitive Linux file system `Hello` and `hello` are two packages.
  On Windows the `\\?\` prefix is dropped. A folder path that is not valid
  Unicode is refused. Moving or renaming the folder makes it a different
  package.
- `pane.json` and the listed components (nothing else) are copied into
  Pane's data folder, under `extensions/packages/<n>/`, and recorded in
  `extensions/installed.json`. Your folder is never written, and the
  installed copy keeps working if the folder changes or is deleted. The
  data folder is `%LOCALAPPDATA%\Pane\data` on Windows,
  `~/Library/Application Support/Pane` on macOS and `$XDG_DATA_HOME/pane`
  (default `~/.local/share/pane`) on Linux; `PANE_DATA_DIR` overrides it.
- Installing a folder that is already installed is refused. Choosing it
  again shows **Update** instead, which replaces the installed copy with
  the folder's current contents under the same identity, whatever its new
  title or version. Two different folders are two packages, even with
  identical contents, and nothing is merged or switched between them.
- Installing or updating also installs the missing
  [required dependencies](#dependencies-on-other-extensions) the manifest
  declares, first, or explains why it cannot and installs nothing.
- The [capabilities](#capabilities) the package uses are listed in the
  preview, each with who provides it. The `default` provider a use names
  is installed with the package when no installed package provides the
  capability. It is planned, installed and rolled back as a required
  dependency is, and the install stops with the reason when it cannot be
  installed or does not provide the capability. A capability nobody
  provides stops nothing: the package waits for a provider
  ([details](../docs/dependencies.md#installing)).
- Installed commands are listed from the manifests alone. No guest runs
  until you open a command or another extension calls one of the
  package's [operations](#operations). A damaged installed copy stays
  listed with its problem.

### Disabling and uninstalling

Pane's Settings window lists every installed package under its sidebar's
**Extensions** group, each with a page of its own (root search's **Manage
Extensions** opens it there). The page shows whether the package is
enabled and its source, so copies with the same title can be told apart,
and its switch disables or enables it. Only that installation changes. A
single command can be turned off with its own switch on the page, which
leaves the rest of the package working.

A disabled package's commands leave root search (they are not shown
greyed out), an open command of it closes, its running instances are
dropped and it can no longer save settings, so none of its code runs. This
happens as soon as you switch it, before the choice is written. If the
choice cannot be written, the package is enabled again with the reason.
Switching again while the choice is being written does nothing.

The choice is recorded in `installed.json` (`"disabled": true`) and holds
after restarting Pane and after an Update. Its settings are kept, and
enabling it brings its commands back with them. The package stays
installed at the same identity; choosing its folder again shows it as
disabled.

**Uninstall** in the Actions menu of the package's page in Settings asks
first, showing how many settings and content records the package keeps.
It offers **Uninstall and keep saved data**, **Uninstall and delete saved
data** or **Cancel**. Either way Pane removes the installed copy, the
package's cache and its local credentials, without running it, and leaves
its source folder and anything outside Pane's data folder alone. Kept
settings and content stay with the source identity: installing the same
folder again finds them, and another folder never does
([details](../docs/extension-data.md#uninstalling-an-extension)).

### Reloading a package while Pane stays open

After rebuilding a component, reload the package instead of restarting
Pane. On the package's page in Settings (its sidebar's **Extensions**
group), the Actions menu of every enabled package from a folder has
**Reload**. Choosing it reads the package's source folder again and
replaces only that package. Pane and every other package keep running,
including a designed view of another package that is open. It works the
same for Rust, JavaScript and TypeScript packages, since Pane sees only
components.

A reload goes through two stages, and a failure in each is reported
differently:

1. Checks. The folder is checked exactly as an install checks it
   (manifest, built components, WASI 0.3 imports, the extension API
   shape), without running anything. If that fails, nothing is replaced:
   the package keeps running the code installed before, and the status
   says "Dev was not reloaded: <reason>. It keeps running its installed
   code."
2. Start. Otherwise the new copy replaces the installed one and the old
   instances are stopped (an open command, form or custom view of the
   package closes; root search then selects its command). Then the new
   code starts: Pane starts each of the package's commands available on
   this system and asks it for its view (`render`). Success shows
   "Reloaded Dev".

   If a command fails to initialize (it traps, or its component cannot
   load or be instantiated), its instances are stopped again and the
   package is reported as failed to start. An error the command returns
   from `render` itself, such as asking the user to sign in first, is an
   ordinary answer and not a failure to start.

   On a failure to start, the package's row says "Failed to start" and a
   **Retry starting <title>** row appears under its Reload row with the
   diagnostics (for a trap, the guest backtrace). Pane also writes them to
   its standard error. The earlier code is not restored. Retry starts the
   same code again; to fix it, rebuild and reload.

What a reload keeps and what it does not:

- Settings are kept. They belong to the package identity, so the new code
  reads what the old code saved ([Keeping settings](#keeping-settings)),
  including anything saved by a start that then failed. Nothing is
  migrated or undone.
- Nothing live is carried over. The old instance's memory, an open view's
  state and a running call are not transferred to the new code, and there
  is no API for an extension to hand transient state to its replacement:
  save what must survive in settings. An answer from the old code that
  arrives after the reload (for example a command that was opening) is not
  shown.
- A disabled package has no Reload row and is not reloaded; enable it
  first.
- Reload is also what [development mode](#developing-a-package-build-and-reload-on-save)
  does after each save that builds. The Update in the install screen still
  replaces the copy too, without the start stage.

### Developing a package: build and reload on save

Instead of rebuilding and pressing Reload after each change, choose
**Develop** in the Actions menu of the package's page in Settings (one
per enabled package). Pane then watches the package's source folder.
After each save it runs the package's build there and reloads the package
when the build succeeds:

- A folder with `Cargo.toml` is built with
  `cargo build --release --target wasm32-wasip2` (with cargo's JSON
  messages, which say where it built the component), so `pane.json` names
  its component under `target/wasm32-wasip2/release/`. Pane takes the file
  of that name cargo built this time, even with another target folder.
- A folder with `package.json` is built with Pane's own JavaScript build
  (`npm ci --ignore-scripts && tsc -p tsconfig.json && esbuild --bundle &&
  componentize`) for each component `pane.json` names, such as
  `dist/<name>.wasm`. It needs Node.js and npm alone. The componentizer it
  uses depends on the Pane:
  - a `pane-ext` or Pane from a checkout componentizes with its own linked
    componentizer;
  - an installed Pane uses the componentizer of the package's own
    installed `@pane-app/cli` platform package
    (`node_modules/@pane-app/cli-<target>`, which `npm install` provides;
    a package without one is explained), or the folder
    `PANE_COMPONENTIZER` names.

Each build puts the components in a staging folder under Pane's data
folder, and runs with Pane's environment (less what `cargo run` set for
Pane itself). Once development is on, any write to the folder, such as
`git pull` or an autosave, runs the build, `build.rs` included.

A build that fails replaces nothing. The command keeps running its
installed code, the status line shows the first error, and **Why <title>
did not build** shows the end of the build's output and the path of a log
file with all of it.

A build that succeeds is reloaded from its staging folder as **Reload
<title>** does, and its components are then copied where `pane.json`
names them. That includes a failure to start, which pauses the package
with Retry and does not restore the earlier code; the next save that
builds recovers it.

Saving again while a build runs makes that build obsolete: it is never
reloaded, and the folder is built again (after three in a row, Pane waits
for the next save). The package's [extension log](#printing-and-logging)
is kept in `extension.log` beside the build's log while it is developed.

**Stop developing <title>**, disabling or uninstalling the package, or
quitting Pane ends development and kills a running build with the
processes it started. Only that installation is affected: a copy of the
package installed from another folder keeps its own code.

The `hello-rust`, `hello-js` and `hello-ts` samples are ready to try.
[Development mode](../docs/development-mode.md) has the steps, what is
watched and the limits.

## Publishing a package to npm

A Pane package can be published to npm, so that users install it by name
with **Install extension from npm…** (or `pane --install npm:<name>`),
without Node.js or npm ([details](../docs/npm.md)). Pane installs the
package's tarball exactly as it installs a folder and runs nothing else in
it, so publish what is built.

1. Build first. Build every component `pane.json` names, and each native
   helper for every target you support, before packing. Pane never runs
   npm install scripts (`preinstall`, `install`, `postinstall`,
   `prepare`…) and never installs your npm `dependencies`, so a component
   bundling a library must have it built in. A package published without
   its components is explained to users as source-only.
2. Add a `package.json` beside `pane.json`, whose `files` lists
   `pane.json` and what it names, and nothing Pane would not use:

   ```json
   {
     "name": "@your-scope/your-extension",
     "version": "1.0.0",
     "description": "…",
     "license": "…",
     "keywords": ["pane-extension"],
     "files": ["pane.json", "dist/command.wasm", "helpers/"]
   }
   ```

   Its `name` is the package's identity in Pane (every version is the same
   package), and each version you publish is what users install by
   `name@version` or as the latest. Keep `pane.json` at the package's
   root. Its dependencies on other Pane packages are `npm:` sources.
3. Check the tarball with `npm pack --dry-run`, which lists what users
   will download. Helpers keep their files; Pane sets their mode itself.
   Symbolic links, and names some system reads differently or cannot write
   (`\ : < > " | ? *`, a name ending in `.` or a space, `con`, `nul`,
   `com1`…, on every system alike), make Pane refuse the whole tarball.
4. Try it before publishing. `npm pack` makes the `.tgz`. Serve it from a
   registry on this computer and point a development build of Pane at it
   with `PANE_NPM_REGISTRY=http://127.0.0.1:<port>/`
   ([`scripts/npm_registry.py`](../scripts/npm_registry.py) serves the
   tarballs of a folder), then install it by name.
5. Publish with `npm publish` (`--access public` for a scoped name). Pane
   needs the registry's sha512 integrity, which npm gives every version it
   publishes.

The sample [`npm/greeter`](npm/greeter) is such a package, kept
`"private": true` so that it is never published. `cargo xtask guests`
assembles and packs it on each contributor system, without npm. `npm pack`
in the assembled `target/guests/npm/greeter` makes the same list of files,
and its tarball installs alike.

## Publishing a package from a Git repository

A Pane package can also be installed from a public Git repository, with
**Install extension from Git…** (or `pane --install git:<address>`),
without Git, a compiler or any other tool on the user's computer
([details](../docs/git.md)). Pane fetches the one revision the user names
over HTTPS and installs its files exactly as it installs a folder. It
builds nothing and runs nothing from the repository (no hooks, no
submodules, no Git LFS). So what users install must be a **release
revision**: a commit whose tree holds the built components.

1. Keep `pane.json` at the repository's root, naming each component where
   the release revision will hold it, such as `dist/command.wasm`. A
   revision without them (your development branch, if you keep build
   outputs out of it) is explained to users as source-only.
2. Build, then commit the build on a release reference. For example:

   ```sh
   cargo build --release --target wasm32-wasip2
   mkdir -p dist && cp target/wasm32-wasip2/release/command.wasm dist/
   git switch -c release            # or: git switch release
   git add -f dist                  # -f if dist/ is in .gitignore
   git commit -m "Release 1.0.0"
   git tag v1.0.0
   git push origin release v1.0.0
   ```

   Commit every native helper for each target you support, too. Files are
   installed without their execute bit, and Pane sets a helper's mode
   itself.
3. Tell users what to name: `https://<host>/<owner>/<repo>@v1.0.0` (a tag:
   pinned to it) or `…@release` (a branch: tracked, each update fetching
   its newest commit). Without a reference, Pane installs the repository's
   default branch, which is only right if that branch holds the build.
4. Keep the tree portable. Symbolic links, submodules, a `.git` entry, two
   names in a folder that differ only in case, and names some system reads
   differently or cannot write (`\ : < > " | ? *`, a name ending in `.` or
   a space, `con`, `nul`, `com1`…) make Pane refuse the revision on every
   system.
5. Try it before pushing. Serve the repository on this computer
   ([`scripts/repository_server.py`](../scripts/repository_server.py)
   `serve <folder> <port-file>`) and name
   `http://127.0.0.1:<port>/<repo>.git@<reference>` in a development build
   of Pane, which alone accepts a plain `http://` address on a loopback
   address.

The repository's identity is its host and path (`github.com/owner/repo`),
whatever the reference: moving it to another host or path makes another
package. Its dependencies on other Pane packages are `npm:` or `git:`
sources. The sample [`git/greeter`](git/greeter) is such a package's
source.

Uninstalling is not implemented yet.

## Known limits of local packages

These are the known limits of local packages so far:

- An update is not coordinated with a command that is open. The replaced
  copy's code is dropped and its pending calls stop, so an open command of
  the package loses its state.
- Disabling, reloading or updating a package **stops its pending calls**
  (see [generations](../docs/generations.md)). A call waiting inside the
  command at an `await` (a clock, an operation, any async import) ends
  there: nothing after that `await` runs, the instance is dropped and the
  answer is never shown. A call that had not started is not started.

  The settings samples' "Save after waiting" shows it: it saves
  "started", waits ten seconds, then saves "finished", which a disable or
  reload meanwhile prevents. So save what must survive before awaiting,
  and do not count on code after an `await` running.

  A command computing without awaiting is interrupted too, at Pane's next
  epoch tick (10 ms). One that computes for 5 seconds in all without
  finishing is stopped as unresponsive, which counts towards pausing its
  package like a crash
  (#18, [pausing](../docs/pausing.md#when-an-extension-stops-responding)).
  Waiting does not count, but awaiting does not reset the count either, so
  split long work into calls. A native helper has no time limit of Pane's
  own; the command's own timeout (dropping the run) ends it.
- Background services, timers and hotkeys are not part of the extension
  API yet and come with their own tickets. Disabling does not yet consider
  packages that depend on the disabled one (#43).

## What this folder holds

### The SDKs

- `pane-extension`: the Rust SDK, bindings for the contract, published on
  crates.io under that name ([its README](pane-extension/README.md)). Its
  version follows the extension API. It is `no_std`, so only WASI 0.3 is
  imported. It supplies the allocator, a panic handler that logs the panic
  and traps, print and log macros
  ([printing and logging](#printing-and-logging)), `cabi_realloc` and
  `memcmp`/`bcmp` (which string comparisons need).
- `js`: `@pane-app/extension`, the JS/TS SDK, published on npm under that
  name ([its README](js/README.md)). It holds TypeScript declarations for
  the contract (`pane.d.ts`), the adapter that turns thrown values into
  answers, the HTTP helper, the host-function wrappers and the WIT world
  JS/TS commands are built against. It carries no CLI.

`cargo xtask sdks` checks that both SDKs package as they would be
published (`cargo publish --dry-run`, `npm pack`), publishing nothing;
`cargo xtask ci-lints` runs it. Publishing them is a person's step.

### Default extensions live in their own repositories

Pane's default extensions are not in this folder. Five run on every
system: the Calculator, Applications, Quicklinks, Files and Clipboard
History. Three are Windows-only: Run, System Commands and Switch Windows.
Each lives in its own repository under the `pane-app` organization
(`https://github.com/pane-app/<id>`).

A Pane release installs them from the release commits this build pins
([`crates/pane/defaults.json`](../crates/pane/defaults.json); see
[ADR 0045](../docs/adr/0045-official-extensions-live-in-their-own-repositories.md)).
The Windows three's pins name `"platform": "windows"`, so only a Windows
first setup acquires them.

To work on one, clone its repository. Its own CI builds the component,
and `cargo xtask guests` here builds the samples only. A default
extension's repository builds against the `pane-extension` crate on
crates.io, with no Pane checkout:
`cargo build --release --target wasm32-wasip2`. To try it against an SDK
change not published yet, add a `[patch.crates-io]` entry for
`pane-extension` pointing at this checkout's `guests/pane-extension`,
without committing it.

A release commits the built component under the path its `pane.json`
names on its release branch and tags `v<version>`, as its repository's own
workflow does. To try a default without building it, install it from its
repository: `pane --install git:https://github.com/pane-app/calculator@v0.6.4`.

### The samples

The samples are small extensions that exercise the whole contract in Rust,
JavaScript and TypeScript. The tests install them as fixtures and the
smokes drive them.

None of the samples is built into Pane or acquired as a default
extension. Install one by hand with
`pane --install target/guests/packages/<name>` (from a checkout,
`cargo run -p pane -- --install target/guests/packages/sample-rust`), into
a data folder of your own with `PANE_DATA_DIR` if you like. The tests and
smokes that drive the samples register or install them themselves.

- `sample-rust`, `sample-js`, `sample-ts`: the same sample command in
  Rust, JavaScript and TypeScript. All three show the same items, the same
  form and the same color picker, compute the same root result ("reverse
  <text>"), and give the same answers and errors. The contract tests in
  `crates/pane-core/tests/samples.rs` and `crates/pane/tests/window.rs`
  hold each of them to that.
- `sample-settings`, `sample-settings-js`, `sample-settings-ts`: the same
  command in Rust, JavaScript and TypeScript, which keeps a chosen
  greeting style in Pane's settings ([Keeping settings](#keeping-settings))
  and one value of each other kind of data
  ([content, cache and credentials](#keeping-content-cache-and-credentials)).
  They are the fixtures for disabling and re-enabling a package and for
  clearing its cache, held alike by `crates/pane-core/tests/disable.rs`
  and `crates/pane-core/tests/clear_cache.rs`.
  - Their **Crash** item crashes on purpose: three crashes within five
    minutes pause the package until Retry
    ([pausing](../docs/pausing.md), `crates/pane-core/tests/pausing.rs`).
  - Their **Stop responding** item computes without waiting until Pane
    stops it (`crates/pane-core/tests/unresponsive.rs`).
- `sample-files`, `sample-files-js`, `sample-files-ts`: the same host
  import and `open-file` results in Rust, JavaScript and TypeScript, the
  same contract the Files default extension holds, listing a typed
  folder's entries too
  ([the entries of a folder the user typed](#the-entries-of-a-folder-the-user-typed)).
  Held by `crates/pane-core/tests/file_actions.rs` and
  `crates/pane-core/tests/typed_folders.rs`.
- `sample-clipboard-js`, `sample-clipboard-ts`: the Clipboard History
  command in JavaScript and TypeScript, over the same host import
  ([Clipboard history](#clipboard-history)). Held by
  `crates/pane-core/tests/clipboard.rs`.
- `sample-helper`, `sample-helper-js`, `sample-helper-ts`: a command in
  Rust, JavaScript and TypeScript running a [native helper](#native-helpers)
  its package ships, `helpers/echo`. That is `pane-echo`, an ordinary
  program `cargo xtask guests` builds for the system it runs on and puts
  in all three packages. The samples cover its answer, cancelling it, a
  failing and an undeclared helper, and a slow run that disabling or
  reloading stops. Their packages are `packages/sample-helper`,
  `packages/sample-helper-js` and `packages/sample-helper-ts`, held alike
  by `crates/pane-core/tests/helpers.rs`. The helper sample is no default
  extension (it was one in development builds until #162). A Pane that
  acquired it before keeps it as an ordinary installed package, which its
  page in Settings uninstalls.
- `sample-applications-js`, `sample-applications-ts`: the same host
  import and indexed results in JavaScript and TypeScript. They supply
  "Launch <name>" for each installed application, also found as "Launch
  <alternate title>" and by the application's keywords, and a command
  listing and opening them
  ([Root results supplied ahead of the query](#root-results-supplied-ahead-of-the-query)).
  Held by `crates/pane-core/tests/applications.rs`,
  `application_identity.rs` and `application_names.rs`.
- `sample-operations`, `sample-operations-js`, `sample-operations-ts`:
  each package publishes the operation `greet` and has a command that
  calls another's, Rust calling JavaScript and TypeScript and they calling
  Rust ([Operations](#operations)). Held by
  `crates/pane-core/tests/operations.rs`.
- `sample-dependencies`: a Rust package declaring the JavaScript
  operations sample as a required dependency and the Rust one as an
  optional one, and calling each by its dependency id. Installing it
  installs the JavaScript sample too
  ([Dependencies](#dependencies-on-other-extensions)). Held by
  `crates/pane-core/tests/dependencies.rs`.
- `sample-view`, `sample-view-js`, `sample-view-ts`: the designed view
  sample in Rust, JavaScript and TypeScript (#235, #237, #238, #243). It
  has three commands:
  - a counter whose screen the extension describes as a tree Pane
    renders: a column of a text and a row of buttons;
  - `components`, a gallery of every component of the UI component set,
    from the layout primitives and the shared controls to Markdown, the
    tokens and the raw values. Its fields are live and keyed: typing edits
    at once, the view echoes the value back, "Clear" sets it and "Reorder"
    moves the fields with their state;
  - `loading`, data arriving drawn the moment it lands (#243): a loading
    state, then what the view loaded the moment it arrived, with no timer.
    The arrival asks for the drawing itself.

  They are written with the Rust builder, the JSX runtime's elements and
  JSX itself (`"mode": "designed"` in their `pane.json`,
  [designed-tree.md](../docs/designed-tree.md)). They are held to the same
  three-language parity by `crates/pane-core/tests/designed_views.rs`,
  and by `crates/pane-core/tests/view_push.rs` for the loading command,
  and drawn by the window tests of `crates/pane/tests/designed_views.rs`
  and `crates/pane/tests/designed_components.rs`.
- `sample-nav`, `sample-nav-js`, `sample-nav-ts`: the navigation sample in
  Rust, JavaScript and TypeScript (#239), a designed view with a stack
  Pane owns. Rows push a detail view that pushes deeper, the detail pops
  with a result the rows answer through `onPop`, and the deeper view drops
  the whole stack through the `window.pop-to-root` host function. It uses
  the SDKs' `push`, `replace`, `pop`, the Raycast-style `Action.Push` and
  `navigationTitle`. Held by `crates/pane-core/tests/navigation_stack.rs`
  and the window tests of `crates/pane/tests/navigation_stack.rs`.
- `sample-timer`, `sample-timer-js`, `sample-timer-ts`: the timer sample
  in Rust, JavaScript and TypeScript (#236), a designed view that shows
  its loading state, then advances an elapsed-seconds counter once a
  second through `refresh-after-ms` (`useInterval` in JavaScript and
  TypeScript, `.refresh_after` in Rust). The loading state's arrival asks
  for the drawing that replaces it itself (#243), so it is drawn the
  moment the data lands. Held by `crates/pane-core/tests/view_refresh.rs`.
- `sample-icons`, `sample-icons-js`, `sample-icons-ts`: the icons sample
  in Rust, JavaScript and TypeScript (#139). Its rows have a built-in
  icon, a packaged image with `@light` and `@dark` variants, a light and
  dark pair, a tinted icon, a masked image, an image that fails and draws
  its fallback, the SDK's avatar and progress ring, and every accessory
  (text, a relative date, a tag, an icon alone) with tooltips. One row has
  five accessories, of which a row draws three
  ([icons and accessories](../docs/list-tree.md#icons)).

  Their packages, `packages/sample-icons` and its `-js`/`-ts` copies, have
  an icon of their own (`icon.png`, 512×512) and their "Icons" command
  another (`command.svg`). Their second command has none, so it shows the
  package's. `packages/sample-icons-plain` and its `-js`/`-ts` copies run
  the same components with no icon, so they show a first-letter tile.
  Held alike by `crates/pane-core/tests/icons.rs`, and the Rust ones by
  `crates/pane/tests/icons.rs`.

  The same list also has the icons Pane loads for it (#142): the SDK's
  favicon of a site, a web image the test server holds back (and a second
  row naming it, downloaded once), a web image the server does not have,
  and the SDK's file icon of a file and of an application. The images
  come from the server the `imageServer` setting names (by default
  `http://127.0.0.1:8741`), the files from the `iconFile` (by default `~`)
  and `iconApplication` (by default Windows' Notepad) settings. Held by
  `crates/pane-core/tests/web_icons.rs` and
  `crates/pane/tests/web_icons.rs`, whose image server sets them.
- `npm/greeter`: `@pane-samples/greeter`, the npm-distributed sample. It
  is an npm package holding a `pane.json` and one JavaScript component,
  `sample_npm_js.wasm` (from `sample-npm-js`, prebuilt like the other
  JavaScript samples). Its command answers "Hello from the npm package"
  and its `greet` operation answers "Hello, <name>, from the npm package",
  so it is plain which copy runs. `"private": true` keeps `npm publish`
  from publishing it. `cargo xtask guests` assembles it in
  `target/guests/npm/greeter/` and packs it as `npm pack` does into
  `target/guests/npm/pane-samples-greeter-0.1.0.tgz`
  ([Publishing a package to npm](#publishing-a-package-to-npm)).
  `packages/sample-dependencies-npm` is the dependencies sample's
  component requiring it as `npm:@pane-samples/greeter`. Held by
  `crates/pane-core/tests/npm.rs` and `crates/pane/tests/npm.rs`, from a
  local registry.
- `git/greeter`: the Git-distributed sample, a Rust package's **source**
  (`pane.json` naming `dist/git_greeter.wasm`, `Cargo.toml`, `src/lib.rs`
  and a README of the author's steps). Its command answers "Hello from the
  Git repository" and its `greet` operation answers "Hello, <name>, from
  the Git repository". `cargo xtask guests` builds it and assembles it in
  `target/guests/git/greeter/` with its component under `dist/`, as a
  release revision holds it. The tests and smokes make its repository
  with `git`: the source alone on `main` (a source-only revision) and the
  build on the branch `release`, tagged `v0.1.0`
  ([Publishing a package from a Git repository](#publishing-a-package-from-a-git-repository)).
  Held by `crates/pane-core/tests/repositories.rs` and
  `crates/pane/tests/repositories.rs`, from a repository served on
  127.0.0.1.
- `sample-query`, `sample-query-js`, `sample-query-ts`: Echo, the smallest
  command that takes a query, in Rust, JavaScript and TypeScript. It is a
  no-view command that answers the text the user sends it from root
  search through its alias or as a fallback
  ([A command that takes a query](#a-command-that-takes-a-query),
  [aliases and fallbacks](../docs/aliases.md)). "fail" is refused and
  "crash" crashes on purpose. Their packages are `packages/sample-query`,
  `packages/sample-query-js` and `packages/sample-query-ts`. Held alike by
  `crates/pane-core/tests/aliases.rs`, and the Rust one by
  `crates/pane/tests/aliases.rs`.
- `sample-keywords`, `sample-keywords-js`, `sample-keywords-ts`: the
  keywords sample in Rust, JavaScript and TypeScript, one component
  serving two commands ([A command's keywords](#a-commands-keywords)):
  - "Empty the Bin" (`"keywords": ["trash", "rubbish"]`), a no-view
    command found by its keywords rather than its title;
  - the "Moons" root provider (`"indexedResults": true`), which supplies
    "The Moon", found by its alternate title ("Luna") and its keywords
    ("satellite", "rock") as a command's title and subtitle are found.

  Their packages are `packages/sample-keywords`,
  `packages/sample-keywords-js` and `packages/sample-keywords-ts`. Held
  alike by `crates/pane-core/tests/keywords.rs`.
- `sample-matches`, `sample-matches-js`, `sample-matches-ts`: the
  `when`/`matches` sample in Rust, JavaScript and TypeScript, with four
  no-view commands
  ([Commands for typed addresses and paths](#commands-for-typed-addresses-and-paths)):
  - "Hear an Address" (`"matches": "url"`) and "Hear a Path"
    (`"matches": "file-path"`) are listed only for a typed web address or
    path and answer what it parsed or resolved;
  - "Blank Only" (`"when": "blank"`) is listed only while nothing is
    typed;
  - "Searching Only" (`"when": "searching"`) only while something is.

  Their packages are `packages/sample-matches`,
  `packages/sample-matches-js` and `packages/sample-matches-ts`. Held alike
  by `crates/pane-core/tests/typed_queries.rs`.
- `sample-no-view`, `sample-no-view-js`, `sample-no-view-ts`: the no-view
  sample in Rust, JavaScript and TypeScript, one component serving five
  commands
  ([No-view commands and the launch record](#no-view-commands-and-the-launch-record)):
  - "Report launch" answers its launch record ("fail" answers an error,
    "crash" crashes);
  - "Tick" runs every minute on its own schedule in the background;
  - "Last launches" answers what those two last ran with;
  - "Launch" launches the command its text names with context;
  - "Show launch", a view command, lists its launch record.

  Their packages are `packages/sample-no-view` and its `-js`/`-ts` copies.
  Held alike by `crates/pane-core/tests/no_view.rs`, and the Rust one by
  `crates/pane/tests/no_view.rs`.
- `sample-preferences`, `sample-preferences-js`, `sample-preferences-ts`:
  the preferences sample in Rust, JavaScript and TypeScript
  ([Preferences and the Setup screen](#preferences-and-the-setup-screen)).
  Its package declares an API key (a password, required), units (a
  dropdown, required, with a default), a greeting (text) and "Verbose" (a
  checkbox), and ships a `HELP.md`. Its commands:
  - "Show preferences", a view command, adds a notes folder (required), a
    notes file and an editor (an application) and lists every value it
    receives;
  - "Report preferences", a no-view command, adds "Loud" (a checkbox) and
    shows its values in a toast;
  - "Tick" runs every minute once the package is set up, and "Last tick"
    toasts how often.

  Their packages are `packages/sample-preferences` and its `-js`/`-ts`
  copies. Held alike by `crates/pane-core/tests/preferences.rs`, and the
  Rust one by `crates/pane/tests/preferences.rs`.
- `sample-schedule`, `sample-schedule-js`, `sample-schedule-ts`: the
  schedule sample in Rust, JavaScript and TypeScript. Its Counting command
  declares a `schedule`, so Pane runs its "Run now" item every 60 seconds
  while the package is enabled, without the user asking
  ([Scheduled work](#scheduled-work)). For Pane's checks, its "Run slowly"
  item waits ten seconds, so a disable or reload while it runs stops it;
  "Answer an error" answers an error; and "Crash" traps. Their packages
  are `packages/sample-schedule` and its `-js`/`-ts` copies. Held alike by
  `crates/pane-core/tests/schedules.rs`.
- `sample-actions`, `sample-actions-js`, `sample-actions-ts`: the actions
  sample in Rust, JavaScript and TypeScript (#137). It has:
  - items with several actions in sections, a destructive one, shortcuts
    for every system and one per system, one shortcut that is Pane's own
    Ctrl+K and one that collides once the user gives a Pane key
    Ctrl+Shift+Y, an item with one action and one with none
    ([several actions per item](../docs/list-tree.md));
  - an item with submenus (#140): one given at once, one given when it
    opens and one whose opening fails;
  - (#141) a "Window" item closing the window each way, popping to root
    search and clearing the search; a "Feedback" item showing HUDs, a
    toast updated from animated to success with Open and Retry actions, a
    failure, and the command's row subtitle; and the no-view commands
    "Window functions", "Spin" and "Stumble"
    ([what a command does after it acts](#what-a-command-does-after-it-acts));
  - (#146) a "Confirm" item asking before it acts (a destructive "Delete"
    remembered with "Don't ask again", "Ask" with its own buttons, "Close
    and Ask" while the launcher is hidden, "Ask in the Background") and
    the no-view "Confirm Run";
  - (#145) a "System" item calling each system function and a "Standard
    actions" item.

  Their packages are `packages/sample-actions` and its `-js`/`-ts` copies.
  Held alike by `crates/pane-core/tests/item_actions.rs`, `submenus.rs`,
  `feedback.rs`, `confirmations.rs` and `system.rs`, and the Rust one by
  `crates/pane/tests/item_actions.rs`, `submenus.rs`, `feedback.rs`,
  `confirmations.rs` and `system.rs`.
- `sample-arguments`, `sample-arguments-js`, `sample-arguments-ts`: the
  arguments sample in Rust, JavaScript and TypeScript (#144), one
  component serving three no-view commands ([Arguments](#arguments)):
  - "Greet" asks for a required name, an optional secret (a password) and
    a tone (a dropdown), and toasts what it was given (only the secret's
    length);
  - "Stamp" asks for a required label, for its hotkey and quick slot;
  - "Relay" launches the command its text names with the arguments it
    lists (`background stamp label=x`), or toasts what "Stamp" last kept
    (`last`).

  Their packages are `packages/sample-arguments` and its `-js`/`-ts`
  copies. Held alike by `crates/pane-core/tests/arguments.rs`, and the
  Rust one by `crates/pane/tests/arguments.rs`.
- `hello-rust`, `hello-js`, `hello-ts`: one "Say hello" command each, a
  package built in its own folder, as an author's would be, for
  [development mode](../docs/development-mode.md). Pane builds and reloads
  it after each save
  ([Developing a package](#developing-a-package-build-and-reload-on-save)).
  Held by `crates/pane-core/tests/develop_builds.rs`.

### Packages and prebuilt components

- `packages`: the samples' package manifests (`pane.json`).
  `cargo xtask guests` puts each one with its built component in
  `target/guests/packages/<name>/`, a ready-to-install package. The icons
  sample's packages hold the tile icons its rows draw (`icon.png`, the
  package's, which a command without an icon of its own shows, and
  `command.svg`, its "Icons" command's), held by
  `crates/pane/tests/default_icons.rs`. The default extensions' own
  packages, and the tile icons they ship, live in their repositories.
- `prebuilt`: the JS and TS sample components (both samples in each
  language), committed so that tests and installing a sample need no
  JavaScript toolchain, with `manifest.json` recording their hashes and
  build inputs.

### Test fixtures

- `fixtures/folder-files`: what Files was before #175, over the folder the
  user grants its package ([Files of a granted folder](#files-of-a-granted-folder)),
  keeping that capability covered. Held by
  `crates/pane-core/tests/files.rs`, which writes its `pane.json`.
- `fixtures/faulty`: a test fixture whose actions, form and root results
  return an error or trap, whose actions grow its memory to just under the
  128 MiB cap or past it, and whose counting designed view (its `counter`
  command) refuses, traps and draws a canvas over Pane's limits.
- `fixtures/failing-start`: a test fixture that builds and installs but
  traps the first time it is asked for its view (after saving a setting),
  so a reload to it fails to start and Retry then starts it.
- `fixtures/refusing-view`: a test fixture whose view is always refused
  with an error it returns, which a reload must not report as a failure to
  start.
- `fixtures/operations`: a test fixture installed as several packages to
  drive each way an operation call can fail, cycles and the depth limit.
- `fixtures/mixed-p2`: a negative control that imports WASI 0.2 and must
  be rejected.
- `fixtures/old-api`: a negative control built against extension API 0.1
  as it was before `item` gained `platforms` and custom views, with its
  own copy of that WIT. Pane's type check refuses it at install and when
  it loads.
- `fixtures/mismatched-api`: a negative control whose exports all have the
  names Pane looks for while `form-error` lacks one field, so only the
  type check can refuse it.
- `fixtures/previous-api`: a positive fixture built against the previous
  extension API version from the shared `wit/previous` tree
  ([ADR 0046](../docs/adr/0046-every-breaking-change-to-the-extension-api-gets-a-new-version.md)).
  Its package under `packages/previous-api` names the version it was built
  for, and Pane runs it through the host's adapter while the window lasts.
- `fixtures/trees`: a test fixture whose list tree and answers are JSON
  written by hand, not by `pane-extension`, with fields Pane does not
  know, a newer version, a view Pane cannot show and trees and answers it
  cannot read ([list-tree.md](../docs/list-tree.md)).
- `fixtures/alerts`: a test fixture that rings sounds through
  `pane:extension/alerts` and keeps their handles, then ends its code
  each way it can end (a trap, an endless computation, a stopped root
  search), so the tests check that each way stops the sound
  ([ADR 0053](../docs/adr/0053-extensions-get-the-users-attention-through-a-notification-a-ringing-sound-and-the-launcher.md)).
  It generates the alerts bindings itself from the SDK's WIT.
- `fixtures/designed`: a test fixture whose designed view's trees and
  answers are JSON written by hand, not by `pane-extension`. It has an
  unknown node with and without a fallback, a newer minor and another
  major version, a tree over the node limit, an error, and text that is
  not JSON. Its answers navigate by hand (a push, a pop with a result and
  a replace), drawing the pop events it receives
  ([designed-tree.md](../docs/designed-tree.md)).
