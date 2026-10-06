//! No-view commands through the native window, on GPUI's test platform,
//! with real key events: Enter on a no-view command's row in root search
//! runs it and opens no screen, and its global hotkey runs it without
//! showing the window (ADR 0037). The command is the no-view sample's
//! "Report launch", from `cargo xtask guests`; the system's hotkeys are a
//! fake that records what Pane registers.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext};
use pane::LauncherWindow;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

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

/// What "Report launch" answers when the user launches it from `source`
/// with nothing more.
fn reported(source: &str) -> Status {
    Status::Result(format!(
        "Report: user-initiated from {source}; fallback text: none; context: none; \
         arguments: none"
    ))
}

/// Presses the global hotkey `shortcut`, as the system's adapter would
/// report it, past the window's repeat guard for the Open Pane hotkey.
fn press(window: &Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
    cx.run_until_parked();
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Runs the window until the launcher no longer runs an action: a hidden
/// window draws nothing, so this does not wait for a frame.
fn until_done(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if view.status != Status::Running {
            return view;
        }
        assert!(Instant::now() < deadline, "timed out: {view:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A window over a launcher with the no-view sample installed, through
/// its preview and Enter, as a user installs it.
fn installed(
    cx: &mut TestAppContext,
    system: Arc<FakeSystem>,
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    tempfile::TempDir,
    tempfile::TempDir,
    std::path::PathBuf,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-no-view", &sources.path().join("no-view"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(system);
    let preview = folder.clone();
    let (window, cx) = cx.add_window_view(move |window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&preview, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed No-view sample".into())
    );
    (window, cx, sources, data, folder)
}

#[gpui::test]
fn enter_on_a_no_view_row_runs_it_and_opens_no_screen(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default());
    cx.simulate_input("report launch");
    let view = settle(&window, cx);
    let selected = view.selected.map(|index| view.rows[index].title.as_str());
    assert_eq!(selected, Some("Report launch"));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, reported("root-search"));
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "report launch".into()
        },
        "no screen opened"
    );
    assert!(cx.debug_bounds("status-result").is_some());

    // Enter again runs it again, still from root search.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, reported("root-search"));
    assert!(matches!(view.screen, Screen::Root { .. }));
}

#[gpui::test]
fn a_no_view_hotkey_runs_the_command_without_showing_the_window(cx: &mut TestAppContext) {
    let system = Arc::new(FakeSystem::default());
    let (window, cx, _sources, _data, folder) = installed(cx, system.clone());
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    let shortcut = Shortcut::parse("ctrl+alt+r").unwrap();
    let command = format!("{}#report", PackageIdentity::local(&folder).unwrap().key());
    block_on(
        launcher
            .set_hotkey(&command, Some(shortcut.clone()))
            .expect("the hotkey is accepted"),
    );
    assert!(system.registered.lock().unwrap().contains(&shortcut));

    // The Open Pane hotkey hides the focused launcher (a launcher without
    // focus is brought forward first).
    let open_pane = Shortcut::open_pane_default();
    press(&window, &open_pane, cx);
    if !hidden(&window, cx) {
        press(&window, &open_pane, cx);
    }
    assert!(hidden(&window, cx), "the launcher hid");

    // The command's hotkey runs it, and the window stays hidden.
    press(&window, &shortcut, cx);
    let view = until_done(&window, cx);
    assert_eq!(view.status, reported("hotkey"));
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(hidden(&window, cx), "the window was not shown");

    // A view command's hotkey still shows the window.
    let show = Shortcut::parse("ctrl+alt+s").unwrap();
    let command = format!("{}#show", PackageIdentity::local(&folder).unwrap().key());
    block_on(
        launcher
            .set_hotkey(&command, Some(show.clone()))
            .expect("the hotkey is accepted"),
    );
    press(&window, &show, cx);
    assert!(!hidden(&window, cx), "the window was shown");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Launch record");
}
