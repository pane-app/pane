//! Pane's local crash record (#133) in the native window on GPUI's test
//! platform: a start over a data folder whose logs folder holds the marker
//! of a run that ended without a clean quit lists one root result, "Pane
//! quit unexpectedly last time", whose primary action opens the log folder
//! and whose Actions panel dismisses it; Settings' About page shows the same
//! notice beside its diagnostics, whose copy names the log's folder. The
//! earlier run is this test's own process, its marker written by the
//! system's process table; the start after it asks a fake table that says
//! the process has ended, as it would have after a crash. The tray's clean
//! Quit is `tray.rs`'.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*, px,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::diagnostics::{CrashRecord, Liveness, ProcessTable, SystemProcesses};
use pane_core::{Launcher, LauncherView, Runtime, Screen};

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/setup.rs"]
mod setup;

use a11y::accessibility;
use settle::settle;
use setup::{actions_shortcut, settings_shortcut};

/// What the row, the status line and the About page say.
const NOTICE: &str = "Pane quit unexpectedly last time";

/// A process table in which every process has ended.
struct AllEnded;

impl ProcessTable for AllEnded {
    fn look_up(&self, _process: u32) -> Liveness {
        Liveness::Ended
    }
}

/// The crash record of a start over the logs folder `logs`, which holds
/// the marker of a run that never quit cleanly.
fn after_a_crash(logs: &Path) -> Arc<CrashRecord> {
    // The earlier run: its marker written at its start, and never removed.
    let earlier = CrashRecord::open(logs, "0.0.1", &SystemProcesses);
    assert!(!earlier.ended_unexpectedly(), "a first start");
    assert!(earlier.marker().exists());
    drop(earlier);
    // This start: the earlier run's process has ended.
    let record = CrashRecord::open(logs, pane::APP_VERSION, &AllEnded);
    assert!(record.ended_unexpectedly());
    Arc::new(record)
}

/// Opens the launcher window as the binary does after a crash: over the
/// data folder `data`, keeping the crash record of its logs folder.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let launcher = Launcher::with_packages(Runtime::start(), Vec::new(), data.join("extensions"))
        .with_crash_record(after_a_crash(&data.join("logs")));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

fn titles(view: &LauncherView) -> Vec<String> {
    view.rows.iter().map(|row| row.title.clone()).collect()
}

#[gpui::test]
fn a_start_after_a_crash_lists_the_notice_and_dismissing_it_removes_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open(cx, data.path());
    settle(&window, cx);

    // Root search finds the notice; its primary action opens the log
    // folder.
    cx.simulate_input("unexpectedly");
    let view = settle(&window, cx);
    assert_eq!(titles(&view).first().map(String::as_str), Some(NOTICE));
    assert_eq!(view.selected, Some(0));
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert_eq!(launcher.selected_action().label, "Open log folder");
    let notice = launcher.log_notice().expect("a crash record");
    assert!(notice.quit_unexpectedly);
    assert_eq!(notice.folder, data.path().join("logs"));

    // Its Actions panel dismisses it.
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    cx.simulate_input("dismiss");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(!titles(&view).contains(&NOTICE.to_owned()), "{view:?}");
    let notice = launcher.log_notice().expect("a crash record");
    assert!(!notice.quit_unexpectedly, "the notice is gone");

    // Nor is it found again.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("quit unexpectedly");
    let view = settle(&window, cx);
    assert!(!titles(&view).contains(&NOTICE.to_owned()), "{view:?}");
}

#[gpui::test]
fn the_about_page_shows_the_notice_and_the_diagnostics_name_the_log(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open(cx, data.path());
    settle(&window, cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    settings_cx.simulate_resize(gpui::size(px(740.), px(1100.)));
    settings_cx.run_until_parked();
    click(&mut settings_cx, "section-About");
    settings_cx.run_until_parked();

    // The same notice root search lists, beside the diagnostics, with the
    // folder to open.
    until_text(&mut settings_cx, NOTICE);
    assert!(settings_cx.debug_bounds("about-crash-notice").is_some());
    assert!(settings_cx.debug_bounds("about-log-folder").is_some());

    // The diagnostics the page copies name the log's folder, redacted as
    // the log redacts: the test's folder is under the home folder on
    // Windows and macOS, which the report names `~`.
    click(&mut settings_cx, "about-diagnostics");
    until_text(&mut settings_cx, "Copied to the clipboard");
    let report = settings_cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("the report was copied");
    let logs = data.path().join("logs").display().to_string();
    assert!(
        report.contains(&format!(
            "Log folder: {}",
            pane_core::diagnostics::redacted(&logs)
        )),
        "{report}"
    );
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if let Some(home) = home.filter(|home| home.len() >= 3) {
        let home = Path::new(&home).display().to_string();
        assert!(!report.contains(&home), "{report}");
    }
    assert!(
        report.contains("Last run: Pane quit unexpectedly"),
        "{report}"
    );
}

/// The open Settings windows.
fn settings_windows(cx: &TestAppContext) -> Vec<WindowHandle<SettingsWindow>> {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<SettingsWindow>())
            .collect()
    })
}

/// Clicks the element whose debug selector is `selector` in `cx`.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Runs the window until its accessibility tree contains `text`.
fn until_text(cx: &mut VisualTestContext, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, json) = accessibility(cx);
        if json.contains(text) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {text:?} to be drawn, {json}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
