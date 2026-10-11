//! The Open Pane hotkey (#74): the application-owned global binding that
//! summons the launcher from any application, recorded on the Settings
//! window's General page and applied through the platform's global-shortcut
//! registration. Drives the real windows through GPUI's test platform, as
//! `window.rs` drives the launcher's; the system is a fake that records
//! what Pane registers and can be told which shortcuts another application
//! has, so registration failures, rollbacks, conflicts and restarts are
//! exercised at the same boundary a user sees them. The key's real OS
//! behavior (native registration, activation from another application, key
//! repeat at the OS level) is recorded natively in
//! `docs/evidence/settings-74/`.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{CallError, Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, settle};

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/packages.rs"]
mod packages;
#[path = "support/setup.rs"]
mod setup;
#[path = "support/wait.rs"]
mod wait;

use a11y::a11y;
use packages::package;
use setup::{init_settings, settings_shortcut};
use wait::until_record_holds;

/// The fake system: what Pane registered, and which shortcuts another
/// application has (registration refuses them, as the real adapters'
/// `Taken` does).
#[derive(Default)]
struct FakeSystem {
    registered: Mutex<Vec<Shortcut>>,
    taken: Mutex<Vec<Shortcut>>,
    /// Whether this system's adapter has a keyboard hook of its own, as
    /// Windows' does (#260, #268): the binding kinds #260 adds bind
    /// here, and the recorder's sessions come from the hook. A system
    /// without one — macOS, X11, and the fakes that model them — explains
    /// the kinds and offers no session, and a fresh data folder keeps
    /// today's Open Pane default rather than taking the Windows key.
    hooks: bool,
    /// The senders of the recording sessions handed out (#260), for the
    /// test to feed what the user pressed, as the hook adapter would
    /// report it.
    reporters: Mutex<Vec<pane_core::hotkeys::PressSender>>,
}

impl FakeSystem {
    /// A system whose adapter has a keyboard hook of its own, as Windows'
    /// does: the binding kinds bind, the recorder's sessions come from
    /// the hook, and a fresh data folder's Open Pane default on Windows
    /// is the Windows key alone (#268).
    fn hooking() -> FakeSystem {
        FakeSystem {
            hooks: true,
            ..FakeSystem::default()
        }
    }
}

impl Hotkeys for FakeSystem {
    fn unavailable(&self) -> Option<String> {
        None
    }

    // A system whose adapter has a keyboard hook takes the binding kinds
    // #260 adds; one without explains them, as the systems without one
    // do. It names the platform it models: the current one where that
    // has no hook, macOS standing in where the tests run on Windows — a
    // platform whose kinds the pure half says work — so the kinds are
    // refused wherever the tests run.
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

    fn recording(&self) -> Option<pane_core::hotkeys::RecordingSession> {
        if !self.hooks {
            return None;
        }
        let (sender, presses) = pane_core::hotkeys::channel();
        self.reporters.lock().unwrap().push(sender);
        Some(pane_core::hotkeys::RecordingSession::of(presses, || {}))
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        if self.taken.lock().unwrap().contains(shortcut) {
            return Err(HotkeyError::Taken);
        }
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

/// The fake system of a desktop where global hotkeys cannot be used at
/// all, whose explanation carries the guidance the General page must
/// show.
struct UnavailableSystem(String);

impl Hotkeys for UnavailableSystem {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn register(&self, _shortcut: &Shortcut) -> Result<(), HotkeyError> {
        Err(HotkeyError::Refused(self.0.clone()))
    }

    fn unregister(&self, _shortcut: &Shortcut) {}
}

/// What a fake system has registered, in order.
fn registered(system: &FakeSystem) -> Vec<Shortcut> {
    system.registered.lock().unwrap().clone()
}

/// Opens the Settings window over the launcher `cx` drives, on the page
/// the window opens on (General), as its own window context.
fn open_settings(cx: &mut VisualTestContext) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
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
    let settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    (settings, settings_cx)
}

/// Whether the window `handle` is the focused one, as the platform
/// reports.
fn is_active<T: Render + 'static>(handle: &WindowHandle<T>, cx: &mut VisualTestContext) -> bool {
    cx.cx.update(|cx| handle.is_active(cx)).unwrap_or(false)
}

