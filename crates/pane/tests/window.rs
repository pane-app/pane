//! Drives the native launcher window through GPUI's test platform: real key
//! and mouse events dispatch to the window, which runs real guest components.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, Runtime, Screen, Status};

#[path = "../../pane-core/tests/support/platforms.rs"]
mod platforms;
#[path = "support/settle.rs"]
mod settle;

use settle::{settle, until};

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
        takes_query: false,
        search: false,
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
    open_launcher(cx, Launcher::new(Runtime::start(), commands))
}

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

/// Opens the sample's command with Enter and clicks the row whose debug
/// selector is `row` (`row-<title>`).
fn click_row(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    row: &'static str,
) -> pane_core::LauncherView {
    cx.simulate_keystrokes("enter");
    settle(window, cx);
    let row = cx.debug_bounds(row).expect("row rendered");
    cx.simulate_click(row.center(), Modifiers::none());
    settle(window, cx)
}

fn the_keyboard_opens_the_sample_and_runs_an_action(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, format!("{} sample", sample.language));
    assert!(
        cx.debug_bounds("row-Say hello").is_some(),
        "guest rows are rendered"
    );

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(format!("Hello from the {} guest", sample.language))
    );
    assert!(
        cx.debug_bounds("status-result").is_some(),
        "the answer is rendered"
    );

    cx.simulate_keystrokes("escape");
    assert!(
        matches!(settle(&window, cx).screen, Screen::Root { .. }),
        "{:?}",
        settle(&window, cx).screen
    );
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

/// Opens the sample's command and then its form ("Greet someone", the fifth
/// item) with the keyboard.
fn open_form(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("enter");
    settle(window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = settle(window, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    assert_eq!(view.title, "Greet someone");
}

fn the_keyboard_fills_in_and_submits_the_form(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);
    open_form(&window, cx);
    assert!(
        cx.debug_bounds("field-name").is_some(),
        "the form is rendered"
    );

    // The name field has focus; Tab moves to the greeting, Down chooses the
    // next greeting, and Enter submits.
    cx.simulate_input("Ada");
    cx.simulate_keystrokes("tab down enter");

    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(format!(
            "Good morning, Ada, from the {} guest",
            sample.language
        ))
    );
    assert!(
        cx.debug_bounds("status-result").is_some(),
        "the answer is rendered"
    );
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!((view.screen, view.selected), (Screen::Command, Some(4)));
}

fn a_rejected_field_shows_its_error_and_takes_focus(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);
    open_form(&window, cx);

    // Submit from the greeting with the name left empty.
    cx.simulate_keystrokes("tab enter");

    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error("Name: Enter a name".into()));
    assert!(
        cx.debug_bounds("field-error-name").is_some(),
        "the error is rendered next to the field"
    );
    let (nodes, focused) = accessibility_tree(cx);
    assert_eq!(focused.as_deref(), Some("Name"));
    assert!(
        nodes.contains(&("TextInput".into(), "Name".into(), "Enter a name".into())),
        "{nodes:?}"
    );

    // Focus is back on the name, so typing fixes it.
    cx.simulate_input("Grace");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result(format!("Hello, Grace, from the {} guest", sample.language))
    );
}

fn an_unavailable_action_is_listed_with_its_reason_and_others_still_run(
    cx: &mut TestAppContext,
    sample: &Sample,
) {
    let ((_, available), (_, unavailable), reason) = platforms::sample_items();
    let answer = format!("Ran the {available} in the {} guest", sample.language);
    let (window, cx) = open(cx, sample);
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    let index = |title: &str| view.rows.iter().position(|row| row.title == title).unwrap();

    // Select the unavailable item with the keyboard: it is still listed,
    // scrolled into view, and says why it cannot run here.
    for _ in 0..index(unavailable) {
        cx.simulate_keystrokes("down");
    }
    cx.run_until_parked();
    assert!(row_is_visible(cx, &format!("row-{unavailable}")));
    assert!(
        row_is_visible(cx, &format!("unavailable-reason-{unavailable}")),
        "the selected row shows its reason"
    );
    assert!(
        cx.debug_bounds(selector(&format!("unavailable-reason-{available}")))
            .is_none(),
        "the action for this system shows none"
    );
    let nodes = accessible_nodes(cx);
    let option = node(&nodes, "ListBoxOption", unavailable);
    let description = option["description"].as_str().unwrap_or_default();
    // The row is also marked disabled, which GPUI CE's debug tree does not
    // report, so only the description is checked.
    assert!(description.ends_with(&reason), "{option:#}");
    assert_eq!(focused_label(cx).as_deref(), Some(unavailable));

    // Enter explains instead of running the action.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.status),
        (Screen::Command, Status::Error(reason.clone()))
    );
    let (nodes, _) = accessibility_tree(cx);
    assert!(has(&nodes, "Status", &reason), "{nodes:?}");

    // The action declared for this system, and the others, still run.
    let delta = index(available) as isize - index(unavailable) as isize;
    let key = if delta > 0 { "down" } else { "up" };
    for _ in 0..delta.abs() {
        cx.simulate_keystrokes(key);
    }
    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).status, Status::Result(answer));
    for _ in 0..index(available) {
        cx.simulate_keystrokes("up");
    }
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result(format!("Hello from the {} guest", sample.language))
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
    the_keyboard_fills_in_and_submits_the_form,
    a_rejected_field_shows_its_error_and_takes_focus,
    an_unavailable_action_is_listed_with_its_reason_and_others_still_run,
    keys_change_the_color_the_view_shows,
    the_pointer_chooses_and_drags_across_swatches,
);

/// The label of the node assistive technology treats as focused.
fn focused_label(cx: &mut VisualTestContext) -> Option<String> {
    accessibility_tree(cx).1
}

/// The open form's value of field `id`.
fn field_value(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, id: &str) -> String {
    let view = cx.read_entity(window, |window, _| window.launcher().view());
    let form = view.form().expect("a form is open");
    let field = form.fields.iter().find(|field| field.id == id);
    field.expect("the field exists").value.clone()
}

#[gpui::test]
fn tab_and_shift_tab_visit_each_control_once_in_order(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));

    // Two full rounds each way: a control with two tab stops (such as the
    // text field and a wrapper tracking its focus) would appear twice in a
    // row. The greeting group reports its chosen option as focused, like a
    // list reports its selected row. The footer's menu button joins the
    // order after the form's controls.
    let mut forward = Vec::new();
    for _ in 0..8 {
        cx.simulate_keystrokes("tab");
        forward.push(focused_label(cx));
    }
    let mut backward = Vec::new();
    for _ in 0..8 {
        cx.simulate_keystrokes("shift-tab");
        backward.push(focused_label(cx));
    }

    let labels = |order: [&str; 8]| order.map(|label| Some(label.to_owned()));
    assert_eq!(
        forward,
        labels([
            "Hello",
            "Greet",
            "More actions",
            "Name",
            "Hello",
            "Greet",
            "More actions",
            "Name"
        ])
    );
    assert_eq!(
        backward,
        labels([
            "More actions",
            "Greet",
            "Hello",
            "Name",
            "More actions",
            "Greet",
            "Hello",
            "Name"
        ])
    );
}

