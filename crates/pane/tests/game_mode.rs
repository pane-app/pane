//! Game mode through the Settings window's Keyboard page (#261, #125):
//! the on/off choice and the programs to treat as games are recorded in
//! Pane's own game-mode record, and the launcher's hotkeys pause and
//! return through the fake foreground source, as the system's own
//! foreground event hook would decide them on Windows. Drives the real
//! windows through GPUI's test platform; the fake source is the seam
//! `pane_core::game_mode` names, so the tests' launchers offer the
//! choice on every system.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::game_mode::{Foreground, ForegroundSource, ForegroundTold, GameMode};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, Runtime};
use tempfile::TempDir;

#[path = "support/setup.rs"]
mod setup;

use setup::{init_settings, settings_shortcut};

/// How long a test waits for anything it waits for.
const LIMIT: Duration = Duration::from_secs(10);

/// The fake system: what Pane registered, so a pause empties it and a
/// resume fills it back, the Open Pane default included.
#[derive(Default)]
struct FakeHotkeys {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn kind_unavailable(&self, shortcut: &Shortcut) -> Option<String> {
        // This fake models a system without Pane's own keyboard hook, as
        // macOS' and X11's adapters are: the kinds only the hook
        // recognizes are explained (macOS stands in where the test
        // binary runs on Windows, so a fresh data folder keeps today's
        // Open Pane default rather than taking the Windows key).
        let modeled = match pane_core::Platform::current() {
            Some(pane_core::Platform::Windows) => Some(pane_core::Platform::Macos),
            platform => platform,
        };
        pane_core::hotkeys::kinds_unavailable(shortcut, modeled)
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

impl FakeHotkeys {
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

/// The foreground source as the tests fake it: the seam's other half,
/// reporting what the test says is in front, as the Windows watcher
/// reports each window that comes to the front.
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

/// What one test keeps: Pane's data folder.
struct World {
    data: TempDir,
}

impl World {
    fn new() -> World {
        World {
            data: tempfile::tempdir().unwrap(),
        }
    }

    /// Pane's own records' folder, where `game-mode.json` is kept.
    fn records(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher over this data folder with the fake hotkeys and the
    /// fake foreground source, so the Keyboard page offers game mode.
    fn launcher(&self, hotkeys: &Arc<FakeHotkeys>, foreground: &Arc<FakeForeground>) -> Launcher {
        Launcher::with_packages(Runtime::start(), vec![], self.records())
            .with_hotkeys(hotkeys.clone())
            .with_foreground(foreground.clone())
    }
}

/// Opens the launcher window over `launcher`, then Settings on its
/// Keyboard page, tall enough that the whole page is in reach.
fn open_page(
    cx: &mut TestAppContext,
    launcher: Launcher,
    data: &Path,
) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.executor().allow_parking();
    init_settings(Some(data), cx);
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = cx
        .cx
        .update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<SettingsWindow>())
                .next()
        })
        .expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    settings_cx.simulate_resize(gpui::size(gpui::px(860.), gpui::px(1600.)));
    settings_cx.run_until_parked();
    click(&mut settings_cx, "section-Keyboard");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("keyboard").is_some(),
        "the Keyboard page is drawn"
    );
    (settings, settings_cx)
}

/// Clicks the element whose debug selector is `selector` in `cx`.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Whether the element whose debug selector is `selector` is drawn.
fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

/// Runs `cx` until the game mode record holds `held`, parking first so
/// that the change's task settles — its busy flag clears and the page
/// redraws — before the caller clicks anything again.
fn until_record(cx: &mut VisualTestContext, records: &Path, held: &str) {
    until_record_holds(cx, records, held, true);
}

/// Runs `cx` until the game mode record no longer holds `held`.
fn until_record_clears(cx: &mut VisualTestContext, records: &Path, held: &str) {
    until_record_holds(cx, records, held, false);
}

