//! Searching an online service inside its command through the native
//! window, on GPUI's test platform, with real key events: typing in root
//! search finds Package search (the search sample from `cargo xtask
//! guests`) without asking its service; opening it gives the same field to
//! its own search, whose results the window lists. The service is the
//! fixture package registry, served on 127.0.0.1 by the test.

#[path = "../../pane-core/tests/support/service.rs"]
mod service;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};
use service::Service;

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

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

    // Enter opens it: the field, now its own, is empty and keeps focus.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        }
    );
    assert_eq!(field_text(&window, cx), "");
    assert!(cx.debug_bounds("search").is_some(), "the field is rendered");

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

    // Typing searches the service; its results are the rows.
    cx.simulate_input("aurora");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: "aurora".into()
        }
    );
    assert_eq!(titles(&view), ["aurora-charts", "aurora-cli"]);
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

    // Nothing found: "No results" for the text.
    cx.simulate_input("zzz");
    settle(&window, cx);
    assert!(cx.debug_bounds("no-results").is_some());

    // Escape clears the search, then leaves the command.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        }
    );
    assert_eq!(field_text(&window, cx), "");
    assert_eq!(
        titles(&view),
        ["Type to search the package registry", "Service address"]
    );
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
}
