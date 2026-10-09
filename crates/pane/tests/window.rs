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

// Pane registers no sample command (#162): the tests that drive the
// samples register them themselves.
#[path = "support/samples.rs"]
mod samples;

#[path = "support/paint.rs"]
mod paint;

#[path = "../../pane-core/tests/support/artifacts.rs"]
mod artifacts;

use settle::{enter_flow, settle, settle_shown, until};

#[path = "support/wait.rs"]
mod wait;

use wait::{frame, settle_frames};

#[path = "support/a11y.rs"]
mod a11y;

use a11y::{announcement, no_row_has_focus};

/// How long the announcer waits after the last keystroke before it says
/// the selected row (#132), on the test platform's controlled clock.
const SETTLE: Duration = Duration::from_millis(300);

/// Lets typing settle for the announcer: its time runs on the test
/// platform's clock, which only the test advances.
fn typing_settles(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(SETTLE);
    cx.run_until_parked();
}

/// Runs the window until its announcer says `expected` (#132): the
/// results typing waits for may still arrive from the extension
/// runtime's thread.
fn until_announced(cx: &mut VisualTestContext, expected: &str) {
    wait::until(cx, |cx| (announcement(cx) == expected).then_some(()));
}

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

/// Opens the Rust sample's command from root search with the pointer —
/// a click on its row — the one way into a view that arrives (a keyboard
/// open lands at once).
fn open_by_click(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> pane_core::LauncherView {
    let row = cx
        .debug_bounds("row-Rust sample")
        .expect("the command's root row");
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
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result(format!("Hello from the {} guest", sample.language))
    );
    assert!(
        cx.debug_bounds("toast-success").is_some(),
        "the toast is rendered"
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
        settle_shown(&window, cx),
        Status::Result(format!("Waited 50 ms inside the {} guest", sample.language))
    );
}

