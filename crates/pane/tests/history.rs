//! Root search's recent queries (#206) through the native window, on
//! GPUI's test platform, with real key events: Up on an empty query, the
//! first row selected, restores the previous query with the argument
//! values typed with it; repeated Up walks back through the entries;
//! typing ends the walk; and Up with another row selected, or a typed
//! query the walk did not restore, moves the selection as it always
//! did — the history never hijacks navigation. The commands are the
//! Rust arguments sample's "Greet" (a required name, an optional secret,
//! a tone) and "Stamp" (a required label), from `cargo xtask guests`.

use std::path::PathBuf;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, Runtime, Screen};

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_bare};

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

/// A window over a launcher with the Rust arguments sample installed,
/// keeping its records in a data folder of the test's own.
fn installed(
    cx: &mut TestAppContext,
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    tempfile::TempDir,
    PathBuf,
) {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let folder = assembled_package("sample-arguments", &sources.path().join("arguments"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let pending = launcher.install_package(&folder);
    cx.foreground_executor().block_on(pending);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx, data, folder)
}

/// The query field's text.
fn query_text(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> String {
    let field = cx.read_entity(window, |window, _| window.query_field());
    cx.read_entity(&field, |field, _| field.as_str().to_owned())
}

/// The argument field `name`'s text, as the window shows it.
fn argument_text(window: &Entity<LauncherWindow>, cx: &VisualTestContext, name: &str) -> String {
    let field = cx.read_entity(window, |window, _| window.argument_field(name));
    match field {
        Some(field) => cx.read_entity(&field, |field, _| field.as_str().to_owned()),
        None => panic!("no argument field {name}"),
    }
}

/// Types `query` — the row `row` comes selected — fills the selected
/// row's argument fields as the keyboard does when `values` is given
/// (Tab into each, the value typed), and clears the query with Escape —
/// twice when a field held the focus, as one Escape leaves a focused
/// field for the query first — so the query is recorded as a recent one
/// (#206).
fn type_and_clear(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    query: &str,
    row: &str,
    values: &[&str],
) {
    cx.simulate_input(query);
    let view = settle(window, cx);
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some(row),
        "{query} selects the {row} row"
    );
    for value in values {
        cx.simulate_keystrokes("tab");
        cx.simulate_input(value);
    }
    // Escape from a focused field returns the focus to the query; the
    // next Escape clears it, recording the query with its values.
    if !values.is_empty() {
        cx.simulate_keystrokes("escape");
    }
    cx.simulate_keystrokes("escape");
    let view = settle_bare(window, cx);
    assert_eq!(
        view.screen,
        Screen::Root { query: "".into() },
        "{query} was cleared"
    );
}

#[gpui::test]
fn up_restores_the_previous_query_with_its_argument_values(cx: &mut TestAppContext) {
    let (window, cx, _data, _folder) = installed(cx);

    // "greet" typed with the name filled: Escape clears it, recording
    // the query with the value typed into its field.
    type_and_clear(&window, cx, "greet", "Greet", &["Ada"]);

    // Up on the empty query, the first row selected: the most recent
    // query is restored, its argument values with it — the password
    // empty as it was recorded, the fields' focus left with the query.
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        },
        "the query is restored"
    );
    assert_eq!(query_text(&window, cx), "greet");
    assert_eq!(argument_text(&window, cx, "name"), "Ada");
    assert_eq!(argument_text(&window, cx, "secret"), "");
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Greet")
    );

    // Enter runs the command with the restored values, as typing them
    // would: one key repeats the search.
    cx.simulate_keystrokes("enter");
    let view = settle_bare(&window, cx);
    assert!(
        matches!(&view.status, pane_core::Status::Result(message)
            if message.contains("name=Ada")),
        "{:?}",
        view.status
    );
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        },
        "root search stays as it was"
    );
}

#[gpui::test]
fn repeated_up_walks_back_and_typing_ends_the_walk(cx: &mut TestAppContext) {
    let (window, cx, _data, _folder) = installed(cx);

    // Two searches recorded, the newest first.
    type_and_clear(&window, cx, "stamp", "Stamp", &["tag"]);
    type_and_clear(&window, cx, "greet", "Greet", &["Ada"]);

    // Up restores the most recent one, and Up again the one before,
    // while the query the walk restored stands.
    cx.simulate_keystrokes("up");
    settle(&window, cx);
    assert_eq!(query_text(&window, cx), "greet");
    assert_eq!(argument_text(&window, cx, "name"), "Ada");
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "stamp".into()
        },
        "the walk went one back"
    );
    assert_eq!(query_text(&window, cx), "stamp");
    assert_eq!(argument_text(&window, cx, "label"), "tag");

    // Up past the oldest restores nothing: the key does nothing more.
    cx.simulate_keystrokes("up");
    settle(&window, cx);
    assert_eq!(query_text(&window, cx), "stamp");

    // Typing ends the walk: the query typed on from the restored one is
    // the user's, and Up never takes it to the history.
    cx.simulate_input("x");
    settle(&window, cx);
    assert_eq!(query_text(&window, cx), "stampx");
    cx.simulate_keystrokes("up");
    settle(&window, cx);
    assert_eq!(
        query_text(&window, cx),
        "stampx",
        "a typed query is not the history's"
    );
}

#[gpui::test]
fn up_with_another_row_selected_moves_the_selection_not_the_history(cx: &mut TestAppContext) {
    let (window, cx, _data, _folder) = installed(cx);

    // "t" matches both of the sample's commands — a query that lists
    // rows to move through — and "greet" one of them. Both cleared,
    // recorded as recent queries.
    cx.simulate_input("t");
    let view = settle(&window, cx);
    assert!(view.rows.len() >= 2, "the query lists both rows");
    cx.simulate_keystrokes("escape");
    settle_bare(&window, cx);
    type_and_clear(&window, cx, "greet", "Greet", &["Ada"]);

    // Up restores the most recent query, and Up again walks back to the
    // one that lists rows.
    cx.simulate_keystrokes("up");
    settle(&window, cx);
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(query_text(&window, cx), "t");
    assert_eq!(view.selected, Some(0), "the first row is selected");

    // A row other than the first selected — Down moved the selection —
    // ends the walk, and Up moves the selection as it always did: the
    // query stands, the walk never resumes.
    cx.simulate_keystrokes("down");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(1), "Down moved the selection");
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0), "Up moved the selection back");
    assert_eq!(
        query_text(&window, cx),
        "t",
        "the history did not take the key"
    );
    // The walk is over: another Up moves nothing, and the query stands.
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(query_text(&window, cx), "t");
    assert_eq!(view.selected, Some(0));
}
