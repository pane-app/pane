# Pane

A small, extensible desktop launcher for Windows, macOS and Linux, inspired by Raycast and Pi.

Pane is in early development. There is no installable application yet; the current slice is a native GPUI CE window running one sample command implemented three times, as Rust, JavaScript and TypeScript extensions, through a WASI 0.3 component interface. JavaScript and TypeScript run on a pinned, patched componentize-qjs (QuickJS), which remains provisional.

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
```

No JavaScript toolchain is needed for these: the JS and TS sample components are committed prebuilt in `guests/prebuilt/`. Rebuilding them from source with `cargo xtask js-guests` also needs Python 3.12+, git and Node.js 22+ on any of the three OSes; see [guests/README.md](guests/README.md#writing-a-javascript-or-typescript-command).

In the window, use the arrow keys to select, Enter to open a command or run an item, and Escape to go back; clicking a row runs it too.

Layout:

- `wit/extension.wit`: the host/guest contract for one extension command.
- `crates/pane-core`: the launcher model (the public host interface the tests drive) and the extension runtime, a Wasmtime 49.0.1 engine registering only WASI 0.3.
- `crates/pane`: the GPUI CE window.
- `guests/`: extension guests, including [the Rust, JavaScript and TypeScript sample commands](guests/README.md), TypeScript declarations for the contract, the prebuilt JS/TS components and test fixtures.
- `tools/componentize-js`: the JS/TS toolchain, pinned upstream componentize-qjs plus Pane's patch queue, and its build script.
- `scripts/smoke-*`: native GUI smoke runs used by CI, which uploads their screenshots.

## Project documents

- [Specification](https://github.com/hoangvu12/pane/issues/1)
- [Implementation tickets](https://github.com/hoangvu12/pane/issues?q=is%3Aissue+label%3Aimplementation)
- [Cross-platform contributor requirements](https://github.com/hoangvu12/pane/issues/1#cross-platform-contributor-requirement)
- [Current decisions](docs/current-decisions.md)
- [Handoff and evidence limits](docs/HANDOFF.md)
- [Domain vocabulary](CONTEXT.md)

Contributors on all three operating systems must have working build/run/test workflows early in development. Existing prototype evidence covers Windows only; macOS and Linux support remains to be validated. Research scripts may depend on the temporary toolchains and local paths documented alongside them.

The 52 implementation issues are published; work starts when each issue's blockers are complete. Research results and prototype source are under `docs/research/`; they are not production support guarantees.

## Licensing

- The application (everything not listed below, including `crates/`, `xtask/` and `scripts/`) is licensed under [GPL-3.0-or-later](LICENSE-GPL).
- The extension contract (`wit/`), everything under `guests/` (guest bindings, TypeScript declarations, sample extensions and fixtures) and Pane's own files in `tools/componentize-js/` are licensed under [Apache-2.0](guests/LICENSE-APACHE) OR [MIT](guests/LICENSE-MIT), at your option. Code compiled into an extension therefore imposes no license on it.
- The patches in `tools/componentize-js/patches/` modify componentize-qjs and are licensed under [Apache-2.0](tools/componentize-js/patches/LICENSE-componentize-qjs), like it. The QuickJS runtime they build is part of every JS/TS component, so the prebuilt components in `guests/prebuilt/` also carry that runtime's terms; see [its notice](guests/prebuilt/NOTICE.md). `guests/js/wit/deps/clocks.wit` is WASI's, copied from wasmtime-wasi 49.0.1 (Apache-2.0 WITH LLVM-exception).
- Contributions are accepted under the same terms with a DCO sign-off; see [CONTRIBUTING.md](CONTRIBUTING.md).

Third-party dependencies keep their own licenses; `cargo deny` checks them in CI ([audit](docs/research/licensing-audit.md)). Release notice bundles and the per-release source procedure follow that audit and are not yet produced.
