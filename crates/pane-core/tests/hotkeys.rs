//! Global hotkeys through the launcher's public interface: the user assigns
//! a shortcut to an installed command in Manage extensions, Pane registers
//! it with the system and keeps it across restarts, and pressing it opens
//! the command, a real guest (the settings samples from `cargo xtask
//! guests`). The system is a fake [`Hotkeys`], so which shortcuts other
//! applications use is deterministic; each system's real adapter is checked
//! in `hotkey_adapters.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status, Unavailable};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage extensions…";

/// The system as the tests set it up.
#[derive(Default)]
struct FakeSystem {
    /// What Pane registered, in order.
    registered: Mutex<Vec<Shortcut>>,
    /// Shortcuts other applications use.
    taken: Mutex<Vec<Shortcut>>,
    /// Why hotkeys cannot be used at all, if they cannot.
    unavailable: Option<String>,
}

impl FakeSystem {
    fn new() -> Arc<FakeSystem> {
        Arc::new(FakeSystem::default())
    }

    fn registered(&self) -> Vec<String> {
        self.registered
            .lock()
            .unwrap()
            .iter()
            .map(Shortcut::id)
            .collect()
    }

    fn take(&self, shortcut: &str) {
        self.taken.lock().unwrap().push(key(shortcut));
    }

    /// The user presses `shortcut` in another application: the launcher is
    /// told only if Pane registered it, and the returned future opens what
    /// it opens.
    fn press(&self, launcher: &Launcher, shortcut: &str) -> bool {
        let shortcut = key(shortcut);
        if !self.registered.lock().unwrap().contains(&shortcut) {
            return false;
        }
        let opening = launcher
            .press_hotkey(&shortcut)
            .expect("a registered hotkey opens its command");
        block_on(opening);
        true
    }
}

impl Hotkeys for FakeSystem {
    fn unavailable(&self) -> Option<String> {
        self.unavailable.clone()
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        if let Some(reason) = &self.unavailable {
            return Err(HotkeyError::Refused(reason.clone()));
        }
        if self.taken.lock().unwrap().contains(shortcut) {
            return Err(HotkeyError::Taken);
        }
        let mut registered = self.registered.lock().unwrap();
        assert!(
            !registered.contains(shortcut),
            "{shortcut} registered twice"
        );
        registered.push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        let mut registered = self.registered.lock().unwrap();
        let before = registered.len();
        registered.retain(|kept| kept != shortcut);
        assert_ne!(before, registered.len(), "{shortcut} was not registered");
    }
}

