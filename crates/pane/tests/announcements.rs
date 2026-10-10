//! Screen-reader announcements in the launcher's window (#132), with real
//! key events on GPUI's test platform, read from GPUI's accessibility
//! tree: the focus stays in root search's field (or a command's list, or
//! the Actions panel's field) and no row reports itself as focused; every
//! row says where it is in its list; and the window's one hidden live
//! region, the announcer, says "<title>, <i> of <n>" as the user moves the
//! selection, a command's name and count as it opens, "No results", the
//! selected row once typing settles, and the footer's message before a
//! selection that changed with it.
//!
//! What the tests cannot see: GPUI CE's debug tree does not report a
//! node's live setting, so that the announcer is a polite live region is
//! the code's (`features/announcer.rs`), not a test's; and what a screen
//! reader speaks is the native runs' (Narrator, NVDA, VoiceOver, Orca).

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{
    CommandMatches, CommandRegistration, CommandWhen, Launcher, LauncherView, Runtime, Screen,
};

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/packages.rs"]
mod packages;
#[path = "support/samples.rs"]
mod samples;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/wait.rs"]
mod wait;

use a11y::{a11y, announcement, focused_label, no_row_has_focus};
use packages::assembled_package;
use settle::{enter_flow, settle};

/// How long the announcer waits after the last keystroke (#132).
const SETTLE: Duration = Duration::from_millis(300);

/// How long a selection waits behind the footer's message it came with.
const STATUS_LEAD: Duration = Duration::from_millis(500);

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The launcher window over `launcher`, on root search.
fn open_launcher(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    // Guest replies arrive from the real runtime thread.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    settle(&window, cx);
    (window, cx)
}

/// The launcher window over the three sample commands.
fn open_samples(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let launcher = Launcher::new(Runtime::start(), samples::sample_commands());
    open_launcher(cx, launcher)
}

/// Lets typing settle for the announcer: its time runs on the test
/// platform's clock, which only the test advances.
fn typing_settles(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(SETTLE);
    cx.run_until_parked();
}

/// Runs the window until its announcer says `expected`: results arrive
/// from the extension runtime's thread.
fn until_announced(cx: &mut VisualTestContext, expected: &str) {
    wait::until(cx, |cx| (announcement(cx) == expected).then_some(()));
}

