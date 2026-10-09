//! The standard actions (#145) in the launcher's window, with real key
//! events, the Rust actions sample and a recording system: Enter on
//! "Standard actions" runs Copy, which copies, closes the window and shows
//! "Copied to Clipboard" in the HUD's window; the variant that keeps the
//! window open shows a toast in the footer instead, and the window stays.
//! The core's rules (each host function, every standard action, the other
//! languages) are `pane-core`'s `system.rs`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use pane::LauncherWindow;
use pane_core::system::Clip;
use pane_core::tray::TrayAction;
use pane_core::{
    CommandMatches, CommandRegistration, CommandWhen, Launcher, LauncherView, Runtime, Screen,
    Status,
};

#[path = "support/settle.rs"]
mod settle;
// The recording system pane-core's tests use.
#[path = "../../pane-core/tests/support/system.rs"]
mod recording;

use recording::{Done, RecordingSystem};
use settle::{settle, settle_shown};

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The launcher with the Rust actions sample as its one command and a
/// recording system, opened with Enter, "Standard actions" selected.
fn opened(
    cx: &mut TestAppContext,
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    Arc<RecordingSystem>,
) {
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join("sample_actions.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    let command = CommandRegistration {
        id: "actions-sample".into(),
        title: "Actions sample".into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    };
    let system = Arc::new(RecordingSystem::default());
    let launcher = Launcher::new(Runtime::start(), vec![command]).with_system(system.clone());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("actions sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Actions sample"),
        "{:?}",
        view.status
    );
    select("Standard actions", &window, cx);
    (window, cx, system)
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

/// Selects the row titled `title` with the arrow keys, from "Alpha note".
fn select(title: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    for _ in 0..12 {
        if selected_title(&settle(window, cx)) == title {
            return;
        }
        cx.simulate_keystrokes("down");
    }
    panic!("no row {title}");
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

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

fn hud(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> Option<String> {
    cx.read_entity(window, |window, _| window.hud())
}

/// Lets `time` pass on the test clock, and the window draw what it
/// changed.
fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

/// Enter runs Copy, the primary standard action: it copies, closes the
/// window, then the HUD's window shows "Copied to Clipboard" for its 1.2
/// seconds.
#[gpui::test]
fn copy_closes_the_window_and_shows_its_hud(cx: &mut TestAppContext) {
    let (window, cx, system) = opened(cx);
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    assert_eq!(
        system.take(),
        [Done::Copied {
            clip: Clip::Text("Copied by the actions sample".into()),
            concealed: false,
        }]
    );
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx).as_deref(), Some("Copied to Clipboard"));
    wait(Duration::from_millis(1300), cx);
    assert_eq!(hud(&window, cx), None, "gone after 1.2 seconds");

    // Summoned again, the command's screen is back, as the Launcher
    // setting's default restores it.
    window.update_in(cx, |window, w, cx| {
        window.tray_selected(TrayAction::OpenPane, w, cx)
    });
    let view = settle(&window, cx);
    assert!(!hidden(&window, cx));
    assert_eq!(view.screen, Screen::Command);
}

/// The Copy that keeps the window open says so in a toast in the footer,
/// and shows no HUD.
#[gpui::test]
fn copy_kept_open_shows_a_toast_and_the_window_stays(cx: &mut TestAppContext) {
    let (window, cx, system) = opened(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("keep open");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    assert_eq!(
        system.take(),
        [Done::Copied {
            clip: Clip::Text("Copied by the actions sample".into()),
            concealed: false,
        }]
    );
    assert!(!hidden(&window, cx), "the window stays");
    assert_eq!(hud(&window, cx), None);
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Copied to Clipboard".into())
    );
    assert!(cx.debug_bounds("toast-success").is_some(), "in the footer");
}