/// Presses the Open Pane hotkey `shortcut`, as the system's adapter would
/// report it while any application has focus. The test platform's clock
/// does not move between events, so the press is first moved past the
/// window's repeat guard: two of these are two genuine presses, the
/// guard's window apart. The repeats of a key still held — presses that
/// arrive inside the guard, with the clock standing still as a repeat
/// does — are driven with the window directly in
/// `a_held_key_does_not_toggle_the_launcher_repeatedly`.
fn press(window: &gpui::Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
}

/// Whether the launcher window is hidden by the Open Pane hotkey (the
/// state the window drove, as the test platform's own visibility is not
/// observable from outside GPUI).
fn hidden(window: &gpui::Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// The launcher window's handle, for focus checks.
fn handle_of(cx: &mut VisualTestContext) -> WindowHandle<LauncherWindow> {
    cx.update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the launcher window")
}

/// Clicks the element whose debug selector is `selector` in `cx`.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Runs `cx` until the window's accessibility tree satisfies `done`,
/// reporting the tree when it never does, so a wait that stalls says
/// what the page was showing when it stalled.
fn until_diag(cx: &mut VisualTestContext, mut done: impl FnMut(&str) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let tree = a11y(cx);
        if done(&tree) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the page; tree {tree}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs `cx` until the settings record in `data` holds `id` as the Open
/// Pane hotkey: the save the page started is written off the window's
/// thread.
fn until_record(cx: &mut VisualTestContext, data: &Path, id: &str) {
    let held = format!("\"open_pane\": \"{id}\"");
    until_record_holds(cx, data, &held);
}

#[gpui::test]
fn the_default_hotkey_registers_at_startup_and_toggles_the_launcher(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let handle = handle_of(cx);

    // The record holds nothing yet: the provisional default is what the
    // window applied at startup, through the launcher's registration.
    let default = Shortcut::open_pane_default();
    assert_eq!(registered(&system), vec![default.clone()]);

    // Visible but without focus — the window as the test platform opens
    // it: the hotkey brings the launcher forward.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(is_active(&handle, cx), "the launcher took focus");
    assert!(
        !hidden(&window, cx),
        "a launcher without focus is not hidden"
    );

    // Focused: the hotkey hides the window — Pane keeps running, and the
    // same window and launcher are reused.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the focused launcher hid");
    assert!(
        cx.cx
            .update(|cx| cx.windows().contains(&AnyWindowHandle::from(handle))),
        "the hidden window is still the live launcher window"
    );

    // Hidden: the hotkey shows it again, focused, with its search —
    // typing lands in the query.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the hidden launcher was shown");
    assert!(is_active(&handle, cx));
    cx.simulate_input("zz");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zz"), "the search has focus");
    assert!(matches!(view.screen, Screen::Root { .. }));

    // And focused again, it hides again: repeated use is predictable.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));
}

#[gpui::test]
fn a_held_key_does_not_toggle_the_launcher_repeatedly(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let default = Shortcut::open_pane_default();

    // To hidden.
    press(&window, &default, cx);
    cx.run_until_parked();
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));

    // The repeats of a key still held arrive as presses of the same
    // shortcut, soon after the one that was accepted — with the clock
    // standing still, as a repeat does: they are ignored, so holding the
    // hotkey does not toggle again and again.
    for _ in 0..3 {
        window.update_in(cx, |window, w, cx| window.hotkey_pressed(&default, w, cx));
        cx.run_until_parked();
    }
    assert!(hidden(&window, cx), "the held key's repeats did nothing");

    // A genuine press, after the guard's window, toggles again.
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(&default, w, cx));
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "a later press is a new press");
}

