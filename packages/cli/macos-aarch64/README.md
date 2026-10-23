# @pane-app/cli-macos-aarch64

Pane's command-line tool for extension authors, for macOS arm64. To get
the tool, install `@pane-app/cli`: this package is not meant to be
installed on its own.

## What it holds

- `pane-ext`, the program `@pane-app/cli`'s shim runs.
- The componentizer a JavaScript or TypeScript package's build uses:
  - `componentize-qjs-p3`, Pane's patched componentize-qjs, which builds
    WASI 0.3 components
  - the QuickJS `runtime.wasm`
  - the WASI 0.3 `libc.so` it links into every component

An installed Pane's development mode also spawns this package's
componentizer for a package that installed `@pane-app/cli`.

## How npm installs it

This is one of `@pane-app/cli`'s `optionalDependencies`, at its version.
npm installs it only on matching systems (the `os` and `cpu` in this
package's `package.json`), and `@pane-app/cli`'s shim runs the `pane-ext`
program it holds.

## Versions and publishing

The platform packages release in lockstep with `@pane-app/cli`, at the
same version. The repository's `cli-packages` workflow builds the
programs for every platform and packs the tarballs (`cargo xtask
cli-package` assembles this folder). Publishing them is a person's step,
never CI's.

## Licensing

- `pane-ext` and the package files: GPL-3.0-or-later (`LICENSE-GPL`).
- `componentize-qjs-p3`: Apache-2.0. It is upstream componentize-qjs with
  Pane's patches (`LICENSE-componentize-qjs`), built with the
  repository's Rust.
- `runtime.wasm`: the QuickJS runtime (MIT) via rquickjs, built for
  `wasm32-wasip3` against wasi-sdk.
- `libc.so`: wasi-libc from wasi-sdk 34 (Apache-2.0 WITH LLVM-exception OR
  Apache-2.0 OR MIT; some parts BSD or CC0).
