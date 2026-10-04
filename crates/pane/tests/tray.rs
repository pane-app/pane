//! The tray or menu-bar entry (#79): Pane's item in the system's tray or
//! menu bar, whose menu offers Open Pane, Settings and Quit, and the
//! General page's visibility preference that shows and hides it. Drives
//! the real windows through GPUI's test platform, as `open_pane.rs` does;
//! the system is a fake that records what Pane shows and hides and can be
//! told to refuse, so the preference's application, persistence,
//! rollbacks and the platform's lack of an entry are exercised at the
//! same boundary a user sees them. The menu's selections are driven
//! through the window's own dispatch (`LauncherWindow::tray_selected`),
//! the seam the application's selection loop calls. The entry's real OS
//! behavior (the icon in the notification area, the menu's labels, the
//! status item in the menu bar, the system's refusal of an add) is
//! recorded natively in `docs/evidence/settings-79/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, prelude::*};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::tray::{Tray, TrayAction, TrayError};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// The fake native entry: what Pane showed and hid, in order, and the
/// failure to inject into the next change (as the real adapters' refusals
/// arrive).
#[derive(Default)]
struct FakeTray {
    calls: Mutex<Vec<bool>>,
    fail: Mutex<Option<TrayError>>,
}

impl FakeTray {
    /// Makes the next change fail with `error`.
    fn refuse_next(&self, error: TrayError) {
        *self.fail.lock().unwrap() = Some(error);
    }

    /// What Pane showed and hid, in order.
    fn calls(&self) -> Vec<bool> {
        self.calls.lock().unwrap().clone()
    }
}

impl Tray for FakeTray {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        // The failure to inject is taken out under one lock, so the
        // refused path never locks again while holding it.
        if let Some(error) = self.fail.lock().unwrap().take() {
            return Err(error);
        }
        self.calls.lock().unwrap().push(visible);
        Ok(())
    }
}

/// The fake of a platform whose tray or menu bar Pane cannot use at all,
/// recording what was asked of it, whose explanation the General page
/// must carry.
struct UnavailableTray {
    why: String,
    asked: Mutex<Vec<bool>>,
}

impl Tray for UnavailableTray {
    fn unavailable(&self) -> Option<String> {
        Some(self.why.clone())
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        self.asked.lock().unwrap().push(visible);
        Err(TrayError::Unavailable(self.why.clone()))
    }
}

/// The fake global-hotkey system, as `open_pane.rs`' is: what Pane
/// registered, in order.
#[derive(Default)]
struct FakeHotkeys {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeHotkeys {
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

/// The fake of a desktop where global hotkeys cannot be used at all, as
/// `open_pane.rs`' `UnavailableSystem` is.
struct UnavailableHotkeys(String);

impl Hotkeys for UnavailableHotkeys {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn register(&self, _shortcut: &Shortcut) -> Result<(), HotkeyError> {
        Err(HotkeyError::Refused(self.0.clone()))
    }

    fn unregister(&self, _shortcut: &Shortcut) {}
}

/// The keystroke that opens Settings on this platform.
fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-,"
    }
}

/// Initializes the settings record of `data` in `cx`, as the binary does
/// before its first window opens.
fn init_settings(data: Option<&Path>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            data.map(|data| data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
}

/// Opens the launcher window over `launcher`, with the settings record of
/// `data` initialized and `tray` attached, as the binary orders them.
fn open<'a>(
    cx: &'a mut TestAppContext,
    launcher: Launcher,
    data: Option<&Path>,
    tray: Arc<dyn Tray>,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    init_settings(data, cx);
    cx.update(|cx| pane::settings::attach_tray(tray, cx));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

/// Presses the Open Pane hotkey `shortcut`, as the system's adapter would
/// report it, past the window's repeat guard.
fn press(window: &gpui::Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
}

/// Selects `action` of the native menu, as the adapter would report it:
/// the same dispatch the application's selection loop makes.
fn select(window: &gpui::Entity<LauncherWindow>, action: TrayAction, cx: &mut VisualTestContext) {
    window.update_in(cx, |window, w, cx| window.tray_selected(action, w, cx));
}

/// Whether the launcher window is hidden by the Open Pane hotkey (the
/// state the window drove, as the test platform's own visibility is not
/// observable from outside GPUI).
fn hidden(window: &gpui::Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Whether the window `handle` is the focused one, as the platform
/// reports.
fn is_active<T: Render + 'static>(handle: &WindowHandle<T>, cx: &mut VisualTestContext) -> bool {
    cx.cx.update(|cx| handle.is_active(cx)).unwrap_or(false)
}

/// The launcher window's handle, for focus checks.
fn handle_of(cx: &mut VisualTestContext) -> WindowHandle<LauncherWindow> {
    cx.update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the launcher window")
}

/// The one Settings window, if one is open: the window list is the
/// one-window registry the app itself uses.
fn settings_window(cx: &mut TestAppContext) -> Option<WindowHandle<SettingsWindow>> {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<SettingsWindow>())
            .next()
    })
}

