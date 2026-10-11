//! The loading bar (#248, ADR 0035): the one-pixel line along the rule
//! under the search field, shown only once work the user is waiting for
//! has outlasted its 300 ms threshold, sweeping a soft highlight across
//! itself while the work is pending and fading away when it ends — the
//! footer's "Running…" text replaced. The slow fixture is the calculator
//! tests' own: the faulty guest, whose root provider answers "0 + 0"
//! after about a second of busy work; the quick case is the no-view
//! sample's "Report launch", invoked from root search so the field stays
//! on screen. The window tests run on the test platform's controlled
//! clock, which the bar's whole count moves; the service-held command
//! search covers the status-running kind of waited-for work.

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown, until};

#[path = "support/packages.rs"]
mod packages;

use packages::{assembled_package, slow_provider};

#[path = "support/wait.rs"]
mod wait;

use wait::frame;

#[path = "support/a11y.rs"]
mod a11y;

use a11y::announcement;

#[path = "../../pane-core/tests/support/service.rs"]
mod service;

use service::Service;

/// How far past the loading bar's 300 ms threshold the tests move the
/// clock: past it by a frame, as the bar's own wake asks for.
const PAST: Duration = Duration::from_millis(350);

/// A moment within the bar's fade.
const SOME: Duration = Duration::from_millis(100);

/// The loading bar the last frame drew: its strength (0 hidden to 1
/// shown) and its sweep's highlight (left edge and width as shares of the
/// line), `None` when the last frame drew no line.
fn bar(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> Option<(f32, Option<(f32, f32)>)> {
    cx.read_entity(window, |window, _| window.loading_presentation())
}

/// The titles of the rows `view` lists.
fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// A window over a launcher with the slow fixture installed as a package
/// that computes root results, through its preview and Enter, as a user
/// installs it.
fn slow(
    cx: &mut TestAppContext,
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = slow_provider(&sources.path().join("slow"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let preview = folder;
    let (window, cx) = cx.add_window_view(move |window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&preview, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Installed Slow".into()));
    (window, cx, sources, data)
}

/// A window over a launcher with the no-view sample installed, whose
/// "Report launch" runs quickly from root search, as a user installs it.
fn quick(
    cx: &mut TestAppContext,
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-no-view", &sources.path().join("no-view"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let preview = folder;
    let (window, cx) = cx.add_window_view(move |window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&preview, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed No-view sample".into())
    );
    (window, cx, sources, data)
}

/// The core says the waited-for work is pending, and since when (#248):
/// the root search's wait on its providers.
#[gpui::test]
fn the_core_exposes_the_root_searchs_pending_since(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data) = slow(cx);
    assert_eq!(
        cx.read_entity(&window, |window, _| window.launcher().pending_since()),
        None,
        "no work is pending over a blank query"
    );

    // The query asks the slow fixture, which computes for about a second.
    cx.simulate_input("0 + 0");
    cx.run_until_parked();
    assert!(
        cx.read_entity(&window, |window, _| window.launcher().pending_since())
            .is_some(),
        "the search's wait on its providers is pending"
    );

    // Cleared once the search is replaced by a blank one.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""));
    assert_eq!(
        cx.read_entity(&window, |window, _| window.launcher().pending_since()),
        None
    );
}

/// A slow search shows the line along the rule only once it has outlasted
/// the threshold, sweeping while it waits and fading away when the work
/// ends; the footer's strip says nothing, the announcement speaks the
/// busy state only past the threshold.
#[gpui::test]
fn a_slow_root_search_shows_the_late_loading_bar_and_fades_it_away(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data) = slow(cx);

    // The query asks the slow fixture, which computes for about a second.
    cx.simulate_input("0 + 0");
    settle(&window, cx);

    // Beneath the threshold nothing shows: the window is idle, and the
    // busy state is not announced.
    assert!(cx.debug_bounds("loading-bar").is_none());
    assert_ne!(announcement(cx), "Running…");

    // Past the threshold the line is drawn along the rule under the
    // field: the search header's 64px row, over its hairline.
    frame(cx, PAST);
    let search = cx.debug_bounds("search").expect("the search field");
    let line = cx.debug_bounds("loading-bar").expect("the line is drawn");
    assert_eq!(line.size.height, px(1.), "the line is one pixel");
    assert_eq!(line.bottom(), search.origin.y + px(64.), "along the rule");

    // The busy state is announced, now that the wait has outlasted the
    // threshold.
    assert_eq!(announcement(cx), "Running…");

    // The fade-in is under way and the sweep has entered; the highlight
    // moves along the line as frames are delivered.
    frame(cx, SOME);
    let (strength, sweep) = bar(&window, cx).expect("the line is drawn");
    assert!(
        strength > 0. && strength < 1.,
        "the line fades in: {strength}"
    );
    assert!(sweep.is_some(), "the sweep runs");
    let entered = cx
        .debug_bounds("loading-sweep")
        .expect("the sweep is drawn");
    frame(cx, SOME);
    let swept = cx
        .debug_bounds("loading-sweep")
        .expect("the sweep is drawn");
    assert!(
        swept.origin.x + swept.size.width > entered.origin.x + entered.size.width + px(1.),
        "the highlight sweeps across the line: {swept:?} after {entered:?}"
    );

    // The fade completes; the sweep keeps the frames coming while the
    // work is pending.
    frame(cx, SOME);
    frame(cx, SOME);
    let (strength, _) = bar(&window, cx).expect("the line is drawn");
    assert_eq!(strength, 1., "the line is fully shown");
    assert!(
        frame(cx, Duration::from_millis(25)) >= 1,
        "a frame is asked for"
    );

    // The fixture's answer arrives: the work is over, the line fades away
    // from where it was, and the window is idle again.
    let view = until(&window, cx, |view| !view.rows.is_empty());
    assert_eq!(titles(&view), ["Slow answer"]);
    frame(cx, SOME);
    let (strength, _) = bar(&window, cx).expect("the line still shows");
    assert!(strength < 1., "the line is leaving: {strength}");
    frame(cx, PAST);
    assert!(cx.debug_bounds("loading-bar").is_none(), "the line is gone");
    assert_eq!(
        frame(cx, Duration::from_millis(25)),
        0,
        "the window is idle"
    );
}

/// An action that finishes in a few milliseconds never shows the line and
/// is never announced as busy.
#[gpui::test]
fn a_quick_action_shows_no_line_and_is_never_announced_busy(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data) = quick(cx);
    cx.simulate_input("report launch");
    let view = settle(&window, cx);
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Report launch")
    );

    // Enter runs it from root search, so the field stays on screen: quick
    // work, with frames delivered beneath the threshold.
    cx.simulate_keystrokes("enter");
    frame(cx, SOME);
    assert!(
        cx.debug_bounds("loading-bar").is_none(),
        "no line for quick work"
    );
    assert_ne!(
        announcement(cx),
        "Running…",
        "a quick action is not announced busy"
    );

    // Its answer is what the user reads and hears.
    let shown = settle_shown(&window, cx);
    assert!(
        matches!(&shown, Status::Result(text) if text.starts_with("Report:")),
        "{shown:?}"
    );
    assert!(cx.debug_bounds("loading-bar").is_none());
    assert_ne!(announcement(cx), "Running…");
}