fn key(text: &str) -> Shortcut {
    Shortcut::parse(text).unwrap()
}

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`, with `title` as its package title.
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

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder with `system`'s hotkeys; a new one is
    /// a restart of Pane.
    fn launcher(&self, system: &Arc<FakeSystem>) -> Launcher {
        Launcher::with_packages(Runtime::start(), vec![], self.packages_dir())
            .with_hotkeys(system.clone())
    }

    /// Installs the package `name` from a source folder of its own.
    fn install(&self, launcher: &Launcher, name: &str) -> PathBuf {
        let folder = package(name, &self.sources.path().join(name));
        block_on(launcher.install_package(&folder));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        folder
    }
}

fn row_subtitle(launcher: &Launcher, title: &str) -> String {
    let view = launcher.view();
    let row = view
        .rows
        .iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    row.subtitle.clone().unwrap_or_default()
}

fn activate(launcher: &Launcher, title: &str) {
    select_title(launcher, title);
    block_on(launcher.activate_selected());
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    activate(launcher, MANAGE_ROW);
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// In Manage extensions, assigns `shortcut` to the command titled
/// `command` by pressing it on its hotkey screen.
fn assign(launcher: &Launcher, command: &str, shortcut: &str) {
    manage(launcher);
    activate(launcher, &format!("Hotkey for {command}"));
    assert!(
        matches!(launcher.view().screen, Screen::Hotkey { .. }),
        "{:?}",
        launcher.view()
    );
    block_on(launcher.record_hotkey(key(shortcut)));
}

fn error(launcher: &Launcher) -> String {
    match launcher.view().status {
        Status::Error(error) => error,
        other => panic!("no error: {other:?}"),
    }
}

#[test]
fn an_assigned_hotkey_opens_its_command_from_another_application() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    assign(&launcher, "Greeting", "ctrl+alt+g");
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }), "{view:?}");
    assert_eq!(
        view.status,
        Status::Result(format!("{} now opens Greeting", key("ctrl+alt+g")))
    );
    assert!(
        row_subtitle(&launcher, "Hotkey for Greeting").starts_with(&key("ctrl+alt+g").to_string()),
        "{}",
        row_subtitle(&launcher, "Hotkey for Greeting")
    );

    // Pressed while Pane shows something else, such as root search.
    launcher.back();
    assert!(system.press(&launcher, "ctrl+alt+g"));
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Greeting");
    // The command's own items work as when it is opened from root search.
    activate(&launcher, "Use a formal greeting");
    assert_eq!(
        shown(&launcher),
        Status::Result("Saved the formal greeting".into())
    );
}

#[test]
fn a_hotkey_opens_its_command_from_any_screen_even_an_open_command() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    dirs.install(&launcher, "sample-rust");
    assign(&launcher, "Greeting", "ctrl+alt+g");

    launcher.back();
    activate(&launcher, "Rust sample");
    assert_eq!(launcher.view().title, "Rust sample");
    assert!(system.press(&launcher, "ctrl+alt+g"));
    assert_eq!(launcher.view().title, "Greeting");
    // Escape returns to root search, as after opening it there.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
}

#[test]
fn the_hotkey_is_kept_and_registered_again_after_a_restart() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    drop(launcher);

    let restarted_system = FakeSystem::new();
    let restarted = dirs.launcher(&restarted_system);
    assert_eq!(restarted_system.registered(), ["ctrl+alt+g"]);
    assert!(restarted_system.press(&restarted, "ctrl+alt+g"));
    assert_eq!(restarted.view().title, "Greeting");
    assert!(
        fs::read_to_string(dirs.packages_dir().join("hotkeys.json"))
            .unwrap()
            .contains("ctrl+alt+g")
    );
}

#[test]
fn changing_the_hotkey_releases_the_old_one() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    assign(&launcher, "Greeting", "ctrl+shift+alt+h");
    assert_eq!(system.registered(), ["ctrl+alt+shift+h"]);
    assert!(!system.press(&launcher, "ctrl+alt+g"));

    let restarted_system = FakeSystem::new();
    dirs.launcher(&restarted_system);
    assert_eq!(restarted_system.registered(), ["ctrl+alt+shift+h"]);
}

#[test]
fn removing_the_hotkey_releases_it_and_is_kept_after_a_restart() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");

    manage(&launcher);
    activate(&launcher, "Hotkey for Greeting");
    activate(&launcher, "Remove hotkey");
    assert!(system.registered().is_empty());
    assert_eq!(
        launcher.view().status,
        Status::Result("Greeting has no hotkey now".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    let restarted_system = FakeSystem::new();
    dirs.launcher(&restarted_system);
    assert!(restarted_system.registered().is_empty());
}

#[test]
fn disabling_the_extension_releases_its_hotkey_and_enabling_restores_it() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    let folder = dirs.install(&launcher, "sample-settings");
    let identity = PackageIdentity::local(&folder).unwrap();
    assign(&launcher, "Greeting", "ctrl+alt+g");

    block_on(launcher.set_enabled(&identity, false));
    assert!(system.registered().is_empty(), "released when disabled");
    assert!(!system.press(&launcher, "ctrl+alt+g"));

    // Still disabled after a restart: not registered, but kept.
    let restarted_system = FakeSystem::new();
    let restarted = dirs.launcher(&restarted_system);
    assert!(restarted_system.registered().is_empty());
    block_on(restarted.set_enabled(&identity, true));
    assert_eq!(restarted_system.registered(), ["ctrl+alt+g"]);
    assert!(restarted_system.press(&restarted, "ctrl+alt+g"));
    assert_eq!(restarted.view().title, "Greeting");
}

#[test]
fn the_hotkey_of_a_paused_extension_explains_the_pause() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    // Three crashes pause the settings sample.
    for _ in 0..3 {
        assert!(system.press(&launcher, "ctrl+alt+g"));
        activate(&launcher, "Crash");
    }

    // The hotkey stays the user's and registered; pressed, it says why the
    // command does not open, and runs nothing.
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    assert!(system.press(&launcher, "ctrl+alt+g"));
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(
        error(&launcher),
        "Settings sample is paused after an error; retry it in Manage extensions"
    );
}

#[test]
fn a_shortcut_another_application_uses_is_explained_and_not_assigned() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    system.take("ctrl+alt+g");
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");

    assert_eq!(
        error(&launcher),
        format!(
            "{} cannot be used: another application or the system already uses it. Press \
             another shortcut.",
            key("ctrl+alt+g")
        )
    );
    // Still recording, so the user can press another one.
    assert!(matches!(launcher.view().screen, Screen::Hotkey { .. }));
    assert!(system.registered().is_empty());
    block_on(launcher.record_hotkey(key("ctrl+alt+h")));
    assert_eq!(system.registered(), ["ctrl+alt+h"]);
}

#[test]
fn a_taken_hotkey_keeps_the_one_it_would_replace() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    system.take("ctrl+alt+h");
    assign(&launcher, "Greeting", "ctrl+alt+h");
    assert!(matches!(launcher.view().status, Status::Error(_)));
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    assert!(system.press(&launcher, "ctrl+alt+g"));
}

#[test]
fn a_shortcut_already_opening_another_command_is_refused() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    dirs.install(&launcher, "sample-rust");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    assign(&launcher, "Rust sample", "ctrl+alt+g");
    assert_eq!(
        error(&launcher),
        format!(
            "{} already opens Greeting: remove it there first, or press another shortcut.",
            key("ctrl+alt+g")
        )
    );
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    assert!(system.press(&launcher, "ctrl+alt+g"));
    assert_eq!(launcher.view().title, "Greeting");
}

#[test]
fn a_shortcut_without_ctrl_alt_or_super_or_a_reserved_one_is_refused() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    assign(&launcher, "Greeting", "shift+g");
    assert!(error(&launcher).contains("so that it does not take over typing"));
    // Every system reserves one of these for closing or locking.
    for reserved in ["alt+f4", "super+l", "super+space"] {
        let shortcut = key(reserved);
        if let Some(refusal) = shortcut.refusal() {
            block_on(launcher.record_hotkey(shortcut));
            assert_eq!(error(&launcher), format!("{refusal}."));
        }
    }
    assert!(system.registered().is_empty());
}

#[test]
fn where_global_hotkeys_are_unavailable_the_rows_say_why_and_nothing_else_changes() {
    let dirs = Dirs::new();
    let reason = "Not available on Linux with Wayland: because".to_string();
    let system = Arc::new(FakeSystem {
        unavailable: Some(reason.clone()),
        ..FakeSystem::default()
    });
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    manage(&launcher);
    let view = launcher.view();
    let row = view
        .rows
        .iter()
        .find(|row| row.title == "Hotkey for Greeting")
        .unwrap();
    assert_eq!(
        row.unavailable,
        Some(Unavailable::OnThisSystem(reason.clone()))
    );
    activate(&launcher, "Hotkey for Greeting");
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(launcher.view().status, Status::Error(reason));

    // The extension's commands still open from root search.
    launcher.back();
    activate(&launcher, "Greeting");
    assert_eq!(launcher.view().title, "Greeting");
}

#[test]
fn a_hotkey_taken_by_another_application_meanwhile_is_explained_after_a_restart() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    drop(launcher);

    let restarted_system = FakeSystem::new();
    restarted_system.take("ctrl+alt+g");
    let restarted = dirs.launcher(&restarted_system);
    manage(&restarted);
    let subtitle = row_subtitle(&restarted, "Hotkey for Greeting");
    assert!(
        subtitle.contains("Not active: another application or the system already uses it"),
        "{subtitle}"
    );
    // The command itself is unaffected.
    restarted.back();
    activate(&restarted, "Greeting");
    assert_eq!(restarted.view().title, "Greeting");
}

#[test]
fn escape_leaves_the_hotkey_screen_without_changing_the_hotkey() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    manage(&launcher);
    activate(&launcher, "Hotkey for Greeting");
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
}

#[test]
fn a_hotkey_opens_a_javascript_command_too() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings-js");
    assign(&launcher, "Greeting", "ctrl+alt+j");
    launcher.back();
    assert!(system.press(&launcher, "ctrl+alt+j"));
    assert_eq!(launcher.view().title, "Greeting");
}

#[test]
fn uninstalling_releases_and_forgets_the_hotkey_even_when_saved_data_is_kept() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    let folder = dirs.install(&launcher, "sample-settings");
    let identity = PackageIdentity::local(&folder).unwrap();
    assign(&launcher, "Greeting", "ctrl+alt+g");

    block_on(launcher.uninstall(&identity, SavedData::Keep));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    assert!(system.registered().is_empty());
    assert!(!system.press(&launcher, "ctrl+alt+g"));

    // Installed again from the same folder, it has no hotkey.
    dirs.install(&launcher, "sample-settings");
    assert!(system.registered().is_empty());
    let restarted_system = FakeSystem::new();
    dirs.launcher(&restarted_system);
    assert!(restarted_system.registered().is_empty());
}

#[test]
fn uninstalling_forgets_exactly_its_own_hotkeys_not_those_of_a_longer_source() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    // "x#y" starts with "x" and `#`, as "x"'s command ids do.
    let mut identities = Vec::new();
    for name in ["x", "x#y"] {
        let folder = package("sample-settings", &dirs.sources.path().join(name));
        block_on(launcher.install_package(&folder));
        identities.push(PackageIdentity::local(&folder).unwrap());
    }
    manage(&launcher);
    let long = identities[1].to_string();
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| {
            row.title == "Hotkey for Greeting" && row.subtitle.as_deref().unwrap().ends_with(&long)
        })
        .unwrap();
    launcher.select(index);
    block_on(launcher.activate_selected());
    block_on(launcher.record_hotkey(key("ctrl+alt+g")));

    block_on(launcher.uninstall(&identities[0], SavedData::Keep));
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    let recorded = fs::read_to_string(dirs.packages_dir().join("hotkeys.json")).unwrap();
    assert!(recorded.contains("x#y#greeting"), "{recorded}");
    let restarted_system = FakeSystem::new();
    dirs.launcher(&restarted_system);
    assert_eq!(restarted_system.registered(), ["ctrl+alt+g"]);
}

