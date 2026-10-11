//! Game mode through the launcher's public interface (#125): the
//! settings — off by default, the programs to treat as games — are
//! Pane's own record, and while a game is in front every hotkey, the
//! Open Pane hotkey included, is released and comes back when it leaves,
//! decided on each foreground change the source reports — a system
//! event, never a timer, so the tests drive it by events alone. The
//! foreground source is a fake (the seam `game_mode` names), and the
//! system is a fake [`Hotkeys`] as `hotkeys.rs`'s, so what is registered
//! is deterministic; Windows' real source is the foreground event hook
//! (see `game_mode::windows`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::game_mode::{Foreground, ForegroundSource, ForegroundTold, GameMode};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::{MANAGE_ROW, select_title};

/// The system as the tests set it up.
#[derive(Default)]
struct FakeSystem {
    /// What Pane registered, in order.
    registered: Mutex<Vec<Shortcut>>,
}

impl FakeSystem {
    fn new() -> Arc<FakeSystem> {
        Arc::new(FakeSystem::default())
    }

    /// The shortcuts Pane has registered, as their ids.
    fn registered(&self) -> Vec<String> {
        self.registered
            .lock()
            .unwrap()
            .iter()
            .map(Shortcut::id)
            .collect()
    }
}

impl Hotkeys for FakeSystem {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
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

/// The foreground source as the tests fake it: the seam's other half,
/// holding what the launcher subscribed and reporting what the test
/// says is in front, as the Windows watcher reports each window that
/// comes to the front — from this, the test's own, thread.
#[derive(Default)]
struct FakeForeground {
    told: Mutex<Option<ForegroundTold>>,
}

impl FakeForeground {
    /// The window in front changed: `program`'s, and whether the system
    /// reports a full-screen Direct3D application there.
    fn front(&self, program: Option<&str>, full_screen: bool) {
        let told = self.told.lock().unwrap().clone();
        if let Some(told) = told {
            told.front(&Foreground {
                program: program.map(str::to_owned),
                full_screen,
            });
        }
    }
}

impl ForegroundSource for FakeForeground {
    fn watch(&self, told: ForegroundTold) {
        *self.told.lock().unwrap() = Some(told);
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

    /// A launcher on this data folder with `system`'s hotkeys and
    /// `foreground`'s source; a new one is a restart of Pane.
    fn launcher(&self, system: &Arc<FakeSystem>, foreground: &Arc<FakeForeground>) -> Launcher {
        Launcher::with_packages(Runtime::start(), vec![], self.packages_dir())
            .with_hotkeys(system.clone())
            .with_foreground(foreground.clone())
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

/// In Manage extensions, assigns `shortcut` to the command titled
/// `command` by pressing it on its hotkey screen.
fn assign(launcher: &Launcher, command: &str, shortcut: &str) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::Extensions { .. }),
        "{:?}",
        launcher.view()
    );
    select_title(launcher, &format!("Hotkey for {command}"));
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::Hotkey { .. }),
        "{:?}",
        launcher.view()
    );
    block_on(launcher.record_hotkey(key(shortcut)));
}

/// Turns game mode on, recording it.
fn game_mode_on(launcher: &Launcher, programs: &[&str]) {
    let mode = GameMode {
        on: true,
        programs: programs.iter().copied().map(str::to_owned).collect(),
    };
    block_on(launcher.set_game_mode(mode)).expect("the settings are recorded");
}

#[test]
fn game_mode_is_off_by_default_and_a_game_in_front_releases_nothing() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    assert_eq!(launcher.game_mode(), GameMode::default());

    // A full-screen game in front, twice over: with game mode off, Pane
    // keeps every registration — the game gets the keys only when the
    // user asks for it.
    for _ in 0..2 {
        foreground.front(Some(r"C:\Games\game.exe"), true);
        assert!(!launcher.hotkeys_paused());
        assert_eq!(system.registered(), ["ctrl+alt+g"]);
    }
}

#[test]
fn a_full_screen_game_in_front_pauses_every_hotkey_until_it_leaves() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    // The Open Pane binding, as the window applies the record's at
    // startup.
    launcher
        .sync_open_pane(key("ctrl+alt+space"))
        .expect("the binding registers");
    assert_eq!(
        system.registered(),
        ["ctrl+alt+g", "ctrl+alt+space"],
        "the Open Pane binding registered last"
    );
    game_mode_on(&launcher, &[]);
    assert!(
        fs::read_to_string(dirs.packages_dir().join("game-mode.json"))
            .unwrap()
            .contains("\"on\": true")
    );

    // The game comes to the front: every hotkey is released, the Open
    // Pane binding included, and the state the tray icon's tooltip says
    // is paused. No timer anywhere: the event alone did it.
    foreground.front(Some(r"C:\Games\game.exe"), true);
    assert!(launcher.hotkeys_paused());
    assert!(system.registered().is_empty());

    // It leaves: the hotkeys come back by themselves, through the same
    // registration path — the Open Pane binding first.
    foreground.front(Some(r"C:\Windows\notepad.exe"), false);
    assert!(!launcher.hotkeys_paused());
    assert_eq!(
        system.registered(),
        ["ctrl+alt+space", "ctrl+alt+g"],
        "the Open Pane binding returned first"
    );

    // And again, each way.
    foreground.front(Some(r"C:\Games\game.exe"), true);
    assert!(launcher.hotkeys_paused());
    assert!(system.registered().is_empty());
    foreground.front(Some(r"C:\Windows\notepad.exe"), false);
    assert_eq!(
        system.registered(),
        ["ctrl+alt+space", "ctrl+alt+g"],
        "the Open Pane binding returned first again"
    );
}