#[gpui::test]
fn editing_keys_change_the_text_field(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);

    cx.simulate_input("Ad");
    cx.simulate_keystrokes("left");
    cx.simulate_input("x");
    assert_eq!(field_value(&window, cx, "name"), "Axd");
    cx.simulate_keystrokes("backspace right");
    cx.simulate_input("a");
    assert_eq!(field_value(&window, cx, "name"), "Ada");

    // Space is text in the field, not a key for the form.
    cx.simulate_keystrokes("space");
    assert_eq!(field_value(&window, cx, "name"), "Ada ");
}

/// GPUI CE's test platform offers no public way to reach the window's
/// platform input handler, so composition is driven on the name field's
/// editing state, which the window's input handler forwards to. The test
/// checks that this state belongs to the focused field; typing through the
/// window's input handler is covered by the other form tests.
#[gpui::test]
fn input_method_composition_commits_into_the_text_field(cx: &mut TestAppContext) {
    use gpui::{EntityInputHandler, Focusable};

    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    let input = cx
        .read_entity(&window, |window, _| window.text_field("name"))
        .expect("the name field has an editing state");
    let focused = cx.update(|window, cx| input.focus_handle(cx).is_focused(window));
    assert!(focused, "the name field has keyboard focus");

    // What a platform input method does: mark composing text, then replace
    // it with the committed text.
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        })
    });
    assert_eq!(field_value(&window, cx, "name"), "にほ");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            assert_eq!(input.marked_text_range(window, cx), Some(0..2));
            input.replace_text_in_range(None, "日本", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        })
    });
    assert_eq!(field_value(&window, cx, "name"), "日本");

    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Hello, 日本, from the Rust guest".into())
    );
}

#[gpui::test]
fn clicking_a_choice_and_the_submit_button_submits_the_form(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    cx.simulate_input("Ada");

    let welcome = cx
        .debug_bounds("choice-greeting-welcome")
        .expect("choice rendered");
    cx.simulate_click(welcome.center(), Modifiers::none());
    assert_eq!(field_value(&window, cx, "greeting"), "welcome");
    let submit = cx.debug_bounds("submit").expect("submit button rendered");
    cx.simulate_click(submit.center(), Modifiers::none());

    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Welcome, Ada, from the Rust guest".into())
    );
}

#[gpui::test]
fn the_focused_submit_button_submits_with_space(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    cx.simulate_input("Ada");

    cx.simulate_keystrokes("tab tab space");

    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Hello, Ada, from the Rust guest".into())
    );
}

/// Every accessibility node's properties (`role`, `label`, `value`,
/// `toggled`, ...), as GPUI reports them to assistive technology.
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

#[gpui::test]
fn assistive_technology_sees_the_forms_labelled_controls_and_values(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    cx.simulate_input("Ada");

    let nodes = accessible_nodes(cx);
    node(&nodes, "Form", "Greet someone");
    let name = node(&nodes, "TextInput", "Name");
    assert_eq!(
        (&name["value"], &name["placeholder"]),
        (&"Ada".into(), &"Ada Lovelace".into())
    );
    node(&nodes, "RadioGroup", "Greeting");
    assert_eq!(node(&nodes, "RadioButton", "Hello")["toggled"], "True");
    assert_eq!(
        node(&nodes, "RadioButton", "Good morning")["toggled"],
        "False"
    );
    node(&nodes, "Button", "Greet");
}

#[gpui::test]
fn the_launcher_offers_the_rust_javascript_and_typescript_samples(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    let root = settle(&window, cx);
    let titles: Vec<&str> = root.rows.iter().map(|row| row.title.as_str()).collect();
    // Pane's own Settings row is listed last, whatever is installed (its
    // window is the Settings milestone's work, covered in tests/settings).
    let samples = ["Rust sample", "JavaScript sample", "TypeScript sample"];
    assert_eq!(titles, [samples.as_slice(), &["Settings…"]].concat());

    for (index, title) in samples.iter().enumerate() {
        cx.simulate_keystrokes("enter");
        let view = settle(&window, cx);
        assert_eq!(
            (view.screen, view.title.as_str()),
            (Screen::Command, *title)
        );

        cx.simulate_keystrokes("escape");
        let view = settle(&window, cx);
        assert!(
            matches!(view.screen, Screen::Root { .. }),
            "{:?}",
            view.screen
        );
        for _ in 0..=index {
            cx.simulate_keystrokes("down");
        }
    }
}

#[gpui::test]
fn arrow_keys_move_the_selection(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    cx.simulate_keystrokes("down");
    assert_eq!(settle(&window, cx).selected, Some(1));
    cx.simulate_keystrokes("up");
    assert_eq!(settle(&window, cx).selected, Some(0));
}

/// A debug selector as GPUI's test context takes it, from a name a test
/// builds.
fn selector(name: &str) -> &'static str {
    name.to_owned().leak()
}

/// Whether the element with debug selector `element` (such as `row-<title>`)
/// lies wholly inside the list.
fn row_is_visible(cx: &mut VisualTestContext, element: &str) -> bool {
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let element = cx.debug_bounds(selector(element)).expect("it is rendered");
    element.top() >= list.top() && element.bottom() <= list.bottom()
}

#[gpui::test]
fn the_list_scrolls_to_keep_the_selected_row_visible(cx: &mut TestAppContext) {
    const TITLES: [&str; 12] = [
        "Row 1", "Row 2", "Row 3", "Row 4", "Row 5", "Row 6", "Row 7", "Row 8", "Row 9", "Row 10",
        "Row 11", "Row 12",
    ];
    let commands = TITLES
        .iter()
        .map(|title| command(title, "sample_rust"))
        .collect();
    let (window, cx) = open_with(cx, commands);
    // The size of Pane's window: fewer than half of the rows fit.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    cx.run_until_parked();
    assert!(row_is_visible(cx, "row-Row 1"));
    assert!(!row_is_visible(cx, "row-Row 12"), "the list overflows");

    for _ in 1..TITLES.len() {
        cx.simulate_keystrokes("down");
    }
    cx.run_until_parked();
    assert_eq!(settle(&window, cx).selected, Some(11));
    assert!(
        row_is_visible(cx, "row-Row 12"),
        "the last row is scrolled into view"
    );
    assert!(!row_is_visible(cx, "row-Row 1"));

    for _ in 1..TITLES.len() {
        cx.simulate_keystrokes("up");
    }
    cx.run_until_parked();
    assert!(row_is_visible(cx, "row-Row 1"), "and back to the first");
}

/// Twelve root commands, "Row 1" to "Row 12", all the Rust sample.
fn twelve_rows() -> Vec<CommandRegistration> {
    (1..=12)
        .map(|n| command(&format!("Row {n}"), "sample_rust"))
        .collect()
}