#[gpui::test]
fn the_settings_windows_focus_does_not_count_and_it_stays_open(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let handle = handle_of(cx);
    let default = Shortcut::open_pane_default();

    let (settings, mut settings_cx) = open_settings(cx);
    let settings_handle = AnyWindowHandle::from(settings);
    assert!(is_active(&settings, &mut settings_cx));
    assert!(!is_active(&handle, cx), "the launcher does not have focus");

    // The hotkey while Settings has focus: the launcher is brought
    // forward — Settings' focus is not the launcher's — and Settings
    // stays open.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the launcher was not hidden");
    assert!(is_active(&handle, cx), "the launcher took focus");
    assert!(
        cx.cx.update(|cx| cx.windows().contains(&settings_handle)),
        "Settings stayed open"
    );

    // And with the launcher focused, the hotkey hides the launcher only.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));
    assert!(
        cx.cx.update(|cx| cx.windows().contains(&settings_handle)),
        "Settings stayed open"
    );
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("general").is_some(),
        "Settings still answers on its own page"
    );

    // A hidden launcher keeps running: the same live window answers, and
    // the hotkey shows it again.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx));
}

#[gpui::test]
fn recording_a_new_hotkey_swaps_the_registration_and_persists_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("general").is_some(),
        "the window opens on the General page"
    );

    // Click the recorder, then press the keys of the new binding.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    let recorded = Shortcut::parse("ctrl+alt+b").unwrap();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();

    // The binding was swapped with the system: the new one registered,
    // the default released.
    assert_eq!(registered(&system), vec![recorded.clone()]);
    // The record holds it.
    until_record(&mut settings_cx, data.path(), "ctrl+alt+b");
    // The page shows it, and the recorder is done listening.
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("Open Pane with {recorded}")),
        "the page shows the binding, {tree}"
    );
    assert!(
        !tree.contains("Recording;"),
        "the recorder is no longer listening, {tree}"
    );

    // The new binding summons the launcher; the default no longer does.
    let handle = handle_of(cx);
    press(&window, &recorded, cx);
    cx.run_until_parked();
    assert!(is_active(&handle, cx), "the recorded hotkey activates Pane");
    let default = Shortcut::open_pane_default();
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(is_active(&handle, cx), "the released default does nothing");
    assert!(!hidden(&window, cx));
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing opened");
    assert_eq!(view.status, Status::Idle);
}

#[gpui::test]
fn a_shortcut_another_application_has_is_refused_and_keeps_the_binding(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let taken = Shortcut::parse("ctrl+alt+b").unwrap();
    system.taken.lock().unwrap().push(taken.clone());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let default = Shortcut::open_pane_default();
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // Another application has Ctrl+Alt+B: the system refuses it.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();

    // The refusal is explained on the page, and the recorder keeps
    // listening for another try.
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!(
            "{taken} cannot be used: another application or the system already uses it"
        )),
        "the refusal is explained, {tree}"
    );
    assert!(
        tree.contains("Recording; Open Pane with"),
        "the recorder keeps listening, {tree}"
    );
    // The default binding keeps working, and nothing was kept or saved.
    assert_eq!(registered(&system), vec![default]);
    assert!(!data.path().join("settings.json").exists());

    // Another try lands: the keys are free now.
    system.taken.lock().unwrap().clear();
    settings_cx.simulate_keystrokes("ctrl-alt-c");
    settings_cx.run_until_parked();
    let recorded = Shortcut::parse("ctrl+alt+c").unwrap();
    assert_eq!(registered(&system), vec![recorded]);
    until_record(&mut settings_cx, data.path(), "ctrl+alt+c");
}

