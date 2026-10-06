//! A crash of Pane's extension runtime thread in the native window, on
//! GPUI's test platform: the window redraws by itself with the explanation,
//! a custom view open in the crashed runtime closes, Manage extensions shows
//! why and, after a second crash, restarts it.
//!
//! Faults are injected in debug builds only.
#![cfg(debug_assertions)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Fault, Launcher, LauncherView, Runtime, RuntimeStatus, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::package;

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
    let folder = package(&sources.path().join("hello"));
    let (window, cx, runtime) = open(cx, &data, &folder);
    settle(&window, cx);
    // A custom view is open in the runtime.
    press_enter_on(&window, cx, "Say hello");
    let view = press_enter_on(&window, cx, "Choose a color");
    assert!(matches!(view.screen, Screen::CustomView(_)), "{view:?}");

    runtime.inject(Fault::Crash);

    // The window redraws by itself: the view is closed, and the status line
    // says what happened.
    let view = crashed(
        &runtime,
        &window,
        cx,
        |status| matches!(status, RuntimeStatus::Restarted { .. }),
        "Pane's extension runtime stopped unexpectedly and was started again",
        |screen| *screen == Screen::Command,
    );
    assert_eq!(view.screen, Screen::Command);
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
    press_enter_on(&window, cx, "Manage extensions…");
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
    press_enter_on(&window, cx, "Say hello");
    press_enter_on(&window, cx, "Say hello");
    let shown = settle_shown(&window, cx);
    assert!(matches!(shown, Status::Result(_)), "{shown:?}");
}
