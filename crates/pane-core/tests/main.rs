//! pane-core's integration tests, one module per file beside this one, built
//! as a single binary (`cargo test -p pane-core --test integration`, and
//! `aliases::` and so on to run one file's). The adapters against the real
//! clipboard and hotkeys are test targets of their own in Cargo.toml.

// Each test file still pulls in the support helpers it uses through its
// own `#[path = "support/…"] mod`, as it did when every file was a binary
// of its own, so several modules load the same support file: deliberate,
// each copy private to its file. One shared `support` module declared
// here would replace them.
#![allow(clippy::duplicate_mod)]

mod aliases;
mod application_adapters;
mod application_cache;
mod application_changes;
mod application_icon_adapters;
mod application_icons;
mod application_identity;
mod application_names;
mod application_update;
mod arguments;
mod clear_cache;
mod clipboard;
mod clipboard_view;
mod command_search;
mod confirmations;
mod create;
mod credentials;
mod dependencies;
mod develop;
mod develop_builds;
mod disable;
mod disable_dependents;
mod error_overlay;
mod extension_log;
mod extension_pages;
mod feedback;
mod file_actions;
mod file_index;
mod files;
mod game_mode;
mod helpers;
mod history;
mod hotkey_recognizer;
mod hotkeys;
mod icons;
mod installer;
mod item_actions;
mod keywords;
mod launcher;
mod learning;
mod list_tree;
mod local_channel;
mod memory;
mod no_view;
mod npm;
mod official_extensions;
mod operations;
mod packages;
mod paste;
mod pausing;
mod preferences;
mod programs;
mod publishing;
mod quick_slots;
mod real_input;
mod reload;
mod repositories;
mod result_actions;
mod root_providers;
mod run;
mod runtime_cache;
mod runtime_crash;
mod runtime_timers;
mod samples;
mod schedules;
mod search;
mod search_files;
mod services;
mod stopping;
mod submenus;
mod switch_windows;
mod system;
mod system_commands;
mod system_icon_adapters;
mod typed_folders;
mod typed_queries;
mod uninstall;
mod uninstall_dependents;
mod unresponsive;
mod update;
mod web_icons;

/// Cargo no longer finds the files under `tests/` itself (`autotests =
/// false`), so a file declared neither here nor as a target in Cargo.toml
/// would never be compiled or run.
#[test]
fn every_test_file_is_compiled() {
    let main = include_str!("main.rs");
    let manifest = include_str!("../Cargo.toml");
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
            main.contains(&format!("\nmod {name};"))
                || manifest.contains(&format!("\"tests/{name}.rs\"")),
            "tests/{name}.rs is in no test target: declare `mod {name};` in tests/main.rs"
        );
    }
}
