//! The selected result's actions through the launcher's public interface:
//! what the Actions panel lists for root search's selected row — its
//! primary action, then the alias and hotkey configuration an installed
//! command has — and the flows those open, which return to the search they
//! came from. The commands are real guests (the settings sample from
//! `cargo xtask guests`); the hotkey system is a fake, so registering is
//! deterministic.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, ResultAction, Runtime, Screen, Status};
use tempfile::TempDir;

#[derive(Default)]
struct FakeSystem {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeSystem {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        self.registered.lock().unwrap().push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.registered
            .lock()
            .unwrap()
            .retain(|kept| kept != shortcut);
    }
}

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn package(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

struct Dirs {
    sources: TempDir,
    data: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        }
    }

    /// A launcher on this data folder, with the settings sample installed.
    fn launcher(&self) -> Launcher {
        let launcher = Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_hotkeys(Arc::new(FakeSystem::default()));
        let folder = package("sample-settings", &self.sources.path().join("settings"));
        block_on(launcher.install_package(&folder));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        launcher
    }
}

/// Searches root search for `query` and selects the row titled `title`.
fn search_and_select(launcher: &Launcher, query: &str, title: &str) {
    block_on(launcher.set_query(query));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", launcher.view().rows));
    launcher.select(index);
}

fn labels(launcher: &Launcher) -> Vec<(ResultAction, String)> {
    launcher
        .result_actions()
        .expect("the selected row has actions")
        .items
        .into_iter()
        .map(|item| (item.action, item.label))
        .collect()
}

fn selected_id(launcher: &Launcher) -> String {
    let view = launcher.view();
    view.rows[view.selected.expect("a row is selected")]
        .id
        .clone()
}

#[test]
fn an_installed_commands_actions_are_its_primary_action_pinning_then_its_hotkey_and_alias() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "greet", "Greeting");

    let actions = launcher.result_actions().expect("actions");
    assert_eq!(actions.target, selected_id(&launcher));
    assert_eq!(actions.title, "Greeting");
    let primary = launcher.selected_action();
    assert_eq!(
        labels(&launcher),
        [
            (ResultAction::Invoke, primary.label),
            (ResultAction::Pin, "Pin".into()),
            (ResultAction::ResetRanking, "Reset Ranking".into()),
            (ResultAction::Hotkey, "Assign Hotkey…".into()),
            (ResultAction::Alias, "Add Alias…".into()),
        ]
    );
    assert!(actions.items.iter().all(|item| item.available));
}

#[test]
fn panes_own_rows_offer_only_their_primary_action() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "manage", "Manage Extensions");
    assert_eq!(
        labels(&launcher),
        [(ResultAction::Invoke, launcher.selected_action().label)]
    );
}

#[test]
fn nothing_selected_or_off_root_search_has_no_actions() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.set_query("no such thing at all"));
    if launcher.view().selected.is_none() {
        assert!(launcher.result_actions().is_none());
    }
    search_and_select(&launcher, "manage", "Manage Extensions");
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert!(launcher.result_actions().is_none());
}

#[test]
fn the_alias_action_opens_the_alias_form_and_saving_returns_to_the_search() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "greet", "Greeting");
    let target = selected_id(&launcher);

    assert!(launcher.open_result_action(&target, ResultAction::Alias));
    let form = launcher.view().form().cloned().expect("the alias form");
    launcher.set_field_value(&form.fields[0].id, "gr");
    block_on(launcher.submit_form());

    let view = launcher.view();
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        },
        "back on the search it came from"
    );
    assert_eq!(selected_id(&launcher), target, "the target stays selected");
    assert!(
        matches!(&view.status, Status::Result(done) if done.contains("gr")),
        "{:?}",
        view.status
    );
    assert!(
        labels(&launcher).contains(&(ResultAction::Alias, "Change Alias…".into())),
        "{:?}",
        labels(&launcher)
    );
}

#[test]
fn recording_a_hotkey_from_actions_returns_to_the_search() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "greet", "Greeting");
    let target = selected_id(&launcher);

    assert!(launcher.open_result_action(&target, ResultAction::Hotkey));
    assert!(matches!(launcher.view().screen, Screen::Hotkey { .. }));
    block_on(launcher.record_hotkey(Shortcut::parse("ctrl+alt+g").unwrap()));

    let view = launcher.view();
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        }
    );
    assert_eq!(selected_id(&launcher), target);
    assert!(
        matches!(view.status, Status::Result(_)),
        "{:?}",
        view.status
    );
    assert!(labels(&launcher).contains(&(ResultAction::Hotkey, "Change Hotkey…".into())));
}

#[test]
fn back_from_an_actions_flow_returns_to_the_search() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "greet", "Greeting");
    let target = selected_id(&launcher);

    assert!(launcher.open_result_action(&target, ResultAction::Hotkey));
    launcher.back();
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "greet".into()
        }
    );
    assert_eq!(selected_id(&launcher), target);

    assert!(launcher.open_result_action(&target, ResultAction::Alias));
    launcher.back();
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "greet".into()
        }
    );
}

#[test]
fn the_same_flows_from_manage_extensions_still_return_there() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    // Opened once from Actions and left another way, then from Manage
    // extensions: nothing of the first visit carries over.
    search_and_select(&launcher, "greet", "Greeting");
    let target = selected_id(&launcher);
    assert!(launcher.open_result_action(&target, ResultAction::Hotkey));
    launcher.show_root_search();

    search_and_select(&launcher, "manage", "Manage Extensions");
    block_on(launcher.activate_selected());
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Hotkey for Greeting")
        .expect("the hotkey row");
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Hotkey { .. }));
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

#[test]
fn an_action_for_a_target_no_longer_selected_does_nothing() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "greet", "Greeting");
    let target = selected_id(&launcher);
    search_and_select(&launcher, "manage", "Manage Extensions");

    let before = launcher.view();
    assert!(!launcher.open_result_action(&target, ResultAction::Alias));
    assert!(!launcher.result_action_ready(&target, ResultAction::Invoke));
    assert_eq!(launcher.view(), before);
}

#[test]
fn matching_keeps_the_actions_whose_label_has_the_text() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    search_and_select(&launcher, "greet", "Greeting");
    let actions = launcher.result_actions().expect("actions");

    let matching = |query: &str| -> Vec<ResultAction> {
        actions
            .matching(query)
            .into_iter()
            .map(|item| item.action)
            .collect()
    };
    assert_eq!(matching("").len(), 5, "a blank filter keeps them all");
    assert_eq!(matching(" PIN"), [ResultAction::Pin]);
    assert_eq!(matching("  HOTKEY "), [ResultAction::Hotkey]);
    assert_eq!(matching("alias"), [ResultAction::Alias]);
    assert!(matching("quit").is_empty());
}