#[gpui::test]
fn a_collision_with_a_command_hotkey_is_refused(cx: &mut TestAppContext) {
    let (sources, data): (TempDir, TempDir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let system = Arc::new(FakeSystem::default());
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    // The window opens on the package's preview, as the hotkey tests'
    // fixture does.
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    // As the hotkey tests' fixture waits: the preview's package is
    // prepared off the window's thread, and the install's Enter must find
    // it ready, not the still-loading preview.
    settle(&window, cx);

    // Install the package and give its command a hotkey, through the
    // launcher's own flow (the one Settings drives, #168).
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    enter_flow(&window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Hotkey { .. }),
        "{:?}",
        view.screen
    );
    cx.simulate_keystrokes("ctrl-alt-p");
    let command_shortcut = Shortcut::parse("ctrl+alt+p").unwrap();
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(format!("{command_shortcut} now opens Say hello"))
    );

    // Recording the same keys as the Open Pane hotkey is refused: another
    // command's binding is never silently overwritten.
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-p");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("{command_shortcut} already opens Say hello")),
        "the collision is explained, {tree}"
    );
    // Both bindings keep working exactly as they were: the command's
    // registered, the Open Pane default still the page's binding.
    assert_eq!(
        registered(&system),
        vec![Shortcut::open_pane_default(), command_shortcut.clone()]
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "nothing was kept"
    );

    // Escape cancels the recording, changing nothing.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(
        registered(&system),
        vec![Shortcut::open_pane_default(), command_shortcut]
    );
}

#[gpui::test]
fn a_save_that_fails_rolls_the_registration_back(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // A change that lands and is saved.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();
    let kept = Shortcut::parse("ctrl+alt+b").unwrap();
    assert_eq!(
        registered(&system),
        vec![kept.clone()],
        "the recorded binding is the one that works"
    );
    until_record(&mut settings_cx, data.path(), "ctrl+alt+b");

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    fs::remove_file(data.path().join("settings.json")).unwrap();
    fs::create_dir(data.path().join("settings.json")).unwrap();

    // Another change: it registers with the system, but cannot be saved.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-c");
    until_diag(&mut settings_cx, |tree| {
        tree.contains("Pane could not save your choice")
    });

    // The failure is explained, and the binding the record holds is the
    // one that works: the change that could not be saved did not discard
    // the previous working binding.
    assert_eq!(registered(&system), vec![kept.clone()]);
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("Open Pane with {kept}")),
        "the page shows what the record holds, {tree}"
    );

    // The next press still uses the kept binding: it summons the
    // launcher, and the press after it hides it — the choice that could
    // not be saved never took the binding's place.
    press(&window, &kept, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the kept binding still summons Pane");
    press(&window, &kept, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the kept binding still toggles Pane");
}

#[gpui::test]
fn the_recorded_hotkey_is_registered_by_a_fresh_application(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // A recorded choice.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();
    let recorded = Shortcut::parse("ctrl+alt+b").unwrap();
    until_record(&mut settings_cx, data.path(), "ctrl+alt+b");

    // A fresh application over the same data folder: a new app, nothing
    // carried over but the executors; the settings the record alone.
    let mut fresh = cx.cx.new_app();
    fresh.update(pane::bind_keys);
    fresh.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let fresh_system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(fresh_system.clone());
    let (window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.run_until_parked();

    // The fresh application registered what the record holds, and that
    // binding summons the launcher.
    assert_eq!(registered(&fresh_system), vec![recorded.clone()]);
    let handle = fresh_cx
        .update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the fresh launcher window");
    press(&window, &recorded, fresh_cx);
    fresh_cx.run_until_parked();
    assert!(
        fresh_cx
            .cx
            .update(|cx| handle.is_active(cx))
            .unwrap_or(false),
        "the recorded hotkey summons the fresh launcher"
    );
}

#[gpui::test]
fn escape_cancels_the_recorder_and_captured_keys_do_not_act(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let default = Shortcut::open_pane_default();
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // Recording: the keys pressed while the recorder listens are
    // captured, not acted on. The sidebar's navigation keys move nothing
    // (the page stays the General one).
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("down up");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("general").is_some(),
        "the sidebar did not move to another page"
    );
    assert!(
        settings_cx.debug_bounds("keyboard").is_none(),
        "no other page is showing"
    );
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Recording; Open Pane with"),
        "the recorder is still listening, {tree}"
    );

    // Escape cancels: nothing changed, nothing was registered or saved.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![default.clone()]);
    assert!(!data.path().join("settings.json").exists());
    let tree = a11y(&mut settings_cx);
    assert!(
        !tree.contains("Recording;"),
        "the recorder is no longer listening, {tree}"
    );

    // Tab leaves recording too, changing nothing; so does a second click.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("tab");
    settings_cx.run_until_parked();
    assert!(
        !a11y(&mut settings_cx).contains("Recording;"),
        "Tab cancels"
    );
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    assert!(a11y(&mut settings_cx).contains("Recording;"));
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    assert!(
        !a11y(&mut settings_cx).contains("Recording;"),
        "a second click cancels"
    );
    assert_eq!(registered(&system), vec![default.clone()]);

    // A key the recorder cannot use, pressed while it listens, is
    // explained and keeps it listening (a hotkey needs a modifier, so it
    // does not take over typing) — and still does not act.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("b");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("does not take over typing"),
        "the key without a modifier is explained, {tree}"
    );
    assert!(
        tree.contains("Recording; Open Pane with"),
        "the recorder keeps listening, {tree}"
    );
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![default]);
}

