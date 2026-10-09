# The vendored componentize-qjs

This tree is upstream
[componentize-qjs](https://github.com/andreiltd/componentize-qjs) at the
commit [`tools/componentize-js/pins.json`](../pins.json) names, with Pane's
patch queue from [`tools/componentize-js/patches`](../patches) applied **in
this source** rather than with `git apply` (which
[`pane_js.py`](../pane_js.py) used to do to a fresh download). It is licensed
as upstream is ([`LICENSE`](LICENSE), Apache-2.0), and the patches modify it
under that same license.

Pane links it in-process: `crates/core` (`componentize-qjs`) is a member of
the repository's workspace, built with the repository's stable Rust, and
`pane-build`'s `componentizer` feature (`crates/pane-build`) calls
`componentize_qjs::componentize` with the committed wasm parts
([`../wasm-parts`](../wasm-parts)). `crates/runtime`
(`componentize-qjs-runtime`) builds the QuickJS runtime for `wasm32-wasip3`
with the pinned nightly Rust and wasi-sdk — only
[`pane_js.py wasm-parts`](../pane_js.py) builds it, so the crate is excluded
from the workspace and nothing in Pane compiles it. `cargo xtask guests`
builds `crates/core`'s `p3_build` example and puts it, with the wasm parts,
in `target/guests/componentizer/`, the binary a development build spawns
when it does not link the componentizer.

## What differs from upstream

The patch queue, applied in the source here, each with its rationale kept
as comments beside the code it changed:

- `0001-wasip3-runtime-port`: the runtime links wasi-sdk 34's P3 libc
  instead of the WASI preview1 adapter (see `crates/runtime/src/abi.rs`,
  `crates/core/src/lib.rs`), and the componentizer links that `libc.so`
  into every component.
- `0002-reseed-snapshot-random-state`: reseeds `Math.random` and
  `performance` per instance after the snapshot
  (`crates/runtime/src/instance_init.rs`).
- `0004-task-context-stack-pointer`: generates wit-dylib's adapters with
  the stack pointer in task context slot 0
  (`crates/core/src/lib.rs`, `DylibOpts`).
- `0005-native-output`: the runtime's native `__paneWrite` for stdout and
  stderr (`crates/runtime/src/output.rs`).

And this tree's own changes, beyond the queue:

- **Wasmtime 49.0.1** (upstream pinned 47), matching the version Pane's own
  runtime uses, so no second Wasmtime tree is built. That retires the
  queue's `0003-stub-async-host-imports.patch`: Wasmtime 49's
  `define_unknown_imports_as_traps` stubs an `async func` import itself
  (see the comment at its call in `crates/core/src/lib.rs`).
- **`ComponentizeOpts::libc`** passes the wasm32-wasip3 libc as bytes
  instead of only the process-global `QJS_P3_LIBC` environment variable,
  which the in-process callers prefer; `QJS_P3_LIBC` still works for the
  standalone `p3_build`.
- **The build script** (`crates/core/build.rs`) neither downloads prebuilt
  runtimes nor builds them: Pane always passes the runtime explicitly
  (`Runtime::Custom` with the committed `wasm-parts/runtime.wasm`), so the
  built-in variants are empty constants and the tree carries no network
  access.
- Upstream's CLI crate, napi bindings, npm package and tests are not
  vendored: Pane's builds call the library, and its own componentizer entry
  point is `crates/core/examples/p3_build.rs`.

## Updating

1. Change the commit and archive digest in [`pins.json`](../pins.json), and
   rebase the queue in `../patches` onto the new source; apply it to this
   tree, keeping each patch's rationale as comments.
2. Rebuild the wasm parts and commit them:
   `python tools/componentize-js/pane_js.py wasm-parts
   tools/componentize-js/wasm-parts` (which also rewrites `wasm-parts.json`),
   then `cargo xtask js-guests` and commit the rebuilt `guests/prebuilt/`.
3. `cargo xtask ci` — and expect [`componentizer.yml`](../../
   .github/workflows/componentizer.yml) to build the componentizer for all
   six platforms on the next push to `main`.