/// Every accessibility node's properties in the window `cx` drives.
fn nodes(cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    let tree: serde_json::Value = serde_json::from_str(&a11y(cx)).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

/// The nodes with `role`.
fn with_role(cx: &mut VisualTestContext, role: &str) -> Vec<serde_json::Value> {
    nodes(cx)
        .into_iter()
        .filter(|node| node["role"] == role)
        .collect()
}

/// The selected node with `role`: (label, position in its list, its
/// list's size).
fn selected_of(cx: &mut VisualTestContext, role: &str) -> (String, u64, u64) {
    let nodes = with_role(cx, role);
    let node = nodes
        .iter()
        .find(|node| node["selected"] == true)
        .unwrap_or_else(|| panic!("no selected {role} in {nodes:#?}"));
    (
        node["label"].as_str().unwrap().to_owned(),
        node["position_in_set"].as_u64().unwrap(),
        node["size_of_set"].as_u64().unwrap(),
    )
}

/// The view the window shows.
fn view(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    settle(window, cx)
}

/// What the announcer says of row `index` of `view` after a user's move
/// within one section.
fn said(view: &LauncherView, index: usize) -> String {
    format!(
        "{}, {} of {}",
        view.rows[index].title,
        index + 1,
        view.rows.len()
    )
}

/// Asserts the field (or list, or menu) labelled `focused` keeps the
/// focus, and no row claims it.
fn assert_focus_stays(cx: &mut VisualTestContext, focused: &str) {
    assert_eq!(focused_label(cx).as_deref(), Some(focused));
    let json = a11y(cx);
    assert!(no_row_has_focus(&json), "a row claims the focus: {json}");
}

#[gpui::test]
fn root_search_keeps_its_field_focused_and_says_each_move(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    // Root search says nothing as it opens: the screen reader reads the
    // field as it takes the focus.
    assert_focus_stays(cx, "Search");
    assert_eq!(announcement(cx), "");

    for index in 1..3 {
        cx.simulate_keystrokes("down");
        let shown = view(&window, cx);
        assert_eq!(shown.selected, Some(index));
        assert_focus_stays(cx, "Search");
        assert_eq!(announcement(cx), said(&shown, index));
    }

    // Every row says its role, label, description, selected state, place
    // and the list's size.
    let shown = view(&window, cx);
    let options = with_role(cx, "ListBoxOption");
    assert!(!options.is_empty());
    for option in &options {
        let label = option["label"].as_str().expect("a label");
        let index = shown
            .rows
            .iter()
            .position(|row| row.title == label)
            .unwrap_or_else(|| panic!("{label} is not a row"));
        assert_eq!(option["position_in_set"], index + 1, "{option}");
        assert_eq!(option["size_of_set"], shown.rows.len(), "{option}");
        assert_eq!(
            option["selected"],
            shown.selected == Some(index),
            "{option}"
        );
        if let Some(subtitle) = &shown.rows[index].subtitle {
            assert_eq!(option["description"], subtitle.as_str(), "{option}");
        }
    }

    // Up comes back, said again.
    cx.simulate_keystrokes("up");
    let shown = view(&window, cx);
    assert_eq!(announcement(cx), said(&shown, 1));
}

/// Each move is drawn in a frame of its own, and each replaces the text
/// before it: what is left after the last frame, and after anything that
/// waits has had its time, is the row where the user stopped.
#[gpui::test]
fn several_fast_moves_leave_only_the_last_row(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    let mut heard = Vec::new();
    for index in 1..4 {
        cx.simulate_keystrokes("down");
        let shown = view(&window, cx);
        assert_eq!(shown.selected, Some(index));
        heard.push(announcement(cx));
    }
    let shown = view(&window, cx);
    let expected: Vec<String> = (1..4).map(|index| said(&shown, index)).collect();
    assert_eq!(heard, expected, "each frame replaced the text before it");
    cx.executor().advance_clock(STATUS_LEAD + SETTLE);
    cx.run_until_parked();
    assert_eq!(announcement(cx), said(&shown, 3), "no earlier text remains");
}

#[gpui::test]
fn typing_says_the_selected_row_once_settled_if_it_changed(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    let blank = view(&window, cx);
    assert_eq!(
        blank.selected.map(|index| blank.rows[index].title.as_str()),
        Some("JavaScript sample"),
        "root search's first row, by the blank query's no-query order (#199)"
    );

    // Typing that keeps the same first row: nothing, then or later.
    cx.simulate_input("java");
    view(&window, cx);
    assert_eq!(announcement(cx), "");
    typing_settles(cx);
    assert_eq!(announcement(cx), "");
    assert_focus_stays(cx, "Search");

    // Typing that changes it: nothing for each keystroke, then the row,
    // once, after the results settle.
    cx.simulate_keystrokes("backspace backspace backspace backspace");
    cx.simulate_input("ru");
    let shown = view(&window, cx);
    assert_eq!(shown.query(), Some("ru"));
    // What the announcer holds after each frame the test draws: it
    // changes once, from nothing to the row.
    let mut heard = vec![announcement(cx)];
    assert_eq!(heard[0], "", "nothing while typing");
    typing_settles(cx);
    until_announced(cx, &said(&shown, 0));
    heard.push(announcement(cx));
    for _ in 0..3 {
        typing_settles(cx);
        heard.push(announcement(cx));
    }
    let changes = heard.windows(2).filter(|pair| pair[0] != pair[1]).count();
    assert_eq!(changes, 1, "said once: {heard:?}");

    // A move while typing settles is said at once (the first row was
    // said for "ru"; Down twice reaches another row of "sample").
    cx.simulate_keystrokes("backspace backspace");
    cx.simulate_input("sample");
    cx.simulate_keystrokes("down down");
    let shown = view(&window, cx);
    assert_eq!(shown.selected, Some(2));
    assert_eq!(announcement(cx), said(&shown, 2));
}

#[gpui::test]
fn a_query_with_no_results_says_its_first_fallback_and_a_move_into_the_fallbacks_names_them(
    cx: &mut TestAppContext,
) {
    // Echo, the query sample, offered as a fallback through the extension
    // list, as `aliases.rs` sets it.
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-query", &sources.path().join("query"));
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = open_launcher(cx, launcher);
    window.update_in(cx, |window, w, cx| window.preview_package(&folder, w, cx));
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    enter_flow(&window, cx);
    let shown = view(&window, cx);
    let fallback = shown
        .rows
        .iter()
        .position(|row| row.title == "Fallback: Echo")
        .expect("the fallback's row");
    cx.read_entity(&window, |window, _| window.launcher().select(fallback));
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("escape");
    let shown = settle(&window, cx);
    assert!(matches!(shown.screen, Screen::Root { .. }), "{shown:?}");

    // Nothing matches: the notice heads the fallbacks, and root search
    // itself selects the first (ADR 0031). Once typing settles the
    // announcer says it, a selection Pane changed to another row.
    cx.simulate_input("zqx");
    let shown = view(&window, cx);
    assert_eq!(shown.selected, Some(0));
    assert!(cx.debug_bounds("no-results").is_some());
    typing_settles(cx);
    until_announced(cx, "Echo, 1 of 1");
    assert_focus_stays(cx, "Search");

    // A match above the fallback keeps it selected, and a move into the
    // fallbacks names their section first.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("manage");
    typing_settles(cx);
    let shown = view(&window, cx);
    assert_eq!(shown.rows[0].title, "Manage Extensions");
    assert_eq!(shown.selected, Some(0));
    until_announced(cx, "Manage Extensions, 1 of 2");
    cx.simulate_keystrokes("down");
    let shown = view(&window, cx);
    assert_eq!(shown.selected, Some(1));
    assert_eq!(announcement(cx), "Fallbacks: Echo, 2 of 2");
    assert_focus_stays(cx, "Search");
}

#[gpui::test]
fn opening_a_command_says_its_name_and_count_then_its_row(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    // Typed first: the blank query's first row is not the Rust sample's
    // (#199's no-query order).
    cx.simulate_input("rust");
    cx.simulate_keystrokes("enter");
    let shown = view(&window, cx);
    assert_eq!(shown.screen, Screen::Command);
    let count = shown.rows.len();
    // A list with no field keeps the focus on the list itself.
    assert_focus_stays(cx, "Rust sample");
    assert_eq!(
        announcement(cx),
        format!("Rust sample, {count} results. {}", said(&shown, 0))
    );

    cx.simulate_keystrokes("down");
    let shown = view(&window, cx);
    assert_focus_stays(cx, "Rust sample");
    assert_eq!(announcement(cx), said(&shown, 1));
    let (label, position, size) = selected_of(cx, "ListBoxOption");
    assert_eq!(
        (label.as_str(), position, size),
        (shown.rows[1].title.as_str(), 2, count as u64)
    );
}

/// The launcher with the Rust actions sample as its one command, opened
/// with Enter.
fn open_actions_sample(
    cx: &mut TestAppContext,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join("sample_actions.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    let command = CommandRegistration {
        id: "actions-sample".into(),
        title: "Actions sample".into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
        keywords: Vec::new(),
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    };
    let (window, cx) = open_launcher(cx, Launcher::new(Runtime::start(), vec![command]));
    cx.simulate_input("actions sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let shown = settle(&window, cx);
    assert_eq!(shown.screen, Screen::Command);
    (window, cx)
}

/// What the announcer says as the Actions panel opens, read from the
/// panel's own nodes: its name, how many entries it lists, then the
/// selected one.
fn panel_opening(cx: &mut VisualTestContext) -> String {
    let dialog = with_role(cx, "Dialog");
    let name = dialog[0]["label"].as_str().unwrap().to_owned();
    let (label, position, size) = selected_of(cx, "MenuItem");
    let noun = if size == 1 { "command" } else { "commands" };
    format!("{name}, {size} {noun}. {label}, {position} of {size}")
}

/// Asserts the open panel's field keeps the focus, every entry says its
/// place, and a move down is said.
fn assert_panel_follows_moves(cx: &mut VisualTestContext) {
    assert_focus_stays(cx, "Search actions");
    let entries = with_role(cx, "MenuItem");
    let size = entries.len() as u64;
    for entry in &entries {
        assert_eq!(entry["size_of_set"], size, "{entry}");
        assert!(entry["position_in_set"].is_u64(), "{entry}");
    }
    let (first, ..) = selected_of(cx, "MenuItem");
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    let (label, position, size) = selected_of(cx, "MenuItem");
    assert_ne!(label, first, "Down moved the selection");
    assert_focus_stays(cx, "Search actions");
    let said = announcement(cx);
    assert!(
        said.ends_with(&format!("{label}, {position} of {size}")),
        "{said}"
    );
}

#[gpui::test]
fn the_actions_panel_keeps_its_field_focused_and_says_its_entries(cx: &mut TestAppContext) {
    // Over a command's list.
    let (window, cx) = open_actions_sample(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    cx.run_until_parked();
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    assert_eq!(announcement(cx), panel_opening(cx));
    assert_panel_follows_moves(cx);

    // Closed, the list is back with its focus, and nothing is said again.
    let said = announcement(cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!cx.read_entity(&window, |window, _| window.actions_open()));
    assert_focus_stays(cx, "Actions sample");
    assert_eq!(announcement(cx), said);
}

#[gpui::test]
fn the_actions_panel_over_root_search_says_its_entries(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    cx.run_until_parked();
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    assert_eq!(announcement(cx), panel_opening(cx));
    assert!(announcement(cx).starts_with("Actions for Rust sample, "));
    assert_panel_follows_moves(cx);
}

/// The order of a message and a selection drawn in the same frame. The
/// message is set through the launcher here: a sample's toast arrives from
/// the extension runtime's thread, so whether it is drawn in the frame of
/// the next key or before it is a race no test can fix; a real toast is
/// checked on its own below.
#[gpui::test]
fn the_footers_message_is_said_before_the_selection_that_came_with_it(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    // The message and another selection reach the same frame: both are set
    // without drawing, then the window draws. (A key is no way to do it:
    // the test platform draws the message's frame before it handles Down.)
    cx.read_entity(&window, |window, _| {
        window.launcher().show_error("Something failed");
        window.launcher().select(1);
    });
    window.update(cx, |_, cx| cx.notify());
    let shown = view(&window, cx);
    assert_eq!(shown.selected, Some(1));
    assert_eq!(announcement(cx), "Something failed");
    // The footer keeps its status role, named by its message.
    assert!(
        with_role(cx, "Status")
            .iter()
            .any(|node| node["label"] == "Something failed"),
        "the footer's status"
    );

    // The selection follows once the message has had its time.
    cx.executor().advance_clock(STATUS_LEAD);
    cx.run_until_parked();
    assert_eq!(announcement(cx), said(&shown, 1));
}

/// A sample's real toast is said, and a move after it is said at once:
/// the toast, which stays, holds nothing back.
#[gpui::test]
fn a_commands_toast_is_said_and_a_move_after_it_at_once(cx: &mut TestAppContext) {
    let (window, cx) = open_actions_sample(cx);
    let first = view(&window, cx).rows[0].title.clone();
    // Enter runs the first note's "Open", whose toast names both.
    cx.simulate_keystrokes("enter");
    until_announced(cx, &format!("Open: {first}"));
    cx.simulate_keystrokes("down");
    let shown = view(&window, cx);
    assert_eq!(shown.selected, Some(1));
    assert_eq!(announcement(cx), said(&shown, 1));
}

/// The footer menu keeps the focus in its list and says its one item, its
/// place and the menu's size; Down has nowhere to go; opened again, it is
/// said again.
#[gpui::test]
fn the_footer_menu_says_its_item_and_again_when_it_opens_again(cx: &mut TestAppContext) {
    let (_window, cx) = open_samples(cx);
    let open_menu = |cx: &mut VisualTestContext| {
        let button = cx.debug_bounds("footer-menu").expect("the menu button");
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        cx.run_until_parked();
    };
    open_menu(cx);
    assert_focus_stays(cx, "Pane menu");
    assert_eq!(announcement(cx), "Settings, 1 of 1");
    let (label, position, size) = selected_of(cx, "MenuItem");
    assert_eq!((label.as_str(), position, size), ("Settings", 1, 1));
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_eq!(announcement(cx), "Settings, 1 of 1");
    assert_focus_stays(cx, "Pane menu");

    // Closed and opened again: the same text, cleared for a frame and set
    // again, so it is said again (the clearing frame is the announcer's
    // unit tests').
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    open_menu(cx);
    assert_eq!(announcement(cx), "Settings, 1 of 1");
}

#[gpui::test]
fn a_selection_pane_changes_itself_is_said_only_for_another_row(cx: &mut TestAppContext) {
    let (window, cx) = open_samples(cx);
    cx.simulate_keystrokes("down");
    let shown = view(&window, cx);
    let before = announcement(cx);
    assert_eq!(before, said(&shown, 1));

    // Pane draws the list again, the selection where it was: nothing.
    window.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.read_entity(&window, |window, _| window.launcher().select(1));
    window.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(announcement(cx), before);

    // Pane selects another row itself: it is said.
    cx.read_entity(&window, |window, _| window.launcher().select(2));
    window.update(cx, |_, cx| cx.notify());
    let shown = view(&window, cx);
    assert_eq!(announcement(cx), said(&shown, 2));
    assert_focus_stays(cx, "Search");
}