#[gpui::test]
fn resetting_returns_to_the_default_through_the_same_checks(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // A recorded choice, away from the default.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();
    let recorded = Shortcut::parse("ctrl+alt+b").unwrap();
    until_record(&mut settings_cx, data.path(), "ctrl+alt+b");
    assert_eq!(registered(&system), vec![recorded]);

    // Reset: back to the provisional default, registered and saved
    // through the same path a recording takes.
    click(&mut settings_cx, "open-pane-reset");
    settings_cx.run_until_parked();
    let default = Shortcut::open_pane_default();
    assert_eq!(registered(&system), vec![default.clone()]);
    until_record(&mut settings_cx, data.path(), &default.id());
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("Open Pane with {default}")),
        "the page shows the default, {tree}"
    );

    // And the reset binding toggles.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(is_active(&handle_of(cx), cx));
}

#[gpui::test]
fn the_hotkey_stays_available_while_the_runtime_has_failed(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(
        Err(CallError::RuntimeUnavailable("the runtime is gone".into())),
        Vec::new(),
    )
    .with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let default = Shortcut::open_pane_default();
    let handle = handle_of(cx);

    // The application-owned binding registered as ever, with no extension
    // runtime to run.
    assert_eq!(registered(&system), vec![default.clone()]);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(is_active(&handle, cx), "the launcher was summoned");
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the launcher was hidden");
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx));
}

#[gpui::test]
fn where_global_hotkeys_cannot_be_used_the_page_explains(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let reason = "Not available on Linux with Wayland: Wayland does not let an application \
                  see keys pressed in other applications, and Pane does not use the desktop's \
                  global shortcuts portal yet. Assign a shortcut in the desktop's keyboard \
                  settings instead, or run Pane on X11.";
    let system = Arc::new(UnavailableSystem(reason.to_owned()));
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // The General page says the binding is not active, with the adapter's
    // own explanation: the Wayland limitation and the desktop-shortcut
    // guidance, not a silent failure — the Settings window stays the
    // entry point that explains it.
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Not active: Not available on Linux with Wayland"),
        "the limitation is explained, {tree}"
    );
    assert!(
        tree.contains("Assign a shortcut in the desktop's keyboard settings instead"),
        "the guidance is given, {tree}"
    );

    // A recording is refused with the same explanation, and keeps the
    // state as it was.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Not available on Linux with Wayland"),
        "the refusal carries the explanation, {tree}"
    );
    assert!(
        tree.contains("Recording; Open Pane with"),
        "the recorder keeps listening, {tree}"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "nothing was kept"
    );
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();

    // The launcher window itself is as it was: a registration that cannot
    // happen never hid the only usable window.
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