fn a_validation_error_is_rendered(cx: &mut TestAppContext, sample: &Sample) {
    let (window, cx) = open(cx, sample);

    click_row(&window, cx, "row-Validate settings");

    assert_eq!(
        settle_shown(&window, cx),
        Status::Error(
            "The extension reported an error: Invalid settings: port must be between 1 and 65535"
                .into()
        )
    );
    assert!(
        cx.debug_bounds("toast-failure").is_some(),
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
    // The list keeps the focus, and the announcer says the row, its place
    // and that it cannot run here (#132).
    assert_eq!(focused_label(cx).as_deref(), Some(view.title.as_str()));
    assert_eq!(
        announcement(cx),
        format!(
            "{unavailable}, {} of {}, unavailable",
            index(unavailable) + 1,
            view.rows.len()
        )
    );

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
    assert_eq!(settle_shown(&window, cx), Status::Result(answer));
    for _ in 0..index(available) {
        cx.simulate_keystrokes("up");
    }
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
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
            "Pane menu",
            "Name",
            "Hello",
            "Greet",
            "Pane menu",
            "Name"
        ])
    );
    assert_eq!(
        backward,
        labels([
            "Pane menu",
            "Greet",
            "Hello",
            "Name",
            "Pane menu",
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

/// The form is drawn with the Settings field families (#99), the
/// launcher's own copies of their styling gone: each field a label 8px
/// over its control — a text field a 34px well (black 24% under its
/// ring), a choice the segmented track (black 24%) whose chosen segment
/// takes the white 12% wash — and the submit control a 30px button on
/// white 8%. A rejected field's error stands under its control.
#[gpui::test]
fn the_form_draws_the_settings_field_families(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_form(&window, cx);
    // The form arrives over the view transition's fade; its fills are read
    // once it has settled.
    settle_frames(cx);

    let name = cx.debug_bounds("field-name").expect("the name field");
    assert_eq!(name.size.height, px(34.), "a field's well");
    assert!(paint::paints_fill_at(cx, name, 0x0000003D), "black 24%");
    let label = cx
        .debug_bounds("field-label-name")
        .expect("the name's label");
    assert_eq!(name.top(), label.bottom() + px(8.), "the label over it");

    let greeting = cx.debug_bounds("field-greeting").expect("the choice");
    assert_eq!(greeting.size.height, px(36.), "a segmented track");
    assert!(paint::paints_fill_at(cx, greeting, 0x0000003D), "black 24%");
    let welcome = cx
        .debug_bounds("choice-greeting-welcome")
        .expect("a segment");
    cx.simulate_click(welcome.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(field_value(&window, cx, "greeting"), "welcome");
    assert_eq!(welcome.size.height, px(30.), "a segment");
    assert!(
        paint::paints_fill_at(cx, welcome, 0xFFFFFF1F),
        "the chosen segment's white 12%"
    );

    let submit = cx.debug_bounds("submit").expect("the submit button");
    assert_eq!(submit.size.height, px(30.), "a button");
    assert!(paint::paints_fill_at(cx, submit, 0xFFFFFF14), "white 8%");

    // Submitted with the name empty: the error stands under the field.
    cx.simulate_click(submit.center(), Modifiers::none());
    settle(&window, cx);
    let error = cx
        .debug_bounds("field-error-name")
        .expect("the field's error");
    let name = cx.debug_bounds("field-name").expect("the name field");
    assert!(error.top() >= name.bottom(), "{error:?} under {name:?}");
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
    let (window, cx) = open_with(cx, samples::sample_commands());
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
/// lies wholly inside the list. The list draws only the rows in view and a
/// few past its edges (#165): a row it did not draw is not visible.
fn row_is_visible(cx: &mut VisualTestContext, element: &str) -> bool {
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let Some(element) = cx.debug_bounds(selector(element)) else {
        return false;
    };
    element.top() >= list.top() && element.bottom() <= list.bottom()
}

/// The launcher's frame is the reference's: at its 760×518 client the
/// search header, the list and the footer divide the whole panel — 64,
/// 404 and 50 — edge to edge, with no border taking a pixel from any side
/// (the panel's inner edge is an inset ring that takes no layout space).
#[gpui::test]
fn the_launcher_divides_its_reference_client_edge_to_edge(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, samples::sample_commands());
    cx.simulate_resize(pane::launcher_client_size());
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle);
    let search = cx.debug_bounds("search").expect("root search is rendered");
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let footer = cx
        .debug_bounds("status-idle")
        .expect("the footer is rendered");
    assert_eq!(search.origin, gpui::point(px(0.), px(0.)));
    assert_eq!(search.size.width, px(760.));
    assert_eq!((list.left(), list.top()), (px(0.), px(64.)));
    assert_eq!(list.size, gpui::size(px(760.), px(404.)));
    assert_eq!((footer.left(), footer.top()), (px(0.), px(468.)));
    assert_eq!(footer.size, gpui::size(px(760.), px(50.)));
}

/// Selecting a half-shown row with the pointer does not scroll the list:
/// scrolling it into view under a still pointer would put another row
/// under the pointer, which the next small movement would select and
/// scroll in turn. The keys' selection still scrolls.
#[gpui::test]
fn the_pointer_selects_a_half_shown_row_without_scrolling(cx: &mut TestAppContext) {
    let titles: Vec<String> = (1..=12).map(|n| format!("Row {n}")).collect();
    let commands = titles
        .iter()
        .map(|title| command(title, "sample_rust"))
        .collect();
    let (window, cx) = open_with(cx, commands);
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    cx.run_until_parked();
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let (index, half) = titles
        .iter()
        .enumerate()
        .find_map(|(index, title)| {
            let row = cx.debug_bounds(selector(&format!("row-{title}")))?;
            (row.top() < list.bottom() && row.bottom() > list.bottom()).then_some((index, row))
        })
        .expect("a row the list's bottom edge cuts");
    let at = gpui::point(half.center().x, list.bottom() - px(3.));
    arrive(cx, at - gpui::point(px(1.), px(0.)));
    cx.simulate_mouse_move(at, None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(settle(&window, cx).selected, Some(index));
    assert!(row_is_visible(cx, "row-Row 1"), "the list did not scroll");
    assert!(!row_is_visible(
        cx,
        selector(&format!("row-{}", titles[index]))
    ));

    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_eq!(settle(&window, cx).selected, Some(index + 1));
    assert!(
        row_is_visible(cx, selector(&format!("row-{}", titles[index + 1]))),
        "the keys' selection scrolls into view"
    );
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
    let view = settle(&window, cx);

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
    // The list itself keeps the focus; the announcer said the command as
    // it opened, then its selected row (#132).
    assert_eq!(focused.as_deref(), Some("Rust sample"));
    let count = view.rows.len();
    assert_eq!(
        announcement(cx),
        format!("Rust sample, {count} results. Say hello, 1 of {count}")
    );

    // The action's toast is what the footer's status announces, and the
    // announcer says it too.
    cx.simulate_keystrokes("down enter");
    settle(&window, cx);
    let (nodes, focused) = accessibility_tree(cx);
    assert_eq!(focused.as_deref(), Some("Rust sample"));
    assert!(
        has(&nodes, "Status", "Waited 50 ms inside the Rust guest"),
        "{nodes:?}"
    );
    assert_eq!(announcement(cx), "Waited 50 ms inside the Rust guest");
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

/// Asserts that the screen shown has no heading line above its content
/// and that the footer's left, at rest, names it: the command's icon and
/// the screen's title, inside the footer strip and left of its buttons
/// (#162).
fn assert_named_in_the_footer(cx: &mut VisualTestContext, what: &str) {
    assert!(
        cx.debug_bounds("screen-heading").is_none(),
        "{what}: a heading line above the content"
    );
    let lead = cx
        .debug_bounds("footer-command")
        .unwrap_or_else(|| panic!("{what}: the footer names no command"));
    let strip = cx
        .debug_bounds("status-idle")
        .unwrap_or_else(|| panic!("{what}: the footer is not at rest"));
    assert!(
        strip.contains(&lead.center()),
        "{what}: the command's name is not in the footer: {lead:?} outside {strip:?}"
    );
    assert!(
        lead.center().x < strip.center().x,
        "{what}: the command's name is not on the footer's left"
    );
    assert!(
        cx.debug_bounds("footer-command-title").is_some(),
        "{what}: the footer shows no title"
    );
}

/// An extension's views start with their content (#162): its list, a form
/// and a custom view opened from it draw no heading line, and the footer's
/// left names the open command instead, as Raycast's footer does. Root
/// search has neither, and its section label stays.
#[gpui::test]
fn an_extension_view_has_no_heading_and_the_footer_names_it(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    assert!(cx.debug_bounds("screen-heading").is_none());
    assert!(
        cx.debug_bounds("footer-command").is_none(),
        "root search names no command in its footer"
    );
    assert!(
        cx.debug_bounds("section-Commands").is_some(),
        "root search's section label stays"
    );

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_named_in_the_footer(cx, "the command's list");

    cx.simulate_keystrokes("down down down down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    assert_eq!(view.title, "Greet someone");
    assert_named_in_the_footer(cx, "a form");
}

/// A custom view opened from an extension's list has no heading line
/// either; the footer's left names it (#162).
#[gpui::test]
fn a_custom_view_has_no_heading_and_the_footer_names_it(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    open_color(&window, cx);
    assert_named_in_the_footer(cx, "a custom view");
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
        5,
        "the view, the menu button, the status line, the announcer and the window: {roles:?}"
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
    assert_eq!(focused_label(cx).as_deref(), Some("Pane menu"));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Color"));
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!((view.screen, view.selected), (Screen::Command, Some(5)));
    // The list has the focus again; its selected row claims none (#132).
    let list = format!("{} sample", sample.language);
    assert_eq!(focused_label(cx).as_deref(), Some(list.as_str()));
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
    let (window, cx) = open_with(cx, samples::sample_commands());
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
    let (window, cx) = open_with(cx, samples::sample_commands());
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
fn the_production_scenario_edits_searches_selects_opens_and_back_navigates(
    cx: &mut TestAppContext,
) {
    // The launcher's own wiring, end to end: the real adapter — the real
    // launcher, its query field, its search, its selection and its
    // navigation — edits, searches, selects, opens and back-navigates
    // through the real sample components.
    let (window, cx) = open_with(cx, samples::sample_commands());
    assert!(
        query_has_focus(&window, cx),
        "root search opens ready to type"
    );

    // Edits: typing reaches the query field the launcher owns.
    cx.simulate_input("script");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("script"));
    // Searches: the real root adapter narrows the real commands.
    assert_eq!(
        row_titles(&window, cx),
        ["JavaScript sample", "TypeScript sample"]
    );

    // Selects: the keyboard moves the real selection, rows rendered.
    cx.simulate_keystrokes("down");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(1));
    assert!(cx.debug_bounds("row-TypeScript sample").is_some());

    // Opens: Enter opens the selected command's own screen.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "TypeScript sample")
    );

    // Back-navigates: Escape returns to root search, ready to edit again.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.query(), Some(""));
    assert!(query_has_focus(&window, cx));
    cx.simulate_input("rust");
    assert_eq!(row_titles(&window, cx), ["Rust sample"]);
}

#[gpui::test]
fn a_query_that_matches_nothing_says_so_and_escape_clears_it(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, samples::sample_commands());
    cx.simulate_input("zzz");
    let view = settle(&window, cx);
    assert!(view.rows.is_empty());
    assert!(
        cx.debug_bounds("no-results").is_some(),
        "the empty state is shown"
    );
    // The notice names the query and, with no fallback, says where one is
    // offered (#96).
    let nodes = accessible_nodes(cx);
    let notice = node(&nodes, "Note", "Nothing matches “zzz”");
    let description = notice["description"].as_str().unwrap_or_default();
    assert!(description.contains("in Settings"), "{description}");
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
    let (window, cx) = open_with(cx, samples::sample_commands());
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
    let (window, cx) = open_with(cx, samples::sample_commands());
    cx.simulate_input("script");
    settle(&window, cx);

    let nodes = accessible_nodes(cx);
    let search = node(&nodes, "EditableComboBox", "Search");
    assert_eq!(
        (&search["value"], &search["placeholder"]),
        (&"script".into(), &"Search apps and commands…".into())
    );
    node(&nodes, "ListBox", "Results");
    node(&nodes, "ListBoxOption", "TypeScript sample");
    // The search field keeps the focus whatever is selected (#132); the
    // announcer says the selected result once typing has settled.
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
    typing_settles(cx);
    let count = row_titles(&window, cx).len();
    until_announced(cx, &format!("JavaScript sample, 1 of {count}"));
    cx.simulate_keystrokes("down");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
    assert_eq!(announcement(cx), format!("TypeScript sample, 2 of {count}"));

    // With nothing selected, the search field is still the focused node.
    cx.simulate_input("zzz");
    settle(&window, cx);
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
    typing_settles(cx);
    until_announced(cx, "No results");
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

/// A computed answer is drawn as the answer card (#96): under its
/// command's title, named for what was typed and its answer, the selected
/// result, whose primary action copies the answer. An expression with no
/// answer shows the notice in its place and the field keeps focus; the
/// expression completed brings the card back, selected.
#[gpui::test]
fn a_computed_answer_shows_as_the_card_under_its_commands_title(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let launcher = with_calculator(cx, data.path());
    let (window, cx) = open_launcher(cx, launcher);

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);
    let label = cx
        .debug_bounds("section-Calculator")
        .expect("the card is labelled with its command's title");
    let card = cx.debug_bounds("row-42").expect("the answer is drawn");
    assert!(cx.debug_bounds("answer-value").is_some(), "as the card");
    assert_eq!(
        card.top(),
        label.bottom() + px(4.),
        "the list's gap and the card's margin"
    );
    assert_eq!(card.size.height, px(20. + 44. + 16.));
    let nodes = accessible_nodes(cx);
    node(&nodes, "ListBoxOption", "6*7 = 42");
    // The footer's primary button gives way to the install's status
    // ("Installed Calculator") here; Enter below is the primary action.
    // The field keeps the focus, and the announcer says the card as it is
    // named once typing has settled (#132).
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
    assert!(no_row_has_focus(&a11y::a11y(cx)));
    assert!(query_has_focus(&window, cx));
    typing_settles(cx);
    until_announced(cx, "6*7 = 42, 1 of 1");

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Copied 42 to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("42".into())
    );

    // No answer: the notice for the query, no card, the field focused.
    cx.simulate_input("*");
    wait_for_rows(&window, cx, &[]);
    assert!(cx.debug_bounds("no-results").is_some());
    assert!(cx.debug_bounds("answer-value").is_none());
    assert!(query_has_focus(&window, cx));
    node(&accessible_nodes(cx), "Note", "Nothing matches “6*7*”");
    typing_settles(cx);
    until_announced(cx, "No results");

    // Completed, the card is back, selected.
    cx.simulate_input("2");
    wait_for_rows(&window, cx, &["84"]);
    assert!(cx.debug_bounds("no-results").is_none());
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
    typing_settles(cx);
    until_announced(cx, "6*7*2 = 84, 1 of 1");
}

/// A colour answer shows as the card with a swatch (#196): under
/// "Color" instead of the calculator's title, the swatch under the
/// answer's value, taller than the plain card by its height and the gap
/// above it, named for assistive technology by the colour's value, and
/// Enter copies the hex.
#[gpui::test]
fn a_colour_answer_shows_as_the_card_with_a_swatch(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let launcher = with_calculator(cx, data.path());
    let (window, cx) = open_launcher(cx, launcher);

    cx.simulate_input("#3aa");
    wait_for_rows(&window, cx, &["#33AAAA"]);
    let label = cx
        .debug_bounds("section-Color")
        .expect("the card is labelled with its own section");
    let card = cx.debug_bounds("row-#33AAAA").expect("the answer is drawn");
    assert!(cx.debug_bounds("answer-value").is_some(), "as the card");
    let swatch = cx
        .debug_bounds("answer-swatch")
        .expect("the colour is drawn as a swatch");
    // The swatch sits under the answer's value, within the card.
    assert!(swatch.top() >= card.top() + px(20. + 44.));
    assert!(swatch.bottom() <= card.bottom());
    assert_eq!(card.size.height, px(20. + 44. + 4. + 28. + 16.));
    assert_eq!(card.top(), label.bottom() + px(4.));
    // The swatch is a colour well named by the colour's value.
    node(&accessible_nodes(cx), "ColorWell", "#33AAAA");
    assert!(query_has_focus(&window, cx));
    typing_settles(cx);
    until_announced(cx, "#3aa = #33AAAA, 1 of 1");

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Copied #33AAAA to the clipboard".into())
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("#33AAAA".into())
    );
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
                ..Default::default()
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
    // The field keeps the focus; the announcer says the selected result
    // once the applications have been listed and typing has settled
    // (#132).
    assert_eq!(focused.as_deref(), Some("Search"), "the field");
    typing_settles(cx);
    until_announced(cx, "Firefox, 1 of 1");

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Opened Firefox".into()));
    assert_eq!(*system.opened.lock().unwrap(), ["/apps/Firefox.desktop"]);
    assert!(query_has_focus(&window, cx), "typing goes on in the field");
}

/// A system with two applications of one name, which the host tells apart
/// by their distinctions.
struct TwoPythons;

impl pane_core::applications::Applications for TwoPythons {
    fn installed(&self) -> Result<Vec<pane_core::applications::Application>, String> {
        Ok(["Python311", "Python312"]
            .map(|folder| pane_core::applications::Application {
                id: format!("python-{folder}"),
                name: "Python".into(),
                location: format!(r"C:\{folder}"),
                distinction: Some(folder.into()),
                ..Default::default()
            })
            .into())
    }

    fn open(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
}

/// A pinned application sharing its name with another says what tells it
/// apart: as its tile's tooltip, and with its name to assistive
/// technology.
#[gpui::test]
fn a_pin_sharing_its_title_says_what_tells_it_apart(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let runtime = Runtime::start().unwrap();
    runtime.set_applications(std::sync::Arc::new(TwoPythons));
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/applications");
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
        .with_quick_slots(data.path());
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.back();
    // Pin the second Python, found by its name.
    cx.foreground_executor()
        .block_on(launcher.set_query("python"));
    let rows = launcher.view().rows;
    let index = rows
        .iter()
        .position(|row| row.subtitle.as_deref() == Some("Python312"))
        .unwrap_or_else(|| panic!("no second Python in {rows:?}"));
    launcher.select(index);
    let (change, recorded) =
        launcher.change_quick_slots(&rows[index].id, pane_core::ResultAction::Pin);
    assert!(
        matches!(change, pane_core::SlotChange::Changed(_)),
        "{change:?}"
    );
    cx.foreground_executor().block_on(recorded);
    cx.foreground_executor().block_on(launcher.set_query(""));
    let (window, cx) = open_launcher(cx, launcher);
    settle(&window, cx);

    let nodes = accessible_nodes(cx);
    let pin = node(&nodes, "Button", "Pinned 1: Python");
    assert_eq!(pin["description"], "Python312", "{pin:#}");

    let tile = cx.debug_bounds("slot-1").expect("the pin's tile");
    cx.simulate_mouse_move(tile.center(), None::<MouseButton>, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(700));
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("tooltip-Python312").is_some(),
        "the tile's tooltip"
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
/// the same action; and the toast the action shows takes the hint's place
/// in the strip, beside the buttons.
#[gpui::test]
fn the_footer_button_runs_the_selected_action_like_enter(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    // A narrow window: the strip, the button and its label all have to fit.
    cx.simulate_resize(gpui::size(px(380.), px(420.)));
    settle(&window, cx);

    // The button sits at the strip's right, before the Actions button —
    // the far left holds the Pane mark and the hint — and the keycap sits
    // inside the button, at its right end.
    let footer = cx
        .debug_bounds("status-idle")
        .expect("the idle strip is rendered");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action button is rendered");
    let actions = cx
        .debug_bounds("actions-button")
        .expect("the Actions button is rendered");
    assert!(
        button.right() < actions.left() && actions.right() <= footer.right(),
        "the buttons are right-aligned: {button:?}, {actions:?} in {footer:?}"
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
    let keycap = cx
        .debug_bounds("primary-action-keys")
        .expect("the keycap is rendered");
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
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Rust guest".into())
    );
    // The item's toast speaks in the strip, in the hint's place; with no
    // actions of its own, it leaves the footer's buttons where they are.
    assert!(
        cx.debug_bounds("status-toast").is_some(),
        "the toast owns the strip"
    );
    assert!(
        cx.debug_bounds("toast-success").is_some(),
        "the toast is rendered"
    );
    assert!(
        cx.debug_bounds("primary-action").is_some(),
        "a toast without actions keeps the footer's own buttons"
    );

    // Back at root search the button is root search's action again.
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
    let (window, cx) = open_with(cx, samples::sample_commands());
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

    assert_eq!(
        settle_shown(&window, cx),
        Status::Idle,
        "the button dispatched nothing"
    );
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
        settle_shown(&window, cx),
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
    // The install's result owns the strip; showing root search afresh
    // leaves the launcher idle, so the strip is the action. (Applications
    // is a root provider, with no command row to open and leave, #164.)
    launcher.show_root_search();
    let (window, cx) = open_launcher(cx, launcher);

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

    // The extension list, which Settings enters (#168): the launcher
    // window draws its screens while the launcher holds them.
    enter_flow(&window, cx);

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
    enter_flow(&window, cx);

    let nodes = accessible_nodes(cx);
    node(&nodes, "ListBoxOption", "Hello");
    node(&nodes, "Button", "Enable");
}

/// A long status takes the hint's place, and the primary action steps
/// aside while Actions stays; the message stays readable: it wraps within
/// the strip's room and the strip grows with it.
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

    let footer = cx
        .debug_bounds("status-error")
        .expect("the footer is rendered");
    let text = cx
        .debug_bounds("status-message")
        .expect("the message is rendered");
    assert!(
        cx.debug_bounds("primary-action").is_none(),
        "no primary action while a status shows"
    );
    let actions = cx
        .debug_bounds("actions-button")
        .expect("Actions stays while a status shows");
    assert!(
        cx.debug_bounds("footer-hint").is_none(),
        "the message takes the hint's place"
    );
    assert!(
        text.right() <= actions.left(),
        "the message wraps short of Actions: {text:?}, {actions:?}"
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

/// Opening a command with the pointer is a view transition: the content that changes —
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

    // A click opens the command: the frame that draws the new screen
    // starts the arrival, the full shift below rest.
    let view = open_by_click(&window, cx);
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

/// A keyboard open lands at once: Enter is the launcher's most repeated
/// key, so the view it opens is drawn settled on the frame that shows it,
/// and a later change nothing opened — backing out and opening again with
/// Enter — never inherits an arrival either.
#[gpui::test]
fn opening_with_the_keyboard_lands_at_once(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    settle_frames(cx);

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert!(
        arriving(&window, cx).is_none(),
        "Enter drew the command settled"
    );

    // A click-armed open, used up by the screen it opened, leaves nothing
    // armed behind it: back out, and Enter's next open lands at once too.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    open_by_click(&window, cx);
    assert!(arriving(&window, cx).is_some(), "the click arrived");
    settle_frames(cx);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert!(
        arriving(&window, cx).is_none(),
        "Enter after a click still lands at once"
    );
}

/// A click that opens nothing — the command's row runs an action and the
/// screen stays — leaves no arrival armed behind it: the next keyboard
/// open (Enter into the command's form) still lands at once.
#[gpui::test]
fn a_click_that_opens_nothing_leaves_the_next_keyboard_open_settled(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    let view = click_row(&window, cx, "row-Say hello");
    assert_eq!(view.screen, Screen::Command, "the click opened nothing");
    settle_frames(cx);

    cx.simulate_keystrokes("down down down down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    assert!(
        arriving(&window, cx).is_none(),
        "Enter after a click that opened nothing drew the form settled"
    );
}

/// Backing out lands at once: root search is drawn settled on the frame
/// that shows it, with no arrival from either side.
#[gpui::test]
fn backing_out_lands_at_once(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    // The entrance completed; nothing is pending.
    settle_frames(cx);
    assert!(arriving(&window, cx).is_none());

    // Escape backs out to root search, settled.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(
        arriving(&window, cx).is_none(),
        "the return drew root settled"
    );
    // Only the functional scroll relayout may still ask for a frame; no
    // transition starts once it has run.
    settle_frames(cx);
    assert!(arriving(&window, cx).is_none());
}

/// A rapid open/back/open: backing out drops the arrival in flight, so
/// root lands settled with no departed screen flashing back, and opening
/// again starts a fresh arrival.
#[gpui::test]
fn rapid_open_back_open_drops_the_arrival_and_starts_fresh(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);

    // Open, back and open again, with no test-clock time passing between
    // them: each navigation's frame has already drawn.
    open_by_click(&window, cx);
    let (offset, _) = arriving(&window, cx).expect("the command's content is arriving");

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(
        arriving(&window, cx).is_none(),
        "backing out dropped the arrival"
    );

    let view = open_by_click(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    let (fresh, _) = arriving(&window, cx).expect("the command's content is arriving again");
    assert!(
        (fresh - offset).abs() < 0.05,
        "the reopening started the full shift again: {fresh} from {offset}"
    );

    // The arrival then progresses and completes like any other.
    assert!(frame(cx, Duration::from_millis(40)) >= 1);
    let (progressed, _) = arriving(&window, cx).expect("the content is still arriving");
    assert!(progressed < fresh, "the arrival progressed toward rest");
    settle_frames(cx);
    assert!(arriving(&window, cx).is_none());
    // And the screen the user navigated to is what is drawn — the rapid
    // reversal left no stale view behind (settle drew and checked it).
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
}

/// Navigation, focus and typing take effect immediately: while an arrival
/// is still in flight, Escape backs out without waiting for it, the query
/// field has focus at once, typing lands, and Enter dispatches.
#[gpui::test]
fn navigation_and_typing_take_effect_while_an_arrival_is_in_flight(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    open_by_click(&window, cx);
    assert!(
        arriving(&window, cx).is_some(),
        "the command's content is arriving"
    );

    // Back out mid-arrival: root is drawn at once, settled, with focus.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(arriving(&window, cx).is_none(), "the return drew settled");
    assert!(
        query_has_focus(&window, cx),
        "focus moved to the query at once"
    );

    // Typing lands at once.
    cx.simulate_input("Rust");
    let view = settle(&window, cx);
    assert_eq!(view.search_field(), Some("Rust"));

    // So does dispatch: Enter opens the best match.
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
/// are root's — and root is drawn settled, with no overlay of the command
/// fading out. The command's answer,
/// arriving after the user left, updates the status without navigating
/// back to the departed screen.
#[gpui::test]
fn escaping_mid_arrival_cancels_it_without_a_trace_of_the_departed_screen(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    open_by_click(&window, cx);

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
    // Backing out dropped the opening's arrival: root is drawn settled.
    assert!(
        arriving(&window, cx).is_none(),
        "the return drew root settled"
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
    let view = open_by_click(&window, cx);
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

/// Root search's rows follow the reference's pointer (#94): movement onto
/// a row selects it at once — the selected wash arrives with no fade, and
/// the window asks for no frame — and the footer and Enter act on it,
/// once. Off root search, an opened command's items share root search's
/// washes: their hover wash arrives at once, with the window idle.
#[gpui::test]
fn root_rows_select_under_the_moving_pointer_at_once(cx: &mut TestAppContext) {
    let (window, cx) = open_with(
        cx,
        vec![
            command("Rust sample", RUST.component),
            command("JavaScript sample", JAVASCRIPT.component),
        ],
    );
    let view = settle(&window, cx);
    settle_frames(cx);
    assert_eq!(view.selected, Some(0));

    let row = cx
        .debug_bounds("row-JavaScript sample")
        .expect("an unselected row");
    // The first event after the window shows only records where the
    // pointer is (a window appearing under a resting pointer gets one).
    arrive(cx, row.center());
    assert_eq!(
        settle(&window, cx).selected,
        Some(0),
        "the first event selected nothing"
    );
    cx.simulate_mouse_move(
        row.center() + gpui::point(px(1.), px(0.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(1), "the pointer's movement selected it");
    assert_eq!(settle_frames(cx), 0, "the root wash does not fade");

    // The footer's action follows what the pointer selected: Pane's own
    // rows name their actions differently from a command's.
    let own = view
        .rows
        .iter()
        .position(|row| !["Rust sample", "JavaScript sample"].contains(&row.title.as_str()))
        .expect("Pane lists its own rows after the commands");
    let own_row = cx
        .debug_bounds(selector(&format!("row-{}", view.rows[own].title)))
        .expect("Pane's own row is drawn");
    cx.simulate_mouse_move(own_row.center(), None::<MouseButton>, Modifiers::none());
    assert_eq!(settle(&window, cx).selected, Some(own));
    let nodes = accessible_nodes(cx);
    assert!(
        !nodes
            .iter()
            .any(|node| node["role"] == "Button" && node["label"] == "Open command"),
        "the footer names the selected row's own action"
    );
    cx.simulate_mouse_move(row.center(), None::<MouseButton>, Modifiers::none());
    assert_eq!(settle(&window, cx).selected, Some(1));
    node(&accessible_nodes(cx), "Button", "Open command");

    // Enter opens what the pointer selected.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command));
    assert_eq!(view.title, "JavaScript sample");
    settle_frames(cx);

    // The opened command's items share root search's visuals (#100):
    // their washes change at once, and hovering selects nothing.
    let item = view
        .rows
        .get(1)
        .map(|row| format!("row-{}", row.title))
        .expect("the sample lists more than one item");
    let item = cx
        .debug_bounds(selector(&item))
        .expect("an unselected item");
    cx.simulate_mouse_move(item.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        settle_frames(cx),
        0,
        "the item's hover wash asks for no frame"
    );
    assert_eq!(
        settle(&window, cx).selected,
        Some(0),
        "an item's hover selects nothing"
    );
}

/// Root search's rows sit under section labels from the launcher's
/// presentation: "Commands" over a blank query's list — no claim of
/// recent use — and "Results" with their count over a query's, each row
/// a label's height below it. (Scrolling to the selection past a label is
/// `the_list_scrolls_to_keep_the_selected_row_visible`'s.)
#[gpui::test]
fn root_rows_sit_under_their_section_labels(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    let label = cx
        .debug_bounds("section-Commands")
        .expect("a blank query's rows are its commands");
    let first = cx.debug_bounds("row-Alpha").expect("the first row");
    assert_eq!(label.size.height, px(30.));
    assert_eq!(first.top(), label.bottom() + px(2.), "the list's 2px gap");
    assert!(cx.debug_bounds("section-Results").is_none());

    cx.simulate_input("char");
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, "Charlie");
    let label = cx
        .debug_bounds("section-Results")
        .expect("a query's rows are its results");
    let first = cx.debug_bounds("row-Charlie").expect("the match");
    assert_eq!(first.top(), label.bottom() + px(2.));
    assert!(cx.debug_bounds("section-Commands").is_none());
}

/// Three root rows — Alpha, Bravo and Charlie, which open the Rust,
/// JavaScript and TypeScript samples — the pointer outside the window.
fn three_rows(cx: &mut TestAppContext) -> (gpui::Entity<LauncherWindow>, &mut VisualTestContext) {
    let (window, cx) = open_with(
        cx,
        vec![
            command("Alpha", RUST.component),
            command("Bravo", JAVASCRIPT.component),
            command("Charlie", TYPESCRIPT.component),
        ],
    );
    settle(&window, cx);
    (window, cx)
}

/// The pointer's first event in the window, at `at`: it only records
/// where the pointer is.
fn arrive(cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>) {
    cx.simulate_mouse_move(at, None::<MouseButton>, Modifiers::none());
}

fn center_of(cx: &mut VisualTestContext, row: &'static str) -> gpui::Point<gpui::Pixels> {
    cx.debug_bounds(row)
        .unwrap_or_else(|| panic!("{row} is drawn"))
        .center()
}

/// Move to B, then Down with the pointer resting on B: C stays selected —
/// a resting pointer never undoes the keys, even when the platform repeats
/// its position — and moving again over B selects B.
#[gpui::test]
fn a_resting_pointer_leaves_the_keys_selection_alone(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    let bravo = center_of(cx, "row-Bravo");
    arrive(cx, bravo - gpui::point(px(1.), px(0.)));
    cx.simulate_mouse_move(bravo, None::<MouseButton>, Modifiers::none());
    assert_eq!(settle(&window, cx).selected, Some(1));
    cx.simulate_keystrokes("down");
    assert_eq!(settle(&window, cx).selected, Some(2));
    // The same position again is not movement.
    cx.simulate_mouse_move(bravo, None::<MouseButton>, Modifiers::none());
    assert_eq!(
        settle(&window, cx).selected,
        Some(2),
        "the keys' selection stays"
    );
    // Real movement over B selects it again.
    cx.simulate_mouse_move(
        bravo + gpui::point(px(6.), px(0.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    assert_eq!(settle(&window, cx).selected, Some(1));
    // The keys stay in range at the ends.
    cx.simulate_keystrokes("up up up");
    assert_eq!(settle(&window, cx).selected, Some(0));
    let last = settle(&window, cx).rows.len() - 1;
    for _ in 0..=last {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(settle(&window, cx).selected, Some(last));
}

/// A click on an unselected row with no movement before it selects it;
/// a click on the selected row runs it. A pointer that moved onto a row
/// selected it already, so an ordinary click runs it, once.
#[gpui::test]
fn a_click_selects_an_unselected_row_and_runs_the_selected_one(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    let bravo = center_of(cx, "row-Bravo");
    cx.simulate_click(bravo, Modifiers::none());
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing ran");
    assert_eq!(view.selected, Some(1), "the click selected Bravo");
    cx.simulate_click(bravo, Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "JavaScript sample"),
        "the second click ran Bravo"
    );

    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let charlie = center_of(cx, "row-Charlie");
    arrive(cx, charlie - gpui::point(px(1.), px(0.)));
    cx.simulate_mouse_move(charlie, None::<MouseButton>, Modifiers::none());
    cx.simulate_click(charlie, Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "TypeScript sample"),
        "moving then clicking ran Charlie"
    );
    assert!(
        !matches!(view.status, Status::Error(_)),
        "{:?}",
        view.status
    );
}

/// The selection-freeze input at the root interaction boundary: while a
/// layer owns the selected target (the contextual Actions panel, #95),
/// moving over or clicking another row changes nothing; the keys still
/// move the selection. Thawed, movement selects again.
#[gpui::test]
fn a_frozen_selection_ignores_the_pointer(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    window.update(cx, |this, cx| this.freeze_pointer_selection(true, cx));
    let bravo = center_of(cx, "row-Bravo");
    arrive(cx, bravo - gpui::point(px(1.), px(0.)));
    cx.simulate_mouse_move(bravo, None::<MouseButton>, Modifiers::none());
    cx.simulate_click(bravo, Modifiers::none());
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.selected, Some(0), "the frozen target stayed");
    cx.simulate_keystrokes("down down");
    assert_eq!(
        settle(&window, cx).selected,
        Some(2),
        "the keys still move it"
    );

    window.update(cx, |this, cx| this.freeze_pointer_selection(false, cx));
    cx.simulate_mouse_move(
        bravo + gpui::point(px(4.), px(0.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    assert_eq!(
        settle(&window, cx).selected,
        Some(1),
        "thawed, movement selects"
    );
}

fn actions_open(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.actions_open())
}

/// Whether the Actions panel's search field has keyboard focus.
fn actions_filter_has_focus(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    use gpui::Focusable;
    let Some(filter) = cx.read_entity(window, |window, _| window.actions_filter()) else {
        return false;
    };
    cx.update(|window, cx| filter.focus_handle(cx).is_focused(window))
}

/// Open actions' default binding on this system (Ctrl+K deletes to the
/// end of the line in a macOS field).
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The open binding opens the selected result's actions with focus in
/// their search; Escape closes only the panel, giving focus back to the
/// query, and changes neither the query nor the selection.
#[gpui::test]
fn the_open_binding_shows_the_selected_results_actions_and_escape_closes_only_them(
    cx: &mut TestAppContext,
) {
    let (window, cx) = three_rows(cx);
    cx.simulate_input("a");
    settle(&window, cx);
    cx.simulate_keystrokes("down");
    let before = settle(&window, cx);
    let target = before.selected;

    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(actions_open(&window, cx));
    assert!(actions_filter_has_focus(&window, cx), "typing filters");
    assert!(cx.debug_bounds("actions-panel").is_some());
    assert!(cx.debug_bounds("actions-dimmer").is_some());
    let primary = cx.read_entity(&window, |window, _| window.launcher().selected_action());
    assert!(
        cx.debug_bounds(selector(&format!("action-{}", primary.label)))
            .is_some(),
        "the primary action is listed"
    );

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert!(cx.debug_bounds("actions-dimmer").is_none());
    assert!(query_has_focus(&window, cx), "focus returns to the query");
    assert_eq!(view.query(), Some("a"), "the query is untouched");
    assert_eq!(view.selected, target, "the target is untouched");

    // The next Escape follows the launcher's own rules: it clears the
    // query.
    cx.simulate_keystrokes("escape");
    assert_eq!(settle(&window, cx).query(), Some(""));
}

/// The footer's Actions button and the binding open and close the same
/// panel; while it is open the button shows it pressed.
#[gpui::test]
fn the_actions_button_and_the_binding_toggle_one_panel(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    let button = center_of(cx, "actions-button");
    arrive(cx, button - gpui::point(px(1.), px(0.)));
    cx.simulate_click(button, Modifiers::none());
    settle(&window, cx);
    assert!(actions_open(&window, cx), "the button opens it");

    // A click on the button while open lands outside the panel: it closes
    // it, consumed, and does not reopen it.
    cx.simulate_click(button, Modifiers::none());
    settle(&window, cx);
    assert!(!actions_open(&window, cx), "a second click closes it");

    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(actions_open(&window, cx));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(!actions_open(&window, cx), "the binding toggles it too");
}

/// Enter in the panel runs the primary action on the panel's target, as
/// the footer's button and Enter on the list do.
#[gpui::test]
fn the_primary_action_runs_on_the_target_from_the_panel(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    cx.simulate_keystrokes("down");
    settle(&window, cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    // Bravo is the JavaScript sample's component, which titles its view.
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "JavaScript sample")
    );
}

/// Typing filters the panel by label, without touching root search's
/// query; text that matches nothing says so, and runs nothing.
#[gpui::test]
fn typing_filters_the_actions_and_nothing_matching_says_so(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("zzz");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""), "root search's query is untouched");
    assert!(cx.debug_bounds("actions-empty").is_some());

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing ran");
}

/// A click outside the panel closes it and is consumed: the result under
/// the pointer is neither selected nor run.
#[gpui::test]
fn an_outside_click_closes_the_panel_without_invoking_what_it_covered(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    let charlie = center_of(cx, "row-Charlie");
    cx.simulate_click(charlie, Modifiers::none());
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing ran");
    assert_eq!(view.selected, Some(0), "nor was it selected");
}

/// While the panel is open, pointer movement over the results leaves its
/// target selected.
#[gpui::test]
fn the_pointer_cannot_change_the_panels_target(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    let bravo = center_of(cx, "row-Bravo");
    arrive(cx, bravo - gpui::point(px(1.), px(0.)));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_mouse_move(bravo, None::<MouseButton>, Modifiers::none());
    assert_eq!(settle(&window, cx).selected, Some(0));
}

/// A target that leaves the results while the panel is open (a new
/// search behind it, a package removed or disabled) shows its entries
/// unavailable, and Enter runs nothing.
#[gpui::test]
fn a_target_gone_from_behind_the_panel_runs_nothing(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    cx.simulate_keystrokes("down");
    settle(&window, cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    // Bravo leaves the results behind the open panel.
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    cx.foreground_executor()
        .block_on(launcher.set_query("charlie"));
    redraw(&window, cx);
    assert_eq!(row_titles(&window, cx), ["Charlie"]);
    assert!(actions_open(&window, cx));

    // Neither Enter nor a click on the entry runs it.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing ran");
    let entry = center_of(cx, "action-Open command");
    cx.simulate_click(entry, Modifiers::none());
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing ran");
}

/// With no result selected, the panel says there is nothing to act on.
#[gpui::test]
fn with_nothing_selected_the_panel_says_so(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    cx.simulate_input("zzzz");
    let view = settle(&window, cx);
    assert_eq!(view.selected, None);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.debug_bounds("actions-empty").is_some());
    assert!(
        cx.debug_bounds("actions-header").is_none(),
        "no target to name"
    );
    cx.simulate_keystrokes("enter");
    assert!(matches!(settle(&window, cx).screen, Screen::Root { .. }));
}

/// An installed command's actions route to its existing alias
/// configuration; the form returns to the search it came from.
#[gpui::test]
fn an_installed_commands_alias_action_opens_its_alias_form(cx: &mut TestAppContext) {
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
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = open_launcher(cx, launcher.clone());
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.show_root_search();
    redraw(&window, cx);
    cx.simulate_input("say");
    settle(&window, cx);

    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.debug_bounds("action-Add Alias…").is_some());
    assert!(cx.debug_bounds("action-group-Pane").is_some());
    cx.simulate_input("alias");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{view:?}");
    assert_eq!(view.title, "Alias for Say hello");

    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "say".into()
        }
    );
}

/// The footer menu acts on the row it opened for: while it is open,
/// moving over another row leaves the selection where it was; once it
/// closes, movement selects again.
#[gpui::test]
fn an_open_footer_menu_holds_the_selection_against_the_pointer(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    let menu = center_of(cx, "footer-menu");
    arrive(cx, menu - gpui::point(px(1.), px(0.)));
    cx.simulate_click(menu, Modifiers::none());
    settle(&window, cx);
    assert!(cx.debug_bounds("menu").is_some(), "the menu is open");

    let bravo = center_of(cx, "row-Bravo");
    cx.simulate_mouse_move(bravo, None::<MouseButton>, Modifiers::none());
    assert_eq!(
        settle(&window, cx).selected,
        Some(0),
        "the menu's target stayed"
    );

    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed");
    cx.simulate_mouse_move(
        bravo + gpui::point(px(4.), px(0.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    assert_eq!(
        settle(&window, cx).selected,
        Some(1),
        "closed, movement selects"
    );
}

/// The footer's buttons take the reference's `.fbtn` feedback: a hover
/// wash that changes at once, never a fade, and a press that moves no
/// geometry; the click acts the moment it happens.
#[gpui::test]
fn the_footer_buttons_change_their_washes_at_once(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, &RUST);
    settle(&window, cx);
    settle_frames(cx);
    let button = cx
        .debug_bounds("primary-action")
        .expect("the primary action");
    let at_rest = button.origin;

    cx.simulate_mouse_move(button.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_mouse_down(button.center(), MouseButton::Left, Modifiers::none());
    let held = cx
        .debug_bounds("primary-action")
        .expect("the button is held");
    assert_eq!(
        held.origin, at_rest,
        "the press moved no geometry: {:?} vs {:?}",
        held, at_rest
    );
    assert_eq!(settle_frames(cx), 0, "the hover wash asks for no frame");
    cx.simulate_mouse_up(button.center(), MouseButton::Left, Modifiers::none());
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Command),
        "the release activated the selected row at once"
    );
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
    let view = open_by_click(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    assert!(
        arriving(&window, cx).is_none(),
        "reduced motion drew the command's content settled"
    );

    // Reduced motion engaged mid-arrival ends it on the next frame. Back
    // out (which lands at once), begin an opening under full motion, then
    // flip the preference.
    cx.update(|_, cx| cx.set_reduce_motion(false));
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    settle_frames(cx);
    open_by_click(&window, cx);
    assert!(
        arriving(&window, cx).is_some(),
        "the opening began under full motion"
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
    open_by_click(&window, cx);
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

/// Pane's Clipboard History in the split view (#102, #166), through the
/// window: the real default extension from `cargo xtask guests`, acquired
/// from an artifact source on 127.0.0.1, over a fake system clipboard that
/// never touches the real one. It records from the first start; the search
/// field has no badge and no tabs follow it, a type dropdown at its right
/// filters by kind, rows are grouped by day, the detail shows the record's
/// Information, and the Actions panel (Ctrl+K) holds the record's and the
/// history's actions.
mod clipboard_split {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, prelude::*};
    use pane::LauncherWindow;
    use pane_core::clipboard::{
        CaptureState, ClipboardSystem, Content, ManualClock, Markers, Observation, Sink, Watch,
    };
    use pane_core::defaults::ArtifactSource;
    use pane_core::tray::TrayAction;
    use pane_core::{DefaultExtension, Launcher, PackageIdentity, Runtime, Screen};
    use tempfile::TempDir;

    use super::artifacts::Artifacts;
    use super::{open_launcher, settle, until};

    #[derive(Default)]
    struct Kept {
        sink: Option<Arc<dyn Sink>>,
        written: Vec<String>,
        /// The images Pane put on the clipboard, as PNGs (#167).
        images: Vec<Vec<u8>>,
        /// The files Pane put on the clipboard, a list per write (#167).
        files: Vec<Vec<std::path::PathBuf>>,
    }

    /// A system clipboard that records what Pane writes and reports only
    /// the copies a test makes.
    #[derive(Clone, Default)]
    struct FakeClipboard(Arc<Mutex<Kept>>);

    struct FakeWatch(Arc<Mutex<Kept>>);

    impl Drop for FakeWatch {
        fn drop(&mut self) {
            self.0.lock().unwrap().sink = None;
        }
    }

    impl FakeClipboard {
        /// `text` copied from `source`: whether Pane watched.
        fn copy(&self, text: &str, source: Option<&str>) -> bool {
            let Some(sink) = self.0.lock().unwrap().sink.clone() else {
                return false;
            };
            let ticket = sink.reading();
            sink.observed(
                ticket,
                Observation {
                    content: Content::Text(text.into()),
                    markers: Markers::default(),
                    source: source.map(str::to_owned),
                },
            );
            true
        }

        /// `content` (an image or files, #167) copied from `source`:
        /// whether Pane watched.
        fn copy_content(&self, content: Content, source: Option<&str>) -> bool {
            let Some(sink) = self.0.lock().unwrap().sink.clone() else {
                return false;
            };
            let ticket = sink.reading();
            sink.observed(
                ticket,
                Observation {
                    content,
                    markers: Markers::default(),
                    source: source.map(str::to_owned),
                },
            );
            true
        }

        fn written(&self) -> Vec<String> {
            self.0.lock().unwrap().written.clone()
        }

        fn written_images(&self) -> Vec<Vec<u8>> {
            self.0.lock().unwrap().images.clone()
        }

        fn written_files(&self) -> Vec<Vec<std::path::PathBuf>> {
            self.0.lock().unwrap().files.clone()
        }
    }

    impl ClipboardSystem for FakeClipboard {
        fn unavailable(&self) -> Option<String> {
            None
        }

        fn watch(&self, sink: Arc<dyn Sink>) -> Result<Watch, String> {
            self.0.lock().unwrap().sink = Some(sink);
            Ok(Watch::new(FakeWatch(self.0.clone())))
        }

        fn write_text(&self, text: &str) -> Result<(), String> {
            self.0.lock().unwrap().written.push(text.into());
            Ok(())
        }

        fn write_image(&self, png: &[u8]) -> Result<(), String> {
            self.0.lock().unwrap().images.push(png.to_vec());
            Ok(())
        }

        fn write_files(&self, paths: &[std::path::PathBuf]) -> Result<(), String> {
            self.0.lock().unwrap().files.push(paths.to_vec());
            Ok(())
        }
    }

    /// The assembled Clipboard History package's files.
    fn package_files() -> Vec<(String, Vec<u8>)> {
        let folder = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/clipboard-history");
        assert!(
            folder.is_dir(),
            "{} is missing; run `cargo xtask guests`",
            folder.display()
        );
        let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| {
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                (name, fs::read(&path).unwrap())
            })
            .collect();
        files.sort();
        files
    }

    /// One test's Pane: its data, artifact source, clock and clipboard.
    struct World {
        data: TempDir,
        artifacts: Artifacts,
        clipboard: FakeClipboard,
        clock: Arc<ManualClock>,
    }

    impl World {
        fn new() -> World {
            let world = World {
                data: tempfile::tempdir().unwrap(),
                artifacts: Artifacts::start(),
                clipboard: FakeClipboard::default(),
                clock: ManualClock::at(1_791_208_920_000),
            };
            let files = package_files();
            let manifest: serde_json::Value = serde_json::from_slice(
                &files
                    .iter()
                    .find(|(path, _)| path == "pane.json")
                    .expect("the package has a pane.json")
                    .1,
            )
            .unwrap();
            let borrowed: Vec<(&str, Vec<u8>)> = files
                .iter()
                .map(|(path, contents)| (path.as_str(), contents.clone()))
                .collect();
            world.artifacts.publish(
                "clipboard-history",
                manifest["version"].as_str().unwrap(),
                &borrowed,
            );
            world
        }

        /// Pane with Clipboard History acquired as its default extension,
        /// recording from the first start, and `texts` copied in order (the
        /// last newest), a minute apart.
        fn launcher(&self, cx: &mut TestAppContext, texts: &[&str]) -> Launcher {
            cx.executor().allow_parking();
            let launcher = Launcher::with_packages(
                Runtime::start(),
                vec![],
                self.data.path().join("extensions"),
            )
            .with_defaults(
                ArtifactSource::local(self.artifacts.url()).unwrap(),
                vec![DefaultExtension {
                    id: "clipboard-history".into(),
                    title: "Clipboard History".into(),
                }],
            )
            .with_clock(self.clock.clone())
            .with_clipboard(Arc::new(self.clipboard.clone()));
            cx.foreground_executor()
                .block_on(launcher.acquire_defaults());
            for text in texts {
                assert!(self.clipboard.copy(text, Some("notepad.exe")));
                self.clock.advance(std::time::Duration::from_secs(60));
            }
            launcher.show_root_search();
            launcher
        }
    }

    const COMMAND: &str = "default:clipboard-history#clipboard-history";

    /// Opens the command whose root row has id `id`, through the launcher.
    fn open_command(cx: &mut TestAppContext, launcher: &Launcher, id: &str) {
        launcher.show_root_search();
        cx.foreground_executor()
            .block_on(launcher.set_query("clipboard"));
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id == id)
            .expect("the command's row");
        launcher.select(index);
        cx.foreground_executor()
            .block_on(launcher.activate_selected());
        assert_eq!(launcher.view().screen, Screen::Command);
    }

    /// Opens the window over `launcher` and the history in it, as a user
    /// does: typing in root search, then Enter.
    fn open_history(
        cx: &mut TestAppContext,
        launcher: Launcher,
    ) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
        let (window, cx) = open_launcher(cx, launcher);
        cx.simulate_input("clipboard history");
        until(&window, cx, |view| {
            view.rows.first().is_some_and(|row| row.id == COMMAND)
        });
        cx.simulate_keystrokes("enter");
        until(&window, cx, |view| view.screen == Screen::Command);
        assert!(split_shown(&window, cx), "the split view shows");
        (window, cx)
    }

    fn split_shown(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
        cx.read_entity(window, |window, _| window.clipboard_split_shown())
    }

    fn listed(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
        cx.read_entity(window, |window, _| {
            window
                .launcher()
                .clipboard_history()
                .map(|view| {
                    view.records
                        .iter()
                        .map(|record| record.text.to_string())
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
    }

    /// Runs the window until the launcher no longer runs an action: a
    /// hidden window draws nothing, so this does not wait for a frame.
    fn done(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            cx.run_until_parked();
            let status = cx.read_entity(window, |window, _| window.launcher().view().status);
            if status != pane_core::Status::Running {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[gpui::test]
    fn a_click_selects_a_record_without_pasting_it_and_enter_pastes_it(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["first", "second", "third"]);
        let (window, cx) = open_history(cx, launcher);
        // Newest first, the newest selected and previewed as text.
        assert_eq!(listed(&window, cx), ["third", "second", "first"]);
        for row in ["clip-third", "clip-second", "clip-first"] {
            assert!(cx.debug_bounds(row).is_some(), "{row} is drawn");
        }
        assert!(cx.debug_bounds("clipboard-preview-text").is_some());
        assert!(cx.debug_bounds("section-Today").is_some());

        click(cx, "clip-first");
        settle(&window, cx);
        assert!(world.clipboard.written().is_empty(), "a click only selects");
        // Paste is the primary action, beside Actions.
        for button in ["clipboard-paste", "clipboard-actions"] {
            assert!(cx.debug_bounds(button).is_some(), "{button} is drawn");
        }

        // Enter pastes it; where Pane cannot paste yet (this launcher
        // reaches no system), it copies it instead, closes the window and
        // says so in the HUD.
        cx.simulate_keystrokes("enter");
        done(&window, cx);
        assert_eq!(world.clipboard.written(), ["first"]);
        assert!(cx.read_entity(&window, |window, _| window.hidden()));
        assert_eq!(
            cx.read_entity(&window, |window, _| window.hud()).as_deref(),
            Some(pane_core::system::PASTE_FALLBACK)
        );
    }

    /// Clipboard History draws no heading line above its content (#162):
    /// the footer's left names the command by its icon and title, as
    /// Raycast's does, and when the selected record was copied is said in
    /// its Information under the preview (#166). Its day's section label
    /// stays.
    #[gpui::test]
    fn the_footer_names_clipboard_history_and_no_heading_is_drawn(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["first"]);
        let (_window, cx) = open_history(cx, launcher);
        assert!(cx.debug_bounds("screen-heading").is_none());
        assert!(cx.debug_bounds("section-Today").is_some());
        let lead = cx
            .debug_bounds("footer-command")
            .expect("the footer names the command");
        let strip = cx.debug_bounds("status-idle").expect("the footer at rest");
        assert!(strip.contains(&lead.center()), "{lead:?} outside {strip:?}");
        assert!(lead.center().x < strip.center().x, "on the footer's left");
        assert!(
            cx.debug_bounds("icon-footer-command").is_some(),
            "the command's own icon, drawn as its package ships it"
        );
        assert!(
            cx.debug_bounds("clipboard-info-Copied").is_some(),
            "when it was copied, in the Information under the preview"
        );
    }

    /// Copying closes the window and says "Copied to Clipboard" in the
    /// HUD, as every Copy action does.
    fn copied(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
        cx.run_until_parked();
        assert!(
            cx.read_entity(window, |window, _| window.hidden()),
            "copying closes the window"
        );
        assert_eq!(
            cx.read_entity(window, |window, _| window.hud()).as_deref(),
            Some("Copied to Clipboard")
        );
    }

    #[gpui::test]
    fn the_keys_move_the_selection_and_the_actions_copy_and_delete(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["first", "second", "third"]);
        let (window, cx) = open_history(cx, launcher);

        // Down past the end stays on the last record. Ctrl+D deletes the
        // selected record through the existing delete; the selection falls
        // back to the first record left.
        cx.simulate_keystrokes("down down down up ctrl-d");
        settle(&window, cx);
        assert_eq!(listed(&window, cx), ["third", "first"]);
        // The Actions panel's Delete Entry does the same.
        cx.simulate_keystrokes(super::OPEN_ACTIONS);
        settle(&window, cx);
        assert!(cx.debug_bounds("actions-panel").is_some());
        click(cx, "action-Delete Entry");
        settle(&window, cx);
        assert_eq!(listed(&window, cx), ["first"]);

        // Ctrl+Enter, the secondary action, copies it again, closes the
        // window and says so in the HUD.
        cx.simulate_keystrokes("ctrl-enter");
        copied(&window, cx);
        assert_eq!(world.clipboard.written(), ["first"]);

        // Shown again, the view is back; the footer's Copy does the same.
        window.update_in(cx, |window, w, cx| {
            window.tray_selected(TrayAction::OpenPane, w, cx)
        });
        settle(&window, cx);
        assert!(split_shown(&window, cx), "the view is restored");
        cx.simulate_keystrokes(super::OPEN_ACTIONS);
        settle(&window, cx);
        click(cx, "action-Copy to Clipboard");
        copied(&window, cx);
        assert_eq!(world.clipboard.written(), ["first", "first"]);
    }

    #[gpui::test]
    fn a_query_that_matches_nothing_previews_nothing_and_copies_nothing(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["alpha", "beta"]);
        let (window, cx) = open_history(cx, launcher);

        // The search reaches the source program too.
        cx.simulate_input("NOTEPAD");
        settle(&window, cx);
        assert!(cx.debug_bounds("clip-alpha").is_some());
        cx.simulate_keystrokes("escape");
        cx.simulate_input("zzz");
        settle(&window, cx);
        assert!(cx.debug_bounds("clipboard-empty").is_some());
        assert!(cx.debug_bounds("clipboard-preview-text").is_none());
        assert!(
            cx.debug_bounds("clipboard-paste").is_none(),
            "no primary action"
        );
        cx.simulate_keystrokes("enter");
        settle(&window, cx);
        assert!(world.clipboard.written().is_empty());

        // Escape clears the query, then leaves for root search.
        cx.simulate_keystrokes("escape");
        settle(&window, cx);
        assert!(cx.debug_bounds("clipboard-preview-text").is_some());
        cx.simulate_keystrokes("escape");
        let view = settle(&window, cx);
        assert!(matches!(view.screen, Screen::Root { .. }));
        assert!(!split_shown(&window, cx));
    }

    #[gpui::test]
    fn the_actions_panel_pauses_and_resumes_recording(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["kept"]);
        let (window, cx) = open_history(cx, launcher);
        let capture = |window: &Entity<LauncherWindow>, cx: &mut VisualTestContext| {
            cx.read_entity(window, |window, _| {
                window
                    .launcher()
                    .clipboard_history()
                    .map(|view| view.capture)
            })
        };
        assert_eq!(capture(&window, cx), Some(CaptureState::On));

        cx.simulate_keystrokes(super::OPEN_ACTIONS);
        settle(&window, cx);
        // The history's own actions, beside the record's.
        for entry in [
            "action-Paste",
            "action-Copy to Clipboard",
            "action-Delete Entry",
            "action-Pause Recording",
            "action-Clear History…",
            "action-Disabled Applications…",
        ] {
            assert!(cx.debug_bounds(entry).is_some(), "{entry} is listed");
        }
        click(cx, "action-Pause Recording");
        settle(&window, cx);
        assert_eq!(capture(&window, cx), Some(CaptureState::Paused));
        assert!(
            !world.clipboard.copy("while paused", None),
            "nothing is watched"
        );

        cx.simulate_keystrokes(super::OPEN_ACTIONS);
        settle(&window, cx);
        click(cx, "action-Resume Recording");
        settle(&window, cx);
        assert_eq!(capture(&window, cx), Some(CaptureState::On));
        assert!(world.clipboard.copy("again", None));
    }

    #[gpui::test]
    fn the_actions_filter_runs_recording_actions_with_enter(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["kept"]);
        let (window, cx) = open_history(cx, launcher);

        for (label, selector, expected) in [
            (
                "Pause Recording",
                "action-Pause Recording",
                CaptureState::Paused,
            ),
            (
                "Resume Recording",
                "action-Resume Recording",
                CaptureState::On,
            ),
        ] {
            cx.simulate_keystrokes(super::OPEN_ACTIONS);
            settle(&window, cx);
            cx.simulate_input(label);
            settle(&window, cx);
            assert!(cx.debug_bounds(selector).is_some());
            assert!(cx.debug_bounds("action-Paste").is_none());
            cx.simulate_keystrokes("enter");
            settle(&window, cx);
            let capture = cx.read_entity(&window, |window, _| {
                window.launcher().clipboard_history().unwrap().capture
            });
            assert_eq!(capture, expected);
            assert!(world.clipboard.written().is_empty(), "Enter must not paste");
        }
    }

    /// Clear History asks first, over the view, then deletes every record.
    #[gpui::test]
    fn clear_history_asks_then_clears(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["one", "two"]);
        let (window, cx) = open_history(cx, launcher);
        cx.simulate_keystrokes(super::OPEN_ACTIONS);
        settle(&window, cx);
        click(cx, "action-Clear History…");
        settle(&window, cx);
        assert!(cx.debug_bounds("confirmation").is_some(), "it asks first");
        click(cx, "confirmation-primary");
        settle(&window, cx);
        assert!(listed(&window, cx).is_empty());
        assert!(cx.debug_bounds("clipboard-empty").is_some());
    }

    /// #192: the view is drawn again, frame after frame, without making its
    /// records again while the history does not change: the launcher shares
    /// them. Moving the selection and searching work over the same records;
    /// a copy kept, or a record deleted, makes them once more.
    #[gpui::test]
    fn drawing_again_with_nothing_changed_makes_no_records(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["alpha", "beta", "gamma"]);
        let (window, cx) = open_history(cx, launcher);
        let made = |window: &Entity<LauncherWindow>, cx: &mut VisualTestContext| {
            cx.read_entity(window, |window, _| {
                window.launcher().clipboard_records_made()
            })
        };
        let before = made(&window, cx);
        assert!(before > 0, "the records were made to be drawn");

        // Two more frames, each drawn after a key, with nothing changed.
        cx.simulate_keystrokes("down");
        settle(&window, cx);
        cx.simulate_keystrokes("up");
        settle(&window, cx);
        assert_eq!(made(&window, cx), before, "no records were made again");

        // Searching filters the same records.
        cx.simulate_input("al");
        settle(&window, cx);
        assert!(cx.debug_bounds("clip-alpha").is_some());
        assert!(cx.debug_bounds("clip-beta").is_none());
        assert!(cx.debug_bounds("clip-gamma").is_none());
        cx.simulate_keystrokes("escape");
        settle(&window, cx);
        assert!(cx.debug_bounds("clip-beta").is_some());
        assert_eq!(made(&window, cx), before);

        // A copy kept changes the history: the records are made once more,
        // for the next frame, and shared again after it.
        assert!(world.clipboard.copy("delta", Some("notepad.exe")));
        cx.simulate_keystrokes("down");
        settle(&window, cx);
        assert_eq!(made(&window, cx), before + 1);
        assert!(cx.debug_bounds("clip-delta").is_some());
        cx.simulate_keystrokes("up");
        settle(&window, cx);
        assert_eq!(made(&window, cx), before + 1);
        assert_eq!(listed(&window, cx), ["delta", "gamma", "beta", "alpha"]);

        // Ctrl+D still deletes the selected record (gamma, chosen above),
        // which changes the history once more.
        cx.simulate_keystrokes("ctrl-d");
        settle(&window, cx);
        assert_eq!(listed(&window, cx), ["delta", "beta", "alpha"]);
        assert!(cx.debug_bounds("clip-gamma").is_none());
        assert_eq!(made(&window, cx), before + 2);
    }

    /// No badge on the search field and no tabs under it (#166): the
    /// footer names the command (#162), and the type dropdown at the
    /// field's right keeps All Types, Text, Links or Colors.
    #[gpui::test]
    fn the_type_dropdown_filters_and_the_field_has_no_badge_or_tabs(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["alpha", "https://example.com", "#ff8800"]);
        let (window, cx) = open_history(cx, launcher);
        for gone in [
            "clipboard-tab-All",
            "clipboard-tab-Text",
            "clipboard-capture",
        ] {
            assert!(cx.debug_bounds(gone).is_none(), "{gone} is gone");
        }
        let lead = cx
            .debug_bounds("footer-command")
            .expect("the footer names the command");
        let back = cx.debug_bounds("clipboard-back").expect("the back button");
        assert!(
            lead.origin.y > back.origin.y + back.size.height,
            "the command is named in the footer, not on the search field"
        );
        assert!(cx.debug_bounds("clipboard-type").is_some());

        click(cx, "clipboard-type");
        settle(&window, cx);
        click(cx, "clipboard-type-links");
        settle(&window, cx);
        assert_eq!(
            cx.read_entity(&window, |window, _| window.clipboard_filter()),
            Some(pane_core::clipboard_view::ClipboardFilter::Links)
        );
        assert!(cx.debug_bounds("clip-https://example.com").is_some());
        assert!(cx.debug_bounds("clip-alpha").is_none());
        assert!(cx.debug_bounds("clip-#ff8800").is_none());

        click(cx, "clipboard-type");
        settle(&window, cx);
        click(cx, "clipboard-type-colors");
        settle(&window, cx);
        assert!(cx.debug_bounds("clip-#ff8800").is_some());
        assert!(cx.debug_bounds("clip-https://example.com").is_none());

        click(cx, "clipboard-type");
        settle(&window, cx);
        click(cx, "clipboard-type-all");
        settle(&window, cx);
        for row in ["clip-alpha", "clip-https://example.com", "clip-#ff8800"] {
            assert!(cx.debug_bounds(row).is_some(), "{row} is listed");
        }
    }

    /// Rows are grouped by day, and the detail shows the selected record's
    /// Information: Source, Type, Characters and Copied.
    #[gpui::test]
    fn rows_are_grouped_by_day_and_the_detail_shows_the_information(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["yesterday's"]);
        world.clock.advance(std::time::Duration::from_secs(86_400));
        assert!(world.clipboard.copy("today's", Some("notepad.exe")));
        let (window, cx) = open_history(cx, launcher);
        assert!(cx.debug_bounds("section-Today").is_some());
        assert!(cx.debug_bounds("section-Yesterday").is_some());
        assert!(cx.debug_bounds("clipboard-information").is_some());
        for row in [
            "clipboard-info-Source",
            "clipboard-info-Type",
            "clipboard-info-Characters",
            "clipboard-info-Copied",
        ] {
            assert!(cx.debug_bounds(row).is_some(), "{row} is shown");
        }
        settle(&window, cx);
    }

    /// #167: a copied image's row shows its thumbnail and its detail the
    /// image with its Dimensions; copied files' row shows the first file's
    /// system icon, titled by its name and how many more, and its detail
    /// lists them; the dropdown's Images and Files keep each; and Copy puts
    /// each back as what it was.
    #[gpui::test]
    fn images_and_files_show_a_thumbnail_and_a_preview_and_filter_by_type(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["alpha"]);
        let pixels: Vec<u8> = [0, 128, 255, 255].repeat(3 * 2);
        let png = pane_core::icons::encode_png(3, 2, &pixels).unwrap();
        let image = pane_core::clipboard::CopiedImage::from_png(png.clone()).unwrap();
        assert!(
            world
                .clipboard
                .copy_content(Content::Image(image), Some("mspaint.exe"))
        );
        world.clock.advance(std::time::Duration::from_secs(60));
        let files = vec![
            world.data.path().join("report.pdf"),
            world.data.path().join("photos"),
        ];
        assert!(
            world
                .clipboard
                .copy_content(Content::Files(files.clone()), Some("explorer.exe"))
        );
        let (window, cx) = open_history(cx, launcher);
        settle(&window, cx);

        // The files, newest, selected: the first file's icon on the row,
        // and the files listed in the preview; no Characters.
        for drawn in [
            "clip-report.pdf +1",
            "icon-clip-file",
            "clipboard-preview-files",
            "clipboard-preview-file-report.pdf",
            "clipboard-preview-file-photos",
            "clip-Image (3×2)",
            "clip-thumbnail",
        ] {
            assert!(cx.debug_bounds(drawn).is_some(), "{drawn} is drawn");
        }
        assert!(cx.debug_bounds("clipboard-info-Characters").is_none());
        assert!(cx.debug_bounds("clipboard-preview-text").is_none());

        // The image: previewed, with its Dimensions.
        click(cx, "clip-Image (3×2)");
        settle(&window, cx);
        assert!(cx.debug_bounds("clipboard-preview-image").is_some());
        assert!(cx.debug_bounds("clipboard-info-Dimensions").is_some());
        assert!(cx.debug_bounds("clipboard-info-Characters").is_none());

        // The dropdown keeps the images, then the files.
        click(cx, "clipboard-type");
        settle(&window, cx);
        click(cx, "clipboard-type-images");
        settle(&window, cx);
        assert!(cx.debug_bounds("clip-Image (3×2)").is_some());
        assert!(cx.debug_bounds("clip-report.pdf +1").is_none());
        assert!(cx.debug_bounds("clip-alpha").is_none());
        click(cx, "clipboard-type");
        settle(&window, cx);
        click(cx, "clipboard-type-files");
        settle(&window, cx);
        assert!(cx.debug_bounds("clip-report.pdf +1").is_some());
        assert!(cx.debug_bounds("clip-Image (3×2)").is_none());
        assert!(cx.debug_bounds("clip-alpha").is_none());

        // Copy (Ctrl+Enter) puts the files back as files.
        cx.simulate_keystrokes("ctrl-enter");
        copied(&window, cx);
        assert_eq!(world.clipboard.written_files(), [files]);
        assert!(world.clipboard.written().is_empty());

        // Shown again, the image copies back as an image.
        window.update_in(cx, |window, w, cx| {
            window.tray_selected(TrayAction::OpenPane, w, cx)
        });
        settle(&window, cx);
        click(cx, "clipboard-type");
        settle(&window, cx);
        click(cx, "clipboard-type-images");
        settle(&window, cx);
        cx.simulate_keystrokes("ctrl-enter");
        copied(&window, cx);
        assert_eq!(world.clipboard.written_images(), [png]);
    }

    #[gpui::test]
    fn a_similarly_titled_package_keeps_its_generic_list(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["kept"]);
        let copy = tempfile::tempdir().unwrap();
        for (path, contents) in package_files() {
            fs::write(copy.path().join(path), contents).unwrap();
        }
        cx.foreground_executor()
            .block_on(launcher.install_package(copy.path()));
        let local = PackageIdentity::local(copy.path()).unwrap();
        open_command(cx, &launcher, &format!("{}#clipboard-history", local.key()));
        let (window, cx) = open_launcher(cx, launcher);
        settle(&window, cx);
        assert!(!split_shown(&window, cx));
        assert!(cx.debug_bounds("row-Resume Recording").is_some());
        assert!(cx.debug_bounds("clipboard-list").is_none());
    }

    /// Clipboard History keeps the focus in its field (#132): its opening
    /// is said with its name and count, then the selected record; each row
    /// says its place and the list's size; one Down is said with the
    /// record's place; and a filter that keeps nothing says "No results".
    #[gpui::test]
    fn clipboard_history_keeps_its_field_focused_and_says_each_record(cx: &mut TestAppContext) {
        let world = World::new();
        let launcher = world.launcher(cx, &["first", "second", "third"]);
        let (window, cx) = open_history(cx, launcher);
        let field = Some("Search clipboard history");
        assert_eq!(super::focused_label(cx).as_deref(), field);
        let opened = super::wait::until(cx, |cx| {
            let said = super::announcement(cx);
            said.ends_with("third, 1 of 3").then_some(said)
        });
        assert!(opened.contains(", 3 results. "), "{opened}");

        let options: Vec<serde_json::Value> = super::accessible_nodes(cx)
            .into_iter()
            .filter(|node| node["role"] == "ListBoxOption")
            .collect();
        assert_eq!(options.len(), 3, "{options:#?}");
        for option in &options {
            assert_eq!(option["size_of_set"], 3, "{option}");
            assert!(option["position_in_set"].is_u64(), "{option}");
        }

        cx.simulate_keystrokes("down");
        settle(&window, cx);
        assert_eq!(super::focused_label(cx).as_deref(), field);
        assert!(super::no_row_has_focus(&super::a11y::a11y(cx)));
        assert_eq!(super::announcement(cx), "second, 2 of 3");

        // A filter that keeps nothing.
        cx.simulate_input("zzz");
        settle(&window, cx);
        super::typing_settles(cx);
        super::until_announced(cx, "No results");
        assert_eq!(super::focused_label(cx).as_deref(), field);
    }
}

