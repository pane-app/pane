//! Giving a command an alias and making it a fallback through the native
//! window, on GPUI's test platform, then reaching it from root search with
//! real key events: the alias and some text, and the first fallback,
//! preselected when nothing else matches (ADR 0031), reached by Enter and
//! by its number chord alike. The command is Echo, the query sample from
//! `cargo xtask guests`, which shows a toast with the text it is sent.

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// The selected row's node, as assistive technology sees the list: its
/// label, its place in the list and the list's size.
fn selected_row(cx: &mut VisualTestContext) -> (String, u64, u64) {
    let tree: serde_json::Value = serde_json::from_str(&a11y::a11y(cx)).unwrap();
    let rows: Vec<serde_json::Value> = tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .filter(|aria| aria["role"] == "ListBoxOption")
        .collect();
    let selected = rows
        .iter()
        .find(|row| row["selected"] == true)
        .unwrap_or_else(|| panic!("no selected row in {rows:#?}"));
    (
        selected["label"].as_str().unwrap().to_owned(),
        selected["position_in_set"].as_u64().unwrap(),
        selected["size_of_set"].as_u64().unwrap(),
    )
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
    let folder = assembled_package("sample-query", &sources.path().join("query"));
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

    // The extension list, as Settings enters it (#168).
    enter_flow(&window, cx);
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
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Echo heard “hello”".into())
    );
    assert!(cx.debug_bounds("toast-success").is_some());

    // Text nothing matches: the notice heads the fallbacks and root
    // search itself selects the first, so Enter sends it the text, trimmed
    // (ADR 0031). Nothing was sent to Echo while typing: it stopped with
    // the query and has not started again once the runtime has served
    // every call asked for before it answers.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let echo = cx
        .read_entity(&window, |window, _| {
            window.launcher().packages()[0].location.clone()
        })
        .join("sample_query.wasm");
    runtime.forget([echo.clone()]);
    cx.simulate_input("zqx words ");
    let view = settle(&window, cx);
    assert_eq!(titles(&view), ["Echo"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “zqx words” · fallback")
    );
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.status, Status::Idle);
    let notice = cx.debug_bounds("no-results").expect("the notice is drawn");
    let fallback = cx.debug_bounds("row-Echo").expect("the fallback is drawn");
    assert!(fallback.top() > notice.bottom(), "the fallback is below it");
    // The preselected fallback is the selection assistive technology
    // sees (#194).
    assert_eq!(selected_row(cx), ("Echo", 1, 1));
    cx.run_until_parked();
    assert!(!block_on(runtime.running()).contains(&echo), "typing sent nothing");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Echo heard “zqx words”".into())
    );
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zqx words "), "root search stays as it was");
    assert!(block_on(runtime.running()).contains(&echo));

    // A fallback row is an ordinary row for Ctrl and a digit: the chord
    // picks the preselected first fallback as any other row.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("zqx again");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    cx.simulate_keystrokes("ctrl-1");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Echo heard “zqx again”".into())
    );
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zqx again"));
}
