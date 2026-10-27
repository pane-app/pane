//! An attention launch through the native window, on GPUI's test platform
//! (ADR 0053, #557): a user-initiated launch of an extension's own view
//! command, asked from a call no window was shown for, shows the hidden
//! launcher on it with the keyboard focus in the opened view, so Escape acts
//! on it at once, and leaves the Settings window open; shown so, it hides
//! once another application takes the focus, as any launcher does (#571).
//! The window asks the
//! launcher while the launch waits, telling it how long the keyboard and
//! the pointer have been still: a stand-in here, so no real desktop is
//! needed. The command is the no-view sample's "Show launch", from
//! `cargo xtask guests`; the call stands in for its guest's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle, prelude::*};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::develop::Toolchains;
use pane_core::{LaunchType, Launcher, PackageIdentity, Runtime, Screen};
use tempfile::TempDir;

#[path = "support/packages.rs"]
mod packages;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/setup.rs"]
mod setup;

use packages::assembled_package;
use settle::settle;
use setup::settings_shortcut;

/// The open Settings windows.
fn settings_windows(cx: &TestAppContext) -> Vec<WindowHandle<SettingsWindow>> {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<SettingsWindow>())
            .collect()
    })
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Whether anything in the launcher's window has the keyboard focus: the
/// query field, or the window's own handle, which a list with no search
/// field of its own takes and Enter and Escape act through.
fn has_focus(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.focused(cx).is_some())
}

/// The rows of the launcher's screen, by title.
fn titles(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> Vec<String> {
    cx.read_entity(window, |window, _| window.launcher().view())
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

/// What [`shown_by_an_attention_launch`] leaves: the launcher's window
/// and its context, whether another application holds the focus as the
/// test says (#571), and the folders kept for the test's length.
type Shown<'a> = (
    Entity<LauncherWindow>,
    &'a mut VisualTestContext,
    Arc<AtomicBool>,
    [TempDir; 2],
);

/// The launcher with the no-view sample installed, hidden with Settings
/// open, then shown on "Show launch" by an attention launch once the user
/// stops typing.
fn shown_by_an_attention_launch(cx: &mut TestAppContext) -> Shown<'_> {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-no-view", &sources.path().join("alarm"));
    let alarm = PackageIdentity::local(&folder).unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (changed, changes) = pane_core::changes::channel();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_development(Arc::new(Toolchains::from_env(None)), changed);
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    // How long the keyboard and the pointer have been still, in
    // milliseconds, as the test says.
    let idle = Arc::new(AtomicU64::new(0));
    let elsewhere = Arc::new(AtomicBool::new(false));
    let (window, cx) = cx.add_window_view({
        let (idle, elsewhere) = (idle.clone(), elsewhere.clone());
        move |window, cx| {
            let mut launcher = LauncherWindow::new(launcher, window, cx);
            launcher.follow_changes(changes, window, cx);
            launcher
                .set_input_idle(move || Some(Duration::from_millis(idle.load(Ordering::SeqCst))));
            launcher.set_focus_elsewhere(move || elsewhere.load(Ordering::SeqCst));
            launcher
        }
    });
    settle(&window, cx);

    // Settings is open, and the launcher is hidden without the focus.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 1, "Settings opened");
    window.update_in(cx, |launcher, window, cx| {
        window.blur(cx);
        launcher.start_hidden(window, cx);
    });
    cx.run_until_parked();
    assert!(hidden(&window, cx));

    // A timer fires: the extension asks for its own view, while the user
    // types elsewhere.
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    launcher
        .launch_as(
            &alarm,
            None,
            "show",
            LaunchType::UserInitiated,
            Some(r#"{"alarm":"07:00"}"#),
            false,
        )
        .expect("the launch is accepted");
    for _ in 0..5 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    assert!(hidden(&window, cx), "not over the user's typing");

    // The user stops typing: the launcher shows the command.
    idle.store(5_000, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        let screen = cx.read_entity(&window, |window, _| window.launcher().view().screen);
        if !hidden(&window, cx) && screen == Screen::Command {
            break;
        }
        assert!(Instant::now() < deadline, "timed out: {screen:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
    (window, cx, elsewhere, [sources, data])
}

#[gpui::test]
fn an_attention_launch_shows_the_hidden_launcher_with_the_focus_in_its_view(
    cx: &mut TestAppContext,
) {
    let (window, cx, _, _folders) = shown_by_an_attention_launch(cx);
    let rows = titles(&window, cx);
    assert!(
        rows.iter()
            .any(|row| row == r#"Context: {"alarm":"07:00"}"#),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row == "Launch type: user-initiated"),
        "{rows:?}"
    );
    assert!(has_focus(cx), "the opened view has the focus");
    assert_eq!(settings_windows(cx).len(), 1, "Settings stays open");

    // Escape acts on the opened view at once, without a click.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(matches!(view.screen, Screen::Root { .. }), "{view:?}");
}

/// A launcher an attention launch showed hides once another application
/// takes the focus (#571), as one the Open Pane hotkey showed does: the
/// taskbar's clock clicked over the Ringing screen, say. Pane's own
/// Settings window taking the focus leaves it shown. Hidden, the user left
/// what the attention launch opened, so the next one may be shown.
#[gpui::test]
fn an_attention_launched_launcher_hides_when_another_application_takes_the_focus(
    cx: &mut TestAppContext,
) {
    let (window, cx, elsewhere, _folders) = shown_by_an_attention_launch(cx);
    assert!(
        cx.update(|window, _| window.is_window_active()),
        "the attention launch brought the launcher forward"
    );

    // The Settings window, Pane's own, takes the focus: the launcher stays.
    cx.deactivate_window();
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "Pane's own window keeps it shown");

    // Back on the launcher, then the taskbar takes the focus: it hides.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    elsewhere.store(true, Ordering::SeqCst);
    cx.deactivate_window();
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert!(hidden(&window, cx), "another application's focus hid it");
    assert_eq!(settings_windows(cx).len(), 1, "Settings stays open");
}
