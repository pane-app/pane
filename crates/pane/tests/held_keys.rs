//! Keys the window holds while the current query's list is not yet
//! published (#203), on GPUI's test platform, with real key events: the
//! calculator answers within the budget, so typing holds the list for the
//! moment its answer takes, and the faulty fixture (a root provider slow
//! on "0 + 0", with the launcher's clock frozen) holds it for about a
//! second of the test's own time. Keys pressed under a hold are applied
//! once the list is published — to the row it selects, never the previous
//! list's — or after the hold's 300 ms, in the order they were pressed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::clipboard::ManualClock;
use pane_core::{Launcher, LauncherView, Runtime, Status};

#[path = "support/packages.rs"]
mod packages;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/wait.rs"]
mod wait;

use packages::assembled_package;
use settle::{settle, until};

/// How long the window holds a key for the query's list (#203), on the
/// test platform's controlled clock, which only the test advances.
const HOLD: Duration = Duration::from_millis(300);

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The calculator package's folder, from `cargo xtask guests`.
fn calculator_folder() -> PathBuf {
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/calculator");
    assert!(
        folder.exists(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    folder
}

/// A package folder holding one root-results command whose component is
/// the faulty fixture, which answers "0 + 0" only after about a second of
/// busy work.
fn slow_package(sources: &Path) -> PathBuf {
    let faulty = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/faulty.wasm");
    assert!(
        faulty.exists(),
        "{} is missing; run `cargo xtask guests`",
        faulty.display()
    );
    let folder = sources.join("slow");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("pane.json"),
        r#"{ "manifestVersion": 1, "title": "Slow", "apiVersion": "0.1",
  "commands": [{ "id": "command", "title": "Slow answers",
    "component": "command.wasm", "rootResults": true }] }"#,
    )
    .unwrap();
    std::fs::copy(&faulty, folder.join("command.wasm")).unwrap();
    folder
}

/// Installs `folder` in `launcher`, answering once the install's guest
/// check has, and back at root search.
fn install(cx: &mut TestAppContext, launcher: &Launcher, folder: &Path) {
    cx.foreground_executor()
        .block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
}

/// The window over `launcher`, on root search.
fn open_launcher(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

/// A launcher with the calculator installed in `data` (as `window.rs`'s
/// `with_calculator` builds it), and the window over it.
fn calculator_window<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"));
    install(cx, &launcher, &calculator_folder());
    open_launcher(cx, launcher)
}

/// The window over the calculator and the slow command, with the
/// launcher's clock frozen: the list for "0 + 0" stays unpublished until
/// the slow command answers (about a second), so keys pressed under it
/// are held for the test's own time, not the real one.
fn slow_window<'a>(
    cx: &'a mut TestAppContext,
    sources: &Path,
    data: &Path,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"))
        .with_clock(ManualClock::at(0));
    install(cx, &launcher, &calculator_folder());
    install(cx, &launcher, &slow_package(sources));
    open_launcher(cx, launcher)
}

/// Runs the window until the rows it shows are `expected`, answering the
/// view once they are (as `window.rs`'s `wait_for_rows` waits).
fn wait_for_rows(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    expected: &[&str],
) -> LauncherView {
    until(window, cx, |view| {
        view.rows
            .iter()
            .map(|row| row.title.as_str())
            .eq(expected.iter().copied())
    })
}

/// The text in the window's query field.
fn field_text(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let field = cx.read_entity(window, |window, _| window.query_field());
    cx.read_entity(&field, |field, _| field.as_str().to_owned())
}

/// Whether root search's query field has keyboard focus.
fn query_has_focus(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    use gpui::Focusable;
    let input = cx.read_entity(window, |window, _| window.query_field());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// The launcher's view as it is now.
fn view(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    cx.read_entity(window, |window, _| window.launcher().view())
}

/// The keys the window holds, as each keystroke is written.
fn held(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read_entity(window, |window, _| window.held_keys())
}

#[gpui::test]
fn enter_pressed_at_once_with_typing_runs_the_published_first_row(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = calculator_window(cx, data.path());

    // A first query's list, published with its answer.
    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // Typing on and pressing Enter at once: the previous list's row ("42")
    // is not what the user means — Enter waits for the query's list.
    cx.simulate_input("+1");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        view(&window, cx).status,
        Status::Idle,
        "nothing ran while the list is held"
    );

    // The calculator answers within the budget: the published list takes
    // "43", and the held Enter runs it — not the previous list's "42".
    let view = until(&window, cx, |view| matches!(view.status, Status::Result(_)));
    assert_eq!(
        view.status,
        Status::Result("Copied 43 to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("43".into())
    );
    assert_eq!(view.query(), Some("6*7+1"), "root search stays as it was");
}

#[gpui::test]
fn the_open_actions_chord_waits_for_the_querys_list(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = calculator_window(cx, data.path());

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // The chord pressed under the new query's hold opens no panel yet —
    // not over the previous list's row.
    cx.simulate_input("+1");
    cx.simulate_keystrokes(OPEN_ACTIONS);
    assert!(!cx.read_entity(&window, |window, _| window.actions_open()));

    // Published, the held chord opens the panel over the answer's row.
    wait_for_rows(&window, cx, &["43"]);
    wait::until(cx, |cx| {
        cx.read_entity(&window, |window, _| window.actions_open())
            .then_some(())
    });
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("6*7+1"), "root search stays as it was");
}

