# componentize-js: Pane's JS/TS toolchain

Builds JavaScript and TypeScript commands into WASI 0.3-only components. It is
upstream [componentize-qjs](https://github.com/andreiltd/componentize-qjs) at
a pinned commit plus a small patch queue, not a fork. Authoring and the
prerequisites are described in [guests/README.md](../../guests/README.md);
this page is for maintaining the toolchain.

- [`pins.json`](pins.json): the componentize-qjs commit and archive digest,
  the patch order, the Rust nightly and the wasi-sdk release with per-host
  archive digests. (The x86_64 Linux and Windows digests were checked against
  downloads; the others are GitHub's recorded asset digests.) The
  componentizer is built with the stable toolchain in `rust-toolchain.toml`.
- [`patches/`](patches): applied in order with `git apply`.
  `0001-wasip3-runtime-port.patch` builds the runtime for `wasm32-wasip3`
  against wasi-sdk 34's P3 libc; `0002-reseed-snapshot-random-state.patch`
  reseeds `Math.random` and `performance` in each instance after the
  snapshot. Why each is needed, and the evidence, is in
  [the backend validation](../../docs/research/js-backend-validation/README.md).
  `0003-stub-async-host-imports.patch` lets a world import an `async func`
  of the embedding host (Pane's `pane:extension/operations`): the pinned
  Wasmtime 47 stubs unknown imports for the snapshot with sync functions,
  which an async import refuses. Wasmtime 49 does this itself, so moving
  componentize-qjs to it would retire the patch.
  `0004-task-context-stack-pointer.patch` generates wit-dylib's adapters
  with the stack pointer in task context slot 0, where the runtime and the
  P3 libc keep it: with the default global, an adapter's frame overwrote
  the runtime's own once an async export had resumed.
- [`p3_build.rs`](p3_build.rs): the componentizer entry point, compiled as an
  example of the patched crate.
- [`package.json`](package.json) / `package-lock.json`: esbuild 0.28.2 and
  TypeScript 7.0.2.
- [`bundle.mjs`](bundle.mjs) and [`pane_js.py`](pane_js.py): the build.

## Every platform

`pane-ext` will ship the componentizer for Windows, macOS and Linux on x64
and arm64 (spec [#128](https://github.com/pane-app/pane/issues/128)), so
[`componentizer.yml`](../../.github/workflows/componentizer.yml) builds it
for all six whenever this folder changes, or on demand. The wasm parts are
built once, on Linux: `pane_js.py wasm-parts` builds `runtime.wasm` with the
pinned nightly and takes wasi-sdk's WASI 0.3 `libc.so`, recording their
digests in `wasm-parts.json`. Then each platform runs
`pane_js.py componentizer` on its own runner, which checks those digests and
builds `p3_build` against them with the stable Rust alone (no nightly, no
wasi-sdk). It uploads the binary as `componentizer-<target>`, componentizes
the TypeScript sample with it (`PANE_JS_PREBUILT`), and runs `pane-core`'s
TypeScript sample checks and package-install tests on the result.

The first run (37840978895, 2026-10-08) passed on all six with nothing
platform-specific in the build. Each runner image already has rustup, the
MSVC or Xcode linker, Python and Node. Building `p3_build` took 5–10 minutes
cold (20 on `macos-15-intel`), and the sample componentized in under a second
(1.5 seconds on `macos-15-intel`). Wasmtime,
which the componentizer runs to snapshot the runtime, works on Windows
arm64 (`windows-11-arm`) as on the other five. macOS x64 runs on
`macos-15-intel`.

## Updating

1. Change the commit and archive digest in `pins.json` (and the nightly or
   wasi-sdk pins, if needed).
2. Rebase the patches onto the new source and regenerate them with
   `git diff`; keep each patch's header.
3. Run `cargo xtask js-guests`, then `cargo xtask ci`, and commit the patches,
   pins and the rebuilt `guests/prebuilt/`.

Any change to these files or to the sample sources makes
`pane_js.py check` (run by CI) report the prebuilt components as stale until
they are rebuilt. The upstream candidates in the validation report (the
per-instance reseed, the context-slot fix and a `wasm32-wasip3` build option)
and a Wasmtime upgrade would each shrink this queue if accepted upstream.

## Licensing

componentize-qjs is licensed under the Apache License 2.0
([copy](patches/LICENSE-componentize-qjs)); the patches modify it and are
distributed under the same license. The QuickJS runtime they build is linked
into every JS/TS component, alongside wasi-libc from wasi-sdk (Apache-2.0 WITH
LLVM-exception and other permissive licenses). Pane's own files here are
licensed under [Apache-2.0](../../guests/LICENSE-APACHE) OR
[MIT](../../guests/LICENSE-MIT), like `guests/`.