/// Alpha, Bravo and Charlie (the Rust, JavaScript and TypeScript samples,
/// by the ids `sample_rust`, `sample_js` and `sample_ts`) over a data
/// folder keeping the quick slots, whose record first pins `pins` (command
/// ids, in order; no record when empty). The pointer is outside the
/// window.
fn pinned_rows<'a>(
    cx: &'a mut TestAppContext,
    data: &std::path::Path,
    pins: &[&str],
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    if !pins.is_empty() {
        let pins: Vec<serde_json::Value> = pins
            .iter()
            .map(|id| serde_json::json!({ "command": id }))
            .collect();
        let record = serde_json::json!({ "version": 2, "pins": pins });
        std::fs::write(data.join("quick-slots.json"), record.to_string()).unwrap();
    }
    let launcher = Launcher::new(
        Runtime::start(),
        vec![
            command("Alpha", RUST.component),
            command("Bravo", JAVASCRIPT.component),
            command("Charlie", TYPESCRIPT.component),
        ],
    )
    .with_quick_slots(data);
    let (window, cx) = open_launcher(cx, launcher);
    settle(&window, cx);
    (window, cx)
}

/// The pins' titles, in order.
fn slot_titles(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read_entity(window, |window, _| window.launcher().quick_slots())
        .into_iter()
        .map(|slot| slot.title)
        .collect()
}