#[gpui::test]
fn the_selected_row_stays_visible_when_the_window_shrinks(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, twelve_rows());
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    for _ in 0..3 {
        cx.simulate_keystrokes("down");
    }
    cx.run_until_parked();
    assert_eq!(settle(&window, cx).selected, Some(3));
    assert!(row_is_visible(cx, "row-Row 4"));

    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(200.)));
    // The window asks for one more frame once the list's new size is laid
    // out; the test platform delivers no frames by itself, so draw it.
    redraw(&window, cx);

    assert!(
        row_is_visible(cx, "row-Row 4"),
        "the selected row is scrolled back into the smaller list"
    );
}

/// Turns the mouse wheel over the list by `pixels` (negative scrolls down).
fn wheel(cx: &mut VisualTestContext, pixels: f32) {
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(pixels))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
}

/// Redraws the window after the launcher changed outside it.
fn redraw(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    window.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
}

#[gpui::test]
fn the_mouse_wheel_scrolls_away_until_the_rows_reload(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let folder = source.path().join("hello");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("pane.json"),
        r#"{ "manifestVersion": 1, "title": "Hello", "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }] }"#,
    )
    .unwrap();
    std::fs::copy(
        command("hello", "sample_rust").component,
        folder.join("hello.wasm"),
    )
    .unwrap();
    let launcher = Launcher::with_packages(
        Runtime::start(),
        twelve_rows(),
        data.path().join("extensions"),
    );
    let (window, cx) = open_launcher(cx, launcher.clone());
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    // An install that finishes after the user has moved on: it reloads root
    // search in the background and keeps the selected row, the first.
    let install = launcher.install_package(&folder);
    cx.foreground_executor()
        .block_on(launcher.activate_selected());
    launcher.back();
    redraw(&window, cx);
    assert!(row_is_visible(cx, "row-Row 1"));

    wheel(cx, -400.);
    redraw(&window, cx);
    assert!(
        !row_is_visible(cx, "row-Row 1"),
        "redrawing does not undo the wheel"
    );

    cx.foreground_executor().block_on(install);
    redraw(&window, cx);
    let view = launcher.view();
    assert_eq!((view.query(), view.selected), (Some(""), Some(0)));
    assert!(view.rows.iter().any(|row| row.title == "Say hello"));
    assert!(
        row_is_visible(cx, "row-Row 1"),
        "the reloaded list shows the selected row again"
    );
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
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert!(
        cx.debug_bounds("status-error").is_some(),
        "the error is rendered"
    );

    cx.simulate_keystrokes("down enter");
    assert_eq!(settle(&window, cx).title, "Rust sample");
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
    settle(&window, cx);

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
    settle(&window, cx);
    let (nodes, focused) = accessibility_tree(cx);
    assert_eq!(focused.as_deref(), Some("Wait briefly"));
    assert!(
        has(&nodes, "Status", "Waited 50 ms inside the Rust guest"),
        "{nodes:?}"
    );
}

/// Opens the sample's command and then its color picker ("Choose a color",
/// the sixth item) with the keyboard.
fn open_color(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("enter");
    settle(window, cx);
    cx.simulate_keystrokes("down down down down down enter");
    let view = settle(window, cx);
    assert!(
        matches!(view.screen, Screen::CustomView(_)),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Choose a color");
}

/// Waits until the open view shows `expected` as its value, which it does
/// once the guest's answer to the last event has arrived.
fn wait_for_color(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, expected: &str) {
    until(window, cx, |view| {
        view.custom_view().map(|view| view.frame.value.as_str()) == Some(expected)
    });
}

/// The color picker's accessibility node.
fn color_node(cx: &mut VisualTestContext) -> serde_json::Value {
    node(&accessible_nodes(cx), "ColorWell", "Color").clone()
}

fn keys_change_the_color_the_view_shows(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);
    open_color(&window, cx);

    // The view has keyboard focus, and assistive technology reads its value.
    assert_eq!(focused_label(cx).as_deref(), Some("Color"));
    assert_eq!(color_node(cx)["value"], "Blue, #1E88E5");
    // The drawing itself adds no nodes: the view is one control.
    let roles: Vec<String> = accessible_nodes(cx)
        .iter()
        .map(|node| node["role"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        roles.iter().filter(|role| *role == "ColorWell").count(),
        1,
        "{roles:?}"
    );
    assert_eq!(
        roles.len(),
        4,
        "the view, the footer's menu button, the status line and the window: {roles:?}"
    );
    assert!(
        cx.debug_bounds("custom-view").is_some(),
        "the view is drawn"
    );

    cx.simulate_keystrokes("right");
    wait_for_color(&window, cx, "Purple, #8E24AA");
    cx.simulate_keystrokes("down");
    wait_for_color(&window, cx, "Dark purple, #4A148C");
    cx.simulate_keystrokes("home");
    wait_for_color(&window, cx, "Dark red, #B71C1C");
    assert_eq!(color_node(cx)["value"], "Dark red, #B71C1C");

    // The view and the footer's menu button are the screen's tab stops,
    // and Tab visits the button and comes back; Escape closes the view.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("More actions"));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Color"));
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!((view.screen, view.selected), (Screen::Command, Some(5)));
    assert_eq!(focused_label(cx).as_deref(), Some("Choose a color"));
}

fn the_pointer_chooses_and_drags_across_swatches(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);
    open_color(&window, cx);
    let origin = cx
        .debug_bounds("custom-view")
        .expect("the view is drawn")
        .origin;
    let at = |x: f32, y: f32| origin + gpui::point(px(x), px(y));

    cx.simulate_mouse_down(at(10.0, 10.0), MouseButton::Left, Modifiers::none());
    wait_for_color(&window, cx, "Light red, #EF9A9A");
    cx.simulate_mouse_move(at(80.0, 80.0), MouseButton::Left, Modifiers::none());
    wait_for_color(&window, cx, "Dark yellow, #F57F17");
    // Dragging on past the view's edge chooses the nearest swatch.
    cx.simulate_mouse_move(at(120.0, -40.0), MouseButton::Left, Modifiers::none());
    wait_for_color(&window, cx, "Light green, #A5D6A7");
    cx.simulate_mouse_up(at(120.0, -40.0), MouseButton::Left, Modifiers::none());

    // After the release, moving chooses nothing, and a click chooses again.
    cx.simulate_mouse_move(at(260.0, 80.0), None, Modifiers::none());
    cx.simulate_click(at(260.0, 80.0), Modifiers::none());
    wait_for_color(&window, cx, "Dark pink, #880E4F");
    assert_eq!(focused_label(cx).as_deref(), Some("Color"));
}

/// Opens the faulty fixture's counting view, whose value is the number of
/// events it handled, and returns where its drawing area starts.
fn open_counter(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> gpui::Point<gpui::Pixels> {
    cx.simulate_keystrokes("enter");
    settle(window, cx);
    cx.simulate_keystrokes("down down down down down enter");
    let view = settle(window, cx);
    assert!(
        matches!(view.screen, Screen::CustomView(_)),
        "{:?}",
        view.screen
    );
    wait_for_color(window, cx, "0 events");
    cx.debug_bounds("custom-view")
        .expect("the view is drawn")
        .origin
}

fn pointer_held(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.launcher().pointer_held())
}

