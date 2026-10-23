# @pane-app/cli

`pane-ext` is Pane's command-line tool for extension authors. It creates,
develops, checks and packs an extension package from the terminal, beside
the Pane app.

## Installing

```sh
npm install --save-dev @pane-app/cli
npx pane-ext dev          # or a script: "dev": "pane-ext dev"
```

The platform packages release in lockstep with this one, at the same
version, so an install always matches.

## Commands

- `pane-ext new` writes a package from a template.
- `pane-ext dev` builds a package in the terminal, so cargo's, tsc's and
  esbuild's errors appear there, and hands each build to the running Pane.
- `pane-ext check` reports everything Pane would refuse at install, with
  Pane's own messages.
- `pane-ext pack` builds the release components and assembles what users
  download.

## What you need

A JavaScript or TypeScript package needs nothing but Node.js and npm.
`pane-ext` carries the componentizer (Pane's patched componentize-qjs,
which builds WASI 0.3 components), the QuickJS `runtime.wasm` and the
WASI 0.3 `libc.so` in the platform package. It takes esbuild as an
ordinary dependency of this package.

A Rust package needs rustup's stable toolchain with the `wasm32-wasip2`
target, and the `pane-extension` SDK from crates.io.

## How the program reaches your system

This package holds only a small JavaScript shim, the way esbuild, biome
and turbo ship their tools. The `pane-ext` program itself is native. Each
system gets it from one of this package's `optionalDependencies`,
`@pane-app/cli-<target>`, one per platform:

- `cli-linux-x86_64`
- `cli-linux-aarch64`
- `cli-windows-x86_64`
- `cli-windows-aarch64`
- `cli-macos-x86_64`
- `cli-macos-aarch64`

npm installs the one whose `os` and `cpu` match, and the shim runs the
program it holds.

If no platform package matches your system, or npm skipped one after a
failed install, the shim says so and points at the fix:

```sh
npm install @pane-app/cli --force
```

## Versions and publishing

The version follows the extension API and the tool together.

The repository's `cli-packages` workflow builds and packs the packages
(run `cargo xtask cli-package` to assemble them). Publishing them is a
person's step, never CI's.

## Licensing

Pane's own files here are GPL-3.0-or-later (see `LICENSE-GPL`), like the
`pane-ext` program the platform packages carry. The platform packages
also carry the componentizer, which is Apache-2.0, and the wasm parts,
whose licences their README records.
