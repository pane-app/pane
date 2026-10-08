# Pane

A small, extensible desktop launcher for Windows, macOS and Linux, inspired by Raycast and Pi.

Pane is in early development. It is a native GPUI CE launcher with root search, a Settings window and a global hotkey, running Rust, JavaScript and TypeScript extensions through a WASI 0.3 component interface, installing local, npm and Git extension packages, and, on Linux, Windows and macOS, one package a clean machine installs whose first setup downloads Pane's default extensions itself ([installer](docs/installer.md)); the artifact source it downloads from is not deployed yet, so no published installer exists. JavaScript and TypeScript run on a pinned, patched componentize-qjs (QuickJS), which remains provisional.

## Build, run and test

The same commands work on Windows, macOS and Linux.

Prerequisites:

- [rustup](https://rustup.rs). Run `rustup toolchain install` once in the repository to install the toolchain pinned in `rust-toolchain.toml` (Rust 1.98.1 with the `wasm32-wasip2` target, rustfmt and clippy).
- **Windows:** Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC and a Windows SDK).
- **macOS:** Xcode Command Line Tools (`xcode-select --install`).
- **Linux (Debian/Ubuntu names):** `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libxcb1-dev libfontconfig1-dev libfreetype-dev libvulkan1` and a Vulkan driver (`mesa-vulkan-drivers` provides a software fallback).

Commands, from the repository root:

```sh
cargo xtask guests   # build the Rust guests; copy them and the prebuilt JS/TS samples into target/guests/
cargo run -p pane    # open the launcher window
cargo xtask ci       # build guests, check the prebuilt JS/TS samples, then formatting, lints and tests
cargo xtask package-linux  # build Pane's Linux package and the default extensions' artifacts into target/dist/
cargo xtask package-windows  # build the Windows package the same way (on Windows; see docs/installer.md)
cargo xtask package-macos  # build the macOS package the same way (on macOS; see docs/installer.md)
```

