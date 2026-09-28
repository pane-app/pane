# Extension guests

Extensions are WebAssembly components implementing the `pane:extension`
contract in [`wit/extension.wit`](../wit/extension.wit). Pane registers only
WASI 0.3 interfaces; a component that imports WASI 0.2 (for example through
Rust's standard library on `wasm32-wasip2`) is rejected with an explanation.

- `pane-guest`: Rust bindings for the contract. `no_std`, so only WASI 0.3 is
  imported; it supplies the allocator, a trapping panic handler and
  `cabi_realloc`.
- `sample-rust`, `sample-js`, `sample-ts`: the same sample command in Rust,
  JavaScript and TypeScript. All three show the same items and give the same
  answers and errors; the contract tests in `crates/pane-core/tests/samples.rs`
  and `crates/pane/tests/window.rs` hold each of them to that.
- `js`: `@pane/extension`, TypeScript declarations for the contract
  (`pane.d.ts`) and the WIT world JS/TS commands are built against.
- `prebuilt`: the JS and TS sample components, committed so that tests and
  `cargo run -p pane` need no JavaScript toolchain, with `manifest.json`
  recording their hashes and build inputs.
- `fixtures/faulty`: test fixture whose actions return an error or trap.
- `fixtures/mixed-p2`: negative control that imports WASI 0.2 and must be rejected.

## Writing a Rust command

The [sample](sample-rust/src/lib.rs) is the complete example. A command is a
`cdylib` crate depending on `pane-guest` that implements two async functions:

```rust
#![no_std]

use pane_guest::alloc::{string::String, vec};
use pane_guest::{Guest, Item, View};

struct Hello;
pane_guest::export!(Hello);

impl Guest for Hello {
    async fn get_view() -> Result<View, String> {
        Ok(View {
            title: "Hello".into(),
            items: vec![Item { id: "hi".into(), title: "Say hi".into(), subtitle: None }],
        })
    }

    async fn run_action(_item_id: String) -> Result<String, String> {
        Ok("hi!".into())
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
`sample_js.wasm` and `sample_ts.wasm` to use different builds. Installing
arbitrary extensions is not supported yet.

Toolchain used: Rust 1.98.1, `wit-bindgen` 0.62.0, `wasip3` 0.9.0+wasi-0.3.0;
host Wasmtime and wasmtime-wasi 49.0.1.

## Writing a JavaScript or TypeScript command

The [JavaScript](sample-js/src/index.js) and
[TypeScript](sample-ts/src/index.ts) samples are complete examples. A command
is an npm package whose `main` module exports `command` with two async
functions. Pane's types come from `@pane/extension` (a `file:../js`
development dependency); they describe plain values, not engine objects:

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