/// The launcher's pin key on this system: it pins root search's selected
/// result, or unpins it, or unpins the focused slot.
const TOGGLE_PIN: &str = if cfg!(target_os = "macos") {
    "cmd-shift-f"
} else {
    "ctrl-shift-f"
};

/// The pin key's name, as the pin hint tells assistive technology.
const TOGGLE_PIN_NAME: &str = if cfg!(target_os = "macos") {
    "Shift+Command+F"
} else {
    "Ctrl+Shift+F"
};

/// How the slots' Ctrl+digit chords name Ctrl on this system.
const CTRL: &str = if cfg!(target_os = "macos") {
    "Control"
} else {
    "Ctrl"
};

/// The move keys on this system: the focused slot one place later, or
/// earlier.
const MOVE_PIN_DOWN: &str = if cfg!(target_os = "macos") {
    "cmd-alt-down"
} else {
    "ctrl-alt-down"
};
const MOVE_PIN_UP: &str = if cfg!(target_os = "macos") {
    "cmd-alt-up"
} else {
    "ctrl-alt-up"
};

/// Seven pins, none of whose commands is installed: more than the strip's
/// one row of five, and more than the five Ctrl numbers.
const SEVEN_PINS: [&str; 7] = ["one", "two", "three", "four", "five", "six", "seven"];

