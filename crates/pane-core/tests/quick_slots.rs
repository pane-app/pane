//! Quick slots through the launcher's public interface: what can be
//! pinned, how the list of slots grows, moves and shrinks, how a pin resolves
//! through the registry as it is now — a disabled, missing or not yet
//! listed target keeps its slot and says why — what invoking a slot runs,
//! and how the arrangement is recorded in `quick-slots.json`: across a
//! restart, from a record of the first version, never over a record Pane
//! cannot read, and back to what the record holds when a write fails.
//!
//! Most tests pin commands this build registers (their components are
//! never opened); the disabled-command test installs the settings sample,
//! the invocation tests open the Rust sample and the installed
//! applications of a fake system, through the real guests from
//! `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::applications::{Application, Applications};
use pane_core::{
    CommandRegistration, Launcher, PackageIdentity, PinTarget, ResultAction, Runtime, SavedData,
    Screen, SlotChange, Status,
};
use tempfile::TempDir;

/// A built guest, from `target/guests`.
fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// A command this build registers, titled `title`, with the Rust
/// sample's component.
fn command(id: &str, title: &str) -> CommandRegistration {
    CommandRegistration {
        id: id.into(),
        title: title.into(),
        subtitle: None,
        component: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests")
            .join("sample_rust.wasm"),
        takes_query: false,
        search: false,
        when: pane_core::CommandWhen::Always,
        matches: pane_core::CommandMatches::Title,
    }
}

/// Six registered commands.
fn six_commands() -> Vec<CommandRegistration> {
    ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"]
        .iter()
        .map(|title| command(&title.to_lowercase(), title))
        .collect()
}

/// A launcher over `commands` keeping its quick slots in `data`.
fn launcher(data: &Path, commands: Vec<CommandRegistration>) -> Launcher {
    Launcher::with_packages(Runtime::start(), commands, data.join("extensions"))
        .with_quick_slots(data)
}

fn record(data: &Path) -> PathBuf {
    data.join("quick-slots.json")
}

/// Searches root search for `query` and selects the row titled `title`;
/// its id.
fn select(launcher: &Launcher, query: &str, title: &str) -> String {
    block_on(launcher.set_query(query));
    let view = launcher.view();
    let index = view
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", view.rows));
    launcher.select(index);
    view.rows[index].id.clone()
}

/// Pins the row titled `title`, recording it; what the change was.
fn pin(launcher: &Launcher, title: &str) -> SlotChange {
    let target = select(launcher, "", title);
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    block_on(recorded);
    change
}

/// The slots' titles, in order.
fn titles(launcher: &Launcher) -> Vec<String> {
    launcher
        .quick_slots()
        .into_iter()
        .map(|slot| slot.title)
        .collect()
}

fn actions(launcher: &Launcher) -> Vec<(ResultAction, String, bool)> {
    launcher
        .result_actions()
        .expect("the selected row has actions")
        .items
        .into_iter()
        .map(|item| (item.action, item.label, item.available))
        .collect()
}

#[test]
fn a_fresh_installation_has_no_slots_and_no_record() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    assert!(launcher.quick_slots().is_empty());
    assert!(launcher.quick_slots_problem().is_none());
    assert_eq!(launcher.view().status, Status::Idle);
    assert!(!record(data.path()).exists(), "nothing is fabricated");
}

#[test]
fn pinning_a_command_adds_a_slot_and_records_its_identity() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    let target = select(&launcher, "", "Bravo");
    assert!(
        actions(&launcher).contains(&(ResultAction::Pin, "Pin".into(), true)),
        "{:?}",
        actions(&launcher)
    );

    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert_eq!(change, SlotChange::Changed(Some(0)));
    block_on(recorded);

    let slots = launcher.quick_slots();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].target, PinTarget::Command("bravo".into()));
    assert_eq!(slots[0].title, "Bravo");
    assert!(slots[0].ready());
    assert_eq!(
        launcher.view().status,
        Status::Result("Pinned Bravo".into())
    );
    // The record holds the command's id, in the slot's place: never the
    // row's index or its title.
    let text = fs::read_to_string(record(data.path())).unwrap();
    let recorded: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(recorded["version"], 2);
    assert_eq!(
        recorded["pins"],
        serde_json::json!([{ "command": "bravo" }])
    );
    // Pinned, the row's own entry unpins it.
    assert!(
        actions(&launcher).contains(&(ResultAction::Unpin, "Unpin".into(), true)),
        "{:?}",
        actions(&launcher)
    );
}