#[gpui::test]
fn a_digit_chord_waits_for_the_querys_list(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = calculator_window(cx, data.path());

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // Ctrl+1 picks the first row: pressed under the hold, it waits, so it
    // picks the published list's first row — the answer "43", not the
    // previous list's "42".
    cx.simulate_input("+1");
    cx.simulate_keystrokes("ctrl-1");
    assert_eq!(
        view(&window, cx).status,
        Status::Idle,
        "nothing ran while the list is held"
    );

    let view = until(&window, cx, |view| matches!(view.status, Status::Result(_)));
    assert_eq!(
        view.status,
        Status::Result("Copied 43 to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("43".into())
    );
}

#[gpui::test]
fn tab_waits_for_the_querys_list(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = calculator_window(cx, data.path());

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // Tab pressed under the new query's hold moves no focus yet.
    cx.simulate_input("+1");
    cx.simulate_keystrokes("tab");
    assert!(
        query_has_focus(&window, cx),
        "the field keeps focus while the list is held"
    );

    // Published, the held Tab takes its focus move.
    wait_for_rows(&window, cx, &["43"]);
    wait::until(cx, |cx| (!query_has_focus(&window, cx)).then_some(()));
}

#[gpui::test]
fn a_space_typed_while_the_query_could_still_be_an_alias_waits(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.executor().allow_parking();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    // The calculator, and the query sample with "ec" as Echo's alias:
    // "ec" plus a space is the command's invocation.
    install(cx, &launcher, &calculator_folder());
    let folder = assembled_package("sample-query", &sources.path().join("query"));
    install(cx, &launcher, &folder);
    cx.foreground_executor()
        .block_on(launcher.set_alias("echo", "ec").expect("“ec” is one word"));
    let (window, cx) = open_launcher(cx, launcher);

    // "ec" is the alias, so the space could invoke it: held for the
    // query's list, it does not land in the field yet.
    cx.simulate_input("ec");
    cx.simulate_keystrokes("space");
    assert_eq!(field_text(&window, cx), "ec", "the space is held");

    // Characters typed during the hold land behind it, in order: once the
    // list is published, the field reads "ec hello" and the row that sends
    // the text to Echo is selected.
    cx.simulate_input("hello");
    let view = wait_for_rows(&window, cx, &["Echo"]);
    assert_eq!(view.query(), Some("ec hello"));
    assert_eq!(field_text(&window, cx), "ec hello");
    assert_eq!(view.selected, Some(0));
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
}

#[gpui::test]
fn a_held_key_is_applied_after_its_time_when_a_provider_never_answers(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = slow_window(cx, sources.path(), data.path());

    // A first query's list, published with its answer.
    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // "0 + 0" asks the slow command too: the list is held for its answer
    // (about a second), longer than the hold's own time. Enter pressed
    // under it runs nothing yet.
    cx.simulate_input("0 + 0");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        view(&window, cx).status,
        Status::Idle,
        "nothing ran while the list is held"
    );

    // The hold's time is up on the window's own clock, provider or no
    // provider: the key is applied to the selection as it is then — the
    // previous query's answer, "42".
    cx.executor().advance_clock(HOLD);
    cx.run_until_parked();
    let view = until(&window, cx, |view| matches!(view.status, Status::Result(_)));
    assert_eq!(
        view.status,
        Status::Result("Copied 42 to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("42".into())
    );
}

#[gpui::test]
fn an_enter_repeated_during_the_hold_runs_once(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = slow_window(cx, sources.path(), data.path());

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // Two Enters pressed under the hold — an auto-repeat's repeats: one is
    // held, the repeat is dropped.
    cx.simulate_input("0 + 0");
    cx.simulate_keystrokes("enter enter");
    assert_eq!(held(&window, cx), ["enter"], "the repeat is dropped");

    // The slow command answers and the list is published: the held Enter
    // runs once, on the published list's first row.
    let view = until(&window, cx, |view| matches!(view.status, Status::Result(_)));
    assert_eq!(
        view.status,
        Status::Result("Copied 0 to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("0".into())
    );
}

#[gpui::test]
fn a_repeated_chord_toggles_the_panel_once(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = slow_window(cx, sources.path(), data.path());

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);

    // The chord pressed twice under the hold: one is held, the repeat is
    // dropped — a second toggle would close the panel the first opens.
    cx.simulate_input("0 + 0");
    cx.simulate_keystrokes(&format!("{OPEN_ACTIONS} {OPEN_ACTIONS}"));
    assert_eq!(held(&window, cx).len(), 1, "the repeat is dropped");
    assert!(!cx.read_entity(&window, |window, _| window.actions_open()));

    wait::until(cx, |cx| {
        cx.read_entity(&window, |window, _| window.actions_open())
            .then_some(())
    });
}
