//! Drives the native launcher window through GPUI's test platform: real key
//! and mouse events dispatch to the window, which runs real guest components.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, Runtime, Screen, Status};

#[path = "../../pane-core/tests/support/platforms.rs"]
mod platforms;

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

/// Opens the sample's command and then its form ("Greet someone", the fifth
/// item) with the keyboard.
fn open_form(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("enter");
    wait_for_answer(window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = wait_for_answer(window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Form, "Greet someone")
    );
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

    let view = wait_for_answer(&window, cx);
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
    let view = wait_for_answer(&window, cx);
    assert_eq!((view.screen, view.selected), (Screen::Command, Some(4)));
}

fn a_rejected_field_shows_its_error_and_takes_focus(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);
    open_form(&window, cx);

    // Submit from the greeting with the name left empty.
    cx.simulate_keystrokes("tab enter");

    let view = wait_for_answer(&window, cx);
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
        wait_for_answer(&window, cx).status,
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
    let view = wait_for_answer(&window, cx);
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
    let view = wait_for_answer(&window, cx);
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
    assert_eq!(wait_for_answer(&window, cx).status, Status::Result(answer));
    for _ in 0..index(available) {
        cx.simulate_keystrokes("up");
    }
    cx.simulate_keystrokes("enter");
    assert_eq!(
        wait_for_answer(&window, cx).status,
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
    let form = view.form.expect("a form is open");
    let field = form.fields.into_iter().find(|field| field.id == id);
    field.expect("the field exists").value
}

#[gpui::test]
fn tab_and_shift_tab_visit_each_control_once_in_order(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));

    // Two full rounds each way: a control with two tab stops (such as the
    // text field and a wrapper tracking its focus) would appear twice in a
    // row. The greeting group reports its chosen option as focused, like a
    // list reports its selected row.
    let mut forward = Vec::new();
    for _ in 0..6 {
        cx.simulate_keystrokes("tab");
        forward.push(focused_label(cx));
    }
    let mut backward = Vec::new();
    for _ in 0..6 {
        cx.simulate_keystrokes("shift-tab");
        backward.push(focused_label(cx));
    }

    let labels = |order: [&str; 6]| order.map(|label| Some(label.to_owned()));
    assert_eq!(
        forward,
        labels(["Hello", "Greet", "Name", "Hello", "Greet", "Name"])
    );
    assert_eq!(
        backward,
        labels(["Greet", "Hello", "Name", "Greet", "Hello", "Name"])
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
        wait_for_answer(&window, cx).status,
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
        wait_for_answer(&window, cx).status,
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
        wait_for_answer(&window, cx).status,
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
    assert_eq!(wait_for_answer(&window, cx).selected, Some(11));
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
    assert_eq!(wait_for_answer(&window, cx).selected, Some(3));
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
    assert_eq!((view.screen, view.selected), (Screen::Root, Some(0)));
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

/// Opens the sample's command and then its color picker ("Choose a color",
/// the sixth item) with the keyboard.
fn open_color(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("enter");
    wait_for_answer(window, cx);
    cx.simulate_keystrokes("down down down down down enter");
    let view = wait_for_answer(window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::CustomView, "Choose a color")
    );
}

/// Waits until the open view shows `expected` as its value, which it does
/// once the guest's answer to the last event has arrived.
fn wait_for_color(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        let shown = view.custom_view.map(|view| view.frame.value);
        if shown.as_deref() == Some(expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the view shows {shown:?}, not {expected:?}; status {:?}",
            view.status
        );
        std::thread::sleep(Duration::from_millis(5));
    }
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
        3,
        "the view, the status line and the window: {roles:?}"
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

    // The view is the screen's only tab stop, and Escape closes it.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Color"));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Color"));
    cx.simulate_keystrokes("escape");
    let view = wait_for_answer(&window, cx);
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
    wait_for_answer(window, cx);
    cx.simulate_keystrokes("down down down down down enter");
    let view = wait_for_answer(window, cx);
    assert_eq!(view.screen, Screen::CustomView);
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