/// Under reduced motion the line is still, at partial strength, and asks
/// for no frame; it leaves at once when the work ends.
#[gpui::test]
fn under_reduced_motion_the_line_is_still_at_partial_strength(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data) = slow(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));

    cx.simulate_input("0 + 0");
    settle(&window, cx);
    assert!(cx.debug_bounds("loading-bar").is_none());

    // Past the threshold: the line, at half strength, nothing sweeping.
    frame(cx, PAST);
    let (strength, sweep) = bar(&window, cx).expect("the line is drawn");
    assert_eq!(strength, 0.5, "the still line's partial strength");
    assert_eq!(sweep, None, "the line is still");
    assert!(cx.debug_bounds("loading-sweep").is_none());
    assert_eq!(
        frame(cx, Duration::from_millis(25)),
        0,
        "a still line asks for no frame"
    );

    // It leaves at once when the work ends.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert_eq!(bar(&window, cx), None, "the line left at once");
    assert!(cx.debug_bounds("loading-bar").is_none());
}

/// A command's own search, held by its service, shows the same line under
/// its own rule — the status running kind of waited-for work — and fades
/// away when the search is cleared.
#[gpui::test]
fn a_slow_command_search_shows_the_line_under_its_own_rule(cx: &mut TestAppContext) {
    let service = Service::start();
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-search", &sources.path().join("search"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let preview = folder;
    let (window, cx) = cx.add_window_view(move |window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&preview, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // Root search finds the command by its title, Enter opens it.
    cx.simulate_input("package search");
    let view = settle(&window, cx);
    assert_eq!(titles(&view).first(), Some(&"Package search"));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        }
    );

    // Its form points it at this test's service.
    cx.simulate_keystrokes("down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    cx.simulate_input(&service.url());
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(format!("Searching {} from now on", service.url()))
    );
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // A search the service holds for ten seconds: the command is asked,
    // the wait is on and the field is on screen.
    cx.simulate_input("slow");
    cx.run_until_parked();
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(
        matches!(view.status, Status::Running { .. }),
        "the search is pending"
    );
    assert!(
        cx.read_entity(&window, |window, _| window.launcher().pending_since())
            .is_some(),
        "the core says the work is pending"
    );
    assert!(
        cx.debug_bounds("loading-bar").is_none(),
        "beneath the threshold"
    );

    // The request the search makes reaches the service before the test
    // goes on: the service holds the answer ten seconds, and a search
    // stopped before it starts — Escape below, or a newer text — is never
    // started at all, so being asked is what is waited for here, as the
    // answer is what a quick search's settle waits for.
    wait::until(cx, |_| {
        service
            .requests()
            .iter()
            .any(|path| path == "/search?q=slow")
            .then_some(())
    });

    // Past it, the same line along the command's own rule — and the
    // footer's strip says nothing for such work (#248): no "Running…"
    // text, though the strip keeps its selector.
    frame(cx, PAST);
    let search = cx
        .debug_bounds("search")
        .expect("the command's search field");
    let line = cx.debug_bounds("loading-bar").expect("the line is drawn");
    assert_eq!(line.size.height, px(1.));
    assert_eq!(
        line.bottom(),
        search.origin.y + px(64.),
        "along its own rule"
    );
    assert!(
        cx.debug_bounds("status-message").is_none(),
        "no message in the strip"
    );
    assert!(
        cx.debug_bounds("status-running").is_some(),
        "the strip keeps its selector"
    );

    // The search is cleared: the wait ends, and the line fades away.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.status),
        (
            Screen::CommandSearch {
                query: String::new()
            },
            Status::Idle
        )
    );
    frame(cx, PAST);
    assert!(cx.debug_bounds("loading-bar").is_none(), "the line is gone");
    assert_eq!(
        frame(cx, Duration::from_millis(25)),
        0,
        "the window is idle"
    );
    assert_eq!(
        service.requests().last().map(String::as_str),
        Some("/search?q=slow")
    );
}