A development build of `pane` acquires the default extensions only where
`PANE_ARTIFACTS` names an artifact source on this computer
([installer](docs/installer.md#trying-a-first-setup-by-hand)); a release
build acquires them from Pane's published downloads.

No JavaScript toolchain is needed for these: the JS and TS sample components are committed prebuilt in `guests/prebuilt/`. Rebuilding them from source with `cargo xtask js-guests` also needs Python 3.12+, git and Node.js 22+ on any of the three OSes; see [guests/README.md](guests/README.md#writing-a-javascript-or-typescript-command).

In the window, use the arrow keys to select, Enter to open a command or run an item, and Escape to go back; clicking a row runs it too. An open command's view starts with its content, with no heading line above it; the footer's left shows the command's icon and title. Pane registers no sample command of its own: root search lists the commands of installed packages (the default extensions, once acquired), Pane's install rows and Settings…. To try the Rust, JavaScript and TypeScript samples, install them by hand (below). In each sample command, "Greet someone" opens a form: type a name, Tab to the greeting, arrow keys to choose, Enter to submit. "Choose a color", after it, opens a color picker the extension draws itself: arrow keys, Home and End, or a click or drag on the swatches, choose a color. The last two items are declared for some operating systems only; on another system they stay listed with the reason and do not run. **Install extension from folder…** installs a local extension package; see [Packaging and installing a local extension](guests/README.md#packaging-and-installing-a-local-extension). Extensions are managed in Settings (**Manage Extensions** in root search opens it there): each installed package has a page under the sidebar's Extensions group, whose switch disables or enables it — a disabled package keeps its settings across restarts — and whose **Reload** replaces its code with its source folder's current build while Pane stays open; see [Reloading a package](guests/README.md#reloading-a-package-while-pane-stays-open).

To try a package without the folder picker, open the launcher on its install screen, then press Enter to install it:

```sh
cargo run -p pane -- --install target/guests/packages/sample-rust   # pane --install <folder>
cargo run -p pane -- --install target/guests/packages/sample-js     # the JavaScript sample
cargo run -p pane -- --install target/guests/packages/sample-ts     # the TypeScript sample
```

Every sample under `target/guests/packages/` (assembled by `cargo xtask guests`) installs this way; none is built into Pane or acquired as a default extension.

Installed packages go in Pane's data folder (`%LOCALAPPDATA%\Pane\data` on Windows, `~/Library/Application Support/Pane` on macOS, `$XDG_DATA_HOME/pane` or `~/.local/share/pane` on Linux). Set `PANE_DATA_DIR` to another folder to keep test installs out of it; the GUI smoke scripts use `<output-dir>/data`.

Layout:

- `wit/extension.wit`: the host/guest contract for one extension command: a list view, item actions, [forms](docs/forms.md) and [custom views](docs/custom-views.md).
- `crates/pane-core`: the launcher model (the public host interface the tests drive), extension packages (manifest, identity, managed copies) and the extension runtime, a Wasmtime 49.0.1 engine registering only WASI 0.3.
- `crates/pane`: the GPUI CE window, with [root search](docs/root-search.md) as its first screen, and the Settings window.
- `crates/pane-target`: the operating-system and processor names shared by the core, `xtask` and native helpers.
- `xtask/`: the `cargo xtask` build, CI and packaging commands above.
- `guests/`: extension guests, including [the Rust, JavaScript and TypeScript sample commands](guests/README.md) (installed by hand, as above), TypeScript declarations for the contract, the prebuilt JS/TS components and test fixtures.
- `tools/componentize-js`: the JS/TS toolchain, pinned upstream componentize-qjs plus Pane's patch queue, and its build script.
- `scripts/smoke-*`: native GUI smoke runs used by CI, which uploads their screenshots.

## Project documents

- [Specification](https://github.com/hoangvu12/pane/issues/1)
- [Implementation tickets](https://github.com/hoangvu12/pane/issues?q=is%3Aissue+label%3Aimplementation)
- [Cross-platform contributor requirements](https://github.com/hoangvu12/pane/issues/1#cross-platform-contributor-requirement)
- [Current decisions](docs/current-decisions.md)
- [Handoff: where the work stands, and evidence limits](docs/HANDOFF.md)
- [Domain vocabulary](CONTEXT.md)

Contributors on all three operating systems have working build/run/test workflows, and CI builds and tests on all three; per-platform support evidence is in [docs/platforms/](docs/platforms/). Research scripts may depend on the temporary toolchains and local paths documented alongside them.

Work is tracked as specifications and implementation issues; an issue's work starts when its blockers are complete. Research results and prototype source are under `docs/research/`; they are not production support guarantees.

## Licensing

- The application (everything not listed below, including `crates/`, `xtask/` and `scripts/`) is licensed under [GPL-3.0-or-later](LICENSE-GPL).
- The extension contract (`wit/`), everything under `guests/` (guest bindings, TypeScript declarations, sample extensions and fixtures) and Pane's own files in `tools/componentize-js/` are licensed under [Apache-2.0](guests/LICENSE-APACHE) OR [MIT](guests/LICENSE-MIT), at your option. Code compiled into an extension therefore imposes no license on it.
- The patches in `tools/componentize-js/patches/` modify componentize-qjs and are licensed under [Apache-2.0](tools/componentize-js/patches/LICENSE-componentize-qjs), like it. The QuickJS runtime they build is part of every JS/TS component, so the prebuilt components in `guests/prebuilt/` also carry that runtime's terms; see [its notice](guests/prebuilt/NOTICE.md). The WIT files in `wit/deps/` (`wasi:clocks`, `wasi:http` and the packages it names) are WASI's, copied unchanged from wasmtime-wasi-http 49.0.1 (Apache-2.0 WITH LLVM-exception).
- Contributions are accepted under the same terms with a DCO sign-off; see [CONTRIBUTING.md](CONTRIBUTING.md).

Third-party dependencies keep their own licenses; `cargo deny` checks them in CI ([audit](docs/research/licensing-audit.md)). Release notice bundles and the per-release source procedure follow that audit and are not yet produced.
