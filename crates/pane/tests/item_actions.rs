//! Several actions per item (#137) in the launcher's window, with real key
//! events and the Rust actions sample: Enter, Ctrl+Enter and
//! Ctrl+Shift+Enter run an item's first three actions, an action's own
//! shortcut runs it from the list without the Actions panel, a held key's
//! repeats and a double click's second click run nothing more, the footer
//! names the primary action beside the Actions button, and Ctrl+K opens the
//! panel with the item's sections, keycaps and destructive style, filtered
//! by typing and closed by Escape or an outside click. The core's rules
//! (reservation, the other languages) are `pane-core`'s `item_actions.rs`.

use std::path::PathBuf;

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// How the Ctrl modifier is named on this system.
const CTRL: &str = if cfg!(target_os = "macos") {
    "Control"
} else {
    "Ctrl"
};

/// The launcher with the Rust actions sample as its one command, opened
/// with Enter, "Alpha note" selected.
fn opened(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
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
    };
    let launcher = Launcher::new(Runtime::start(), vec![command]);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("actions sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Actions sample"),
        "{:?}",
        view.status
    );
    assert_eq!(selected_title(&view), "Alpha note");
    (window, cx)
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

fn answered(text: &str) -> Status {
    Status::Result(text.into())
}

fn actions_open(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.actions_open())
}

/// Every accessibility node's properties, as GPUI reports them to
/// assistive technology.
fn accessible_nodes(cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    nodes.values().map(|node| node["aria"].clone()).collect()
}

/// The node with this role and label.
fn node<'a>(nodes: &'a [serde_json::Value], role: &str, label: &str) -> &'a serde_json::Value {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
        .unwrap_or_else(|| panic!("no {role} labelled {label:?} in {nodes:#?}"))
}

fn selector(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

/// Enter, Ctrl+Enter and Ctrl+Shift+Enter run the selected item's first,
/// second and third actions, whose toasts the footer shows; on an item
/// with one action the chords run nothing.
#[gpui::test]
fn enter_and_the_action_chords_run_the_first_three_actions(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(settle_shown(&window, cx), answered("Open: Alpha note"));
    cx.simulate_keystrokes("ctrl-enter");
    assert_eq!(settle_shown(&window, cx), answered("Copy: Alpha note"));
    cx.simulate_keystrokes("ctrl-shift-enter");
    assert_eq!(settle_shown(&window, cx), answered("Rename: Alpha note"));
    let view = settle(&window, cx);
    assert_eq!(selected_title(&view), "Alpha note", "the selection stays");
    assert!(!actions_open(&window, cx));

    cx.simulate_keystrokes("down");
    assert_eq!(selected_title(&settle(&window, cx)), "Beta note");
    cx.simulate_keystrokes("ctrl-enter");
    cx.simulate_keystrokes("ctrl-shift-enter");
    assert_eq!(
        settle_shown(&window, cx),
        answered("Rename: Alpha note"),
        "nothing ran"
    );
}

/// An action's own shortcut runs it from the list without opening the
/// panel, with its modifiers matched exactly; Pane's Ctrl+K stays Pane's.
#[gpui::test]
fn an_actions_shortcut_runs_it_from_the_list(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.simulate_keystrokes("ctrl-shift-c");
    assert_eq!(settle_shown(&window, cx), answered("Copy Link: Alpha note"));
    assert!(!actions_open(&window, cx), "the panel stays closed");

    cx.simulate_keystrokes("ctrl-x");
    assert_eq!(settle_shown(&window, cx), answered("Delete: Alpha note"));

    // Not the same modifiers: nothing runs.
    cx.simulate_keystrokes("ctrl-c");
    cx.simulate_keystrokes("ctrl-shift-x");
    assert_eq!(settle_shown(&window, cx), answered("Delete: Alpha note"));

    // "Open Menu" asks for Ctrl+K, which opens the panel instead.
    cx.simulate_keystrokes("ctrl-k");
    assert_eq!(settle_shown(&window, cx), answered("Delete: Alpha note"));
    if !cfg!(target_os = "macos") {
        assert!(actions_open(&window, cx), "Ctrl+K is Pane's");
    }
}

/// A held key's repeats run nothing more, and neither does a double
/// click's second click.
#[gpui::test]
fn a_held_key_or_a_second_click_runs_nothing_more(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(settle_shown(&window, cx), answered("Open: Alpha note"));

    for held in ["ctrl-enter", "ctrl-shift-c", "enter"] {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(held).unwrap(),
            is_held: true,
            prefer_character_input: false,
        });
        assert_eq!(
            settle_shown(&window, cx),
            answered("Open: Alpha note"),
            "a repeat of {held} runs nothing"
        );
    }

    cx.simulate_keystrokes("ctrl-enter");
    assert_eq!(settle_shown(&window, cx), answered("Copy: Alpha note"));
    let row = cx
        .debug_bounds("row-Alpha note")
        .expect("the row is drawn")
        .center();
    cx.simulate_event(gpui::MouseDownEvent {
        position: row,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: row,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
    });
    assert_eq!(
        settle_shown(&window, cx),
        answered("Copy: Alpha note"),
        "a second click runs nothing"
    );
}

