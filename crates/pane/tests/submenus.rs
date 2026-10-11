//! Submenus in the Actions panel (#140) in the launcher's window, with real
//! key events and the Rust actions sample's "Delta note": Enter opens a
//! submenu in place, named in the panel's header, with its sections, keycaps
//! and destructive style; typing filters inside it; Escape steps back to the
//! item's actions, giving back their filter, and closes the panel from
//! there; Ctrl+Enter opens the panel at the submenu its action opens; a
//! submenu the command gives when it opens shows its loading entry until it
//! answers, and its error entry when it fails, keeping the panel open; an
//! entry's shortcut works only while its submenu is shown; and a held
//! Enter's repeats or a double click's second click run no entry. The
//! core's rules (the other languages, late answers) are `pane-core`'s
//! `submenus.rs`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{
    CommandMatches, CommandRegistration, CommandWhen, Fault, Launcher, LauncherView, Runtime,
    Screen, Status, SubmenuState,
};

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

/// What the "Tag…" submenu's error entry says.
const TAGS_FAILED: &str = "The extension reported an error: The tags could not be loaded";

/// The launcher with the Rust actions sample as its one command, opened
/// with Enter, "Delta note" selected, and its runtime, to hold its answers
/// back.
fn opened(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext, Runtime) {
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
    let runtime = Runtime::start().unwrap();
    let launcher = Launcher::new(Ok(runtime.clone()), vec![command]);
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
    cx.simulate_keystrokes("down down down");
    assert_eq!(selected_title(&settle(&window, cx)), "Delta note");
    (window, cx, runtime)
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

/// The text in the Actions panel's search field.
fn filter_text(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let filter = cx
        .read_entity(window, |window, _| window.actions_filter())
        .expect("the panel is open");
    cx.read_entity(&filter, |filter, _| filter.as_str().to_owned())
}

fn selector(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

/// Whether the last frame drew the panel's entry labelled `label`.
fn entry_drawn(cx: &mut VisualTestContext, label: &str) -> bool {
    cx.debug_bounds(selector(&format!("action-{label}")))
        .is_some()
}

/// Runs the window until its last frame drew the panel's entry labelled
/// `label`, which an answer from the runtime thread brings.
fn until_drawn(cx: &mut VisualTestContext, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        if entry_drawn(cx, label) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the entry {label:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
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

/// Opens the panel and the submenu of the action `typed` finds, with Enter.
fn open_submenu(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, typed: &str) {
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(window, cx);
    assert!(actions_open(window, cx));
    cx.simulate_input(typed);
    settle(window, cx);
    cx.simulate_keystrokes("enter");
    settle(window, cx);
}

/// The panel marks an action that opens a submenu; Enter opens it in place,
/// named in the header, with its sections, keycaps and destructive style,
/// and Enter on an entry runs it and closes the panel.
#[gpui::test]
fn enter_opens_a_submenu_whose_entry_runs(cx: &mut TestAppContext) {
    let (window, cx, _runtime) = opened(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    let nodes = accessible_nodes(cx);
    assert_eq!(
        node(&nodes, "MenuItem", "Open With…")["description"],
        "Opens a submenu"
    );
    assert_eq!(
        node(&nodes, "MenuItem", "Open")["description"],
        serde_json::Value::Null
    );

    cx.simulate_keystrokes("down");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(actions_open(&window, cx), "the panel stays open");
    assert_eq!(view.status, Status::Idle, "opening it ran nothing");
    assert!(entry_drawn(cx, "Notepad"));
    assert!(!entry_drawn(cx, "Open"), "the item's actions give way");
    for section in ["Editors", "Other", "Danger"] {
        assert!(
            cx.debug_bounds(selector(&format!("action-group-{section}")))
                .is_some(),
            "the {section} section's label"
        );
    }
    let nodes = accessible_nodes(cx);
    node(&nodes, "Dialog", "Open With, actions for Delta note");
    let keys = |label: &str| node(&nodes, "MenuItem", label)["keyboard_shortcut"].clone();
    assert_eq!(keys("Notepad"), format!("{CTRL}+Shift+N"));
    assert_eq!(keys("WordPad"), serde_json::Value::Null);
    assert_eq!(keys("Forget Applications"), format!("{CTRL}+Shift+D"));
    assert_eq!(
        node(&nodes, "MenuItem", "Forget Applications")["description"],
        "Destructive"
    );

    cx.simulate_keystrokes("down enter");
    assert_eq!(
        settle_shown(&window, cx),
        answered("Open With WordPad: Delta note")
    );
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(selected_title(&view), "Delta note");
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert_eq!(launcher.submenu(), None, "closed with the panel");
}

/// Typing filters the submenu's entries, as one list without the sections;
/// Escape steps back to the item's actions with their filter, then closes
/// the panel.
#[gpui::test]
fn typing_filters_in_a_submenu_and_escape_steps_back(cx: &mut TestAppContext) {
    let (window, cx, _runtime) = opened(cx);
    open_submenu(&window, cx, "open with");
    assert_eq!(filter_text(&window, cx), "", "a submenu starts unfiltered");
    assert!(entry_drawn(cx, "Browser"));

    cx.simulate_input("pad");
    settle(&window, cx);
    assert!(entry_drawn(cx, "Notepad"));
    assert!(entry_drawn(cx, "WordPad"));
    assert!(!entry_drawn(cx, "Browser"));
    assert!(
        cx.debug_bounds("action-group-Editors").is_none(),
        "filtering drops the sections"
    );

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(actions_open(&window, cx), "Escape steps back one level");
    assert_eq!(filter_text(&window, cx), "open with");
    assert!(entry_drawn(cx, "Open With…"));
    assert!(!entry_drawn(cx, "Notepad"));
    let nodes = accessible_nodes(cx);
    node(&nodes, "Dialog", "Actions for Delta note");
    assert_eq!(view.status, Status::Idle);

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(
        !actions_open(&window, cx),
        "and closes the panel from there"
    );
    assert_eq!(
        (&view.screen, &view.status),
        (&Screen::Command, &Status::Idle)
    );
}

/// An outside click closes the panel from a submenu, running nothing.
#[gpui::test]
fn an_outside_click_closes_the_panel_from_a_submenu(cx: &mut TestAppContext) {
    let (window, cx, _runtime) = opened(cx);
    open_submenu(&window, cx, "open with");
    assert!(entry_drawn(cx, "Notepad"));
    let alpha = cx
        .debug_bounds("row-Alpha note")
        .expect("the row is drawn")
        .center();
    cx.simulate_click(alpha, Modifiers::none());
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(view.status, Status::Idle, "nothing ran");
    assert_eq!(selected_title(&view), "Delta note");
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert_eq!(launcher.submenu(), None);
}

/// Ctrl+Enter on an item whose secondary action opens a submenu opens the
/// panel at it; Escape then steps back to the item's actions.
#[gpui::test]
fn the_secondary_chord_opens_the_panel_at_its_submenu(cx: &mut TestAppContext) {
    let (window, cx, _runtime) = opened(cx);
    cx.simulate_keystrokes("ctrl-enter");
    let view = settle(&window, cx);
    assert!(actions_open(&window, cx));
    assert_eq!(view.status, Status::Idle);
    assert!(entry_drawn(cx, "Notepad"));

    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert!(actions_open(&window, cx));
    assert!(entry_drawn(cx, "Open With…"));
    assert!(entry_drawn(cx, "Open"));
}

/// A submenu the command gives when it opens shows its loading entry until
/// the command answers, then the answer; one whose opening fails shows its
/// error entry, and the panel stays open.
#[gpui::test]
fn a_lazy_submenu_shows_its_loading_and_error_entries(cx: &mut TestAppContext) {
    let (window, cx, runtime) = opened(cx);
    // The runtime holds the answer back until released.
    runtime.inject(Fault::Hang);
    open_submenu(&window, cx, "move");
    assert!(entry_drawn(cx, "Loading…"), "loading until it answers");
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert_eq!(
        launcher.submenu().map(|submenu| submenu.state),
        Some(SubmenuState::Loading)
    );
    let nodes = accessible_nodes(cx);
    node(&nodes, "Dialog", "Move to List, actions for Delta note");
    runtime.inject(Fault::Release);
    until_drawn(cx, "Inbox");
    assert!(!entry_drawn(cx, "Loading…"));
    assert!(
        cx.debug_bounds("action-group-Asked 1 time").is_some(),
        "asked once"
    );
    // Typing filters the answer.
    cx.simulate_input("later");
    settle(&window, cx);
    assert!(entry_drawn(cx, "Later"));
    assert!(!entry_drawn(cx, "Inbox"));

    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert!(!actions_open(&window, cx));

    open_submenu(&window, cx, "tag");
    until_drawn(cx, TAGS_FAILED);
    let view = settle(&window, cx);
    assert!(actions_open(&window, cx), "a failure keeps the panel open");
    assert_eq!(view.status, Status::Idle);
    // The error entry runs nothing.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(actions_open(&window, cx));
    assert_eq!(view.status, Status::Idle);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert!(entry_drawn(cx, "Tag…"), "back at the item's actions");
}

/// An entry's shortcut runs it only while its submenu is shown: neither
/// from the list nor from the item's actions.
#[gpui::test]
fn an_entrys_shortcut_works_only_in_its_submenu(cx: &mut TestAppContext) {
    let (window, cx, _runtime) = opened(cx);
    cx.simulate_keystrokes("ctrl-shift-b");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "not from the list");
    assert!(!actions_open(&window, cx));

    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-shift-b");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "not from the item's actions");
    assert!(actions_open(&window, cx));

    cx.simulate_input("open with");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-shift-b");
    assert_eq!(
        settle_shown(&window, cx),
        answered("Open With Browser: Delta note")
    );
    assert!(!actions_open(&window, cx));
}

/// A held Enter's repeats run nothing the submenu it opened lists, and
/// neither does a double click's second click.
#[gpui::test]
fn a_held_enter_or_a_second_click_runs_no_entry(cx: &mut TestAppContext) {
    let (window, cx, _runtime) = opened(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("open with");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert!(entry_drawn(cx, "Notepad"));
    for _ in 0..3 {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
            is_held: true,
            prefer_character_input: false,
        });
        assert_eq!(
            settle_shown(&window, cx),
            Status::Idle,
            "a repeat runs nothing"
        );
        assert!(actions_open(&window, cx));
    }

    // Back at the item's actions, a double click on "Open With…": the first
    // click opens the submenu, the second runs nothing in it.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let open_with = cx
        .debug_bounds("action-Open With…")
        .expect("the entry is drawn")
        .center();
    cx.simulate_click(open_with, Modifiers::none());
    settle(&window, cx);
    assert!(entry_drawn(cx, "Notepad"), "the first click opened it");
    cx.simulate_event(gpui::MouseDownEvent {
        position: open_with,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: open_with,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
    });
    assert_eq!(
        settle_shown(&window, cx),
        Status::Idle,
        "the second click runs nothing"
    );
    assert!(actions_open(&window, cx));
}
