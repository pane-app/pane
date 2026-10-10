//! The files Pane's JavaScript and TypeScript build embeds (`#218`, spec
//! `#128`), so that a build needs nothing but Node.js and npm: the JS/TS SDK
//! (`guests/js`, published as `@pane-app/extension`), which the build stages
//! beside the package so its `file:../js` devDependency installs; Pane's WIT
//! and WASI's, which the build assembles into the world a command is
//! componentized against; and the wasm parts the componentizer links, the
//! QuickJS `runtime.wasm` and wasi-sdk's WASI 0.3 `libc.so`, which the
//! `cli-packages` workflow builds once on Linux (`cargo xtask wasm-parts`)
//! and this repository commits (`tools/componentize-js/wasm-parts`).
//!
//! They are embedded with `include_str!`/`include_bytes!` from the committed
//! files, so what this crate builds with is what the repository holds: a
//! change to a source file changes the binary, and the prebuilt-samples
//! check (`cargo xtask ci-lints`) reports the components built from an
//! earlier one as stale.

use std::path::Path;

/// The JS/TS SDK (`guests/js`): every file `npm ci` of a staged package
/// needs of it, and the adapter and `console` the bundled entry imports.
/// Staged at `<work>/js` beside the package, where its `file:../js`
/// devDependency resolves.
const SDK: [(&str, &str); 34] = [
    (
        "LICENSE-APACHE",
        include_str!("../../../guests/js/LICENSE-APACHE"),
    ),
    (
        "LICENSE-MIT",
        include_str!("../../../guests/js/LICENSE-MIT"),
    ),
    ("README.md", include_str!("../../../guests/js/README.md")),
    ("adapt.js", include_str!("../../../guests/js/adapt.js")),
    (
        "applications.d.ts",
        include_str!("../../../guests/js/applications.d.ts"),
    ),
    (
        "clipboard.d.ts",
        include_str!("../../../guests/js/clipboard.d.ts"),
    ),
    (
        "commands.d.ts",
        include_str!("../../../guests/js/commands.d.ts"),
    ),
    (
        "console.d.ts",
        include_str!("../../../guests/js/console.d.ts"),
    ),
    ("console.js", include_str!("../../../guests/js/console.js")),
    ("data.d.ts", include_str!("../../../guests/js/data.d.ts")),
    (
        "feedback-host.d.ts",
        include_str!("../../../guests/js/feedback-host.d.ts"),
    ),
    (
        "feedback.d.ts",
        include_str!("../../../guests/js/feedback.d.ts"),
    ),
    (
        "feedback.js",
        include_str!("../../../guests/js/feedback.js"),
    ),
    (
        "file-index.d.ts",
        include_str!("../../../guests/js/file-index.d.ts"),
    ),
    ("files.d.ts", include_str!("../../../guests/js/files.d.ts")),
    (
        "helpers.d.ts",
        include_str!("../../../guests/js/helpers.d.ts"),
    ),
    ("http.d.ts", include_str!("../../../guests/js/http.d.ts")),
    ("http.js", include_str!("../../../guests/js/http.js")),
    ("icons.d.ts", include_str!("../../../guests/js/icons.d.ts")),
    ("icons.js", include_str!("../../../guests/js/icons.js")),
    ("look.js", include_str!("../../../guests/js/look.js")),
    (
        "operations.d.ts",
        include_str!("../../../guests/js/operations.d.ts"),
    ),
    (
        "package.json",
        include_str!("../../../guests/js/package.json"),
    ),
    ("pane.d.ts", include_str!("../../../guests/js/pane.d.ts")),
    (
        "preferences.d.ts",
        include_str!("../../../guests/js/preferences.d.ts"),
    ),
    (
        "preferences.js",
        include_str!("../../../guests/js/preferences.js"),
    ),
    (
        "programs-host.d.ts",
        include_str!("../../../guests/js/programs-host.d.ts"),
    ),
    (
        "programs.d.ts",
        include_str!("../../../guests/js/programs.d.ts"),
    ),
    (
        "programs.js",
        include_str!("../../../guests/js/programs.js"),
    ),
    (
        "system-host.d.ts",
        include_str!("../../../guests/js/system-host.d.ts"),
    ),
    (
        "system.d.ts",
        include_str!("../../../guests/js/system.d.ts"),
    ),
    ("system.js", include_str!("../../../guests/js/system.js")),
    ("wasi.d.ts", include_str!("../../../guests/js/wasi.d.ts")),
    (
        "wit/world.wit",
        include_str!("../../../guests/js/wit/world.wit"),
    ),
];