#[test]
fn the_arrangement_survives_a_restart_in_its_order() {
    let data = tempfile::tempdir().unwrap();
    {
        let launcher = launcher(data.path(), six_commands());
        for title in ["Delta", "Alpha", "Charlie"] {
            pin(&launcher, title);
        }
    }
    let restarted = launcher(data.path(), six_commands());
    assert_eq!(
        titles(&restarted),
        ["Delta", "Alpha", "Charlie"],
        "the slots' order is the one recorded"
    );
}

#[test]
fn pinning_a_pinned_target_names_its_slot_and_changes_nothing() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    pin(&launcher, "Alpha");
    pin(&launcher, "Bravo");
    let before = fs::read_to_string(record(data.path())).unwrap();

    assert_eq!(pin(&launcher, "Bravo"), SlotChange::AlreadyPinned(1));
    assert_eq!(titles(&launcher), ["Alpha", "Bravo"]);
    assert_eq!(fs::read_to_string(record(data.path())).unwrap(), before);
    assert_eq!(
        launcher.view().status,
        Status::Result("Bravo is already pinned".into())
    );
}

#[test]
fn a_pinned_result_is_removed_and_moved_but_never_past_either_end() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    for title in ["Alpha", "Bravo", "Charlie"] {
        pin(&launcher, title);
    }
    let target = select(&launcher, "", "Alpha");
    // The slot's own panel removes and moves it.
    let own: Vec<_> = launcher
        .quick_slot_actions(&target)
        .expect("the slot's own actions")
        .items
        .into_iter()
        .map(|item| (item.action, item.label, item.available))
        .collect();
    assert_eq!(
        own,
        [
            (ResultAction::Invoke, "Open command".into(), true),
            (ResultAction::Unpin, "Unpin".into(), true),
            (ResultAction::MovePinUp, "Move Up".into(), false),
            (ResultAction::MovePinDown, "Move Down".into(), true),
        ],
        "the first slot cannot move up"
    );
    let (change, _) = launcher.change_quick_slots(&target, ResultAction::MovePinUp);
    assert_eq!(change, SlotChange::Refused);

    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::MovePinDown);
    assert_eq!(change, SlotChange::Changed(Some(1)));
    block_on(recorded);
    assert_eq!(titles(&launcher), ["Bravo", "Alpha", "Charlie"]);

    // Moved to the last slot, it cannot move down.
    let (_, recorded) = launcher.change_quick_slots(&target, ResultAction::MovePinDown);
    block_on(recorded);
    assert_eq!(launcher.quick_slot_of(&target), Some(2));
    assert!(!launcher.quick_slot_action_ready(&target, ResultAction::MovePinDown));
    assert!(launcher.quick_slot_action_ready(&target, ResultAction::MovePinUp));

    // Unpinned, it leaves no gap.
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Unpin);
    assert_eq!(change, SlotChange::Changed(None));
    block_on(recorded);
    assert_eq!(titles(&launcher), ["Bravo", "Charlie"]);
    let restarted = self::launcher(data.path(), six_commands());
    assert_eq!(titles(&restarted), ["Bravo", "Charlie"]);
}

#[test]
fn pinning_never_runs_out_of_room() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    for title in ["Alpha", "Bravo", "Charlie", "Delta", "Echo"] {
        pin(&launcher, title);
    }
    assert_eq!(pin(&launcher, "Foxtrot"), SlotChange::Changed(Some(5)));
    assert_eq!(
        titles(&launcher),
        ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"]
    );
}

#[test]
fn a_first_version_record_is_read_as_its_pins_in_order() {
    let data = tempfile::tempdir().unwrap();
    fs::write(
        record(data.path()),
        r#"{ "version": 1, "slots": [null, { "command": "charlie" }, null, { "command": "alpha" }, null] }"#,
    )
    .unwrap();
    let launcher = launcher(data.path(), six_commands());
    assert!(launcher.quick_slots_problem().is_none());
    assert_eq!(
        titles(&launcher),
        ["Charlie", "Alpha"],
        "its gaps are closed"
    );

    // The next change writes the current version.
    pin(&launcher, "Bravo");
    let text = fs::read_to_string(record(data.path())).unwrap();
    let recorded: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(recorded["version"], 2);
    assert_eq!(
        recorded["pins"],
        serde_json::json!([
            { "command": "charlie" },
            { "command": "alpha" },
            { "command": "bravo" }
        ])
    );
}

#[test]
fn panes_own_rows_cannot_be_pinned() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    let target = select(&launcher, "settings", "Settings…");
    assert!(
        actions(&launcher)
            .iter()
            .all(|(action, ..)| *action == ResultAction::Invoke),
        "{:?}",
        actions(&launcher)
    );
    let (change, _) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert_eq!(change, SlotChange::Refused);
    assert!(launcher.quick_slots().is_empty());
}