/// Clicks the element whose debug selector is `selector` in `cx`.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
}

/// The accessibility tree of the window `cx` drives, as raw JSON, forced
/// on so the tree is built regardless of platform accessibility.
fn a11y(cx: &mut VisualTestContext) -> String {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    cx.update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree")
}

/// Runs `cx` until `done` returns a value, so that work arriving from
/// other threads (a record being written) has landed.
fn until<T>(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if let Some(value) = done(cx) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the window to draw"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs `cx` until the window's accessibility tree satisfies `done`,
/// reporting the tree when it never does.
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

/// Runs `cx` until the settings record in `data` holds `trayVisible` as
/// `shown` (the record's camelCase field name, as the launch-at-login
/// choice's is): the save the page started is written off the window's
/// thread.
fn until_record(cx: &mut VisualTestContext, data: &Path, shown: bool) {
    let record = data.join("settings.json");
    let held = format!("\"trayVisible\": {shown}");
    until(cx, |_| {
        fs::read_to_string(&record)
            .ok()
            .filter(|text| text.contains(&held))?;
        Some(())
    });
}

/// A fake tray whose changes succeed, whose calls a test reads.
fn tray() -> Arc<FakeTray> {
    Arc::new(FakeTray::default())
}

/// Writes a package folder whose one command is the Rust sample, as
/// `open_pane.rs`' fixture does.
fn package(folder: &Path) -> PathBuf {
    let guest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }]
}"#,
    )
    .unwrap();
    fs::copy(guest, folder.join("hello.wasm")).unwrap();
    folder.to_path_buf()
}

#[gpui::test]
fn the_entry_the_record_names_is_shown_when_it_attaches(cx: &mut TestAppContext) {
    // A record that hides the entry.
    let data = tempfile::tempdir().unwrap();
    fs::write(
        data.path().join("settings.json"),
        r#"{ "version": 1, "trayVisible": false }"#,
    )
    .unwrap();
    let system = tray();
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    cx.run_until_parked();

    // The entry Pane starts with is the one the record holds, hidden.
    assert_eq!(system.calls(), vec![false]);

    // A fresh application over no record at all: the entry the default
    // names is shown. Nothing is saved — the default is not a change.
    let mut fresh = cx.cx.new_app();
    let shown = tray();
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, fresh_cx) = open(&mut fresh, launcher, None, shown.clone());
    fresh_cx.run_until_parked();
    assert_eq!(shown.calls(), vec![true]);
}

#[gpui::test]
fn the_toggle_hides_and_shows_the_entry_and_saves_the_preference(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = tray();
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    cx.run_until_parked();

    // The General page's toggle, in the Settings window the launcher's
    // local shortcut opens, on the page it opens on.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_window(&mut cx.cx).expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    assert!(
        settings_cx.debug_bounds("tray-visibility").is_some(),
        "the toggle is drawn on General"
    );

    // Hiding: the entry is hidden with the system, and the choice is
    // saved as what the record holds.
    click(&mut settings_cx, "tray-visibility");
    settings_cx.run_until_parked();
    assert_eq!(system.calls(), vec![true, false], "the entry was hidden");
    until_record(&mut settings_cx, data.path(), false);

    // Showing it again: the same entry, back with the system, and the
    // record holds the preference the page now shows.
    click(&mut settings_cx, "tray-visibility");
    settings_cx.run_until_parked();
    assert_eq!(system.calls(), vec![true, false, true]);
    until_record(&mut settings_cx, data.path(), true);

    // The window is untouched by the preference: it stayed open, and the
    // entry's hide did not hide it.
    assert!(!hidden(&window, cx), "the launcher window stayed visible");
    assert_eq!(
        cx.cx.update(|cx| cx.windows().len()),
        2,
        "the launcher and the one Settings window"
    );
}

#[gpui::test]
fn a_change_the_system_refuses_is_explained_and_saved_as_nothing(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = tray();
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    cx.run_until_parked();

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_window(&mut cx.cx).expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);

    // The system refuses the hide: the reason is explained on the page,
    // the entry is unchanged, and nothing is kept or saved — the record
    // file is not even made.
    system.refuse_next(TrayError::Refused("the notification area is full".into()));
    click(&mut settings_cx, "tray-visibility");
    until_diag(&mut settings_cx, |tree| {
        tree.contains("the system refused it: the notification area is full")
    });
    assert_eq!(system.calls(), vec![true], "the entry was not hidden");
    assert!(!data.path().join("settings.json").exists());

    // The toggle tries again and lands: the entry is hidden and the
    // choice saved, so the refusal did not leave the preference stuck.
    click(&mut settings_cx, "tray-visibility");
    settings_cx.run_until_parked();
    assert_eq!(system.calls(), vec![true, false]);
    until_record(&mut settings_cx, data.path(), false);
}