#[test]
fn an_update_keeps_the_hotkey_and_one_that_drops_the_command_releases_it() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    let folder = dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");

    let update = |launcher: &Launcher| {
        block_on(launcher.preview_package(&folder));
        activate(launcher, "Update");
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
    };
    update(&launcher);
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    assert!(system.press(&launcher, "ctrl+alt+g"));
    assert_eq!(launcher.view().title, "Greeting");

    // The command's id changes: the old one's hotkey is released.
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    fs::write(
        folder.join("pane.json"),
        manifest.replace("\"id\": \"greeting\"", "\"id\": \"renamed\""),
    )
    .unwrap();
    update(&launcher);
    assert!(system.registered().is_empty());
}

#[test]
fn a_press_that_opens_nothing_leaves_pane_as_it_was() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    let folder = dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    let before = launcher.view();
    // Not a hotkey, and one released by disabling its extension (the press
    // was on its way): nothing opens, so the window is not raised either.
    assert!(launcher.press_hotkey(&key("ctrl+alt+h")).is_none());
    block_on(launcher.set_enabled(&PackageIdentity::local(&folder).unwrap(), false));
    let before_disabled = launcher.view();
    assert!(launcher.press_hotkey(&key("ctrl+alt+g")).is_none());
    assert_eq!(launcher.view(), before_disabled);
    assert_ne!(before, before_disabled);
}

