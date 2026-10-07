//! Quicklinks (#149) in the launcher's window, with real key events, the
//! default extension `cargo xtask guests` assembles, and a recording system
//! for opening and the clipboard (pane-core's `support/system.rs`): Create
//! Quicklink opens its form from root search, refuses a malformed link on
//! its field and saves; the saved quicklink is found in root search and
//! Enter opens it; Search Quicklinks lists the actions in the Actions
//! panel, and Ctrl+E (Edit), Ctrl+Shift+C (Copy Link), Delete with its
//! confirmation, Open With… and Enter (Open) do what they name. The core's
//! rules (every action, any target, import and export, indexed results and
//! pins) are pane-core's `quicklinks.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::system::Clip;
use pane_core::tray::TrayAction;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/packages.rs"]
mod packages;
// The recording system pane-core's tests use.
#[path = "../../pane-core/tests/support/system.rs"]
mod recording;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/setup.rs"]
mod setup;

use recording::{Done, RecordingSystem};
use settle::{settle, settle_shown, until};
use setup::actions_shortcut;

/// The folders one test keeps its package and extensions in.
struct Folders {
    _sources: TempDir,
    _data: TempDir,
}

/// A launcher with Quicklinks installed, a recording system, and the
/// quicklinks `links` (name, link) saved through Create Quicklink.
fn launcher_with(links: &[(&str, &str)]) -> (Launcher, Arc<RecordingSystem>, Folders) {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let folder = packages::assembled_package("quicklinks", &sources.path().join("quicklinks"));
    let system = Arc::new(RecordingSystem::default());
    let runtime = Runtime::start().unwrap();
    runtime.set_applications(system.clone());
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
        .with_system(system.clone());
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Quicklinks".into())
    );
    for (name, link) in links {
        to_root(&launcher);
        block_on(launcher.set_query("create quicklink"));
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == "Create Quicklink")
            .expect("Create Quicklink is listed");
        launcher.select(index);
        block_on(launcher.activate_selected());
        launcher.set_field_value("name", name);
        launcher.set_field_value("link", link);
        block_on(launcher.submit_form());
    }
    to_root(&launcher);
    block_on(launcher.set_query(""));
    (
        launcher,
        system,
        Folders {
            _sources: sources,
            _data: data,
        },
    )
}

fn to_root(launcher: &Launcher) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
}