#[gpui::test]
fn a_press_on_the_views_border_is_not_sent_to_the_view(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, vec![command("Faulty", "faulty")]);
    let origin = open_counter(&window, cx);
    // Inside the focus ring's padding, left of the drawing area.
    let border = origin + gpui::point(px(-3.0), px(5.0));

    cx.simulate_mouse_down(border, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(border, MouseButton::Left, Modifiers::none());
    // Had the press or release been sent, the view would count them first.
    cx.simulate_keystrokes("up");

    wait_for_color(&window, cx, "1 events");
}

#[gpui::test]
fn a_release_outside_the_window_ends_the_drag(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, vec![command("Faulty", "faulty")]);
    let origin = open_counter(&window, cx);
    let at = |x: f32| origin + gpui::point(px(x), px(10.0));
    cx.simulate_mouse_down(at(10.0), MouseButton::Left, Modifiers::none());
    wait_for_color(&window, cx, "1 events");

    // The button went up outside the window, which reported no release:
    // the next move arrives without it.
    cx.simulate_mouse_move(at(20.0), None, Modifiers::none());

    wait_for_color(&window, cx, "2 events");
    assert!(!pointer_held(&window, cx));
    cx.simulate_mouse_move(at(30.0), None, Modifiers::none());
    cx.simulate_keystrokes("up");
    wait_for_color(&window, cx, "3 events");
}

#[gpui::test]
fn leaving_the_window_during_a_drag_ends_it(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, vec![command("Faulty", "faulty")]);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let origin = open_counter(&window, cx);
    let at = origin + gpui::point(px(10.0), px(10.0));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    wait_for_color(&window, cx, "1 events");

    cx.deactivate_window();

    wait_for_color(&window, cx, "2 events");
    assert!(!pointer_held(&window, cx));
}

/// The titles of the rows on screen.
fn row_titles(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    let view = cx.read_entity(window, |window, _| window.launcher().view());
    view.rows.into_iter().map(|row| row.title).collect()
}

/// Whether root search's query field has keyboard focus.
fn query_has_focus(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    use gpui::Focusable;
    let input = cx.read_entity(window, |window, _| window.query_field());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn typing_in_root_search_narrows_the_results_and_enter_opens_the_best_match(
    cx: &mut TestAppContext,
) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    assert!(
        query_has_focus(&window, cx),
        "root search opens ready to type"
    );

    cx.simulate_input("typescr");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("typescr"));
    assert_eq!(row_titles(&window, cx), ["TypeScript sample"]);
    assert!(cx.debug_bounds("row-TypeScript sample").is_some());
    assert!(cx.debug_bounds("row-Rust sample").is_none());

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "TypeScript sample")
    );
}

#[gpui::test]
fn arrow_keys_move_through_the_matches_while_the_query_keeps_focus(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    cx.simulate_input("script");
    assert_eq!(
        row_titles(&window, cx),
        ["JavaScript sample", "TypeScript sample"]
    );

    cx.simulate_keystrokes("down");
    assert_eq!(settle(&window, cx).selected, Some(1));
    cx.simulate_keystrokes("up");
    assert_eq!(settle(&window, cx).selected, Some(0));
    assert!(query_has_focus(&window, cx));
    // Editing keys still edit the query.
    cx.simulate_keystrokes("backspace backspace backspace");
    assert_eq!(settle(&window, cx).query(), Some("scr"));

    cx.simulate_keystrokes("down enter");
    assert_eq!(settle(&window, cx).title, "TypeScript sample");
}

#[gpui::test]
fn a_query_that_matches_nothing_says_so_and_escape_clears_it(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    cx.simulate_input("zzz");
    let view = settle(&window, cx);
    assert!(view.rows.is_empty());
    assert!(
        cx.debug_bounds("no-results").is_some(),
        "the empty state is shown"
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).status, Status::Idle);
    assert!(cx.debug_bounds("status-idle").is_some(), "nothing failed");

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""));
    assert_eq!(row_titles(&window, cx).len(), 4, "the samples and Settings");
    let text = cx.read_entity(&window, |window, cx| {
        window.query_field().read(cx).as_str().to_owned()
    });
    assert_eq!(text, "", "the field shows the cleared query");
}

#[gpui::test]
fn coming_back_to_root_search_starts_an_empty_search_with_focus(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    cx.simulate_input("rust");
    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
    assert!(!query_has_focus(&window, cx));
    // Typing in a command does not search root.
    cx.simulate_input("x");

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""));
    assert!(query_has_focus(&window, cx));
    cx.simulate_input("java");
    assert_eq!(row_titles(&window, cx), ["JavaScript sample"]);
}

/// Composition is driven on the query field's editing state, as in
/// `input_method_composition_commits_into_the_text_field`, after checking
/// that it is the focused one.
#[gpui::test]
fn input_method_composition_searches_root(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;

    let (window, cx) = open_with(
        cx,
        vec![
            command("日本語の辞書", "sample_rust"),
            command("English dictionary", "sample_js"),
        ],
    );
    assert!(query_has_focus(&window, cx));
    let input = cx.read_entity(&window, |window, _| window.query_field());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        })
    });
    assert_eq!(
        settle(&window, cx).query(),
        Some("にほ"),
        "composing text is searched as it is typed"
    );
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "日本", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        })
    });
    assert_eq!(row_titles(&window, cx), ["日本語の辞書"]);

    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).title, "Rust sample");
}

#[gpui::test]
fn assistive_technology_sees_the_search_field_and_the_selected_result(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    cx.simulate_input("script");
    settle(&window, cx);

    let nodes = accessible_nodes(cx);
    let search = node(&nodes, "EditableComboBox", "Search");
    assert_eq!(
        (&search["value"], &search["placeholder"]),
        (&"script".into(), &"Search commands".into())
    );
    node(&nodes, "ListBox", "Results");
    node(&nodes, "ListBoxOption", "TypeScript sample");
    assert_eq!(focused_label(cx).as_deref(), Some("JavaScript sample"));
    cx.simulate_keystrokes("down");
    assert_eq!(focused_label(cx).as_deref(), Some("TypeScript sample"));

    // With nothing selected, the search field itself is focused.
    cx.simulate_input("zzz");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
}

/// A launcher with the calculator package from `cargo xtask guests`
/// installed in `data`.
fn with_calculator(cx: &mut TestAppContext, data: &std::path::Path) -> Launcher {
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/calculator");
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"));
    // The install's guest check answers from the runtime thread.
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
    launcher
}

/// Lets the window apply answers computed from the query until the rows
/// are `expected`.
fn wait_for_rows(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, expected: &[&str]) {
    until(window, cx, |view| {
        view.rows
            .iter()
            .map(|row| row.title.as_str())
            .eq(expected.iter().copied())
    });
}

