//! Assigning a global hotkey through the native window, on GPUI's test
//! platform: in Manage extensions the user chooses a command's hotkey row
//! and presses the keys, and a press of the hotkey reported by the system
//! opens the command in the window. The system is a fake that records what
//! Pane registers; the real adapters are checked in pane-core's
//! `hotkey_adapters.rs` and the GUI smokes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};
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

/// Writes a package folder whose one command is the Rust sample.
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

/// Lets the window apply replies that arrive from other threads.
fn settle(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if view.status != Status::Running {
            return view;
        }
        assert!(Instant::now() < deadline, "the launcher did not finish");
        std::thread::sleep(Duration::from_millis(5));
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

    // Root lists Say hello, the install row, then Manage extensions…; the
    // extension list holds Hello's state, reload, cache and uninstall rows, then the
    // hotkey of Say hello.
    cx.simulate_keystrokes("down down enter");
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
    assert_eq!(
        *system.registered.lock().unwrap(),
        std::slice::from_ref(&shortcut)
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
        settle(&window, cx).status,
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