/// Pane's WIT, copied into the world's `deps/pane-extension/`.
const PANE_WIT: [(&str, &str); 16] = [
    ("extension.wit", include_str!("../../../wit/extension.wit")),
    ("commands.wit", include_str!("../../../wit/commands.wit")),
    ("feedback.wit", include_str!("../../../wit/feedback.wit")),
    ("system.wit", include_str!("../../../wit/system.wit")),
    ("data.wit", include_str!("../../../wit/data.wit")),
    (
        "preferences.wit",
        include_str!("../../../wit/preferences.wit"),
    ),
    (
        "root-results.wit",
        include_str!("../../../wit/root-results.wit"),
    ),
    (
        "operations.wit",
        include_str!("../../../wit/operations.wit"),
    ),
    (
        "applications.wit",
        include_str!("../../../wit/applications.wit"),
    ),
    ("search.wit", include_str!("../../../wit/search.wit")),
    ("helpers.wit", include_str!("../../../wit/helpers.wit")),
    ("files.wit", include_str!("../../../wit/files.wit")),
    ("clipboard.wit", include_str!("../../../wit/clipboard.wit")),
    ("service.wit", include_str!("../../../wit/service.wit")),
    ("programs.wit", include_str!("../../../wit/programs.wit")),
    (
        "file-index.wit",
        include_str!("../../../wit/file-index.wit"),
    ),
];

/// WASI's WIT (`wit/deps`: clocks, and `wasi:http` with the packages it
/// names), copied into the world's `deps/`.
const WASI_WIT: [(&str, &str); 6] = [
    ("cli.wit", include_str!("../../../wit/deps/cli.wit")),
    ("clocks.wit", include_str!("../../../wit/deps/clocks.wit")),
    (
        "filesystem.wit",
        include_str!("../../../wit/deps/filesystem.wit"),
    ),
    ("http.wit", include_str!("../../../wit/deps/http.wit")),
    ("random.wit", include_str!("../../../wit/deps/random.wit")),
    ("sockets.wit", include_str!("../../../wit/deps/sockets.wit")),
];

/// The QuickJS runtime (`tools/componentize-js/wasm-parts/runtime.wasm`) the
/// componentizer embeds, built for `wasm32-wasip3` with the pinned nightly
/// Rust and wasi-sdk (`cargo xtask wasm-parts`, the `cli-packages` workflow's
/// Wasm parts job, #215): the same bytes on every platform, so one committed
/// file serves them all. Only the
/// componentizer pane-build links (`componentizer` feature) reads it; one
/// spawned as a binary reads the copy its folder holds.
#[cfg(feature = "componentizer")]
pub(crate) const RUNTIME_WASM: &[u8] =
    include_bytes!("../../../tools/componentize-js/wasm-parts/runtime.wasm");

/// wasi-sdk 34's WASI 0.3 `libc.so`
/// (`tools/componentize-js/wasm-parts/libc.so`), which the componentizer
/// links into every component beside the runtime. As [`RUNTIME_WASM`], only
/// the linked componentizer reads it.
#[cfg(feature = "componentizer")]
pub(crate) const LIBC_SO: &[u8] =
    include_bytes!("../../../tools/componentize-js/wasm-parts/libc.so");

/// Writes the SDK into `types`, which the build stages beside the package.
pub(crate) fn write_sdk(types: &Path) -> std::io::Result<()> {
    for (name, contents) in SDK {
        let file = types.join(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, contents)?;
    }
    Ok(())
}

/// Writes the WIT of the world a command is componentized against: Pane's
/// and WASI's into `deps/`, beside the SDK's `world.wit` (which the
/// generated `command.wit` includes).
pub(crate) fn write_deps(wit: &Path) -> std::io::Result<()> {
    let pane = wit.join("deps/pane-extension");
    std::fs::create_dir_all(&pane)?;
    for (name, contents) in PANE_WIT {
        std::fs::write(pane.join(name), contents)?;
    }
    std::fs::create_dir_all(wit.join("deps"))?;
    for (name, contents) in WASI_WIT {
        std::fs::write(wit.join("deps").join(name), contents)?;
    }
    Ok(())
}