/// A blank query shows the pinned home — the "Pinned" label and its strip,
/// on a fresh launcher with nothing pinned only the pin hint, which says
/// how to pin and is no slot — above the rows' "Commands"; a query hides
/// it, and clearing the query brings it back.
#[gpui::test]
fn a_blank_query_shows_the_pinned_home_and_a_query_hides_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &[]);
    let label = cx.debug_bounds("section-Pinned").expect("the home's label");
    let strip = cx.debug_bounds("pinned-strip").expect("the strip");
    let commands = cx
        .debug_bounds("section-Commands")
        .expect("the rows' label");
    assert_eq!(label.size.height, px(30.));
    assert_eq!(strip.top(), label.bottom() + px(2.), "the list's gap");
    assert_eq!(strip.size.height, px(2. + 80. + 6.));
    assert_eq!(commands.top(), strip.bottom() + px(2.));
    let hint = cx.debug_bounds("pin-hint").expect("the pin hint");
    assert_eq!(hint.top(), strip.top() + px(2.));
    assert_eq!(hint.size.height, px(80.));
    assert!(cx.debug_bounds("slot-1").is_none(), "nothing is pinned");
    // The hint says how to pin, and is no slot: no button, no chord.
    let nodes = accessible_nodes(cx);
    let note = node(&nodes, "Note", "Pin");
    let how = note["description"].as_str().unwrap_or_default();
    assert!(how.contains(TOGGLE_PIN_NAME), "{note:#}");
    assert!(how.contains("choose Pin"), "{note:#}");
    assert!(note.get("keyboard_shortcut").is_none(), "{note:#}");
    assert!(
        !nodes.iter().any(|node| node["role"] == "Button"
            && node["label"]
                .as_str()
                .is_some_and(|label| label.starts_with("Pinned "))),
        "no slot is drawn"
    );
    assert!(
        !data.path().join("quick-slots.json").exists(),
        "a fresh launcher pins nothing"
    );

    cx.simulate_input("al");
    settle(&window, cx);
    assert!(
        cx.debug_bounds("pinned-strip").is_none(),
        "a query hides it"
    );
    assert!(cx.debug_bounds("section-Pinned").is_none());
    cx.simulate_keystrokes("escape");
    assert_eq!(settle(&window, cx).query(), Some(""));
    assert!(
        cx.debug_bounds("pinned-strip").is_some(),
        "clearing restores it"
    );
}