/// The footer names the selected item's primary action, with Enter's
/// keys, beside the Actions button; an item without actions says it has
/// none, and Enter says it cannot be activated.
#[gpui::test]
fn the_footer_names_the_primary_action_and_offers_the_panel(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    let nodes = accessible_nodes(cx);
    let primary = node(&nodes, "Button", "Open");
    assert_eq!(primary["keyboard_shortcut"], "Enter");
    assert!(
        cx.debug_bounds("actions-button").is_some(),
        "Ctrl+K's button"
    );

    cx.simulate_keystrokes("down down");
    assert_eq!(selected_title(&settle(&window, cx)), "Gamma note");
    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "No actions");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(
        matches!(&view.status, Status::Error(why) if why.contains("cannot be activated")),
        "{:?}",
        view.status
    );

    // The panel opens on it, and says it has none.
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(actions_open(&window, cx));
    assert!(cx.debug_bounds("actions-empty").is_some());
}

/// Ctrl+K lists the item's actions in their labelled sections, each with
/// the keys that run it from the list, the destructive one drawn as such,
/// and the one whose shortcut Pane keeps without keys.
#[gpui::test]
fn the_panel_shows_sections_keycaps_and_the_destructive_style(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(actions_open(&window, cx));
    assert!(cx.debug_bounds("actions-dimmer").is_some());
    for section in ["Edit", "Share", "Danger"] {
        assert!(
            cx.debug_bounds(selector(&format!("action-group-{section}")))
                .is_some(),
            "the {section} section's label"
        );
    }

    let nodes = accessible_nodes(cx);
    let keys = |label: &str| node(&nodes, "MenuItem", label)["keyboard_shortcut"].clone();
    assert_eq!(keys("Open"), "Enter");
    assert_eq!(keys("Copy"), format!("{CTRL}+Enter"));
    assert_eq!(keys("Rename"), format!("{CTRL}+R"));
    assert_eq!(keys("Copy Link"), format!("{CTRL}+Shift+C"));
    assert_eq!(keys("Delete"), format!("{CTRL}+X"));
    assert_eq!(
        keys("Open Menu"),
        serde_json::Value::Null,
        "Pane's Ctrl+K is not the action's"
    );
    assert_eq!(
        node(&nodes, "MenuItem", "Delete")["description"],
        "Destructive"
    );
    assert_eq!(
        node(&nodes, "MenuItem", "Copy")["description"],
        serde_json::Value::Null
    );
}

/// Typing filters the actions by title, as one list without the sections;
/// Enter runs what is selected on the item the panel opened for, and closes
/// the panel.
#[gpui::test]
fn typing_filters_the_actions_and_enter_runs_one(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("link");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Idle,
        "the list's selection is untouched"
    );
    assert!(cx.debug_bounds("action-Copy Link").is_some());
    assert!(cx.debug_bounds("action-Open").is_none());
    assert!(
        cx.debug_bounds("action-group-Share").is_none(),
        "filtering drops the sections"
    );

    cx.simulate_keystrokes("enter");
    assert_eq!(settle_shown(&window, cx), answered("Copy Link: Alpha note"));
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(selected_title(&view), "Alpha note");
}

/// Escape closes only the panel, and an outside click closes it without
/// running the row it covered.
#[gpui::test]
fn escape_and_an_outside_click_close_only_the_panel(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(
        (&view.screen, &view.status),
        (&Screen::Command, &Status::Idle),
        "the command stays open"
    );

    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    let beta = cx
        .debug_bounds("row-Beta note")
        .expect("the row is drawn")
        .center();
    cx.simulate_click(beta, Modifiers::none());
    assert_eq!(settle_shown(&window, cx), Status::Idle, "nothing ran");
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(selected_title(&view), "Alpha note");
}
