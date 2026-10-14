//! Searching an online service inside its command through the native
//! window, on GPUI's test platform, with real key events: typing in root
//! search finds Package search (the search sample from `cargo xtask
//! guests`) without asking its service; opening it gives its designed List
//! the header's search field (#240), whose text is told to the view
//! through the List's search-text event, throttled, and whose results the
//! launcher's own rows list draws. The service is the fixture package
//! registry, served on 127.0.0.1 by the test.

#[path = "../../pane-core/tests/support/service.rs"]
mod service;

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};
use service::Service;

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown, until};

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// The text in the window's query field.
fn field_text(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let field = cx.read_entity(window, |window, _| window.query_field());
    cx.read_entity(&field, |field, _| field.as_str().to_owned())
}

/// Points the open Package search at `address`, through the setting its
/// searches read.
fn use_service(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, address: &str) {
    let identity = cx.read_entity(window, |window, _| {
        window
            .launcher()
            .packages()
            .into_iter()
            .next()
            .expect("the search sample is installed")
            .identity
    });
    let saved = window.update(cx, |window, _| window.launcher().clone());
    let saved = saved.set_preference(&identity, "service", Some(address));
    cx.foreground_executor().block_on(saved).expect("saved");
    settle(window, cx);
}

#[gpui::test]
fn the_commands_own_search_field_lists_the_service_results(cx: &mut TestAppContext) {
    let service = Service::start();
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-search", &sources.path().join("search"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // Root search finds the command by its title, and asks no service.
    cx.simulate_input("package search");
    let view = settle(&window, cx);
    assert_eq!(titles(&view).first(), Some(&"Package search"));
    assert_eq!(service.requests(), Vec::<String>::new());

    // Enter opens it: the designed List owns the header's search field,
    // empty and focused, with the view's navigation title above it.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::DesignedView { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Package search");
    assert_eq!(field_text(&window, cx), "");
    assert!(cx.debug_bounds("search").is_some(), "the field is rendered");
    assert!(cx.debug_bounds("designed-title").is_some(), "the title row");
    assert!(
        cx.debug_bounds("designed-empty-Type to search the package registry")
            .is_some()
    );

    // Typing searches the service; its results are the rows, and the
    // field holds the text typed. A designed view answers its searches
    // asynchronously — the answer asks for its own drawing — so the rows
    // are waited for, not read at once. The view throttles what it is
    // told: the typing's text waits out the throttle's time on the
    // executor's clock, which this platform's parked runs never move.
    cx.simulate_input("aurora");
    cx.executor().advance_clock(Duration::from_millis(300));
    let view = until(&window, cx, |view| {
        view.rows
            .first()
            .is_some_and(|row| row.title == "aurora-charts")
    });
    assert_eq!(titles(&view), ["aurora-charts", "aurora-cli"]);
    assert_eq!(field_text(&window, cx), "aurora");
    assert!(cx.debug_bounds("row-aurora-cli").is_some());
    assert_eq!(
        service.requests().last().map(String::as_str),
        Some("/search?q=aurora")
    );

    // Enter shows the selected package's details in a toast.
    cx.simulate_keystrokes("down enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result(
            "aurora-cli 0.9.3 (Apache-2.0): Command-line parsing with subcommands".into()
        )
    );

    // Escape clears the search field, then leaves the command.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::DesignedView { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(field_text(&window, cx), "");
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

#[gpui::test]
fn backspace_in_the_empty_search_field_pops_the_view(cx: &mut TestAppContext) {
    let service = Service::start();
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-search", &sources.path().join("search"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    settle(&window, cx);
    // The preview's Enter installs it, as the first test does, before
    // its service is set.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    use_service(&window, cx, &service.url());
    cx.simulate_input("package search");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // The field holds text: Backspace deletes it, as a field's does.
    cx.simulate_input("aurora");
    settle(&window, cx);
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::DesignedView { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(field_text(&window, cx), "auror");

    // The field empty: Backspace leaves the command (#240, the general
    // order the launcher polish's, #123).
    for _ in "auror".chars() {
        cx.simulate_keystrokes("backspace");
    }
    settle(&window, cx);
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}