#[gpui::test]
fn typing_an_expression_shows_its_answer_and_enter_copies_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let launcher = with_calculator(cx, data.path());
    let (window, cx) = open_launcher(cx, launcher);

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);
    assert!(
        cx.debug_bounds("row-42").is_some(),
        "the answer is rendered"
    );
    assert!(query_has_focus(&window, cx), "typing goes on in the field");
    // Typing on: the answer follows the query.
    cx.simulate_input("+1");
    wait_for_rows(&window, cx, &["43"]);

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Copied 43 to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("43".into())
    );
    assert_eq!(view.query(), Some("6*7+1"), "root search stays as it was");

    // An incomplete expression has no answer and nothing failed.
    cx.simulate_input("*");
    wait_for_rows(&window, cx, &[]);
    assert!(cx.debug_bounds("no-results").is_some());
}

/// A system with two applications, recording which one Pane opens.
#[derive(Default)]
struct TwoApplications {
    opened: std::sync::Mutex<Vec<String>>,
}

impl pane_core::applications::Applications for TwoApplications {
    fn installed(&self) -> Result<Vec<pane_core::applications::Application>, String> {
        Ok(["Firefox", "Files"]
            .map(|name| pane_core::applications::Application {
                id: format!("/apps/{name}.desktop"),
                name: name.into(),
                location: "/apps".into(),
            })
            .into())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(id.into());
        Ok(())
    }
}

#[gpui::test]
fn typing_an_applications_name_shows_it_and_enter_opens_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = std::sync::Arc::new(TwoApplications::default());
    let runtime = Runtime::start().unwrap();
    runtime.set_applications(system.clone());
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/applications");
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"));
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.back();
    let (window, cx) = open_launcher(cx, launcher);

    cx.simulate_input("fire");
    wait_for_rows(&window, cx, &["Firefox"]);
    assert!(
        cx.debug_bounds("row-Firefox").is_some(),
        "the application is rendered"
    );
    let (nodes, focused) = accessibility_tree(cx);
    assert!(has(&nodes, "ListBoxOption", "Firefox"), "{nodes:?}");
    assert_eq!(focused.as_deref(), Some("Firefox"), "the selected result");

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Opened Firefox".into()));
    assert_eq!(*system.opened.lock().unwrap(), ["/apps/Firefox.desktop"]);
    assert!(query_has_focus(&window, cx), "typing goes on in the field");
}

/// Records the links the window's launcher is asked to open, so that no
/// browser opens.
#[derive(Default)]
struct RecordedLinks(std::sync::Mutex<Vec<String>>);

impl pane_core::LinkOpener for RecordedLinks {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0.lock().unwrap().push(url.into());
        Ok(())
    }
}

#[gpui::test]
fn a_quicklink_created_in_its_form_is_found_and_opened_from_root_search(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/quicklinks");
    let links = std::sync::Arc::new(RecordedLinks::default());
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_link_opener(links.clone());
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.back();
    let (window, cx) = open_launcher(cx, launcher);

    // Quicklinks is the first row; its first item creates a quicklink.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.title, "Create quicklink");
    // An address without a scheme is rejected on its field.
    cx.simulate_input("Pane issues");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("github.com/hoangvu12/pane/issues");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Error("URL: Enter a web address starting with http:// or https://".into())
    );
    // To the start of the field: text fields on macOS have no binding for
    // Home (Mac keyboards have none), only Command-Left.
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-left"
    } else {
        "home"
    });
    cx.simulate_input("https://");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Saved quicklink “Pane issues”".into())
    );

    cx.simulate_keystrokes("escape escape");
    cx.simulate_input("pane iss");
    wait_for_rows(&window, cx, &["Pane issues"]);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Opened https://github.com/hoangvu12/pane/issues".into())
    );
    assert_eq!(
        *links.0.lock().unwrap(),
        ["https://github.com/hoangvu12/pane/issues"]
    );
}

#[gpui::test]
fn a_long_error_wraps_grows_and_scrolls_inside_the_footer(cx: &mut TestAppContext) {
    // The kind of message a picker or download failure reports: long
    // enough to wrap past the footer's 50px floor and past its cap.
    let detail = "the operation could not be completed because the target \
                  system refused the connection and every retry failed, so \
                  nothing was installed and the previous state was kept";
    let message =
        format!("Could not open a folder picker: {detail}. {detail}. {detail}. {detail}.");
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    launcher.show_error(message.clone());
    let (window, cx) = open_launcher(cx, launcher);

    // A narrow window: the message wraps within the footer's width — not
    // one line clipped at the window's right edge — the footer grows past
    // its 50px floor, and the message is taller than the capped strip, so
    // the overflow must scroll rather than disappear.
    cx.simulate_resize(gpui::size(px(380.), px(420.)));
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error(message.clone()));
    let footer = cx
        .debug_bounds("status-error")
        .expect("the footer is rendered");
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        text.right() <= footer.right(),
        "the message wraps within the footer, not past its right edge"
    );
    assert!(
        text.size.height > px(60.),
        "the message wrapped to several lines: {:?}",
        text.size.height
    );
    assert!(
        footer.size.height > px(50.),
        "the footer grew past its 50px floor: {:?}",
        footer.size.height
    );
    assert!(
        footer.size.height <= px(147.5),
        "the footer is capped at 35% of the panel: {:?}",
        footer.size.height
    );
    assert!(
        text.size.height > footer.size.height,
        "the overflow is scrollable, not cut"
    );

    // A short window: the cap follows the panel down (35% of 200px), so
    // the list keeps most of the window, and the overflow still scrolls.
    cx.simulate_resize(gpui::size(px(640.), px(200.)));
    settle(&window, cx);
    let footer = cx
        .debug_bounds("status-error")
        .expect("the footer is rendered");
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        footer.size.height <= px(70.5),
        "the cap follows the panel height: {:?}",
        footer.size.height
    );
    assert!(text.size.height > footer.size.height);

    // The wheel over the footer scrolls the message itself, the same
    // event the list's wheel test dispatches (negative scrolls down): the
    // message's painted position moves up.
    let before = text.top();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: footer.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-80.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    redraw(&window, cx);
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        text.top() < before,
        "the message scrolled up within the footer"
    );

    // Scrolled far down, the wheel reaches the end: the last line lands
    // inside the strip.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: footer.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-4000.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    redraw(&window, cx);
    let footer = cx
        .debug_bounds("status-error")
        .expect("the footer is rendered");
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        text.bottom() <= footer.bottom() + px(1.),
        "the last line can be scrolled into view"
    );

    // And back up: the first line is reachable again.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: footer.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(4000.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    redraw(&window, cx);
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        text.top() >= before - px(1.),
        "the first line scrolls back into view"
    );
}

