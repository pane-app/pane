//! An extension that stops responding (#18), in the native window on
//! GPUI's test platform: while its command computes without waiting, the
//! window keeps answering keys; Pane stops the call after the compute
//! limit and shows why, and the third time pauses the package, with the
//! reason and Retry rendered.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Limits, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, settle};

/// Copies the assembled Rust settings sample into `folder`.
fn package(folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-settings");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for file in ["pane.json", "sample_settings.wasm"] {
        fs::copy(assembled.join(file), folder.join(file)).unwrap();
    }
    folder.to_path_buf()
}

/// The limits the test runs with: a guest call may compute for two
/// seconds.
fn limits() -> Limits {
    Limits {
        compute: Duration::from_secs(2),
        ..Limits::default()
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &TempDir,
    folder: &Path,
) -> (Runtime, Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    runtime.set_limits(limits());
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
    futures::executor::block_on(launcher.install_package(folder));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (runtime, window, cx)
}

fn view(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    cx.run_until_parked();
    cx.read_entity(window, |window, _| window.launcher().view())
}

fn press_enter_on(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", launcher.view().rows));
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

/// What the sample saved under `key` in its settings.
fn saved(data: &TempDir, identity: &PackageIdentity, key: &str) -> Option<String> {
    let text = fs::read_to_string(data.path().join("extensions/settings.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["packages"][identity.key()][key]
        .as_str()
        .map(str::to_owned)
}

#[gpui::test]
fn a_command_that_stops_responding_leaves_the_window_usable_and_pauses_the_third_time(
    cx: &mut TestAppContext,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("settings"));
    let identity = PackageIdentity::local(&folder).unwrap();
    let (runtime, window, cx) = open(cx, &data, &folder);
    settle(&window, cx);

    // The command starts computing without waiting; with no compute limit
    // to speak of, its call ends only once Pane lowers it.
    runtime.set_limits(Limits {
        compute: Duration::from_secs(3600),
        ..limits()
    });
    press_enter_on(&window, cx, "Greeting");
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    let busy = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Stop responding")
        .unwrap();
    launcher.select(busy);
    cx.simulate_keystrokes("enter");
    let deadline = Instant::now() + Duration::from_secs(60);
    while saved(&data, &identity, "busy").as_deref() != Some("started") {
        assert!(Instant::now() < deadline, "it did not start");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }

    // Meanwhile the window answers: Escape returns to root search, and
    // Manage extensions opens, while the guest still computes.
    cx.simulate_keystrokes("escape");
    assert!(matches!(view(&window, cx).screen, Screen::Root { .. }));
    let manage = enter_flow(&window, cx);
    assert!(matches!(manage.screen, Screen::Extensions { .. }));
    assert_eq!(
        saved(&data, &identity, "busy").as_deref(),
        Some("started"),
        "the guest still computes"
    );
    // The limit applies to the call already running, which is stopped.
    runtime.set_limits(limits());
    cx.simulate_keystrokes("escape");

    // Run again, it is stopped after the limit and says why.
    press_enter_on(&window, cx, "Greeting");
    let stopped = press_enter_on(&window, cx, "Stop responding");
    assert!(
        matches!(&stopped.status, Status::Error(text)
            if text.starts_with("The extension stopped responding: it computed for 2 seconds")),
        "{:?}",
        stopped.status
    );
    assert!(cx.debug_bounds("status-error").is_some());

    // The third time pauses it: the toast says so, and its command's
    // reason is rendered.
    let paused = press_enter_on(&window, cx, "Stop responding");
    assert!(matches!(paused.screen, Screen::Root { .. }), "{paused:?}");
    let Status::Error(toast) = &paused.status else {
        panic!("expected the toast, got {:?}", paused.status);
    };
    assert!(
        toast.starts_with(
            "Settings sample stopped responding 3 times within 5 minutes and is paused"
        ),
        "{toast}"
    );
    assert!(
        cx.debug_bounds("unavailable-reason-Greeting").is_some(),
        "the paused command's reason is rendered"
    );
    assert_eq!(saved(&data, &identity, "busy").as_deref(), Some("started"));

    // Manage extensions offers Retry, which starts it again.
    enter_flow(&window, cx);
    assert!(cx.debug_bounds("row-Retry Settings sample").is_some());
    let retried = press_enter_on(&window, cx, "Retry Settings sample");
    assert_eq!(
        retried.status,
        Status::Result("Started Settings sample".into())
    );
}