#[gpui::test]
fn a_platform_without_an_entry_is_explained_not_toggled(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let why = "Not available on Linux: the desktop's tray speaks \
               StatusNotifierItem over DBus, which Pane does not speak yet";
    let system = Arc::new(UnavailableTray {
        why: why.into(),
        asked: Mutex::default(),
    });
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    cx.run_until_parked();

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_window(&mut cx.cx).expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);

    // The page carries the platform's own explanation beside the row,
    // without any change attempted: the switch is not even offered where
    // the platform has no entry, as the launch-at-login switch is not
    // where that integration cannot manage a registration.
    until_diag(&mut settings_cx, |tree| {
        tree.contains(why) && tree.contains("Show in")
    });

    // A click on the unoffered switch does nothing at all: the
    // unavailable entry is not represented as a successful toggle, no
    // change is asked of the adapter beyond the startup application the
    // record's preference made, and nothing is kept or saved.
    click(&mut settings_cx, "tray-visibility");
    settings_cx.run_until_parked();
    assert_eq!(system.asked.lock().unwrap().clone(), vec![true]);
    assert!(!data.path().join("settings.json").exists());
}

#[gpui::test]
fn open_pane_from_the_tray_summons_a_hidden_launcher_and_never_hides_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = tray();
    let hotkeys = Arc::new(FakeHotkeys::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(hotkeys);
    let (window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    let handle = handle_of(cx);
    let default = Shortcut::open_pane_default();

    // The hotkey's two presses: bring forward, then hide — the state the
    // tray must work from.
    press(&window, &default, cx);
    cx.run_until_parked();
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the launcher was hidden");

    // The tray's Open Pane summons it: shown, focused, with its search —
    // typing lands in the query — and no second window comes of it.
    select(&window, TrayAction::OpenPane, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the hidden launcher was shown");
    assert!(is_active(&handle, cx), "the summoned launcher took focus");
    cx.simulate_input("zz");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zz"), "the search has focus");
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(
        cx.cx.update(|cx| cx.windows().len()),
        1,
        "the same launcher window, no second one"
    );

    // And with the launcher focused, the tray's Open Pane does not hide
    // it: only the hotkey toggles, because its press is the user's other
    // hand on the same control; the tray's item says Open Pane and does
    // only that.
    select(&window, TrayAction::OpenPane, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the tray never hides the launcher");
    assert!(is_active(&handle, cx));
}

#[gpui::test]
fn settings_from_the_tray_opens_and_focuses_the_one_window(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = tray();
    let hotkeys = Arc::new(FakeHotkeys::default());
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(hotkeys);
    let (window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    let default = Shortcut::open_pane_default();

    // The launcher is hidden: the entry's actions must not need it.
    press(&window, &default, cx);
    cx.run_until_parked();
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));

    // Settings opens from the tray: one window, focused, while the
    // launcher stays hidden.
    select(&window, TrayAction::Settings, cx);
    cx.run_until_parked();
    let settings = settings_window(&mut cx.cx).expect("Settings opened");
    assert!(is_active(&settings, cx), "the Settings window took focus");
    assert!(hidden(&window, cx), "the launcher stayed hidden");
    assert_eq!(
        cx.cx.update(|cx| cx.windows().len()),
        2,
        "the launcher and the one Settings window, no duplicates"
    );

    // A second selection focuses the same window: the settings requests
    // converge, and no second window or extension runtime is started —
    // the Settings window shares the launcher the window holds.
    select(&window, TrayAction::Settings, cx);
    cx.run_until_parked();
    assert_eq!(
        settings_window(&mut cx.cx),
        Some(settings),
        "the one window"
    );
    assert_eq!(
        cx.cx.update(|cx| cx.windows().len()),
        2,
        "still the two windows"
    );
    assert!(is_active(&settings, cx), "the one window was focused");
}

#[gpui::test]
fn quit_from_the_tray_releases_the_entry_and_the_hotkey_registrations(cx: &mut TestAppContext) {
    let (sources, data): (TempDir, TempDir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    // A command hotkey a Pane that ran before recorded, which the
    // installed package's command is offered with: both it and the
    // application's own binding are registered while Pane runs.
    let command = format!("{}#hello", PackageIdentity::local(&folder).unwrap().key());
    let dir = data.path().join("extensions");
    fs::create_dir_all(&dir).unwrap();
    // The record is built with the JSON macros, whose escaping carries
    // whatever the identity's key holds — a local folder's key is a path,
    // with backslashes on Windows that a format string would leave raw.
    let mut hotkeys = serde_json::Map::new();
    hotkeys.insert(command, serde_json::json!("ctrl+alt+g"));
    fs::write(
        dir.join("hotkeys.json"),
        serde_json::json!({ "version": 1, "hotkeys": hotkeys }).to_string(),
    )
    .unwrap();

    let hotkeys = Arc::new(FakeHotkeys::default());
    let system = tray();
    let launcher =
        Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"))
            .with_hotkeys(hotkeys.clone());
    init_settings(Some(data.path()), cx);
    cx.update(|cx| pane::settings::attach_tray(system.clone(), cx));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    // The window opens on the package's preview; Enter installs it, which
    // registers its command's recorded hotkey.
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    let default = Shortcut::open_pane_default();
    let recorded = Shortcut::parse("ctrl+alt+g").unwrap();
    let registered = hotkeys.registered.lock().unwrap().clone();
    assert!(
        registered.contains(&recorded),
        "the command's hotkey is registered, {registered:?}"
    );
    assert!(
        registered.contains(&default),
        "the application's own binding is registered, {registered:?}"
    );

    // The tray's Quit: Pane's own resources are released — the native
    // entry hidden, every global hotkey registration let go, the
    // application's own binding among them — and then the platform is
    // asked to quit, which the test platform does not act on (nothing
    // here can assert a process exit; the native run records it, and
    // the runtime's helpers go with the quit hooks the shutdown runs).
    select(&window, TrayAction::Quit, cx);
    cx.run_until_parked();
    assert_eq!(
        system.calls(),
        vec![true, false],
        "the entry was removed by the quit"
    );
    assert!(
        hotkeys.registered.lock().unwrap().is_empty(),
        "every hotkey registration was released"
    );
    assert!(
        !cx.read_entity(&window, |window, _| {
            window.launcher().opens_pane(&default)
        }),
        "the application's own binding is not registered"
    );
}

#[gpui::test]
fn a_save_that_fails_rolls_the_entry_back_to_the_record(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = tray();
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    cx.run_until_parked();

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_window(&mut cx.cx).expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);

    // A change that lands and is saved.
    click(&mut settings_cx, "tray-visibility");
    settings_cx.run_until_parked();
    assert_eq!(system.calls(), vec![true, false]);
    until_record(&mut settings_cx, data.path(), false);

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    fs::remove_file(data.path().join("settings.json")).unwrap();
    fs::create_dir(data.path().join("settings.json")).unwrap();

    // Another change: it shows the entry with the system, but cannot be
    // saved.
    click(&mut settings_cx, "tray-visibility");
    until_diag(&mut settings_cx, |tree| {
        tree.contains("Pane could not save your choice")
    });

    // The failure is explained, and the entry the record holds is the one
    // in effect: the change that could not be saved did not leave the
    // native state ahead of what Pane actually saved.
    assert_eq!(
        system.calls(),
        vec![true, false, true, false],
        "the entry was rolled back to the record"
    );
}

#[gpui::test]
fn hiding_the_entry_keeps_the_windows_and_the_other_entry_points(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    // The global binding failed: this launcher has no working hotkey, so
    // the launcher window is the only usable recovery window, and nothing
    // here may hide it.
    let hotkeys = Arc::new(UnavailableHotkeys("Not available on this desktop".into()));
    let system = tray();
    let launcher = Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(hotkeys);
    let (window, cx) = open(cx, launcher, Some(data.path()), system.clone());
    cx.run_until_parked();

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_window(&mut cx.cx).expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);

    // The entry is hidden while the binding does not work: the launcher
    // window stays open and visible — it is not the thing the preference
    // hides — and the change is saved as the record holds it.
    click(&mut settings_cx, "tray-visibility");
    settings_cx.run_until_parked();
    assert!(!hidden(&window, cx), "the launcher window stayed visible");
    assert!(is_active(&settings, cx), "the Settings window kept focus");
    until_record(&mut settings_cx, data.path(), false);

    // The Settings window's own entry point is untouched: a fresh request
    // focuses the same one window, not a second one.
    select(&window, TrayAction::Settings, cx);
    cx.run_until_parked();
    assert_eq!(settings_window(&mut cx.cx), Some(settings));
    assert_eq!(cx.cx.update(|cx| cx.windows().len()), 2);

    // And the launcher is still the live window it was: open, unhidden
    // and idle at root search.
    assert!(!hidden(&window, cx));
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.status, Status::Idle);
}