/// The idle footer's selected action: its button, right-aligned in the
/// strip, with the Enter keycap beside the label; the button and Enter run
/// the same action; and a status — running, a result, an error — owns the
/// strip while it shows, in place of the action.
#[gpui::test]
fn the_footer_button_runs_the_selected_action_like_enter(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    // A narrow window: the strip, the button and its label all have to fit.
    cx.simulate_resize(gpui::size(px(380.), px(420.)));
    settle(&window, cx);

    // The button sits in the strip's right half — the far left stays free
    // for the app menu a later slice delivers there — and the keycap sits
    // inside the button, at its right end.
    let footer = cx
        .debug_bounds("status-idle")
        .expect("the idle strip is rendered");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    assert!(
        button.left() > footer.left() + footer.size.width / 2.,
        "the button is right-aligned: {button:?} in {footer:?}"
    );
    assert!(button.right() <= footer.right(), "inside the strip");
    assert!(
        button.top() >= footer.top() && button.bottom() <= footer.bottom(),
        "the button is centered in the strip: {button:?} in {footer:?}"
    );
    let (above, below) = (
        button.top() - footer.top(),
        footer.bottom() - button.bottom(),
    );
    assert!(
        (above - below).abs() <= px(1.),
        "the button is centered in the strip: {above:?} above, {below:?} below"
    );
    let keycap = cx.debug_bounds("keycap").expect("the keycap is rendered");
    assert!(
        keycap.left() > button.left() && keycap.right() <= button.right(),
        "the keycap sits inside the button: {keycap:?} in {button:?}"
    );

    // The definition supplies the label from the action's identity — a
    // selected extension command in root search opens it — and the keycap
    // names its key, on the button and to assistive technology.
    let nodes = accessible_nodes(cx);
    let action = node(&nodes, "Button", "Open command");
    assert_eq!(action["keyboard_shortcut"].as_str(), Some("Enter"));
    node(&nodes, "Image", "Enter");

    // Clicking the button opens the selected command, as Enter does.
    cx.simulate_click(button.center(), Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );

    // The command's own screen names what activating its selected item
    // does, and clicking the button there runs it, as Enter does.
    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Run item");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    cx.simulate_click(button.center(), Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Hello from the Rust guest".into())
    );
    assert!(
        cx.debug_bounds("status-result").is_some(),
        "the answer is rendered"
    );
    assert!(
        cx.debug_bounds("primary-action").is_none(),
        "a status owns the strip while it shows, not the idle action"
    );

    // Back at root search the launcher is idle again, and the strip is the
    // action again.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Open command");
}

/// The footer's Submit button submits the form, as Enter does, beside the
/// form's own submit control.
#[gpui::test]
fn the_footer_button_submits_the_form_like_enter(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    cx.simulate_input("Ada");

    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Submit");
    node(&nodes, "Button", "Greet");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    cx.simulate_click(button.center(), Modifiers::none());

    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Hello, Ada, from the Rust guest".into())
    );
}

/// With nothing selected, the button stays — named for the action there
/// would be — but a click dispatches nothing.
#[gpui::test]
fn the_footer_button_cannot_run_an_action_with_nothing_selected(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, pane::sample_commands());
    cx.simulate_input("zzz");
    let view = settle(&window, cx);
    assert_eq!(view.selected, None);

    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Open command");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    cx.simulate_click(button.center(), Modifiers::none());

    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "nothing was dispatched");
    assert_eq!(view.query(), Some("zzz"));
    assert!(
        cx.debug_bounds("status-idle").is_some(),
        "the idle strip is unchanged"
    );
}

/// An unavailable result keeps its button — disabled, named for what it
/// cannot do — and its explanation where it always was, on its row: a
/// click dispatches nothing, while Enter still explains, as it always has.
#[gpui::test]
fn the_footer_button_does_not_dispatch_an_unavailable_action(cx: &mut TestAppContext) {
    let ((_, available), (_, unavailable), reason) = platforms::sample_items();
    let (window, cx) = open(cx, &RUST);
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    let index = |title: &str| view.rows.iter().position(|row| row.title == title).unwrap();
    for _ in 0..index(unavailable) {
        cx.simulate_keystrokes("down");
    }
    cx.run_until_parked();

    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Unavailable");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    cx.simulate_click(button.center(), Modifiers::none());

    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "the button dispatched nothing");
    assert!(
        row_is_visible(cx, &format!("unavailable-reason-{unavailable}")),
        "the row's explanation stays visible"
    );

    // Enter keeps its behavior: it shows the reason.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error(reason));

    // What selected the unavailable row is undone, and the others still
    // run — the row, not the button, was the dispatch.
    let delta = index(available) as isize - index(unavailable) as isize;
    let key = if delta > 0 { "down" } else { "up" };
    for _ in 0..delta.abs() {
        cx.simulate_keystrokes(key);
    }
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result(format!("Ran the {available} in the Rust guest"))
    );
}

/// A double click on the button dispatches the action exactly once: the
/// second press lands on the stale frame that still shows the button while
/// the first press's action is already running, and the definition — which
/// the click checks again at click time — refuses it. The host records the
/// opens, so a second dispatch would be visible.
#[gpui::test]
fn a_running_action_cannot_be_dispatched_again_through_the_footer_button(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = std::sync::Arc::new(TwoApplications::default());
    let runtime = Runtime::start().unwrap();
    runtime.set_applications(system.clone());
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/applications");
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"));
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.back();
    let (window, cx) = open_launcher(cx, launcher);

    // The install's result owns the strip; open the command and come back
    // so the launcher is idle again and the strip is the action.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    cx.simulate_input("fire");
    wait_for_rows(&window, cx, &["Firefox"]);
    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Open application");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.simulate_click(button.center(), Modifiers::none());

    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Opened Firefox".into()));
    assert_eq!(
        *system.opened.lock().unwrap(),
        ["/apps/Firefox.desktop"],
        "the action dispatched exactly once"
    );
}

/// A launcher with the Hello package from `cargo xtask guests` installed in
/// `data` and `source`, as the wheel test installs it.
fn installed_hello(
    cx: &mut TestAppContext,
    data: &std::path::Path,
    source: &std::path::Path,
) -> Launcher {
    let folder = source.join("hello");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("pane.json"),
        r#"{ "manifestVersion": 1, "title": "Hello", "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }] }"#,
    )
    .unwrap();
    std::fs::copy(
        command("hello", "sample_rust").component,
        folder.join("hello.wasm"),
    )
    .unwrap();
    let launcher =
        Launcher::with_packages(Runtime::start(), twelve_rows(), data.join("extensions"));
    // The install's guest check answers from the runtime thread.
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.back();
    launcher
}

/// The button's label comes from the action's identity — what activating
/// the selected row does — never from the row's title: the extension
/// list's first row is the package itself, titled "Hello", and the button
/// says what activating it does there, following the package's state as it
/// changes.
#[gpui::test]
fn the_footer_button_labels_the_action_from_identity_not_the_row_title(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let launcher = installed_hello(cx, data.path(), source.path());
    let (window, cx) = open_launcher(cx, launcher);

    let manage = cx
        .debug_bounds("row-Manage extensions…")
        .expect("the row is rendered");
    cx.simulate_click(manage.center(), Modifiers::none());
    settle(&window, cx);

    let nodes = accessible_nodes(cx);
    node(&nodes, "ListBoxOption", "Hello");
    node(&nodes, "Button", "Disable");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    cx.simulate_click(button.center(), Modifiers::none());
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Disabled Hello".into())
    );

    // The row is still titled "Hello"; the action's identity turned with
    // the package's state, so re-entering the list (the change's result
    // owned the strip until then) offers to enable it now.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let manage = cx
        .debug_bounds("row-Manage extensions…")
        .expect("the row is rendered");
    cx.simulate_click(manage.center(), Modifiers::none());
    settle(&window, cx);

    let nodes = accessible_nodes(cx);
    node(&nodes, "ListBoxOption", "Hello");
    node(&nodes, "Button", "Enable");
}