#[gpui::test]
fn the_open_pane_recorder_records_the_kinds_a_session_reports(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::hooking());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // The names the row shows for the kinds: the platform's own for a
    // modifier (Control on macOS), "Right " before a side's.
    let ctrl = if cfg!(target_os = "macos") {
        "Control"
    } else {
        "Ctrl"
    };
    // Click the recorder: it listens, and a recording session starts with
    // the adapter (#260) — the fake hands one whose reports the test feeds
    // as the user's presses: the kinds the window's own keystrokes cannot
    // name, pressed without the Start menu the Windows key alone opens. A
    // side of a modifier records first: the row names it as Windows does,
    // "Right Ctrl".
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    let reported = system.reporters.lock().unwrap().clone();
    assert_eq!(reported.len(), 1, "the recorder asked for one session");
    let right_ctrl = Shortcut::parse("tap:rctrl").unwrap();
    reported[0].send(right_ctrl.clone());
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![right_ctrl.clone()]);
    until_record(&mut settings_cx, data.path(), "tap:rctrl");
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("Open Pane with Right {ctrl}")),
        "the row names the side, {tree}"
    );

    // A double tap records the same way: "Ctrl Ctrl".
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    let reported = system.reporters.lock().unwrap().clone();
    let double = Shortcut::parse("double:ctrl").unwrap();
    reported[1].send(double.clone());
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![double.clone()]);
    until_record(&mut settings_cx, data.path(), "double:ctrl");
    assert!(
        a11y(&mut settings_cx).contains(&format!("Open Pane with {ctrl} {ctrl}")),
        "the row names the double tap"
    );

    // The Windows key alone: "Win" on Windows, the platform's own name
    // elsewhere; the recorder shows the binding and the record holds its
    // textual form.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    let reported = system.reporters.lock().unwrap().clone();
    let tap = Shortcut::parse("tap:win").unwrap();
    reported[2].send(tap.clone());
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![tap.clone()]);
    until_record(&mut settings_cx, data.path(), "tap:win");
    assert!(
        a11y(&mut settings_cx).contains(&format!("Open Pane with {tap}")),
        "the row names the tap"
    );

    // Escape still cancels, session and all: the binding is kept, the
    // recorder stops listening, and no fourth session outlives it.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    assert_eq!(system.reporters.lock().unwrap().len(), 4);
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![tap.clone()]);
    assert!(
        !a11y(&mut settings_cx).contains("Recording;"),
        "the recorder is no longer listening"
    );
    until_record_holds(&mut settings_cx, data.path(), "\"open_pane\": \"tap:win\"");
}

#[gpui::test]
fn a_fresh_data_folder_starts_with_the_fresh_install_default(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    // The fake whose adapter has a keyboard hook, as Windows' does: a
    // fresh data folder on Windows gets the Windows key alone as its Open
    // Pane hotkey; the other systems, whose adapters have no hook to
    // recognize the tap, keep today's default (#268, ADR 0039).
    let system = Arc::new(FakeSystem::hooking());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));

    let fresh = if cfg!(target_os = "windows") {
        Shortcut::parse("tap:win").unwrap()
    } else {
        Shortcut::open_pane_default()
    };
    assert_eq!(registered(&system), vec![fresh.clone()]);
    // The resolution writes nothing: the folder stays as fresh on the
    // disk as it was, so the next save of any preference is what records
    // the binding the fresh install came to hold.
    assert!(!data.path().join("settings.json").exists());

    // The binding summons the launcher as today's default does: shown
    // without focus, it takes the focus; focused, it hides.
    press(&window, &fresh, cx);
    cx.run_until_parked();
    assert!(
        is_active(&handle_of(cx), cx),
        "the fresh default summons the launcher"
    );
    press(&window, &fresh, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the focused launcher hid");
}