#[test]
fn invoking_a_slot_opens_its_command_and_a_slot_past_the_list_runs_nothing() {
    let data = tempfile::tempdir().unwrap();
    built("sample_rust.wasm");
    let launcher = launcher(data.path(), vec![command("rust", "Rust sample")]);
    pin(&launcher, "Rust sample");
    block_on(launcher.set_query("")); // back at the home

    block_on(launcher.activate_quick_slot(1));
    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "a slot past the list invokes nothing"
    );
    block_on(launcher.activate_quick_slot(0));
    let view = launcher.view();
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    // Off root search, the slots do nothing.
    block_on(launcher.activate_quick_slot(0));
    assert_eq!(launcher.view().screen, Screen::Command);
}

/// Copies the assembled package `name` to `folder`.
fn package(name: &str, folder: &Path) -> PathBuf {
    let assembled = built(&format!("packages/{name}"));
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
}

#[test]
fn a_disabled_commands_slot_says_why_runs_nothing_and_resolves_again_once_enabled() {
    let data = tempfile::tempdir().unwrap();
    let sources = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), vec![]);
    let folder = package("sample-settings", &sources.path().join("settings"));
    install(&launcher, &folder);
    pin(&launcher, "Greeting");
    let identity = PackageIdentity::local(&folder).unwrap();

    block_on(launcher.set_enabled(&identity, false));
    launcher.show_root_search();
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "Greeting", "it keeps its slot and its name");
    assert_eq!(
        slot.unavailable.as_deref(),
        Some("Settings sample is disabled")
    );
    block_on(launcher.activate_quick_slot(0));
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing ran");
    assert_eq!(
        view.status,
        Status::Error("Settings sample is disabled".into())
    );
    // It can still be removed from its own slot's actions.
    let target = slot.target.key();
    assert!(launcher.quick_slot_action_ready(&target, ResultAction::Unpin));
    assert!(!launcher.quick_slot_action_ready(&target, ResultAction::Invoke));

    block_on(launcher.set_enabled(&identity, true));
    assert!(launcher.quick_slots()[0].ready(), "the same identity again");
}

#[test]
fn a_slots_opening_answered_after_its_package_was_replaced_or_uninstalled_is_not_shown() {
    let data = tempfile::tempdir().unwrap();
    let sources = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), vec![]);
    let folder = package("sample-settings", &sources.path().join("settings"));
    install(&launcher, &folder);
    pin(&launcher, "Greeting");
    let identity = PackageIdentity::local(&folder).unwrap();

    // Invoked from the code before a reload, answered after it.
    let opening = launcher.activate_quick_slot(0);
    block_on(launcher.reload(&identity));
    block_on(opening);
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "the replaced generation's view is not shown: {:?}",
        view.screen
    );
    assert!(launcher.quick_slots()[0].ready(), "the same identity again");

    // Invoked before an uninstall, answered after it.
    let opening = launcher.activate_quick_slot(0);
    block_on(launcher.uninstall(&identity, SavedData::Keep));
    block_on(opening);
    let view = launcher.view();
    assert!(
        !matches!(view.screen, Screen::Command),
        "the uninstalled generation's view is not shown: {:?}",
        view.screen
    );
    launcher.show_root_search();
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "greeting", "it keeps its slot, named by its id");
    assert_eq!(
        slot.unavailable.as_deref(),
        Some("Its extension is not installed")
    );
}

#[test]
fn a_missing_target_keeps_its_slot_says_why_and_can_be_removed() {
    let data = tempfile::tempdir().unwrap();
    fs::write(
        record(data.path()),
        r#"{ "version": 2, "pins": [{ "command": "local:/nowhere#gone" }] }"#,
    )
    .unwrap();
    let launcher = launcher(data.path(), six_commands());
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "gone", "named by its id, never a guess");
    assert_eq!(
        slot.unavailable.as_deref(),
        Some("Its extension is not installed")
    );
    let target = slot.target.key();
    let own = launcher
        .quick_slot_actions(&target)
        .expect("its slot's actions");
    assert_eq!(
        own.items
            .iter()
            .map(|item| (item.action, item.available))
            .collect::<Vec<_>>(),
        [
            (ResultAction::Invoke, false),
            (ResultAction::Unpin, true),
            (ResultAction::MovePinUp, false),
            (ResultAction::MovePinDown, false),
        ]
    );
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Unpin);
    assert_eq!(change, SlotChange::Changed(None));
    block_on(recorded);
    assert!(launcher.quick_slots().is_empty());
}

