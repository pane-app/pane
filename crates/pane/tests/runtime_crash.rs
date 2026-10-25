//! A crash of Pane's extension runtime thread in the native window, on
//! GPUI's test platform: the window redraws by itself with the explanation,
//! a custom view open in the crashed runtime closes, Manage extensions shows
//! why and, after a second crash, restarts it; a toast shown by the call
//! whose answer the crash lost does not take the place of Pane's error.
//!
//! Faults are injected in debug builds only.
#![cfg(debug_assertions)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Fault, Launcher, LauncherView, Runtime, RuntimeStatus, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::{assembled_package, color_package};

/// A window whose launcher tells it of background changes, as Pane's does,
/// with the package in `folder` installed.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &TempDir,
    folder: &Path,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext, Runtime) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    let (changed, changes) = pane_core::changes::channel();
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
            .with_development(Arc::new(Toolchains::from_env(None)), changed);
    futures::executor::block_on(launcher.install_package(folder));
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    (window, cx, runtime)
}

/// Waits until the runtime reports its crash as `wanted`, and the window
/// shows it: the status line says so (the crash report, or a lost call's
/// answer) with `text`, and `screen` holds.
fn crashed(
    runtime: &Runtime,
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    wanted: fn(&RuntimeStatus) -> bool,
    text: &str,
    screen: fn(&Screen) -> bool,
) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let view = settle(window, cx);
        let shown = matches!(&view.status, Status::Error(error) if error.contains(text));
        if wanted(&runtime.status()) && shown && screen(&view.screen) {
            return view;
        }
        assert!(
            Instant::now() < deadline,
            "no crash shown: {:?}, {view:?}",
            runtime.status()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn press_enter_on(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let view = launcher.view();
    let index = view
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", view.rows));
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

#[gpui::test]
fn a_runtime_crash_is_explained_and_the_runtime_restarted_from_the_window(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = color_package(&sources.path().join("hello"));
    let (window, cx, runtime) = open(cx, &data, &folder);
    settle(&window, cx);
    // A designed view is open in the runtime: the list's color item
    // launches the package's color command.
    press_enter_on(&window, cx, "Say hello");
    let view = press_enter_on(&window, cx, "Choose a color");
    assert!(matches!(view.screen, Screen::DesignedView(_)), "{view:?}");

    runtime.inject(Fault::Crash);

    // The window redraws by itself: the view is closed, and the status line
    // says what happened.
    let view = crashed(
        &runtime,
        &window,
        cx,
        |status| matches!(status, RuntimeStatus::Restarted { .. }),
        "Pane's extension runtime stopped unexpectedly and was started again",
        |screen| matches!(screen, Screen::Root { .. }),
    );
    assert!(matches!(view.screen, Screen::Root { .. }));
    let Status::Error(toast) = &view.status else {
        panic!("expected the explanation, got {:?}", view.status);
    };
    assert!(
        toast.starts_with("Pane's extension runtime stopped unexpectedly and was started again"),
        "{toast}"
    );
    assert!(cx.debug_bounds("status-error").is_some());

    // Manage extensions shows why, on a screen of its own.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    enter_flow(&window, cx);
    let view = press_enter_on(&window, cx, "Why the extension runtime stopped");
    assert!(matches!(view.screen, Screen::RuntimeDetails { .. }));
    assert!(
        cx.debug_bounds("detail-Pane started it again by itself.")
            .is_some(),
        "details are rendered"
    );
    assert!(view.rows.is_empty(), "it runs: nothing to restart");

    // A second crash soon after stops it; the details screen offers Restart.
    runtime.inject(Fault::Crash);
    let view = crashed(
        &runtime,
        &window,
        cx,
        |status| matches!(status, RuntimeStatus::Stopped { .. }),
        "not restarted",
        |screen| matches!(screen, Screen::RuntimeDetails { .. }),
    );
    assert!(matches!(view.screen, Screen::RuntimeDetails { .. }));
    assert!(
        cx.debug_bounds("row-Restart the extension runtime")
            .is_some()
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Restarted the extension runtime".into())
    );
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(runtime.status(), RuntimeStatus::Running);

    // The command runs again.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    // The earlier Escape left an empty root, which hid the window; hidden,
    // the action's toast would be a HUD (#141), so the window is shown
    // again first, as the hotkey shows it.
    window.update_in(cx, |this, window, cx| this.show_again(window, cx));
    settle(&window, cx);
    press_enter_on(&window, cx, "Say hello");
    press_enter_on(&window, cx, "Say hello");
    let shown = settle_shown(&window, cx);
    assert!(matches!(shown, Status::Result(_)), "{shown:?}");
}

/// Moves the pointer off the window.
fn pointer_leaves(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(
        gpui::point(px(-100.), px(-100.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    cx.run_until_parked();
}

/// Lets `time` pass on the test clock, and the window draw what it
/// changed.
fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

/// A window with the settings sample installed, its Greeting command
/// open, and the pointer off the window.
fn at_greeting<'a>(
    cx: &'a mut TestAppContext,
    sources: &TempDir,
    data: &TempDir,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext, Runtime) {
    let folder = assembled_package("sample-settings", &sources.path().join("settings"));
    let (window, cx, runtime) = open(cx, data, &folder);
    settle(&window, cx);
    pointer_leaves(cx);
    press_enter_on(&window, cx, "Greeting");
    (window, cx, runtime)
}

/// Runs Count with the runtime made to crash once Count has run (and
/// shown its toast, "Counted …"), before its answer is sent, and waits
/// until the window shows that the answer was lost.
fn count_and_lose_the_answer(
    runtime: &Runtime,
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) {
    runtime.inject(Fault::CrashBeforeAnswer {
        item: "count".into(),
    });
    press_enter_on(window, cx, "Count");
    crashed(
        runtime,
        window,
        cx,
        |status| matches!(status, RuntimeStatus::Restarted { .. }),
        "does not run this again",
        |screen| matches!(screen, Screen::Command),
    );
}

/// The toast a call showed before the runtime lost its answer does not
/// take the place of Pane's error (#527), here when the window draws the
/// error first: the error shows, and once it has had its time the footer
/// is at rest — Count's toast does not come back.
#[gpui::test]
fn a_lost_calls_toast_does_not_follow_the_runtime_error(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx, runtime) = at_greeting(cx, &sources, &data);

    count_and_lose_the_answer(&runtime, &window, cx);

    assert!(cx.debug_bounds("status-error").is_some());
    assert!(
        cx.debug_bounds("toast-success").is_none(),
        "not Count's toast"
    );
    wait(Duration::from_millis(3100), cx);
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "the error had its time");
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert_eq!(launcher.toast(), None);
    assert!(cx.debug_bounds("toast").is_none(), "{view:?}");
}

/// The other order (#527): a toast of Count's is drawn first, with the
/// pointer over it, and then the runtime loses Count's next answer. Pane's
/// error takes the footer at once, and however long the pointer rests
/// there, no toast of Count's comes back under it to be held: main run
/// 40110558671's macOS frame 203 showed "Counted 2" for the whole 15
/// seconds it was taken in, the error having had its time.
#[gpui::test]
fn a_hovered_toast_gives_way_to_the_runtime_error(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx, runtime) = at_greeting(cx, &sources, &data);
    press_enter_on(&window, cx, "Count");
    let over = cx
        .debug_bounds("toast")
        .expect("Count's toast is drawn")
        .center();
    assert!(cx.debug_bounds("toast-success").is_some());
    cx.simulate_mouse_move(over, None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();

    count_and_lose_the_answer(&runtime, &window, cx);

    assert!(cx.debug_bounds("status-error").is_some());
    assert!(
        cx.debug_bounds("toast-success").is_none(),
        "not Count's toast"
    );
    // The error has its time, and the pointer is over the footer again, as
    // a resting pointer is found over whatever the footer draws next.
    wait(Duration::from_millis(3100), cx);
    cx.simulate_mouse_move(over, None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    wait(Duration::from_secs(10), cx);
    let view = settle(&window, cx);
    assert_eq!(launcher.toast(), None, "{view:?}");
    assert!(cx.debug_bounds("toast-success").is_none(), "{view:?}");

    pointer_leaves(cx);
    wait(Duration::from_millis(3100), cx);
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "the error had its time");
    assert_eq!(launcher.toast(), None);
    assert!(cx.debug_bounds("toast").is_none(), "{view:?}");
}
