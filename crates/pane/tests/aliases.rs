//! Giving a command an alias and making it a fallback through the native
//! window, on GPUI's test platform, then reaching it from root search with
//! real key events: the alias and some text, and a fallback the user moves
//! to. The command is Echo, the query sample from `cargo xtask guests`,
//! which answers the text it is sent.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// Copies the assembled query sample to `folder`.
fn package(folder: &Path) -> PathBuf {
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-query");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// Selects the row titled `title` on the screen shown and presses Enter.
fn press_enter_on(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let view = launcher.view();
    let index = titles(&view)
        .iter()
        .position(|row| *row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(&view)));
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

#[gpui::test]
fn an_alias_and_a_fallback_set_in_the_window_send_the_typed_text_to_the_command(
    cx: &mut TestAppContext,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("query"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    press_enter_on(&window, cx, "Manage extensions…");
    let view = press_enter_on(&window, cx, "Alias for Echo");
    assert_eq!(view.title, "Alias for Echo");
    assert!(matches!(view.screen, Screen::Form(_)));
    cx.simulate_input("ec");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Typing “ec” now finds Echo".into())
    );
    let selected = view.selected.map(|index| view.rows[index].title.as_str());
    assert_eq!(selected, Some("Alias for Echo"));

    // Its form starts with the alias.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    let field = cx.read_entity(&window, |window, _| window.text_field("alias").unwrap());
    let text = cx.read_entity(&field, |field, _| field.as_str().to_owned());
    assert_eq!(text, "ec");
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    let view = press_enter_on(&window, cx, "Fallback: Echo");
    assert_eq!(
        view.status,
        Status::Result("Echo is now offered for any text typed in root search".into())
    );

    // The alias and text: the row that sends it is selected; Enter sends it.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    assert_eq!(titles(&view)[0], "Echo");
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Echo heard “hello”".into()));
    assert!(cx.debug_bounds("status-result").is_some());

    // Text nothing matches: "No results", then the fallback, which Enter
    // does not choose by itself; Down does. Nothing was sent to Echo: it
    // stopped with the query and has not started again once the runtime
    // has served every call asked for before it answers.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let echo = cx
        .read_entity(&window, |window, _| {
            window.launcher().packages()[0].location.clone()
        })
        .join("sample_query.wasm");
    runtime.forget([echo.clone()]);
    cx.simulate_input("zqx");
    let view = settle(&window, cx);
    assert_eq!(titles(&view), ["Echo"]);
    assert_eq!(view.selected, None);
    assert_eq!(view.status, Status::Idle);
    assert!(cx.debug_bounds("no-results").is_some());
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(!block_on(runtime.running()).contains(&echo));
    cx.run_until_parked();
    assert_eq!(view.status, Status::Idle);
    cx.simulate_keystrokes("down enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Echo heard “zqx”".into()));
    assert_eq!(view.query(), Some("zqx"));
    assert!(block_on(runtime.running()).contains(&echo));
}