/// Chooses the vertical layout of the pinned home in the settings record
/// of `data`, as the Launcher page writes it, and initializes the host
/// settings from it before the launcher window is made.
fn vertical_layout(cx: &mut TestAppContext, data: &std::path::Path) {
    std::fs::write(
        data.join("settings.json"),
        r#"{ "version": 1, "pinnedLayout": "vertical" }"#,
    )
    .unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
}

/// The vertical layout lists the pins as result rows, in order, under the
/// "Pinned" label and above the results, with no strip of tiles and no
/// pin hint; a query hides them, and clearing it brings them back.
#[gpui::test]
fn the_vertical_layout_lists_the_pins_as_rows_above_the_results(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    vertical_layout(cx, data.path());
    let (window, cx) = pinned_rows(cx, data.path(), &["sample_ts", "sample_rust"]);
    let label = cx.debug_bounds("section-Pinned").expect("the home's label");
    let first = cx.debug_bounds("slot-1").expect("the first pin's row");
    let second = cx.debug_bounds("slot-2").expect("the second pin's row");
    let commands = cx
        .debug_bounds("section-Commands")
        .expect("the rows' label");
    assert!(label.bottom() <= first.top(), "{label:?} {first:?}");
    assert!(first.bottom() <= second.top(), "{first:?} {second:?}");
    assert!(second.bottom() <= commands.top(), "{second:?} {commands:?}");
    assert_eq!(first.left(), second.left(), "one column");
    assert_eq!(first.size, second.size, "rows of one size");
    assert!(
        first.size.width > first.size.height * 4.,
        "a row, not a tile: {first:?}"
    );
    assert!(cx.debug_bounds("pinned-strip").is_none(), "no strip");
    assert!(cx.debug_bounds("pin-hint").is_none(), "no pin hint");
    // In order, each named as a pin.
    let nodes = accessible_nodes(cx);
    node(&nodes, "Button", "Pinned 1: Charlie");
    node(&nodes, "Button", "Pinned 2: Alpha");

    cx.simulate_input("al");
    settle(&window, cx);
    assert!(cx.debug_bounds("slot-1").is_none(), "a query hides them");
    assert!(cx.debug_bounds("section-Pinned").is_none());
    cx.simulate_keystrokes("escape");
    assert_eq!(settle(&window, cx).query(), Some(""));
    assert!(
        cx.debug_bounds("slot-1").is_some(),
        "clearing brings them back"
    );
}

