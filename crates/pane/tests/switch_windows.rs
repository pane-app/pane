//! Switch Windows in the launcher's window (#470): the arrow keys move
//! the selection through the windows its designed List shows, as they
//! move it through root search's rows, while the keyboard is in the
//! List's search field — the field the view opens on. The field is the
//! query input under the query field's own context, so the selection keys
//! bound there take Up and Down ahead of the text element's own (which
//! moved the caret and left the selection where it was), and the caret
//! keys still move the caret. The Switch Windows sample stands in for the
//! default extension (pane-app/switch-windows), whose screen is the same
//! host-filtered List, with a fake of the open windows.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{Entity, TestAppContext, VisualTestContext};
use pane::LauncherWindow;
use pane_core::switch_windows::{SwitchWindows, Window, WindowsError};
use pane_core::{Launcher, LauncherView, Runtime, Screen};

#[path = "support/settle.rs"]
mod settle;
#[path = "support/setup.rs"]
mod setup;

use settle::{settle, until_state};
use setup::init_settings;

/// The open windows, stood in for: the ones the test lists, in order.
#[derive(Clone, Default)]
struct FakeWindows {
    listed: Arc<Mutex<Vec<Window>>>,
}

impl SwitchWindows for FakeWindows {
    fn list(&self) -> Result<Vec<Window>, WindowsError> {
        Ok(self.listed.lock().unwrap().clone())
    }

    fn activate(&self, _id: &str) -> Result<(), WindowsError> {
        Ok(())
    }
}

/// One window of the fake's list.
fn window(id: &str, title: &str, application: &str) -> Window {
    Window {
        id: id.into(),
        title: title.into(),
        application: application.into(),
        icon: None,
        minimized: false,
        maximized: false,
        elsewhere: false,
        elevated: false,
    }
}

/// Copies the assembled Switch Windows sample to `folder`.
fn package(folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-switch-windows");
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

/// The title of the selected row.
fn selected_title(view: &LauncherView) -> Option<&str> {
    view.selected.map(|index| view.rows[index].title.as_str())
}

/// The launcher window with the sample installed, listing `fake`'s
/// windows.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
    folder: &Path,
    fake: FakeWindows,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    init_settings(Some(data), cx);
    cx.update(pane::bind_keys);
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"))
        .with_switch_windows(Arc::new(fake));
    futures::executor::block_on(launcher.install_package(folder));
    let (sender, changes) = pane_core::changes::channel();
    let launcher = launcher.with_changes(sender);
    cx.add_window_view(|window, cx| {
        let mut launcher_window = LauncherWindow::new(launcher, window, cx);
        launcher_window.follow_changes(changes, window, cx);
        launcher_window
    })
}

#[gpui::test]
fn the_arrow_keys_move_through_the_windows(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("switch-windows"));
    let fake = FakeWindows::default();
    *fake.listed.lock().unwrap() = vec![
        window("1", "notes.txt - Notepad", "Notepad"),
        window("2", "Standup - Slack", "Slack"),
        window("3", "roadmap.md - VS Code", "VS Code"),
    ];
    let (window, cx) = open(cx, data.path(), &folder, fake);
    settle(&window, cx);
    cx.simulate_input("Switch Windows sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = until_state(&window, cx, |view| {
        matches!(view.screen, Screen::DesignedView(_)) && view.rows.len() == 3
    });
    assert_eq!(selected_title(&view), Some("notes.txt - Notepad"));
    cx.simulate_keystrokes("down");
    let view = settle(&window, cx);
    assert_eq!(selected_title(&view), Some("Standup - Slack"));
    cx.simulate_keystrokes("down");
    let view = settle(&window, cx);
    assert_eq!(selected_title(&view), Some("roadmap.md - VS Code"));
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(selected_title(&view), Some("Standup - Slack"));

    // Typing still filters, and the arrows still move through what is
    // left; Left, Right, Home and End move the caret in the field.
    cx.simulate_input("no");
    let view = until_state(&window, cx, |view| search_of(view) == Some("no"));
    assert_eq!(selected_title(&view), Some("notes.txt - Notepad"));
    cx.simulate_keystrokes("left");
    cx.simulate_input("x");
    until_state(&window, cx, |view| search_of(view) == Some("nxo"));
    cx.simulate_keystrokes("home");
    cx.simulate_input("a");
    until_state(&window, cx, |view| search_of(view) == Some("anxo"));
    cx.simulate_keystrokes("end");
    cx.simulate_input("b");
    until_state(&window, cx, |view| search_of(view) == Some("anxob"));
    cx.simulate_keystrokes("home right");
    cx.simulate_input("c");
    until_state(&window, cx, |view| search_of(view) == Some("acnxob"));
}

/// The text of the open designed List's search field.
fn search_of(view: &LauncherView) -> Option<&str> {
    match &view.screen {
        Screen::DesignedView(designed) => designed.list.as_ref().map(|list| list.search.as_str()),
        _ => None,
    }
}
