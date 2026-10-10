# @pane-app/cli

Pane's command-line tool for extension authors: `pane-ext`, which creates,
develops, checks and packs an extension package from the terminal, beside
the Pane app. `pane-ext dev` builds a package here (cargo's and tsc's and
esbuild's errors appear here) and hands each build to the running Pane;
`pane-ext check` reports everything Pane would refuse at install, with
Pane's own messages; `pane-ext pack` builds the release components and
assembles what users download. `pane-ext new` is to follow.

This package holds only a small JavaScript shim (as esbuild, biome and
turbo ship their tools). The `pane-ext` program itself is native, so each
system gets it from one of this package's `optionalDependencies` —
`@pane-app/cli-<target>`, one per platform: `cli-linux-x86_64`,
`cli-linux-aarch64`, `cli-windows-x86_64`, `cli-windows-aarch64`,
`cli-macos-x86_64`, `cli-macos-aarch64`. npm installs the one whose `os`
and `cpu` match, and the shim runs the program it holds. If no platform
package matches your system, or npm skipped one after a failed install,
the shim says so and points at the fix (`npm install @pane-app/cli
--force`).

A JavaScript or TypeScript package needs nothing but Node.js and npm:
`pane-ext` carries the componentizer (Pane's patched componentize-qjs,
building WASI 0.3 components), the QuickJS `runtime.wasm` and the WASI 0.3
`libc.so` in the platform package, and takes esbuild as an ordinary
dependency here. A Rust package needs rustup's stable toolchain with the
`wasm32-wasip2` target and the `pane-extension` SDK from crates.io.

## Installing

```sh
npm install --save-dev @pane-app/cli
npx pane-ext dev          # or a script: "dev": "pane-ext dev"
```

The platform packages release in lockstep with this one, at the same
version, so an install always matches.

## Versions and publishing

The version follows the extension API and the tool together. The packages
are built and packed by the repository's `cli-packages` workflow (run
`cargo xtask cli-package` to assemble them); publishing them is a person's
step, never CI's.

## Licensing

Pane's own files here are GPL-3.0-or-later (see `LICENSE-GPL`), like the
`pane-ext` program the platform packages carry. The platform packages also
carry the componentizer, which is Apache-2.0, and the wasm parts, whose
licences their README records.