/// With nothing pinned, the vertical layout shows nothing of the home: no
/// label, no rows and no pin hint, only the results.
#[gpui::test]
fn the_vertical_layout_shows_nothing_while_nothing_is_pinned(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    vertical_layout(cx, data.path());
    let (_window, cx) = pinned_rows(cx, data.path(), &[]);
    assert!(cx.debug_bounds("section-Pinned").is_none(), "no label");
    assert!(cx.debug_bounds("slot-1").is_none(), "no pin");
    assert!(cx.debug_bounds("pin-hint").is_none(), "no pin hint");
    assert!(cx.debug_bounds("pinned-strip").is_none(), "no strip");
    assert!(
        cx.debug_bounds("section-Commands").is_some(),
        "the results show"
    );
}

/// The Actions panel pins the selected result after the last pin and
/// records it; the slot then shows the result with its chord, Ctrl+1,
/// which opens it — once, one screen deep.
#[gpui::test]
fn pinning_from_the_actions_panel_adds_a_slot_whose_chord_opens_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &[]);
    cx.simulate_keystrokes("down");
    settle(&window, cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.debug_bounds("action-Pin").is_some());
    cx.simulate_input("pin");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(view.status, Status::Result("Pinned Bravo".into()));
    assert!(query_has_focus(&window, cx), "focus is back in the query");
    assert_eq!(slot_titles(&window, cx), ["Bravo"]);
    assert!(data.path().join("quick-slots.json").exists());
    let nodes = accessible_nodes(cx);
    let pinned = node(&nodes, "Button", "Pinned 1: Bravo");
    assert_eq!(
        pinned["keyboard_shortcut"],
        format!("{CTRL}+1"),
        "{pinned:#}"
    );

    cx.simulate_keystrokes("ctrl-1");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "JavaScript sample")
    );
    cx.simulate_keystrokes("escape");
    assert!(
        matches!(settle(&window, cx).screen, Screen::Root { .. }),
        "it opened once"
    );
}

