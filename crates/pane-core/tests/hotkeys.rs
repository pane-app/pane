//! Global hotkeys through the launcher's public interface: the user assigns
//! a shortcut to an installed command in Manage extensions, Pane registers
//! it with the system and keeps it across restarts, and pressing it opens
//! the command, a real guest (the settings samples from `cargo xtask
//! guests`). The system is a fake [`Hotkeys`], so which shortcuts other
//! applications use is deterministic; it can answer as Windows' adapter
//! does instead, taking a shortcut the system refuses through a keyboard
//! hook of its own and reporting that hook's state (#252, #259), and the
//! fresh data folder's Open Pane default is decided against it too (#268,
//! ADR 0039). Each system's real adapter is checked in
//! `hotkey_adapters.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::hotkeys::{HookHealth, HotkeyError, Hotkeys, Kind, Route, Shortcut, Side};
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status, Unavailable};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage Extensions";

/// The system as the tests set it up.
#[derive(Default)]
struct FakeSystem {
    /// What Pane registered, in order.
    registered: Mutex<Vec<Shortcut>>,
    /// Shortcuts other applications use.
    taken: Mutex<Vec<Shortcut>>,
    /// Why hotkeys cannot be used at all, if they cannot.
    unavailable: Option<String>,
    /// Whether this system's adapter falls back to a keyboard hook of
    /// its own when the system refuses a registration, as Windows' does
    /// (#252): a taken shortcut is then not an error but a binding
    /// through the hook. A system without one (macOS, X11) keeps the
    /// old behavior: taken is an error, explained.
    hooks: bool,
    /// The shortcuts dispatched through the hook rather than the system's
    /// registration.
    hooked: Mutex<Vec<Shortcut>>,
    /// The state of the hook, as the system reports it while a binding is
    /// dispatched through it (#259).
    health: Mutex<HookHealth>,
}

impl FakeSystem {
    fn new() -> Arc<FakeSystem> {
        Arc::new(FakeSystem::default())
    }