#[gpui::test]
fn a_settings_record_keeps_its_hotkey_rather_than_taking_the_fresh_default(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    // A record an earlier Pane wrote: whatever it names is what this Pane
    // registers, even where the adapter could take the Windows key — only
    // a fresh data folder gets the fresh-install default (#268).
    std::fs::write(
        data.path().join("settings.json"),
        r#"{ "version": 2, "open_pane": "ctrl+alt+b" }"#,
    )
    .unwrap();
    let system = Arc::new(FakeSystem::hooking());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    assert_eq!(
        registered(&system),
        vec![Shortcut::parse("ctrl+alt+b").unwrap()],
        "the record's hotkey is registered, not the fresh default"
    );
}

#[gpui::test]
fn the_use_the_windows_key_choice_sets_it_in_one_step(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::hooking());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // Another hotkey is set, so the choice shows beside the recorder —
    // on Windows the fresh default would have been the Windows key, so
    // one is recorded first; everywhere else the default already is
    // another hotkey.
    click(&mut settings_cx, "open-pane-recorder");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    settings_cx.run_until_parked();
    until_record(&mut settings_cx, data.path(), "ctrl+alt+b");

    // The choice is there, with the note that says where the Start menu
    // remains and that only the lone tap changes.
    let tree = a11y(&mut settings_cx);
    assert!(tree.contains("Use the Windows key"), "{tree}");
    assert!(tree.contains("Start menu stays reachable"), "{tree}");
    assert!(tree.contains("Ctrl+Esc"), "{tree}");
    assert!(tree.contains("Win+E, Win+D, Win+L"), "{tree}");

    // One step: the Windows key alone replaces the binding — registered
    // before the one it replaces is released — and the record holds its
    // textual form.
    click(&mut settings_cx, "general-use-windows-key");
    settings_cx.run_until_parked();
    let windows_key = Shortcut::parse("tap:win").unwrap();
    assert_eq!(registered(&system), vec![windows_key.clone()]);
    until_record_holds(&mut settings_cx, data.path(), "\"open_pane\": \"tap:win\"");
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("Open Pane with {windows_key}")),
        "the row names the binding, {tree}"
    );
    assert!(
        !tree.contains("Use the Windows key"),
        "the choice is gone once taken, {tree}"
    );

    // The Windows key alone summons the launcher.
    press(&window, &windows_key, cx);
    cx.run_until_parked();
    assert!(is_active(&handle_of(cx), cx));
}

#[gpui::test]
fn where_the_windows_key_alone_cannot_be_taken_the_choice_explains_itself(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    // The fake without a hook, as the other systems' adapters are: the
    // choice is explained rather than offered — the reason under its
    // name, the row dimmed — and today's default keeps working (which is
    // also what a fresh install here falls back to registering, #268).
    let system = Arc::new(FakeSystem::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(system.clone());
    init_settings(Some(data.path()), cx);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_settings(cx);
    settings_cx.run_until_parked();

    // Another hotkey is set (today's default), so the choice shows,
    // explained: the kinds the Windows key alone needs do not work here.
    let tree = a11y(&mut settings_cx);
    assert!(tree.contains("Use the Windows key"), "{tree}");
    assert!(
        tree.contains("lone modifier taps work only on Windows"),
        "the reason is shown, {tree}"
    );

    // The choice takes no step: the click does nothing, the binding stays
    // the default, and nothing is saved.
    click(&mut settings_cx, "general-use-windows-key");
    settings_cx.run_until_parked();
    assert_eq!(registered(&system), vec![Shortcut::open_pane_default()]);
    assert!(!data.path().join("settings.json").exists());
}
