# __TITLE__

A Pane extension in Rust, started from the `list` template.

Its command opens a list of items, and each item runs an action when
chosen. `Say hello` shows a toast, as every item's action tells the user
what it did. The list is `render` in `src/lib.rs`. Edit it and save, and
Pane rebuilds and reloads the command while it keeps running.

## Trying it

You need Rust installed through rustup. `rustup` alone is enough, because
the toolchain file picks the rest. Then run:

```
pane-ext dev
```

It builds the package with `cargo build --release --target wasm32-wasip2`
and hands the build to the running Pane, starting one if none is running.
After each save it builds the package again and has Pane reload it, until
Ctrl+C. Pane's development-mode documentation describes what happens when
a save does not build.

Two more commands:

- `pane-ext check` reports what Pane would refuse at install.
- `pane-ext pack` builds the release components and assembles what the
  package's users will download.

## What is here

- `pane.json`: the package's manifest, with its title, its icon and its
  commands. An editor checks it against the schema `"$schema"` names,
  which ships inside the [`@pane-app/extension`] SDK and on Pane's own
  page. Pane itself ignores fields it does not know.
- `src/lib.rs`: the command, written with Pane's Rust SDK
  ([`pane-extension`]). One component (the WebAssembly file `pane.json`
  names) serves every command the package declares. `render` and `run`
  receive the command's id, so the file matches on it. Add a command with
  `pane-ext new command .`.
- `Cargo.toml`: the package and its one dependency, the SDK from
  crates.io. Inside a Pane checkout, point `pane-extension` at the
  repository's own copy (`path = "../../guests/pane-extension"`) to build
  against the SDK as it is there, with changes not yet published.
- `rust-toolchain.toml`: stable Rust, `rustfmt` and `clippy`, and the
  `wasm32-wasip2` target the component builds for.
- `rustfmt.toml`: the formatter's settings, matching Pane's own.
- `icon.png`: the package's placeholder icon, 512×512. Replace it with
  your own.
- `.gitignore`: keeps `target/`, the build's output, out of the
  repository.

[`pane-extension`]: https://crates.io/crates/pane-extension
[`@pane-app/extension`]: https://www.npmjs.com/package/@pane-app/extension
