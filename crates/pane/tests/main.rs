//! The launcher window's integration tests, one module per file beside this
//! one, built as a single binary (`cargo test -p pane --test integration`,
//! and `window::` and so on to run one file's).

// Each test file still pulls in the support helpers it uses through its
// own `#[path = "support/…"] mod`, as it did when every file was a binary
// of its own, so several modules load the same support file: deliberate,
// each copy private to its file. One shared `support` module declared
// here would replace them.
#![allow(clippy::duplicate_mod)]

mod aliases;
mod command_search;
mod compact_pins;
mod develop;
mod hotkeys;
mod install;
mod item_actions;
mod keyboard;
mod launcher_settings;
mod npm;
mod open_pane;
mod repositories;
mod runtime_crash;
mod settings;
mod settings_search;
mod shortcuts;
mod tray;
mod unresponsive;
mod update;
mod window;

/// Cargo no longer finds the files under `tests/` itself (`autotests =
/// false`), so a file not declared here would never be compiled or run.
#[test]
fn every_test_file_is_compiled() {
    let main = include_str!("main.rs");
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for entry in std::fs::read_dir(tests).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        if name == "main" {
            continue;
        }
        assert!(
            main.contains(&format!("\nmod {name};")),
            "tests/{name}.rs is in no test target: declare `mod {name};` in tests/main.rs"
        );
    }
}