#[test]
fn a_listed_program_pauses_the_hotkeys_windowed_or_not() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    game_mode_on(&launcher, &["Helldivers.EXE"]);

    // A windowed game the system does not report as full-screen: its
    // program is one the settings list, matched by file name whatever
    // the case and wherever it is installed.
    foreground.front(Some(r"D:\Steam\steamapps\common\HD2\helldivers.exe"), false);
    assert!(launcher.hotkeys_paused());
    assert!(system.registered().is_empty());

    // A window of a program nothing lists, or one whose program could not
    // be read, is not a game.
    foreground.front(Some(r"D:\Steam\steamapps\common\HD2\other.exe"), false);
    assert!(!launcher.hotkeys_paused());
    foreground.front(None, false);
    assert!(!launcher.hotkeys_paused());
}

#[test]
fn turning_game_mode_off_brings_the_hotkeys_back() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    launcher.sync_open_pane(key("ctrl+alt+space")).unwrap();
    game_mode_on(&launcher, &[]);
    foreground.front(None, true);
    assert!(launcher.hotkeys_paused());
    assert!(system.registered().is_empty());

    // The game still in front, the user turns game mode off: the hotkeys
    // come back at once, the choice recorded.
    let off = GameMode {
        on: false,
        programs: Vec::new(),
    };
    block_on(launcher.set_game_mode(off)).expect("the settings are recorded");
    assert!(!launcher.hotkeys_paused());
    assert_eq!(system.registered(), ["ctrl+alt+space", "ctrl+alt+g"]);
    assert!(
        !fs::read_to_string(dirs.packages_dir().join("game-mode.json"))
            .unwrap()
            .contains("\"on\"")
    );

    // And the events still arrive, deciding nothing.
    foreground.front(None, true);
    assert!(!launcher.hotkeys_paused());
    assert_eq!(system.registered(), ["ctrl+alt+space", "ctrl+alt+g"]);
}

#[test]
fn game_mode_s_settings_are_kept_across_a_restart() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");
    game_mode_on(&launcher, &["game.exe"]);
    drop(launcher);

    // A fresh Pane over the same data folder: the settings are in force,
    // and the source's next report decides on them.
    let restarted_system = FakeSystem::new();
    let restarted = dirs.launcher(&restarted_system, &foreground);
    assert_eq!(
        restarted.game_mode(),
        GameMode {
            on: true,
            programs: vec!["game.exe".into()],
        }
    );
    assert!(
        fs::read_to_string(dirs.packages_dir().join("game-mode.json"))
            .unwrap()
            .contains("\"game.exe\"")
    );
    foreground.front(Some(r"E:\Games\game.exe"), false);
    assert!(restarted.hotkeys_paused());
}

#[test]
fn a_game_mode_change_that_cannot_be_recorded_goes_back() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");
    assign(&launcher, "Greeting", "ctrl+alt+g");
    game_mode_on(&launcher, &[]);
    foreground.front(None, true);
    assert!(launcher.hotkeys_paused());

    // The record cannot be written: what was recorded is back in Pane,
    // with the pause following it.
    let file = dirs.packages_dir().join("game-mode.json");
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    let off = GameMode {
        on: false,
        programs: Vec::new(),
    };
    let failed = block_on(launcher.set_game_mode(off));
    assert!(failed.is_err(), "{failed:?}");
    assert!(launcher.game_mode().on);
    assert!(launcher.hotkeys_paused());
    assert!(system.registered().is_empty());
}

#[test]
fn a_program_that_is_not_a_name_cannot_be_recorded() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let foreground = Arc::new(FakeForeground::default());
    let launcher = dirs.launcher(&system, &foreground);
    dirs.install(&launcher, "sample-settings");

    // A path would never match what game mode matches by, so it is
    // refused — nothing changes and nothing is written.
    let path = GameMode {
        on: true,
        programs: vec![r"C:\Games\game.exe".into()],
    };
    let refused = block_on(launcher.set_game_mode(path));
    assert!(refused.is_err(), "{refused:?}");
    assert!(
        refused.unwrap_err().contains("is not a program name"),
        "the reason names the program"
    );
    assert_eq!(launcher.game_mode(), GameMode::default());
    assert!(
        !dirs.packages_dir().join("game-mode.json").exists(),
        "nothing was written"
    );
}

#[test]
fn without_a_foreground_source_game_mode_is_not_offered() {
    let dirs = Dirs::new();
    let system = FakeSystem::new();
    let launcher = Launcher::with_packages(Runtime::start(), vec![], dirs.packages_dir())
        .with_hotkeys(system.clone());
    assert!(!launcher.game_mode_offered());
    let foreground = Arc::new(FakeForeground::default());
    assert!(dirs.launcher(&system, &foreground).game_mode_offered());
}