/// The launcher's window over `launcher`.
fn window_of(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// Selects the row titled `title` with the arrow keys.
fn select(title: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    for _ in 0..8 {
        if selected_title(&settle(window, cx)) == title {
            return;
        }
        cx.simulate_keystrokes("down");
    }
    panic!("no row {title}");
}

/// Types `query` into root search and presses Enter on `title`.
fn enter_on(
    query: &str,
    title: &str,
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> LauncherView {
    cx.simulate_input(query);
    settle(window, cx);
    select(title, window, cx);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

/// Runs the window until the launcher no longer runs an action: a hidden
/// window draws nothing, so this does not wait for a frame.
fn done(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
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

/// Runs the window until a form is on screen, drawn; its view.
fn form_shown(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    until(window, cx, |view| view.form().is_some())
}

/// The values of the fields of the form on screen, in order.
fn values(view: &LauncherView) -> Vec<String> {
    view.form()
        .expect("a form is open")
        .fields
        .iter()
        .map(|field| field.value.clone())
        .collect()
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

fn hud(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> Option<String> {
    cx.read_entity(window, |window, _| window.hud())
}

/// Summons the window again, as the tray's Open Pane does.
fn summon(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    window.update_in(cx, |window, w, cx| {
        window.tray_selected(TrayAction::OpenPane, w, cx)
    });
    settle(window, cx);
    assert!(!hidden(window, cx));
}

/// Runs the window until the launcher asks for a confirmation and the
/// window draws it.
fn asked(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (asked, drawn) = cx.read_entity(window, |window, _| {
            (
                window.launcher().confirmation(),
                window.confirmation_drawn(),
            )
        });
        if let Some(asked) = asked
            && drawn.map(|(id, _)| id) == Some(asked.id)
            && cx.debug_bounds("confirmation").is_some()
        {
            return;
        }
        assert!(Instant::now() < deadline, "no confirmation was drawn");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens Search Quicklinks from root search with Enter.
fn open_search(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    let view = enter_on("search quicklinks", "Search Quicklinks", window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Quicklinks"),
        "{:?}",
        view.status
    );
}

/// Create Quicklink opens its form from root search; Tab moves between
/// its fields, a link without a scheme is refused on its field, and once
/// fixed Enter saves it and returns to root search, where typing finds it
/// and Enter opens it.
#[gpui::test]
fn create_quicklink_opens_its_form_and_saves_from_the_keyboard(cx: &mut TestAppContext) {
    let (launcher, system, _folders) = launcher_with(&[]);
    let (window, cx) = window_of(cx, launcher);

    cx.simulate_input("create quicklink");
    settle(&window, cx);
    select("Create Quicklink", &window, cx);
    cx.simulate_keystrokes("enter");
    let view = form_shown(&window, cx);
    assert_eq!(view.title, "Create Quicklink");
    assert_eq!(values(&view), ["", "", ""], "a new one starts empty");

    cx.simulate_input("Pane issues");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("github.com/hoangvu12/pane/issues");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Error(
            "Link: Enter a link with its scheme, such as https:// or mailto:, or the full path \
             of a file, folder or application"
                .into()
        )
    );
    assert!(view.form().is_some(), "the form stays open");
    // To the start of the field: text fields on macOS have no binding for
    // Home (Mac keyboards have none), only Command-Left.
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-left"
    } else {
        "home"
    });
    cx.simulate_input("https://");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Created “Pane issues”".into())
    );
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "{view:?}");

    cx.simulate_input("pane iss");
    let view = until(&window, cx, |view| {
        view.rows.iter().any(|row| row.title == "Pane issues")
    });
    assert_eq!(selected_title(&view), "Pane issues");
    cx.simulate_keystrokes("enter");
    let view = done(&window, cx);
    assert_eq!(view.status, Status::Result("Opened Pane issues".into()));
    assert_eq!(
        system.take(),
        [Done::Opened {
            target: "https://github.com/hoangvu12/pane/issues".into(),
            application: None,
        }]
    );

    // Escape from the form leaves the command for root search.
    cx.simulate_keystrokes("escape");
    let view = enter_on("create quicklink", "Create Quicklink", &window, cx);
    assert!(view.form().is_some());
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "{view:?}");
}

/// Search Quicklinks: the Actions panel lists every action, Ctrl+E opens
/// the form filled in, Ctrl+Shift+C copies the link, closing the window
/// with a HUD, Delete asks first and Enter confirms it, Open With… opens
/// with the application chosen, and Enter opens.
#[gpui::test]
fn search_quicklinks_actions_work_from_the_keyboard(cx: &mut TestAppContext) {
    let (launcher, system, _folders) = launcher_with(&[
        ("Docs", "https://docs.example.com"),
        ("News", "https://news.example.com"),
    ]);
    system.take();
    let (window, cx) = window_of(cx, launcher);
    open_search(&window, cx);
    assert_eq!(titles(&settle(&window, cx)), ["Docs", "News"]);

    // The panel lists them all, Delete drawn destructive.
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    for action in [
        "Open",
        "Open With…",
        "Copy Link",
        "Edit",
        "Duplicate",
        "Delete",
    ] {
        assert!(
            cx.debug_bounds(&format!("action-{action}")).is_some(),
            "{action} is in the panel"
        );
    }
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // Ctrl+E: the form, filled in; Escape leaves it.
    cx.simulate_keystrokes("ctrl-e");
    let view = form_shown(&window, cx);
    assert_eq!(view.title, "Edit “Docs”");
    assert_eq!(values(&view), ["Docs", "https://docs.example.com", ""]);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // Ctrl+Shift+C: copied, the window closed, then the HUD.
    open_search(&window, cx);
    cx.simulate_keystrokes("ctrl-shift-c");
    done(&window, cx);
    assert_eq!(
        system.take(),
        [Done::Copied {
            clip: Clip::Text("https://docs.example.com".into()),
            concealed: false,
        }]
    );
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx).as_deref(), Some("Copied to Clipboard"));
    summon(&window, cx);

    // Delete, through the panel: asked first, Enter confirms.
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command, "the list was kept");
    select("Docs", &window, cx);
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    cx.simulate_input("delete");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    asked(&window, cx);
    assert!(cx.debug_bounds("confirmation-destructive").is_some());
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Deleted “Docs”".into())
    );
    assert_eq!(titles(&settle(&window, cx)), ["News"]);

    // Open With…: its submenu, filtered to Zed, and Enter.
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    cx.simulate_input("open with");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let deadline = Instant::now() + Duration::from_secs(60);
    while cx.debug_bounds("action-Zed").is_none() {
        assert!(Instant::now() < deadline, "Open With… did not list Zed");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    cx.simulate_input("zed");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    assert_eq!(
        system.take(),
        [Done::Opened {
            target: "https://news.example.com".into(),
            application: Some("app:Zed".into()),
        }]
    );
    assert!(hidden(&window, cx));
    summon(&window, cx);

    // Enter: Open.
    select("News", &window, cx);
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    assert_eq!(
        system.take(),
        [Done::Opened {
            target: "https://news.example.com".into(),
            application: None,
        }]
    );
    assert!(hidden(&window, cx));
}
