# __TITLE__: a Pane extension in Rust, started from the `no-view` template.

A command that runs without opening a screen: `run` in `src/lib.rs` does
its work and tells the user what happened with a toast, the way a
no-view command does. Root search sends it text through its alias or as
a fallback, which `run` answers in the toast. Edit it and save, and Pane
rebuilds and reloads the command while it keeps running.

## What is here

- `pane.json` — the package's manifest: its title, its icon and its
  commands, here one whose `"mode"` is `"no-view"` and which
  `takesQuery`. An editor checks it against the schema `"$schema"` names,
  which ships inside the [`@pane-app/extension`] SDK and on Pane's own
  page; Pane itself ignores fields it does not know.
- `src/lib.rs` — the command, written with Pane's Rust SDK
  ([`pane-extension`]). One component (the WebAssembly file `pane.json`
  names) serves every command the package declares: `render` and `run`
  receive the command's id, so the file matches on it. Add a command with
  `pane-ext new command .`.
- `Cargo.toml` — the package and its one dependency, the SDK from
  crates.io. Inside a Pane checkout, point `pane-extension` at the
  repository's own copy (`path = "../../guests/pane-extension"`) until the
  SDK is published.
- `rust-toolchain.toml` — stable Rust, `rustfmt` and `clippy`, and the
  `wasm32-wasip2` target the component builds for.
- `rustfmt.toml` — the formatter's settings, matching Pane's own.
- `icon.png` — the package's placeholder icon, 512×512; replace it with
  your own.
- `.gitignore` — keeps `target/`, the build's output, out of the
  repository.

## Trying it

With Rust installed through rustup (`rustup` alone is enough: the
toolchain file picks the rest):

```
pane-ext dev
```

builds the package with `cargo build --release --target wasm32-wasip2`,
hands the build to the running Pane (starting one if none is running),
then builds it again after each save and has Pane reload it, until
Ctrl+C. Pane's development-mode documentation describes what happens when
a save does not build.

`pane-ext check` reports what Pane would refuse at install, and
`pane-ext pack` builds the release components and assembles what its
users will download.

[`pane-extension`]: https://crates.io/crates/pane-extension
[`@pane-app/extension`]: https://www.npmjs.com/package/@pane-app/extension