#[test]
fn a_hotkey_removed_right_after_it_was_assigned_stays_removed_after_a_restart() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    manage(&launcher);
    activate(&launcher, "Hotkey for Greeting");
    // Assigned, then removed before the assignment has been recorded; the
    // removal's record is written first.
    let assigned = launcher.record_hotkey(key("ctrl+alt+g"));
    activate(&launcher, "Hotkey for Greeting");
    select_title(&launcher, "Remove hotkey");
    let removed = launcher.activate_selected();
    block_on(removed);
    block_on(assigned);
    assert!(system.registered().is_empty());

    let restarted_system = FakeSystem::new();
    dirs.launcher(&restarted_system);
    assert!(restarted_system.registered().is_empty());
}

#[test]
fn a_hotkey_that_cannot_be_recorded_is_undone_in_pane_and_on_disk() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    // The record can no longer be replaced: a folder stands in its place.
    let record = dirs.packages_dir().join("hotkeys.json");
    fs::remove_file(&record).unwrap();
    fs::create_dir(&record).unwrap();

    assign(&launcher, "Greeting", "ctrl+alt+h");
    assert!(
        error(&launcher).starts_with("Could not keep the hotkey"),
        "{}",
        error(&launcher)
    );
    // Pane's memory matches what was last recorded: Ctrl+Alt+G.
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    assert!(system.press(&launcher, "ctrl+alt+g"));
    manage(&launcher);
    assert!(
        row_subtitle(&launcher, "Hotkey for Greeting").starts_with(&key("ctrl+alt+g").to_string())
    );
}