/// Runs `cx` until the game mode record holds `held`, or does not when
/// `held` is false.
fn until_record_holds(cx: &mut VisualTestContext, records: &Path, held: &str, want: bool) {
    let record = records.join("game-mode.json");
    let deadline = Instant::now() + LIMIT;
    loop {
        cx.run_until_parked();
        let holds = fs::read_to_string(&record)
            .map(|text| text.contains(held))
            .unwrap_or(false);
        if holds == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "game-mode.json never {} {held:?}",
            if want { "held" } else { "dropped" }
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn the_keyboard_page_toggles_game_mode_on_and_records_it(cx: &mut TestAppContext) {
    let world = World::new();
    let hotkeys = Arc::new(FakeHotkeys::default());
    let foreground = Arc::new(FakeForeground::default());
    let launcher = world.launcher(&hotkeys, &foreground);
    assert_eq!(launcher.game_mode(), GameMode::default());

    let (_settings, mut settings_cx) = open_page(cx, launcher.clone(), world.data.path());
    assert!(
        drawn(&mut settings_cx, "keyboard-game-mode"),
        "the Game mode section is drawn"
    );
    click(&mut settings_cx, "keyboard-game-mode");
    until_record(&mut settings_cx, &world.records(), "\"on\": true");
    assert!(launcher.game_mode().on);
    // The Open Pane default is registered beside the choice, untouched.
    assert_eq!(hotkeys.registered(), [Shortcut::open_pane_default().id()]);

    // Off again, recorded: the record holds no `on` while it is off.
    click(&mut settings_cx, "keyboard-game-mode");
    until_record_clears(&mut settings_cx, &world.records(), "\"on\": true");
    assert!(!launcher.game_mode().on);
}

#[gpui::test]
fn the_keyboard_page_names_programs_to_treat_as_games(cx: &mut TestAppContext) {
    let world = World::new();
    let hotkeys = Arc::new(FakeHotkeys::default());
    let foreground = Arc::new(FakeForeground::default());
    let launcher = world.launcher(&hotkeys, &foreground);
    let (_settings, mut settings_cx) = open_page(cx, launcher.clone(), world.data.path());

    click(&mut settings_cx, "keyboard-game-mode");
    until_record(&mut settings_cx, &world.records(), "\"on\": true");
    // A program is named in the field and added, listed with its Remove.
    click(&mut settings_cx, "keyboard-game-program-field");
    settings_cx.simulate_input("helldivers.exe");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "keyboard-game-add");
    until_record(&mut settings_cx, &world.records(), "helldivers.exe");
    assert!(
        drawn(&mut settings_cx, "keyboard-game-program-helldivers.exe"),
        "the program is listed"
    );
    assert_eq!(launcher.game_mode().programs, ["helldivers.exe"]);

    // Removed, and gone from the record.
    click(&mut settings_cx, "keyboard-game-remove-helldivers.exe");
    until_record_clears(&mut settings_cx, &world.records(), "helldivers.exe");
    assert!(launcher.game_mode().programs.is_empty());
    assert!(!drawn(
        &mut settings_cx,
        "keyboard-game-program-helldivers.exe"
    ));
}

#[gpui::test]
fn a_game_in_front_pauses_the_hotkeys_and_the_tooltip_state_says_so(cx: &mut TestAppContext) {
    let world = World::new();
    let hotkeys = Arc::new(FakeHotkeys::default());
    let foreground = Arc::new(FakeForeground::default());
    let launcher = world.launcher(&hotkeys, &foreground);
    let (_settings, mut settings_cx) = open_page(cx, launcher.clone(), world.data.path());

    click(&mut settings_cx, "keyboard-game-mode");
    until_record(&mut settings_cx, &world.records(), "\"on\": true");
    assert_eq!(hotkeys.registered(), [Shortcut::open_pane_default().id()]);

    // A full-screen game comes to the front: every registration is
    // released — the Open Pane binding included — and the state the tray
    // icon's tooltip says is paused. The event alone did it, no timer.
    foreground.front(Some(r"C:\Games\game.exe"), true);
    assert!(launcher.hotkeys_paused());
    assert!(hotkeys.registered().is_empty());

    // A windowed game the settings list pauses them too; add it first.
    click(&mut settings_cx, "keyboard-game-program-field");
    settings_cx.simulate_input("windowed.exe");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "keyboard-game-add");
    until_record(&mut settings_cx, &world.records(), "windowed.exe");
    assert!(
        launcher
            .game_mode()
            .programs
            .contains(&"windowed.exe".to_owned())
    );
    foreground.front(Some(r"E:\Games\Windowed.exe"), false);
    assert!(launcher.hotkeys_paused());

    // The game leaves the front: the hotkeys come back by themselves.
    foreground.front(Some(r"C:\Windows\notepad.exe"), false);
    assert!(!launcher.hotkeys_paused());
    assert_eq!(hotkeys.registered(), [Shortcut::open_pane_default().id()]);
}

#[gpui::test]
fn without_a_foreground_source_the_choice_is_not_offered(cx: &mut TestAppContext) {
    let world = World::new();
    let hotkeys = Arc::new(FakeHotkeys::default());
    // No foreground source: game mode is offered nowhere, and the switch
    // says so without taking the choice.
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], world.records()).with_hotkeys(hotkeys);
    assert!(!launcher.game_mode_offered());
    let (_settings, mut settings_cx) = open_page(cx, launcher.clone(), world.data.path());
    assert!(
        drawn(&mut settings_cx, "keyboard-game-mode"),
        "the Game mode section is drawn"
    );
    assert!(!drawn(&mut settings_cx, "keyboard-game-program-field"));
    click(&mut settings_cx, "keyboard-game-mode");
    settings_cx.run_until_parked();
    assert_eq!(launcher.game_mode(), GameMode::default());
    assert!(
        !world.records().join("game-mode.json").exists(),
        "nothing was recorded"
    );
}
