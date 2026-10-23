# Building and running Pane

Pane has no published installer yet: no release is out, and the artifact source that would serve Pane's own updates is not deployed. So you build it from source. The same commands work on Windows, macOS and Linux.

## Prerequisites

- [rustup](https://rustup.rs). Run `rustup toolchain install` once in the repository to install the toolchain pinned in `rust-toolchain.toml` (Rust 1.98.1 with the `wasm32-wasip2` target, rustfmt and clippy).
- Windows: Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC and a Windows SDK).
- macOS: the Xcode Command Line Tools (`xcode-select --install`).
- Linux (Debian/Ubuntu names): `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libxcb1-dev libfontconfig1-dev libfreetype-dev libvulkan1` and a Vulkan driver (`mesa-vulkan-drivers` provides a software fallback).

## Commands

From the repository root:

```sh
cargo xtask guests   # build the Rust guests; copy them and the prebuilt JS/TS samples into target/guests/
cargo run -p pane    # open the launcher window
cargo xtask ci       # build guests, check the prebuilt JS/TS samples, then formatting, lints and tests
cargo xtask package-linux    # build Pane's Linux package and the artifacts of its own updates into target/dist/
cargo xtask package-windows  # build the Windows package the same way (on Windows)
cargo xtask package-macos    # build the macOS package the same way (on macOS)
```

[installer.md](installer.md) explains the packages and how to install them.

You don't need a JavaScript toolchain for any of these, because the JS and TS sample components are committed prebuilt in `guests/prebuilt/`. Rebuilding them from source with `cargo xtask js-guests` also needs Node.js 22+ with npm, and nothing else: no Python, nightly Rust or wasi-sdk (see [the author guide](../guests/README.md#writing-a-javascript-or-typescript-command)). JavaScript and TypeScript run on a pinned, patched componentize-qjs (QuickJS), which is still provisional.

## First setup and updates

On its first start, Pane lists the official extensions with the defaults ticked. Once you've chosen, it fetches them with its own Git client, from the commits this release pins. A development build takes its pins from `PANE_DEFAULTS` when that names a pins file of its own; the tests and smokes do this, pointing at repositories served on the same computer. Otherwise it uses the committed pins ([trying a first setup by hand](installer.md#trying-a-first-setup-by-hand)). A release build always uses the committed pins.

Pane checks for its own updates when it starts. A development build checks only where `PANE_ARTIFACTS` names an artifact source on this computer, and otherwise doesn't check at all. A release build reads Pane's published downloads.

## Using the window

Arrow keys select, Enter opens a command or runs an item, and Escape goes back. Clicking a row runs it too. An open command's view starts with its content, and the footer's left side shows the command's icon and title.

Pane registers no sample command of its own. Root search lists the commands of installed packages (the default extensions, once they're set up), Pane's install rows and Settings…. **Install extension from folder…** installs a local extension package; see [packaging and installing a local extension](../guests/README.md#packaging-and-installing-a-local-extension).

Extensions are managed in Settings, and **Manage Extensions** in root search opens it there. Each installed package has a page under the sidebar's Extensions group. Its switch disables or enables it, and a disabled package keeps its settings across restarts. Its **Reload** replaces its code with its source folder's current build while Pane stays open ([reloading a package](../guests/README.md#reloading-a-package-while-pane-stays-open)).

## Trying the samples

The Rust, JavaScript and TypeScript samples aren't built into Pane or acquired as default extensions, so you install them by hand. To skip the folder picker, open the launcher on a package's install screen and press Enter to install it:

```sh
cargo run -p pane -- --install target/guests/packages/sample-rust   # pane --install <folder>
cargo run -p pane -- --install target/guests/packages/sample-js     # the JavaScript sample
cargo run -p pane -- --install target/guests/packages/sample-ts     # the TypeScript sample
```

Every sample under `target/guests/packages/` (assembled by `cargo xtask guests`) installs this way.

In each sample command, "Greet someone" opens a form: type a name, Tab to the greeting, use the arrow keys to choose, and press Enter to submit. "Choose a color", after it, opens a color picker the extension draws itself; the arrow keys, Home and End, or a click or drag on the swatches choose a color. The last two items are declared for some operating systems only. On another system they stay listed with the reason and don't run.

## Where Pane keeps its data

Installed packages go in Pane's data folder:

- Windows: `%LOCALAPPDATA%\Pane\data`
- macOS: `~/Library/Application Support/Pane`
- Linux: `$XDG_DATA_HOME/pane` or `~/.local/share/pane`

Set `PANE_DATA_DIR` to another folder to keep test installs out of it. The GUI smoke scripts use `<output-dir>/data`.

## Repository layout

- `wit/extension.wit`: the host/guest contract for one extension command: a list view, item actions, [forms](forms.md) and [designed views](designed-tree.md) (whose canvas is what a custom view was).
- `crates/pane-core`: the launcher model (the public host interface the tests drive), extension packages (manifest, identity, managed copies) and the extension runtime, a Wasmtime 49.0.1 engine registering only WASI 0.3.
- `crates/pane`: the GPUI CE window, with [root search](root-search.md) as its first screen, and the Settings window.
- `crates/pane-build`: how a package is built from its source folder, once or after each save. Development mode and `pane-ext` both build with it.
- `crates/pane-ext`: `pane-ext`, the command-line tool beside the app. `pane-ext dev` builds a package in the terminal and hands each build to the running Pane ([development mode](development-mode.md#from-the-terminal-pane-ext-dev)).
- `crates/pane-target`: the operating-system and processor names shared by the core, `xtask` and native helpers.
- `xtask/`: the `cargo xtask` build, CI and packaging commands above.
- `guests/`: extension guests, including [the Rust, JavaScript and TypeScript sample commands](../guests/README.md), TypeScript declarations for the contract, the prebuilt JS/TS components and test fixtures.
- `packages/`: the `@pane-app/cli` and `@pane-app/create` npm packages and the extension template.
- `tools/componentize-js`: the JS/TS toolchain, pinned upstream componentize-qjs plus Pane's patch queue, and its build script.
- `scripts/smoke-*`: native GUI smoke runs used by CI, which uploads their screenshots.

Contributors on all three operating systems have working build, run and test workflows ([the requirement](https://github.com/pane-app/pane/issues/1#cross-platform-contributor-requirement)), and CI builds and tests on all three. Per-platform evidence is in [platforms/](platforms/). Research results and prototype source are under [research/](research/); they aren't production support guarantees, and research scripts may depend on the temporary toolchains and local paths documented beside them.
