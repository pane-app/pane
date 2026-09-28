//! Drives the native launcher window through GPUI's test platform: real key
//! and mouse events dispatch to the window, which runs real guest components.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, Runtime, Screen, Status};

/// A sample command: its component and the language it is written in.
struct Sample {
    component: &'static str,
    language: &'static str,
}

const RUST: Sample = Sample {
    component: "sample_rust",
    language: "Rust",
};
const JAVASCRIPT: Sample = Sample {
    component: "sample_js",
    language: "JavaScript",
};
const TYPESCRIPT: Sample = Sample {
    component: "sample_ts",
    language: "TypeScript",
};

fn command(title: &str, guest: &str) -> CommandRegistration {
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{guest}.wasm"));
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    CommandRegistration {
        id: guest.into(),
        title: title.into(),
        subtitle: None,
        component,
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    sample: &Sample,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let title = format!("{} sample", sample.language);
    open_with(cx, vec![command(&title, sample.component)])
}

fn open_with(
    cx: &mut TestAppContext,
    commands: Vec<CommandRegistration>,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher = Launcher::new(Runtime::start(), commands);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

/// Lets the window apply guest replies, which arrive from the runtime thread.
fn wait_for_answer(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> pane_core::LauncherView {
    // Generous: opening a JS command compiles a 4 MB component first.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if view.status != Status::Running {
            return view;
        }
        assert!(Instant::now() < deadline, "the guest did not answer");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens the sample's command with Enter and clicks the row whose debug
/// selector is `row` (`row-<title>`).
fn click_row(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    row: &'static str,
) -> pane_core::LauncherView {
    cx.simulate_keystrokes("enter");
    wait_for_answer(window, cx);
    let row = cx.debug_bounds(row).expect("row rendered");
    cx.simulate_click(row.center(), Modifiers::none());
    wait_for_answer(window, cx)
}

fn the_keyboard_opens_the_sample_and_runs_an_action(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);

    cx.simulate_keystrokes("enter");
    let view = wait_for_answer(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, format!("{} sample", sample.language));
    assert!(
        cx.debug_bounds("row-Say hello").is_some(),
        "guest rows are rendered"
    );

    cx.simulate_keystrokes("enter");
    let view = wait_for_answer(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(format!("Hello from the {} guest", sample.language))
    );
    assert!(
        cx.debug_bounds("status-result").is_some(),
        "the answer is rendered"
    );

    cx.simulate_keystrokes("escape");
    assert_eq!(wait_for_answer(&window, cx).screen, Screen::Root);
}

fn clicking_a_row_runs_its_action(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);

    let view = click_row(&window, cx, "row-Wait briefly");

    assert_eq!(view.selected, Some(1));
    assert_eq!(
        view.status,
        Status::Result(format!("Waited 50 ms inside the {} guest", sample.language))
    );
}

fn a_validation_error_is_rendered(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);

    let view = click_row(&window, cx, "row-Validate settings");

    assert_eq!(
        view.status,
        Status::Error(
            "The extension reported an error: Invalid settings: port must be between 1 and 65535"
                .into()
        )
    );
    assert!(
        cx.debug_bounds("status-error").is_some(),
        "the error is rendered"
    );
}

/// Declares one window test per check for each sample.
macro_rules! for_each_sample {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[gpui::test] fn $check(cx: &mut gpui::TestAppContext) { super::$check(cx, &super::RUST) })*
        }
        mod javascript {
            $(#[gpui::test] fn $check(cx: &mut gpui::TestAppContext) { super::$check(cx, &super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[gpui::test] fn $check(cx: &mut gpui::TestAppContext) { super::$check(cx, &super::TYPESCRIPT) })*
        }
    };
}

for_each_sample!(
    the_keyboard_opens_the_sample_and_runs_an_action,
    clicking_a_row_runs_its_action,
    a_validation_error_is_rendered,
);