    /// A system whose adapter falls back to its own keyboard hook when
    /// the system refuses a registration (Windows, #252).
    fn hooking() -> Arc<FakeSystem> {
        Arc::new(FakeSystem {
            hooks: true,
            ..FakeSystem::default()
        })
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

    // A system whose adapter falls back to its own keyboard hook takes
    // the binding kinds #260 adds; one without explains them, as the
    // systems without one do. It names the platform it models: the
    // current one where that has no hook, macOS standing in where the
    // tests run on Windows — a platform whose kinds the pure half says
    // work — so the kinds are refused wherever the tests run.
    fn kind_unavailable(&self, shortcut: &Shortcut) -> Option<String> {
        if self.hooks {
            return None;
        }
        let modeled = match pane_core::Platform::current() {
            Some(pane_core::Platform::Windows) => Some(pane_core::Platform::Macos),
            platform => platform,
        };
        pane_core::hotkeys::kinds_unavailable(shortcut, modeled)
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        if let Some(reason) = &self.unavailable {
            return Err(HotkeyError::Refused(reason.clone()));
        }
        // The binding the system refuses (another application has it)
        // and the kinds no registration can express (#260) — the tap
        // kinds, a side-specific modifier, a numpad key, which a
        // registration cannot tell from its counterpart — go through the
        // hook where this system's adapter has one, as Windows' does
        // (ADR 0039): not an error, and the row says the route.
        let through_hook = self.hooks
            && (self.taken.lock().unwrap().contains(shortcut)
                || shortcut.kind() != Kind::Chord
                || shortcut
                    .sides()
                    .iter()
                    .any(|side| matches!(side, Some(Side::Left | Side::Right)))
                || shortcut.key().starts_with("numpad"));
        if through_hook {
            self.hooked.lock().unwrap().push(shortcut.clone());
            let mut registered = self.registered.lock().unwrap();
            assert!(
                !registered.contains(shortcut),
                "{shortcut} registered twice"
            );
            registered.push(shortcut.clone());
            return Ok(());
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
        self.hooked.lock().unwrap().retain(|kept| kept != shortcut);
        let mut registered = self.registered.lock().unwrap();
        let before = registered.len();
        registered.retain(|kept| kept != shortcut);
        assert_ne!(before, registered.len(), "{shortcut} was not registered");
    }

    fn route(&self, shortcut: &Shortcut) -> Route {
        if self.hooked.lock().unwrap().contains(shortcut) {
            Route::Hook
        } else {
            Route::System
        }
    }

    fn hook_health(&self) -> Option<HookHealth> {
        // As Windows' adapter answers: a hook is in use exactly while one
        // of its bindings is.
        (!self.hooked.lock().unwrap().is_empty()).then(|| self.health.lock().unwrap().clone())
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
        "Settings sample is paused after an error; retry it in Settings"
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

#[test]
fn a_shortcut_another_application_uses_is_taken_through_pane_s_keyboard_hook() {
    let dirs = Dirs::new();
    let system = FakeSystem::hooking();
    system.take("ctrl+alt+g");
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    // The system refuses the registration, as another application having
    // the shortcut; Pane's own keyboard hook takes the binding instead
    // (ADR 0039), so it is not an error.
    assign(&launcher, "Greeting", "ctrl+alt+g");
    assert_eq!(system.registered(), ["ctrl+alt+g"]);
    assert!(system.hooked.lock().unwrap().contains(&key("ctrl+alt+g")));
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("{} now opens Greeting", key("ctrl+alt+g")))
    );
    // The row reports the binding's dispatch route, with the
    // elevated-application limitation that comes with it.
    manage(&launcher);
    let subtitle = row_subtitle(&launcher, "Hotkey for Greeting");
    assert!(
        subtitle.contains("through Pane's keyboard hook"),
        "{subtitle}"
    );
    assert!(subtitle.contains("elevated application"), "{subtitle}");
    // The Shortcuts catalog carries the route for the page that shows it.
    let catalog = launcher.shortcut_catalog();
    let greeting = catalog
        .groups
        .iter()
        .flat_map(|group| group.commands.iter())
        .find(|command| command.title == "Greeting")
        .unwrap();
    assert_eq!(greeting.hotkey_route, Route::Hook);
    assert_eq!(greeting.hotkey_inactive, None);

    // The binding works, from whatever application had the focus.
    launcher.back();
    assert!(system.press(&launcher, "ctrl+alt+g"));
    assert_eq!(launcher.view().title, "Greeting");

    // It works the same after a restart, the shortcut still taken, and
    // the row still says the route.
    drop(launcher);
    let restarted_system = FakeSystem::hooking();
    restarted_system.take("ctrl+alt+g");
    let restarted = dirs.launcher(&restarted_system);
    assert_eq!(restarted_system.registered(), ["ctrl+alt+g"]);
    manage(&restarted);
    let subtitle = row_subtitle(&restarted, "Hotkey for Greeting");
    assert!(
        subtitle.contains("through Pane's keyboard hook"),
        "{subtitle}"
    );
    assert!(restarted_system.press(&restarted, "ctrl+alt+g"));
    assert_eq!(restarted.view().title, "Greeting");

    // Removing the hotkey releases the hook binding, as it releases a
    // registration.
    manage(&restarted);
    activate(&restarted, "Hotkey for Greeting");
    activate(&restarted, "Remove hotkey");
    assert!(restarted_system.hooked.lock().unwrap().is_empty());
    assert!(restarted_system.registered().is_empty());
}

#[test]
fn the_open_pane_hotkey_falls_back_to_the_hook_too() {
    let dirs = Dirs::new();
    let system = FakeSystem::hooking();
    system.take("ctrl+alt+space");
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    // The default Open Pane hotkey is taken by another application: the
    // system refuses it, Pane's own keyboard hook takes the binding
    // instead — not an error — and the General page's row says the route
    // through `open_pane_route`.
    launcher.set_open_pane(key("ctrl+alt+space")).unwrap();
    assert_eq!(launcher.open_pane_route(), Route::Hook);
    assert_eq!(launcher.open_pane_problem(), None);
    assert!(launcher.opens_pane(&key("ctrl+alt+space")));
    assert_eq!(system.registered(), ["ctrl+alt+space"]);
}

#[test]
fn the_hook_s_state_is_answered_through_the_launcher() {
    let dirs = Dirs::new();
    let system = FakeSystem::hooking();
    system.take("ctrl+alt+g");
    *system.health.lock().unwrap() = HookHealth {
        reinstalls: 2,
        pinned: true,
        given_up: None,
    };
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    // No binding is dispatched through the hook yet: none is in use, so
    // nothing is answered for — what the Keyboard page and Copy
    // Diagnostics read through the launcher's query (#259).
    assert_eq!(launcher.hook_health(), None);

    // A shortcut another application has is taken through the hook, and
    // the hook's state is answered with it.
    assign(&launcher, "Greeting", "ctrl+alt+g");
    assert_eq!(
        launcher.hook_health(),
        Some(HookHealth {
            reinstalls: 2,
            pinned: true,
            given_up: None,
        })
    );
    assert_eq!(
        launcher.hook_health().unwrap().note(),
        "Installed, its pages are pinned in memory; Windows removed it 2 times and Pane \
         installed it again"
    );

    // Removing the hotkey releases the hook binding, and no hook is in
    // use again.
    manage(&launcher);
    activate(&launcher, "Hotkey for Greeting");
    activate(&launcher, "Remove hotkey");
    assert_eq!(launcher.hook_health(), None);
}

#[test]
fn what_no_program_can_intercept_is_still_refused_through_the_launcher() {
    let dirs = Dirs::new();
    let system = FakeSystem::hooking();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    // Win+L stays refused with the reason, even on a system whose adapter
    // falls back to a keyboard hook: the lock screen is below every hook.
    // (macOS leaves Win+L free, so there is nothing to refuse there.)
    assign(&launcher, "Greeting", "super+l");
    if let Some(refusal) = key("super+l").refusal() {
        assert_eq!(error(&launcher), format!("{refusal}."));
        assert!(matches!(launcher.view().screen, Screen::Hotkey { .. }));
        assert!(system.registered().is_empty());
        assert!(system.hooked.lock().unwrap().is_empty());
    }
}

#[test]
fn a_hotkey_record_of_version_1_still_reads_and_the_new_version_round_trips() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    drop(launcher);

    // The record is written in the new version (#252), and a fresh Pane
    // reads it and registers the binding again.
    let record = dirs.packages_dir().join("hotkeys.json");
    let text = fs::read_to_string(&record).unwrap();
    assert!(text.contains("\"version\": 2"), "{text}");
    let restarted_system = FakeSystem::new();
    let restarted = dirs.launcher(&restarted_system);
    assert_eq!(restarted_system.registered(), ["ctrl+alt+g"]);
    drop(restarted);

    // A version 1 record, as an older Pane wrote it, reads the same.
    fs::write(&record, text.replace("\"version\": 2", "\"version\": 1")).unwrap();
    let older_system = FakeSystem::new();
    let older = dirs.launcher(&older_system);
    assert_eq!(older_system.registered(), ["ctrl+alt+g"]);
    assert!(older_system.press(&older, "ctrl+alt+g"));
    assert_eq!(older.view().title, "Greeting");
}

#[test]
fn the_binding_kinds_bind_fire_and_round_trip_through_the_hook() {
    let dirs = Dirs::new();
    let system = FakeSystem::hooking();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    dirs.install(&launcher, "sample-rust");

    // The tap, double-tap and side-specific kinds, and a numpad key,
    // register through the system's hook, fire as a chord does, and
    // their records round trip their textual forms (#260). Each goes to
    // its own command, so each keeps working beside the others.
    for (command, shortcut) in [("Greeting", "tap:win"), ("Rust sample", "double:ctrl")] {
        assign(&launcher, command, shortcut);
        assert!(
            system.registered().contains(&shortcut.to_owned()),
            "{shortcut} for {command} did not register: {:?}",
            system.registered()
        );
        assert!(system.hooked.lock().unwrap().contains(&key(shortcut)));
        assert!(
            system.press(&launcher, shortcut),
            "{shortcut} for {command} did not fire"
        );
        assert_eq!(launcher.view().title, command);
        launcher.back();
    }
    // The extended key set and a side in a chord, changed onto the same
    // command: the release of the one before it follows.
    assign(&launcher, "Greeting", "rctrl+alt+f13");
    assert!(
        system
            .hooked
            .lock()
            .unwrap()
            .contains(&key("rctrl+alt+f13"))
    );
    assert!(!system.press(&launcher, "tap:win"));
    assert!(system.press(&launcher, "rctrl+alt+f13"));
    assign(&launcher, "Greeting", "ctrl+alt+numpad5");
    assert!(system.press(&launcher, "ctrl+alt+numpad5"));
    assert!(
        fs::read_to_string(dirs.packages_dir().join("hotkeys.json"))
            .unwrap()
            .contains("\"ctrl+alt+numpad5\""),
        "the record holds the kinds' textual forms"
    );
    // A restart reads them back and registers them again.
    drop(launcher);
    let restarted_system = FakeSystem::hooking();
    let restarted = dirs.launcher(&restarted_system);
    assert!(
        restarted_system
            .registered()
            .contains(&"double:ctrl".to_owned())
    );
    assert!(restarted_system.press(&restarted, "double:ctrl"));
    assert_eq!(restarted.view().title, "Rust sample");
}

#[test]
fn a_single_and_a_double_tap_of_the_same_modifier_cannot_coexist() {
    let dirs = Dirs::new();
    let system = FakeSystem::hooking();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");
    dirs.install(&launcher, "sample-rust");

    // A single tap and a double tap of the same modifier bound together
    // are refused ("cannot coexist", #260): one would swallow the other's
    // presses. The refusal names the conflict as today's clashes do.
    assign(&launcher, "Greeting", "tap:win");
    assign(&launcher, "Rust sample", "double:win");
    let refusal = error(&launcher);
    assert!(refusal.contains("cannot coexist"), "{refusal}");
    assert!(
        refusal.contains(&key("double:win").to_string()),
        "{refusal}"
    );
    assert!(refusal.contains("Greeting"), "{refusal}");
    assert!(
        refusal.contains("a single tap and a double tap of the same modifier"),
        "{refusal}"
    );
    assert!(refusal.contains("Remove it there first"), "{refusal}");
    assert_eq!(
        system.registered(),
        ["tap:win"],
        "the refusal changed nothing"
    );
    // The Open Pane hotkey meets the kinds the same way, both ways
    // round: a command's double tap of the modifier Pane's own tap
    // names, and Pane's double tap of one a command's tap names.
    launcher.set_open_pane(key("tap:alt")).unwrap();
    assign(&launcher, "Rust sample", "double:alt");
    assert!(
        error(&launcher).contains("cannot coexist with Pane's"),
        "the Open Pane hotkey's tap is named"
    );
    assert!(
        error(&launcher).contains(&key("tap:alt").to_string()),
        "{:?}",
        launcher.view().status
    );
    assign(&launcher, "Rust sample", "ctrl+alt+h");
    launcher.set_open_pane(key("tap:alt")).unwrap();
    launcher.set_open_pane(key("double:rshift")).unwrap();
    assign(&launcher, "Rust sample", "tap:rshift");
    assert!(
        error(&launcher).contains("cannot coexist with Pane's"),
        "Pane's double tap is named"
    );
    // Different modifiers coexist, and a chord beside a tap does too.
    assign(&launcher, "Rust sample", "double:ctrl");
    assert!(system.press(&launcher, "double:ctrl"));
    assert!(system.press(&launcher, "tap:win"));
}

#[test]
fn the_kinds_unavailable_on_this_system_are_explained_on_their_rows() {
    let dirs = Dirs::new();
    // The fake without a hook models a system whose adapter has none
    // (macOS, X11): the kinds #260 adds are refused and explained, so a
    // record copied from a Windows machine is not a mystery.
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    dirs.install(&launcher, "sample-settings");

    assign(&launcher, "Greeting", "tap:win");
    // The platform the fake without a hook names, as it explains: the
    // current one where that has no hook, macOS standing in on Windows.
    let here = match pane_core::Platform::current() {
        Some(pane_core::Platform::Windows) => pane_core::Platform::Macos.to_string(),
        Some(platform) => platform.to_string(),
        None => "this system".into(),
    };
    assert_eq!(
        error(&launcher),
        format!("Not available on {here}: lone modifier taps work only on Windows for now")
    );
    // The screen stays for another try, and a chord still records.
    assert!(matches!(launcher.view().screen, Screen::Hotkey { .. }));
    block_on(launcher.record_hotkey(key("ctrl+alt+g")));
    assert_eq!(system.registered(), ["ctrl+alt+g"]);

    // The Open Pane hotkey is explained the same way, and the working
    // binding is untouched.
    let refused = launcher
        .set_open_pane(key("tap:win"))
        .expect_err("a tap is unavailable here");
    assert!(
        refused.contains("lone modifier taps work only on Windows"),
        "{refused}"
    );
    assert!(launcher.open_pane_problem().is_none());

    // A record naming a kind this system cannot take is explained on its
    // row — through the unavailable-row mechanism, as the command's own
    // platform unavailability is — and nothing registers; the command
    // still opens from root search as it always does.
    drop(launcher);
    let record = dirs.packages_dir().join("hotkeys.json");
    let text = fs::read_to_string(&record).unwrap();
    fs::write(&record, text.replace("\"ctrl+alt+g\"", "\"double:ctrl\"")).unwrap();
    let restart = FakeSystem::new();
    let restarted = dirs.launcher(&restart);
    assert!(restart.registered().is_empty());
    manage(&restarted);
    let subtitle = row_subtitle(&restarted, "Hotkey for Greeting");
    assert!(
        subtitle.contains(&key("double:ctrl").to_string()),
        "{subtitle}"
    );
    assert!(subtitle.contains("Not available on"), "{subtitle}");
    assert!(subtitle.contains("only on Windows for now"), "{subtitle}");
    activate(&restarted, "Hotkey for Greeting");
    assert!(
        matches!(restarted.view().status, Status::Error(ref error) if error.contains("only on Windows")),
        "{:?}",
        restarted.view().status
    );
    assert!(matches!(restarted.view().screen, Screen::Extensions { .. }));
    // The Shortcuts catalog explains it under the hotkey cell, and the
    // command still opens from root search.
    assert!(
        restarted
            .shortcut_catalog()
            .groups
            .iter()
            .flat_map(|group| group.commands.iter())
            .find(|command| command.title == "Greeting")
            .and_then(|command| command.hotkey_inactive.as_ref())
            .is_some_and(|why| why.contains("only on Windows"))
    );
    restarted.back();
    activate(&restarted, "Greeting");
    assert_eq!(restarted.view().title, "Greeting");
}

#[test]
fn the_fresh_install_default_is_decided_per_system_against_the_adapter() {
    // The fresh-install Open Pane default, decided per system against the
    // adapter that will register it (#268, ADR 0039): the Windows key
    // alone on Windows where the adapter's own hook recognizes the tap,
    // today's default elsewhere — and, where the platform is Windows but
    // the adapter cannot, today's default with the reason the Windows key
    // alone was not taken.
    let hooking = FakeSystem::hooking();
    for (platform, expected) in [
        (Some(pane_core::Platform::Windows), "tap:win"),
        (Some(pane_core::Platform::Macos), "alt+space"),
        (Some(pane_core::Platform::Linux), "ctrl+alt+space"),
        (None, "ctrl+alt+space"),
    ] {
        let fresh = hooking.open_pane_fresh_default(platform);
        assert_eq!(fresh.shortcut, key(expected), "{platform:?}");
        assert_eq!(fresh.why_not_windows_key, None, "{platform:?}");
    }
    // The adapter without a hook, as the other systems' are: the Windows
    // key alone is not taken on Windows, with the reason, and the other
    // platforms keep their defaults with nothing to explain.
    let plain = FakeSystem::new();
    let windows = plain.open_pane_fresh_default(Some(pane_core::Platform::Windows));
    assert_eq!(windows.shortcut, key("ctrl+alt+space"));
    let why = windows.why_not_windows_key.expect("the reason is carried");
    assert!(
        why.contains("The Windows key alone is not the default"),
        "{why}"
    );
    assert!(
        why.contains("lone modifier taps work only on Windows"),
        "{why}"
    );
    for platform in [
        Some(pane_core::Platform::Macos),
        Some(pane_core::Platform::Linux),
        None,
    ] {
        let fresh = plain.open_pane_fresh_default(platform);
        assert_eq!(fresh.why_not_windows_key, None, "{platform:?}");
    }
    assert_eq!(
        plain
            .open_pane_fresh_default(Some(pane_core::Platform::Macos))
            .shortcut,
        key("alt+space")
    );
    assert_eq!(
        plain
            .open_pane_fresh_default(Some(pane_core::Platform::Linux))
            .shortcut,
        key("ctrl+alt+space")
    );
}

#[test]
fn the_fresh_default_registers_through_the_same_path_a_choice_takes() {
    let dirs = Dirs::new();
    // Windows' fresh default, as the window applies it at startup over a
    // fresh data folder: registered and answered through the launcher's
    // own path, exactly as a recorded choice is.
    let system = FakeSystem::hooking();
    let launcher = dirs.launcher(&system);
    let fresh = system.open_pane_fresh_default(Some(pane_core::Platform::Windows));
    launcher.sync_open_pane(fresh.shortcut).unwrap();
    assert_eq!(system.registered(), ["tap:win"]);
    assert!(launcher.opens_pane(&key("tap:win")));

    // A choice the record holds is applied as it is, never upgraded to
    // the fresh default: the record's reader decides fresh from existing
    // (#268), and the launcher registers whatever it is handed. A new
    // launcher over a new system, as a restart of Pane is, whose
    // registrations begin again.
    drop(launcher);
    let restart = FakeSystem::hooking();
    let kept = dirs.launcher(&restart);
    kept.sync_open_pane(key("ctrl+alt+space")).unwrap();
    assert_eq!(restart.registered(), ["ctrl+alt+space"]);
    assert!(!kept.opens_pane(&key("tap:win")));
}

#[test]
fn where_the_adapter_cannot_take_the_windows_key_the_fresh_default_falls_back() {
    let dirs = Dirs::new();
    // A system whose adapter cannot recognize a tap, as one whose hook
    // cannot be installed: the fresh default on Windows falls back to
    // today's, registered in its place, and the reason says why the
    // Windows key alone was not taken (#268).
    let system = FakeSystem::new();
    let launcher = dirs.launcher(&system);
    let fresh = system.open_pane_fresh_default(Some(pane_core::Platform::Windows));
    assert_eq!(fresh.shortcut, key("ctrl+alt+space"));
    launcher.sync_open_pane(fresh.shortcut).unwrap();
    assert_eq!(system.registered(), ["ctrl+alt+space"]);
    assert!(launcher.opens_pane(&key("ctrl+alt+space")));
    assert!(
        fresh
            .why_not_windows_key
            .is_some_and(|why| why.contains("lone modifier taps work only on Windows")),
        "the reason is carried for the page to show"
    );
}
