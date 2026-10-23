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
//!
//! Its search bar, a pushed view's, shows a back button where root
//! search's shows the magnifier (#469): in the same place, so nothing
//! moves, with a hover wash and a tooltip naming the back key. A click
//! goes back one level, the keyboard staying in the search field.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, prelude::*, px, size};
use pane::LauncherWindow;
use pane_core::switch_windows::{SwitchWindows, Window, WindowsError};
use pane_core::{Launcher, LauncherView, Runtime, Screen};

#[path = "support/paint.rs"]
mod paint;
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

/// How long the pointer rests before a tooltip shows, with room to spare.
const TOOLTIP_DELAY: Duration = Duration::from_millis(700);

/// Whether root search's query field has keyboard focus.
fn query_has_focus(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    use gpui::Focusable;
    let input = cx.read_entity(window, |window, _| window.query_field());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn a_pushed_views_search_bar_shows_a_back_button(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("switch-windows"));
    let fake = FakeWindows::default();
    *fake.listed.lock().unwrap() = vec![
        window("1", "notes.txt - Notepad", "Notepad"),
        window("2", "Standup - Slack", "Slack"),
    ];
    let (window, cx) = open(cx, data.path(), &folder, fake);
    settle(&window, cx);

    // Root search shows the magnifier, and no back button.
    let magnifier = cx
        .debug_bounds("search-magnifier")
        .expect("root search shows the magnifier");
    assert!(cx.debug_bounds("search-back").is_none());

    // The pushed view shows the back button in the magnifier's place:
    // the same box across the row, so the field's text starts where it
    // did (the List's navigation title sits above the row).
    cx.simulate_input("Switch Windows sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    until_state(&window, cx, |view| {
        matches!(view.screen, Screen::DesignedView(_)) && view.rows.len() == 2
    });
    let back = cx
        .debug_bounds("search-back")
        .expect("the pushed view shows the back button");
    assert!(cx.debug_bounds("search-magnifier").is_none());
    assert_eq!(
        (back.origin.x, back.size),
        (magnifier.origin.x, magnifier.size),
        "the button takes the magnifier's place"
    );
    assert!(query_has_focus(&window, cx));
    cx.simulate_input("no");
    until_state(&window, cx, |view| search_of(view) == Some("no"));

    // Its 32px square, centred in the 44px hit target, has the split
    // views' back fill at rest and the footer's open wash under the
    // pointer, which also shows the tooltip naming the back key.
    let square = gpui::Bounds::new(back.origin + point(px(6.), px(6.)), size(px(32.), px(32.)));
    let washed = |cx: &mut VisualTestContext, dark: u32, light: u32| {
        paint::paints_fill_at(cx, square, dark) || paint::paints_fill_at(cx, square, light)
    };
    assert!(washed(cx, 0xFFFFFF0F, 0x0000000F), "the resting fill");
    cx.simulate_mouse_move(back.center(), None, Modifiers::none());
    cx.executor().advance_clock(TOOLTIP_DELAY);
    cx.run_until_parked();
    assert!(washed(cx, 0xFFFFFF1A, 0x0000001A), "the hover wash");
    assert!(
        cx.debug_bounds("tooltip-Back (Escape)").is_some(),
        "the tooltip names the back key"
    );

    // A click goes back one level, with text in the field too (the back
    // key would clear it first), and the keyboard stays in the search
    // field.
    cx.simulate_click(back.center(), Modifiers::none());
    let view = until_state(&window, cx, |view| {
        matches!(view.screen, Screen::Root { .. })
    });
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(query_has_focus(&window, cx), "the field keeps the keyboard");
    assert!(cx.debug_bounds("search-back").is_none());
    assert!(cx.debug_bounds("search-magnifier").is_some());
}

/// The text of the open designed List's search field.
fn search_of(view: &LauncherView) -> Option<&str> {
    match &view.screen {
        Screen::DesignedView(designed) => designed.list.as_ref().map(|list| list.search.as_str()),
        _ => None,
    }
}