/// A long status owns the strip in place of the idle action, and stays
/// readable: it wraps within the strip's width and the strip grows with
/// it, as it did before the idle hint became the action.
#[gpui::test]
fn a_long_status_replaces_the_idle_strip_and_stays_readable(cx: &mut TestAppContext) {
    let detail = "the operation could not be completed because the target \
                  system refused the connection and every retry failed, so \
                  nothing was installed and the previous state was kept";
    let message =
        format!("Could not open a folder picker: {detail}. {detail}. {detail}. {detail}.");
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    launcher.show_error(message.clone());
    let (window, cx) = open_launcher(cx, launcher);
    cx.simulate_resize(gpui::size(px(380.), px(420.)));
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error(message));

    assert!(
        cx.debug_bounds("primary-action").is_none(),
        "no idle action button while a status shows"
    );
    let footer = cx
        .debug_bounds("status-error")
        .expect("the footer is rendered");
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        text.right() <= footer.right(),
        "the message wraps within the footer, not past its right edge"
    );
    assert!(
        text.size.height > px(50.),
        "the message wrapped to several lines: {:?}",
        text.size.height
    );
    assert!(
        footer.size.height > px(50.),
        "the footer grew past its 50px floor: {:?}",
        footer.size.height
    );
}

/// The last drawn frame's view transition, as the arriving content's
/// (offset from rest in px — below rest for a view that opens, above for
/// backing out — and its opacity); `None` when the frame drew the content
/// settled, which is also all reduced motion ever reports. See
/// [`LauncherWindow::view_transition`].
fn arriving(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Option<(f32, f32)> {
    cx.read_entity(window, |window, _| window.view_transition())
}

/// Delivers the animation frame the window has asked for, as the native
/// frame loop would, with `elapsed` passing first on the test platform's
/// controlled clock. The test platform delivers no frames on its own, so
/// this is the only thing that advances a running transition; one call
/// draws at most one frame. Returns how many next-frame callbacks ran —
/// `0` means the window had asked for no frame, so nothing drew.
fn frame(cx: &mut VisualTestContext, elapsed: Duration) -> usize {
    cx.executor().advance_clock(elapsed);
    let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    ran
}

/// Delivers frames until the window asks for none, so a transition in
/// flight completes and the functional scroll relayout after a screen
/// change finishes, and returns the frames it delivered. `0` means the
/// window was already idle: no cosmetic and no functional frame was
/// pending. Bounded, so a window that never stopped asking for frames
/// fails the test instead of hanging it.
fn settle_frames(cx: &mut VisualTestContext) -> usize {
    let mut delivered = 0;
    for _ in 0..20 {
        let ran = frame(cx, Duration::from_millis(25));
        if ran == 0 {
            return delivered;
        }
        delivered += ran;
    }
    panic!("the window never stopped asking for animation frames");
}

/// Opening a command is a view transition: the content that changes —
/// the results list — arrives over a brief fade and a tiny shift from
/// below, while the shell chrome (the footer with #71's action strip)
/// stays exactly where it was. The arrival is driven on the controlled
/// clock: it progresses as frames are delivered, completes within its
/// bounded span, and leaves the window asking for no frame at all.
#[gpui::test]
fn opening_a_command_transitions_the_content_and_keeps_the_chrome_still(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    // The first frame drew no transition, and none is pending: a settled
    // window is idle.
    assert_eq!(frame(cx, Duration::ZERO), 0);
    assert!(arriving(&window, cx).is_none());

    // The footer — the idle strip with the action button — is chrome.
    let footer = cx
        .debug_bounds("status-idle")
        .expect("the idle strip is rendered");

    // Enter opens the selected command: the frame that draws the new
    // screen starts the arrival, the full shift below rest.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    let (offset, opacity) = arriving(&window, cx).expect("the command's content is arriving");
    assert!(
        offset > 2.5 && offset < 3.5,
        "the arrival starts the full shift below rest: {offset}"
    );
    assert!(opacity < 0.45, "the arrival starts faint: {opacity}");
    // The chrome did not move with it.
    let footer_now = cx
        .debug_bounds("status-idle")
        .expect("the idle strip is rendered");
    assert_eq!(
        footer_now, footer,
        "the footer (the action strip) stayed still"
    );
    // The content did: the list is drawn displaced from its rest by the
    // arrival's shift (where it lies once settled, below).
    let rows = cx.debug_bounds("rows").expect("the list is rendered");

    // Frames pass, and the arrival progresses without restarting.
    assert!(frame(cx, Duration::from_millis(40)) >= 1);
    let (progressed, _) = arriving(&window, cx).expect("the content is still arriving");
    assert!(
        progressed > 0.05 && progressed < offset,
        "the arrival progressed toward rest: {progressed} from {offset}"
    );
    // Past the entrance's span, the next delivered frame lands the
    // content at rest and asks for no further frame: the window is idle.
    assert!(frame(cx, Duration::from_millis(130)) >= 1);
    assert!(arriving(&window, cx).is_none());
    let settled = cx.debug_bounds("rows").expect("the list is rendered");
    assert_eq!(
        rows.origin.y - settled.origin.y,
        px(offset),
        "the list was shifted exactly the arrival's offset below its rest"
    );
    assert_eq!(settle_frames(cx), 0, "a settled window asks for no frame");
}

/// Backing out is the paired transition: the root content arrives from
/// above rest instead of below, over the quicker return, and settles
/// leaving the window idle.
#[gpui::test]
fn backing_out_transitions_the_root_content_from_above(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    // The entrance completed; nothing is pending.
    settle_frames(cx);
    assert!(arriving(&window, cx).is_none());

    // Escape backs out to root search.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    let (offset, _) = arriving(&window, cx).expect("the root content is arriving");
    assert!(
        offset < -2.5 && offset > -3.5,
        "the return starts the full shift above rest: {offset}"
    );

    // The return is the quicker of the two spans: 130ms — past its 120ms
    // — settles it.
    assert!(frame(cx, Duration::from_millis(130)) >= 1);
    assert!(arriving(&window, cx).is_none());
    assert_eq!(settle_frames(cx), 0, "a settled window asks for no frame");
}

/// A rapid open/back/open retargets each arrival from the presentation on
/// screen — the interrupted offset carries over, so nothing restarts, no
/// departed screen flashes back and the navigation itself is unaffected.
#[gpui::test]
fn rapid_open_back_open_retargets_the_arrival_from_where_it_is(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);

    // Open, back and open again, with no test-clock time passing between
    // them: each navigation's frame has already drawn.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    let (offset, _) = arriving(&window, cx).expect("the command's content is arriving");

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    let (back, _) = arriving(&window, cx).expect("the root content is arriving");
    // The back transition continued from the interrupted presentation —
    // the same offset, not a fresh start from above.
    assert!(
        (back - offset).abs() < 0.05,
        "the back continued the presentation: {back} from {offset}"
    );

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    let (retargeted, _) = arriving(&window, cx).expect("the command's content is arriving again");
    assert!(
        (retargeted - offset).abs() < 0.05,
        "the reopening continued the presentation"
    );

    // The retargeted arrival then progresses and completes like any other.
    assert!(frame(cx, Duration::from_millis(40)) >= 1);
    let (progressed, _) = arriving(&window, cx).expect("the content is still arriving");
    assert!(
        progressed < retargeted,
        "the arrival progressed toward rest"
    );
    settle_frames(cx);
    assert!(arriving(&window, cx).is_none());
    // And the screen the user navigated to is what is drawn — the rapid
    // reversal left no stale view behind (settle drew and checked it).
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
}

