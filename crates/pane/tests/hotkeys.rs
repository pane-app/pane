//! Assigning a global hotkey through the native window, on GPUI's test
//! platform: in Manage extensions the user chooses a command's hotkey row
//! and presses the keys, and a press of the hotkey reported by the system
//! opens the command in the window. The system is a fake that records what
//! Pane registers; the real adapters are checked in pane-core's
//! `hotkey_adapters.rs` and the GUI smokes.

use std::sync::{Arc, Mutex};

use gpui::TestAppContext;
use pane::LauncherWindow;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::package;

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

#[gpui::test]
fn pressing_keys_on_the_hotkey_screen_assigns_them_and_the_hotkey_opens_the_command(
    cx: &mut TestAppContext,
) {
    let (sources, data): (TempDir, TempDir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let system = Arc::new(FakeSystem::default());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(system.clone());
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // Root lists Say hello, the install rows, then Manage extensions…; the
    // extension list holds Hello's state, reload, cache and uninstall rows, then the
    // hotkey of Say hello.
    cx.simulate_keystrokes("down down down down enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Hotkey { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Hotkey for Say hello");
    assert!(
        view.details()
            .iter()
            .any(|line| line.starts_with("Press the keys that should open Say hello")),
        "{:?}",
        view.details()
    );

    // A key without Ctrl, Alt or Super is explained, and the screen stays.
    cx.simulate_keystrokes("p");
    let view = settle(&window, cx);
    assert!(matches!(view.status, Status::Error(_)), "{:?}", view.status);
    assert!(cx.debug_bounds("status-error").is_some());
    assert!(matches!(view.screen, Screen::Hotkey { .. }));

    cx.simulate_keystrokes("ctrl-alt-p");
    let shortcut = Shortcut::parse("ctrl+alt+p").unwrap();
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(format!("{shortcut} now opens Say hello"))
    );
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    // The Open Pane hotkey's default binding is registered with the
    // system beside the command's (#74): the window attached the launcher
    // to the host settings, which applied the record's choice — the
    // provisional default — at startup.
    assert_eq!(
        *system.registered.lock().unwrap(),
        vec![Shortcut::open_pane_default(), shortcut.clone()]
    );

    // Pressed while Pane shows root search with a query typed.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("zzz");
    settle(&window, cx);
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(&shortcut, w, cx));
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Rust sample", "the guest's own view");
    // The command's list has focus: Enter runs its first item.
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Rust guest".into())
    );
    // Escape returns to an empty root search.
    cx.simulate_keystrokes("escape");
    assert_eq!(
        settle(&window, cx).screen,
        Screen::Root {
            query: String::new()
        }
    );
}

#[gpui::test]
fn a_command_hotkey_cannot_take_the_open_pane_keys(cx: &mut TestAppContext) {
    let (sources, data): (TempDir, TempDir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let system = Arc::new(FakeSystem::default());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(system.clone());
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down down down down enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Hotkey { .. }),
        "{:?}",
        view.screen
    );

    // The keys the application's own binding holds — the Open Pane
    // default, which the window registered at startup — are refused:
    // the screen explains them and stays for another try, and nothing
    // is registered or recorded over the working binding.
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "alt-space"
    } else {
        "ctrl-alt-space"
    });
    let open_pane = Shortcut::open_pane_default();
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Error(format!(
            "{open_pane} opens Pane itself: choose another shortcut for Say hello, or change \
             Pane's hotkey in Settings."
        )),
        "the refusal is explained"
    );
    assert!(
        matches!(view.screen, Screen::Hotkey { .. }),
        "the screen stays for another try, {:?}",
        view.screen
    );
    assert_eq!(
        *system.registered.lock().unwrap(),
        vec![open_pane],
        "only the Open Pane default is registered"
    );
    assert!(
        !data.path().join("extensions").join("hotkeys.json").exists(),
        "nothing was recorded"
    );
}
