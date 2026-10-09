//! The Rust, JavaScript and TypeScript sample commands, registered by the
//! test's own launcher. Pane registers no sample command in any build
//! (#162): a contributor installs a sample with `pane --install <folder>`,
//! and a test that drives the samples brings them itself, from the
//! components `cargo xtask guests` built into `target/guests`, with the
//! ids, titles and subtitles the built-in registrations had.

use std::path::PathBuf;

use pane_core::CommandRegistration;

/// The samples: (id, title, subtitle, component file name). Each
/// implements the same command in a different extension language.
const SAMPLES: [(&str, &str, &str, &str); 3] = [
    (
        "rust-sample",
        "Rust sample",
        "A sample command implemented by a Rust extension",
        "sample_rust.wasm",
    ),
    (
        "javascript-sample",
        "JavaScript sample",
        "The same command implemented by a JavaScript extension",
        "sample_js.wasm",
    ),
    (
        "typescript-sample",
        "TypeScript sample",
        "The same command implemented by a TypeScript extension",
        "sample_ts.wasm",
    ),
];

/// The three sample commands, Rust first, as root search lists them.
pub fn sample_commands() -> Vec<CommandRegistration> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    SAMPLES
        .iter()
        .map(|&(id, title, subtitle, file)| CommandRegistration {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            component: dir.join(file),
            takes_query: false,
            search: false,
            when: pane_core::CommandWhen::Always,
            matches: pane_core::CommandMatches::Title,
        })
        .collect()
}