/// Navigation, focus and typing take effect immediately: while an arrival
/// is still in flight, the query field has focus, typing lands on the very
/// frames that carry the transition, and Enter dispatches without waiting
/// for it.
#[gpui::test]
fn typing_and_dispatch_take_effect_while_an_arrival_is_in_flight(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // Back out: the root content's arrival is in flight.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(
        arriving(&window, cx).is_some(),
        "the return is still arriving"
    );
    assert!(
        query_has_focus(&window, cx),
        "focus moved to the query at once"
    );

    // Typing lands while the arrival is in flight: the frame that draws
    // the narrowed results is the same frame that draws the transition.
    cx.simulate_input("Rust");
    let view = settle(&window, cx);
    assert_eq!(view.search_field(), Some("Rust"));
    assert!(
        arriving(&window, cx).is_some(),
        "typing did not wait for the arrival to finish"
    );

    // So does dispatch: Enter opens the best match while the arrival is
    // still in flight.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    settle_frames(cx);
}

/// Escaping mid-arrival cancels the opening: the departing command's
/// content is unmounted at once — the drawn screen is root's, its rows
/// are root's — and the in-flight arrival belongs to the root content,
/// with no overlay of the command fading out. The command's answer,
/// arriving after the user left, updates the status without navigating
/// back to the departed screen.
#[gpui::test]
fn escaping_mid_arrival_cancels_it_without_a_trace_of_the_departed_screen(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // The command's selected item starts running (its answer is still to
    // come) and, with the arrival from opening still in flight, the user
    // backs out of it.
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    // Root's own rows are what is drawn, not a fading-out command.
    assert!(
        row_titles(&window, cx)
            .iter()
            .any(|title| title == "Rust sample"),
        "the root results are drawn, not the command's"
    );
    // The arrival in flight is the root content's, continued from the
    // interrupted forward arrival.
    let (offset, _) = arriving(&window, cx).expect("the root content is arriving");
    assert!(
        (offset - 3.).abs() < 0.5,
        "the arrival continued from below rest: {offset}"
    );

    // The item's answer, landing after the user left, changes nothing
    // about where the user is: the core drops a departed command's pending
    // reply, so the screen stays root and the status stays idle — no
    // stale completion navigates back. (Give the guest's late reply time
    // to land before asserting that it changed nothing.)
    std::thread::sleep(Duration::from_millis(150));
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "the late answer did not navigate back to the departed screen"
    );
    assert_eq!(view.status, Status::Idle);
    // And the cancellation left the window working: opening the command
    // again arrives again.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    assert!(arriving(&window, cx).is_some(), "the command arrives again");
    settle_frames(cx);
}

/// Query and result updates never animate: typing, a changed row set and
/// a moved selection on the same screen kind draw no transition and ask
/// for no cosmetic frame — only the functional scroll relayout's one.
#[gpui::test]
fn typing_selection_and_row_changes_never_transition(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    assert!(arriving(&window, cx).is_none());

    // Typing narrows the results to one: a query update, not a view
    // transition.
    cx.simulate_input("Rus");
    let view = settle(&window, cx);
    assert_eq!(view.search_field(), Some("Rus"));
    assert_eq!(view.selected, Some(0));
    assert!(
        arriving(&window, cx).is_none(),
        "a query update does not animate"
    );
    // So does moving the selection on the narrowed results.
    cx.simulate_keystrokes("down");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    assert!(
        arriving(&window, cx).is_none(),
        "a selection update does not animate"
    );
    // The functional scroll relayout after the rows changed is the only
    // frame the window asked for; once it is delivered, the window is
    // idle.
    settle_frames(cx);
    assert!(arriving(&window, cx).is_none());
}

/// Reduced motion settles every view transition at once: a navigation
/// under it starts no arrival, and reducing motion mid-arrival ends it on
/// the next drawn frame. Either way the window schedules no frame for
/// presentation.
#[gpui::test]
fn reduced_motion_settles_transitions_at_once_without_frames(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);

    // A navigation under reduced motion starts no transition: the frame
    // that draws the new screen is already settled.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    assert!(
        arriving(&window, cx).is_none(),
        "reduced motion drew the command's content settled"
    );

    // Reduced motion engaged mid-arrival ends it on the next frame. Begin
    // a return under full motion, then flip the preference.
    cx.update(|_, cx| cx.set_reduce_motion(false));
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert!(
        arriving(&window, cx).is_some(),
        "the return began under full motion"
    );
    cx.update(|_, cx| cx.set_reduce_motion(true));
    // The frame the arrival had asked for draws settled, and asks for
    // nothing further.
    assert!(frame(cx, Duration::ZERO) >= 1);
    assert!(
        arriving(&window, cx).is_none(),
        "the arrival settled the moment reduced motion engaged"
    );
    assert_eq!(
        settle_frames(cx),
        0,
        "the window asked for no further frame"
    );
}

/// A window that stops drawing mid-arrival — hidden, on the native
/// platform — settles on the first frame it draws later: progress is
/// measured on a clock, not counted in frames, so the time that passed
/// while nothing drew completes the transition and that frame requests
/// nothing. (The test platform has no window visibility; the same state
/// is produced by letting the clock run without delivering a frame.)
#[gpui::test]
fn a_window_that_stops_drawing_settles_its_arrival_on_the_next_frame_it_draws(
    cx: &mut TestAppContext,
) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert!(arriving(&window, cx).is_some());

    // Time passes with no frame delivered and no redraw provoked — a
    // hidden window draws nothing, and the platform delivers none of the
    // frames it asked for.
    cx.executor().advance_clock(Duration::from_secs(5));

    // The window is shown again: the frame it had asked for is delivered,
    // and it is already settled — the time that passed completed the
    // transition — so that frame asks for no animation frame of its own.
    assert!(
        frame(cx, Duration::ZERO) >= 1,
        "the pending frame was delivered on show"
    );
    assert!(
        arriving(&window, cx).is_none(),
        "the arrival settled while the window did not draw"
    );
    assert_eq!(settle_frames(cx), 0, "the shown frame asked for nothing");
}