#[test]
fn an_unreadable_record_is_reported_kept_and_never_replaced() {
    for garbage in [
        "{ not a record",
        r#"{ "version": 3, "pins": [] }"#,
        r#"{ "version": 2, "pins": [{ "command": "alpha" }, { "command": "alpha" }] }"#,
        r#"{ "version": 2, "pins": [null] }"#,
        r#"{ "version": 1, "slots": [{ "row": 3 }] }"#,
    ] {
        let data = tempfile::tempdir().unwrap();
        fs::write(record(data.path()), garbage).unwrap();
        let launcher = launcher(data.path(), six_commands());
        assert!(
            launcher.quick_slots_problem().is_some(),
            "{garbage} is reported"
        );
        assert!(
            matches!(launcher.view().status, Status::Error(_)),
            "{garbage}: {:?}",
            launcher.view().status
        );
        assert!(launcher.quick_slots().is_empty());

        let target = select(&launcher, "", "Alpha");
        assert!(
            actions(&launcher).contains(&(ResultAction::Pin, "Pin".into(), false)),
            "{garbage}: pinning cannot run"
        );
        let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
        block_on(recorded);
        assert_eq!(change, SlotChange::Refused);
        assert!(matches!(launcher.view().status, Status::Error(_)));
        assert_eq!(
            fs::read_to_string(record(data.path())).unwrap(),
            garbage,
            "the record stays as it was"
        );
    }
}

#[test]
fn a_record_that_cannot_be_written_puts_the_saved_arrangement_back_and_says_why() {
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path(), six_commands());
    pin(&launcher, "Alpha");
    // Something that is not a file now stands where the record goes.
    fs::remove_file(record(data.path())).unwrap();
    fs::create_dir(record(data.path())).unwrap();

    let target = select(&launcher, "", "Bravo");
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert_eq!(change, SlotChange::Changed(Some(1)));
    assert_eq!(titles(&launcher), ["Alpha", "Bravo"], "at once");
    block_on(recorded);

    assert_eq!(titles(&launcher), ["Alpha"], "back to what the record held");
    assert!(
        matches!(&launcher.view().status, Status::Error(problem)
            if problem.starts_with("Could not keep the quick slots")),
        "{:?}",
        launcher.view().status
    );
}

/// The installed applications of a fake system, and what was opened.
#[derive(Default)]
struct FakeApplications {
    opened: Mutex<Vec<String>>,
}

impl Applications for FakeApplications {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(["Firefox", "Terminal"]
            .iter()
            .map(|name| Application {
                id: format!("/apps/{name}.app"),
                name: (*name).into(),
                location: "/apps".into(),
                ..Application::default()
            })
            .collect())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(id.to_owned());
        Ok(())
    }
}

/// A launcher whose runtime finds `system`'s applications, keeping its
/// packages and quick slots in `data` and its compiled code in `cache`.
fn with_applications(data: &Path, cache: &Path, system: &Arc<FakeApplications>) -> Launcher {
    let runtime = Runtime::start_with_cache(cache.to_path_buf()).unwrap();
    runtime.set_applications(system.clone());
    Launcher::with_packages(Ok(runtime), vec![], data.join("extensions")).with_quick_slots(data)
}

#[test]
fn an_application_is_pinned_by_its_identity_and_a_cold_home_resolves_and_opens_it() {
    let data = tempfile::tempdir().unwrap();
    let cache: TempDir = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeApplications::default());
    {
        let launcher = with_applications(data.path(), cache.path(), &system);
        install(&launcher, &built("packages/applications"));
        let target = select(&launcher, "fire", "Firefox");
        assert!(
            actions(&launcher).contains(&(ResultAction::Pin, "Pin".into(), true)),
            "{:?}",
            actions(&launcher)
        );
        let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
        assert_eq!(change, SlotChange::Changed(Some(0)));
        block_on(recorded);
        let text = fs::read_to_string(record(data.path())).unwrap();
        assert!(text.contains("/apps/Firefox.app"), "{text}");
        assert!(
            text.contains("#applications"),
            "scoped to its command: {text}"
        );
    }

    // A restart: the home is cold, nothing was typed.
    let launcher = with_applications(data.path(), cache.path(), &system);
    let waiting = &launcher.quick_slots()[0];
    assert!(!waiting.ready());
    assert_eq!(
        waiting.unavailable.as_deref(),
        Some("Waiting for Applications to list it")
    );
    block_on(launcher.resolve_quick_slots());
    assert_eq!(launcher.view().query(), Some(""), "nothing was searched");
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "Firefox");
    assert!(slot.ready());

    // Invoked twice before the first opening answered: it opens once.
    let first = launcher.activate_quick_slot(0);
    let second = launcher.activate_quick_slot(0);
    block_on(futures::future::join(first, second));
    assert_eq!(*system.opened.lock().unwrap(), ["/apps/Firefox.app"]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened Firefox".into())
    );
}