#[gpui::test]
fn the_launcher_offers_the_rust_javascript_and_typescript_samples(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    let root = wait_for_answer(&window, cx);
    let titles: Vec<&str> = root.rows.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Rust sample", "JavaScript sample", "TypeScript sample"]
    );

    for (index, title) in titles.iter().enumerate() {
        cx.simulate_keystrokes("enter");
        let view = wait_for_answer(&window, cx);
        assert_eq!(
            (view.screen, view.title.as_str()),
            (Screen::Command, *title)
        );

        cx.simulate_keystrokes("escape");
        let view = wait_for_answer(&window, cx);
        assert_eq!(view.screen, Screen::Root);
        for _ in 0..=index {
            cx.simulate_keystrokes("down");
        }
    }
}

#[gpui::test]
fn arrow_keys_move_the_selection(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    cx.simulate_keystrokes("enter");
    wait_for_answer(&window, cx);

    cx.simulate_keystrokes("down");
    assert_eq!(wait_for_answer(&window, cx).selected, Some(1));
    cx.simulate_keystrokes("up");
    assert_eq!(wait_for_answer(&window, cx).selected, Some(0));
}

#[gpui::test]
fn a_rejected_extension_shows_an_error_and_navigation_keeps_working(cx: &mut TestAppContext) {
    let (window, cx) = open_with(
        cx,
        vec![
            command("Mixed", "mixed_p2"),
            command("Rust sample", "sample_rust"),
        ],
    );

    cx.simulate_keystrokes("enter");
    let view = wait_for_answer(&window, cx);
    assert_eq!(view.screen, Screen::Root);
    assert!(
        cx.debug_bounds("status-error").is_some(),
        "the error is rendered"
    );

    cx.simulate_keystrokes("down enter");
    assert_eq!(wait_for_answer(&window, cx).title, "Rust sample");
}

/// The window's accessibility tree, as (role, label, description) per node,
/// plus the label of the node assistive technology treats as focused.
fn accessibility_tree(
    cx: &mut VisualTestContext,
) -> (Vec<(String, String, String)>, Option<String>) {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let field = |node: &serde_json::Value, key: &str| {
        node["aria"][key].as_str().unwrap_or_default().to_owned()
    };
    let nodes = tree["nodes"].as_object().unwrap();
    let focused = ["active_descendant_focus", "gpui_focus"]
        .iter()
        .find_map(|key| tree[key].as_str())
        .map(|id| field(&nodes[id], "label"));
    let nodes = nodes
        .values()
        .map(|node| {
            (
                field(node, "role"),
                field(node, "label"),
                field(node, "description"),
            )
        })
        .collect();
    (nodes, focused)
}

fn has(nodes: &[(String, String, String)], role: &str, label: &str) -> bool {
    nodes.iter().any(|(r, l, _)| r == role && l == label)
}

#[gpui::test]
fn assistive_technology_sees_the_list_the_selection_and_the_result(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    cx.simulate_keystrokes("enter");
    wait_for_answer(&window, cx);

    let (nodes, focused) = accessibility_tree(cx);
    assert!(has(&nodes, "ListBox", "Rust sample"), "{nodes:?}");
    assert!(has(&nodes, "ListBoxOption", "Say hello"), "{nodes:?}");
    assert!(
        nodes
            .iter()
            .any(|(_, label, description)| label == "Wait briefly"
                && description == "Await a WASI 0.3 clock, then answer"),
        "{nodes:?}"
    );
    assert_eq!(focused.as_deref(), Some("Say hello"));

    cx.simulate_keystrokes("down enter");
    wait_for_answer(&window, cx);
    let (nodes, focused) = accessibility_tree(cx);
    assert_eq!(focused.as_deref(), Some("Wait briefly"));
    assert!(
        has(&nodes, "Status", "Waited 50 ms inside the Rust guest"),
        "{nodes:?}"
    );
}