/// A click on a slot runs what it holds; a click on the pin hint runs
/// nothing and leaves the query focused, so typing still searches.
#[gpui::test]
fn a_click_on_a_slot_opens_what_it_holds_and_one_on_the_hint_runs_nothing(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &["sample_ts"]);
    assert_eq!(slot_titles(&window, cx), ["Charlie"]);
    let hint = center_of(cx, "pin-hint");
    cx.simulate_click(hint, Modifiers::none());
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "the pin hint runs nothing"
    );
    assert!(
        query_has_focus(&window, cx),
        "typing still reaches the query"
    );

    let charlie = center_of(cx, "slot-1");
    cx.simulate_click(charlie, Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "TypeScript sample")
    );
}

/// A slot's chord runs nothing while the Actions panel has the keys, or
/// while an input method composes in the query; with neither, it opens
/// what the slot holds.
#[gpui::test]
fn a_slots_chord_runs_nothing_while_an_overlay_or_a_composition_has_the_keys(
    cx: &mut TestAppContext,
) {
    use gpui::EntityInputHandler;

    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &["sample_rust"]);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-1");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "{view:?}");
    assert!(actions_open(&window, cx));
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    let input = cx.read_entity(&window, |window, _| window.query_field());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "に", None, window, cx);
        })
    });
    cx.simulate_keystrokes("ctrl-1");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "{view:?}");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "", window, cx);
        })
    });
    settle(&window, cx);

    cx.simulate_keystrokes("ctrl-1");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
}

/// A slot whose target is gone keeps its place and says why, on a click
/// or its chord, running nothing; its own actions, from a secondary click,
/// unpin it — the strip's moves saying left and right.
#[gpui::test]
fn a_slot_whose_target_is_gone_says_why_and_its_own_actions_unpin_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &["local:/nowhere#gone"]);
    cx.simulate_keystrokes("ctrl-1");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(
        view.status,
        Status::Error("Its extension is not installed".into())
    );

    let slot = center_of(cx, "slot-1");
    cx.simulate_mouse_down(slot, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(slot, MouseButton::Right, Modifiers::none());
    settle(&window, cx);
    assert!(actions_open(&window, cx), "the slot's own actions");
    assert!(cx.debug_bounds("action-group-Quick Slot").is_some());
    // The pins lie side by side on the strip: the moves say so.
    assert!(cx.debug_bounds("action-Move Left").is_some());
    assert!(cx.debug_bounds("action-Move Right").is_some());
    assert!(cx.debug_bounds("action-Move Up").is_none());
    // The secondary click leaves focus in the panel's field, not on the
    // slot it pressed: typing filters, and Enter runs the entry.
    assert!(actions_filter_has_focus(&window, cx));
    cx.simulate_input("unpin");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Unpinned gone".into()));
    assert!(slot_titles(&window, cx).is_empty());
    assert!(cx.debug_bounds("pin-hint").is_some(), "the hint is back");
}

/// Pins are not limited to five: with five pinned, Pin adds a sixth, which
/// starts the strip's second row, the pin hint beside it.
#[gpui::test]
fn pinning_past_five_adds_a_pin_on_a_new_row(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let five = ["one", "two", "three", "four", "five"];
    let (window, cx) = pinned_rows(cx, data.path(), &five);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("pin");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(!actions_open(&window, cx));
    assert_eq!(view.status, Status::Result("Pinned Alpha".into()));
    assert_eq!(
        slot_titles(&window, cx),
        ["one", "two", "three", "four", "five", "Alpha"]
    );
    let first = cx.debug_bounds("slot-1").expect("the first pin");
    let sixth = cx.debug_bounds("slot-6").expect("the sixth pin");
    assert_eq!(sixth.left(), first.left());
    assert_eq!(sixth.top(), first.bottom() + px(8.));
    let hint = cx.debug_bounds("pin-hint").expect("the pin hint");
    assert_eq!(hint.top(), sixth.top());
}

/// A pinned result's own actions offer Unpin rather than Pin, which takes
/// its slot out — the pins after it closing the gap — and keeps the
/// search as it was.
#[gpui::test]
fn a_pinned_results_actions_unpin_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &["sample_rust", "sample_ts"]);
    cx.simulate_input("alpha");
    let view = settle(&window, cx);
    assert_eq!(view.rows[view.selected.unwrap()].title, "Alpha");
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.debug_bounds("action-Unpin").is_some());
    assert!(cx.debug_bounds("action-Pin").is_none());
    cx.simulate_input("unpin");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Unpinned Alpha".into()));
    assert_eq!(view.query(), Some("alpha"), "the search stays");
    assert_eq!(slot_titles(&window, cx), ["Charlie"]);
}

/// A slot runs once per press: a held chord's repeats run nothing, and
/// neither does a double click's second click; Enter on the focused slot
/// opens it. Tab passes from the pins to the footer: the pin hint is no
/// tab stop.
#[gpui::test]
fn a_slot_runs_once_per_press_and_the_pin_hint_takes_no_focus(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &["sample_rust"]);
    // From the query, Tab reaches the pin, then the footer's menu.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Alpha"));
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pane menu"));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Alpha"));

    // The system's repeat of a chord held from before is not a press.
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("ctrl-1").unwrap(),
        is_held: true,
        prefer_character_input: false,
    });
    assert!(
        matches!(settle(&window, cx).screen, Screen::Root { .. }),
        "a repeat runs nothing"
    );

    // A double click's second click.
    let slot = center_of(cx, "slot-1");
    cx.simulate_event(gpui::MouseDownEvent {
        position: slot,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: slot,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
    });
    assert!(
        matches!(settle(&window, cx).screen, Screen::Root { .. }),
        "a second click runs nothing"
    );

    // A press is a press: Enter on the focused pin opens it.
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Alpha"));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
}

/// A record that cannot be written puts back the arrangement it holds and
/// says why on the status line.
#[gpui::test]
fn a_pin_that_cannot_be_recorded_is_put_back_and_reported(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &[]);
    // Something that is not a file stands where the record goes.
    std::fs::create_dir(data.path().join("quick-slots.json")).unwrap();
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("pin");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(
        matches!(&view.status, Status::Error(problem)
            if problem.starts_with("Could not keep the quick slots")),
        "{:?}",
        view.status
    );
    assert!(cx.debug_bounds("status-error").is_some());
    assert!(slot_titles(&window, cx).is_empty());
}

/// Seven pins wrap onto a second row of the strip's five columns: the
/// sixth under the first, the pin hint after the seventh, the strip as
/// tall as its two rows.
#[gpui::test]
fn seven_pins_wrap_onto_a_second_row_of_the_strip(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &SEVEN_PINS);
    assert_eq!(slot_titles(&window, cx), SEVEN_PINS);
    let strip = cx.debug_bounds("pinned-strip").expect("the strip");
    let first = cx.debug_bounds("slot-1").expect("the first pin");
    let fifth = cx.debug_bounds("slot-5").expect("the fifth pin");
    let sixth = cx.debug_bounds("slot-6").expect("the sixth pin");
    let seventh = cx.debug_bounds("slot-7").expect("the seventh pin");
    assert_eq!(fifth.top(), first.top(), "five to the first row");
    assert!(fifth.left() > first.left());
    assert_eq!(sixth.left(), first.left(), "the sixth starts the next row");
    assert_eq!(sixth.top(), first.bottom() + px(8.), "the columns' gap");
    assert_eq!(seventh.top(), sixth.top());
    assert_eq!(strip.size.height, px(2. + 80. + 8. + 80. + 6.));
    let hint = cx.debug_bounds("pin-hint").expect("the pin hint");
    assert_eq!(hint.top(), sixth.top());
    assert!(hint.left() > seventh.right());
}

/// With seven pins, Ctrl+1 to Ctrl+5 are the first five and Ctrl+6 picks
/// the first result row; the sixth and seventh pins have no number.
#[gpui::test]
fn with_seven_pins_ctrl_6_picks_the_first_row_and_the_last_pins_have_no_number(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &SEVEN_PINS);
    let nodes = accessible_nodes(cx);
    let fifth = node(&nodes, "Button", "Pinned 5: five");
    assert_eq!(fifth["keyboard_shortcut"], format!("{CTRL}+5"), "{fifth:#}");
    for label in ["Pinned 6: six", "Pinned 7: seven"] {
        let pin = node(&nodes, "Button", label);
        assert!(pin.get("keyboard_shortcut").is_none(), "{pin:#}");
    }

    cx.simulate_keystrokes("ctrl-6");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample"),
        "Ctrl+6 opens the first row, Alpha"
    );
}

/// The pin key pins root search's selected result and, pressed again on
/// the same result, unpins it; focus stays in the query throughout.
#[gpui::test]
fn the_pin_key_pins_the_selected_result_and_unpins_it_again(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &[]);
    cx.simulate_keystrokes("down");
    settle(&window, cx);
    cx.simulate_keystrokes(TOGGLE_PIN);
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Pinned Bravo".into()));
    assert_eq!(slot_titles(&window, cx), ["Bravo"]);
    assert!(query_has_focus(&window, cx));
    assert!(data.path().join("quick-slots.json").exists());

    cx.simulate_keystrokes(TOGGLE_PIN);
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Unpinned Bravo".into()));
    assert!(slot_titles(&window, cx).is_empty());
    assert!(query_has_focus(&window, cx));
}

/// The move keys move the focused slot one place, focus following it to
/// its new place; a move past the first place does nothing. The pin key
/// on a focused slot unpins it, focus going to the slot that took its
/// place.
#[gpui::test]
fn the_move_keys_move_the_focused_slot_and_focus_follows_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_rows(cx, data.path(), &["one", "two", "three"]);
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: one"));

    cx.simulate_keystrokes(MOVE_PIN_DOWN);
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Moved one to place 2".into()));
    assert_eq!(slot_titles(&window, cx), ["two", "one", "three"]);
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 2: one"));

    cx.simulate_keystrokes(MOVE_PIN_UP);
    settle(&window, cx);
    assert_eq!(slot_titles(&window, cx), ["one", "two", "three"]);
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: one"));
    // Already first: nothing moves.
    cx.simulate_keystrokes(MOVE_PIN_UP);
    settle(&window, cx);
    assert_eq!(slot_titles(&window, cx), ["one", "two", "three"]);
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: one"));

    cx.simulate_keystrokes(TOGGLE_PIN);
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Unpinned one".into()));
    assert_eq!(slot_titles(&window, cx), ["two", "three"]);
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: two"));
}

/// The pin hint follows three pins, in the fourth column of the strip's
/// row, a slot's size.
#[gpui::test]
fn the_pin_hint_follows_three_pins(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_window, cx) = pinned_rows(cx, data.path(), &["one", "two", "three"]);
    let third = cx.debug_bounds("slot-3").expect("the third pin");
    let hint = cx.debug_bounds("pin-hint").expect("the pin hint");
    assert_eq!(hint.top(), third.top());
    assert_eq!(hint.size.height, third.size.height);
    // The columns are fractions of the list's width: within a pixel.
    assert!((hint.size.width - third.size.width).abs() < px(1.));
    assert!((hint.left() - (third.right() + px(8.))).abs() < px(1.));
}

/// Five pins fill the strip's row, and the pin hint never adds a row of
/// its own: it does not show.
#[gpui::test]
fn five_pins_leave_no_room_for_the_pin_hint(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let five = ["one", "two", "three", "four", "five"];
    let (_window, cx) = pinned_rows(cx, data.path(), &five);
    assert!(cx.debug_bounds("slot-5").is_some());
    assert!(cx.debug_bounds("pin-hint").is_none());
    let strip = cx.debug_bounds("pinned-strip").expect("the strip");
    assert_eq!(strip.size.height, px(2. + 80. + 6.));
}
