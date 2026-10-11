# @pane-app/cli-linux-aarch64

Pane's command-line tool for extension authors, for Linux arm64: the
`pane-ext` program `@pane-app/cli`'s shim runs, with the componentizer
a JavaScript or TypeScript package's build uses — `componentize-qjs-p3`,
Pane's patched componentize-qjs building WASI 0.3 components, the QuickJS
`runtime.wasm` and the WASI 0.3 `libc.so` it links into every component.
An installed Pane's development mode also spawns this package's
componentizer for a package that installed `@pane-app/cli`.

This is one of `@pane-app/cli`'s `optionalDependencies`, at its version:
npm installs it only on matching systems (that is the `os` and `cpu`
above), and `@pane-app/cli`'s shim runs the `pane-ext` program it holds.
Installing this package on its own is not the way to get the tool —
install `@pane-app/cli`.

## Versions and publishing

The platform packages release in lockstep with `@pane-app/cli`, at the
same version. The repository's `cli-packages` workflow builds the
programs for every platform and packs the tarballs (`cargo xtask
cli-package` assembles this folder); publishing them is a person's step,
never CI's.

## Licensing

- `pane-ext` and the package files: GPL-3.0-or-later (`LICENSE-GPL`).
- `componentize-qjs-p3`: Apache-2.0 — upstream componentize-qjs with
  Pane's patches (`LICENSE-componentize-qjs`), built with the repository's
  Rust.
- `runtime.wasm`: the QuickJS runtime (MIT) via rquickjs, built for
  `wasm32-wasip3` against wasi-sdk.
- `libc.so`: wasi-libc from wasi-sdk 34 (Apache-2.0 WITH LLVM-exception OR
  Apache-2.0 OR MIT; some parts BSD or CC0).
