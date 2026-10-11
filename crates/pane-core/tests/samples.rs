//! Contract checks every sample command passes alike, whether it is written
//! in Rust, JavaScript or TypeScript: the same items, answers and errors
//! through the launcher's public interface, a native WASI 0.3 async wait,
//! fresh state per instance, a form the guest validates, a color picker the
//! guest draws as the canvas of its designed view and changes on keys and
//! pointer input, a root result computed from the query, WASI 0.3-only
//! imports, and memory within the cap Pane puts on each guest.
//!
//! Components come from `cargo xtask guests`; the JavaScript and TypeScript
//! ones are the prebuilt components in `guests/prebuilt/`.

use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{
    CallError, Canvas, CanvasOp, CanvasRole, CommandRegistration, DesignedEvent, DesignedHandler,
    FormValue, GUEST_MEMORY, Launcher, Node, NodeKind, Paint, Runtime, Screen, Status, Unavailable,
};
use wasmtime::component::Component;
use wasmtime::{Config, Engine};

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/settle.rs"]
mod settle;

use feedback::shown;
use guests::guest;
use settle::wait_for_merges;

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

/// (item id, title) of every sample, in order.
const ITEMS: [(&str, &str); 8] = [
    ("greet", "Say hello"),
    ("wait", "Wait briefly"),
    ("validate", "Validate settings"),
    ("random", "Roll a number"),
    ("form", "Greet someone"),
    ("color", "Choose a color"),
    ("windows-only", "Windows-only action"),
    ("not-windows", "macOS and Linux action"),
];

impl Sample {
    fn path(&self) -> PathBuf {
        guest(self.component)
    }

    /// A launcher with this sample's command opened.
    fn open(&self) -> Launcher {
        let launcher = Launcher::new(
            Runtime::start(),
            vec![CommandRegistration {
                id: self.component.into(),
                title: format!("{} sample", self.language),
                subtitle: None,
                component: self.path(),
                takes_query: false,
                keywords: Vec::new(),
                when: pane_core::CommandWhen::Always,
                matches: pane_core::CommandMatches::Title,
            }],
        );
        // The blank query lists the command and Pane's own rows in the
        // no-query order (#199): "Settings…" comes before "TypeScript
        // sample", so the sample's row is chosen by its title.
        let title = format!("{} sample", self.language);
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == title)
            .unwrap_or_else(|| panic!("{title} is not listed"));
        launcher.select(index);
        block_on(launcher.activate_selected());
        assert_eq!(launcher.view().screen, Screen::Command);
        launcher
    }

    /// Runs the item `id` in an opened launcher and returns what it showed:
    /// its toast, or the status line.
    fn run(&self, launcher: &Launcher, id: &str) -> Status {
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id == id)
            .unwrap_or_else(|| panic!("the {} sample has no {id} item", self.language));
        launcher.select(index);
        block_on(launcher.activate_selected());
        shown(launcher)
    }

    /// A launcher with this sample's form command opened (#241: the
    /// "Greet someone" command of the same package, a designed view, like
    /// the color command's — only an installed command can be launched).
    fn open_form(&self) -> Launcher {
        let data = tempfile::tempdir().unwrap();
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(self.component.replace('_', "-"));
        block_on(launcher.install_package(&folder));
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        block_on(launcher.set_query("greet someone"));
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id.ends_with("#form"))
            .expect("the form command is listed");
        launcher.select(index);
        block_on(launcher.activate_selected());
        assert!(
            matches!(launcher.view().screen, Screen::DesignedView(_)),
            "{:?}",
            launcher.view().screen
        );
        launcher
    }

    /// Fills in the form's name and greeting, submits it and returns what
    /// the view draws: its answer text, or the field's error.
    fn submit(&self, launcher: &Launcher, name: &str, greeting: &str) -> Status {
        let submitting = launcher.submit_designed_form(
            None,
            vec![
                ("name".to_owned(), FormValue::Text(name.to_owned())),
                ("greeting".to_owned(), FormValue::Text(greeting.to_owned())),
            ],
        );
        block_on(submitting);
        launcher.view().status
    }

    /// The open form view's field keyed `key`: its placeholder, error and
    /// title, as the tree's props name them (#241: a field's title is over
    /// its control, its error under it, its placeholder inside it — none
    /// a text node).
    fn field(
        &self,
        launcher: &Launcher,
        key: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let view = launcher.view();
        let Screen::DesignedView(shown) = &view.screen else {
            panic!("no form view: {:?}", view.screen)
        };
        fn at(
            node: &pane_core::Node,
            key: &str,
            into: &mut Option<(Option<String>, Option<String>, Option<String>)>,
        ) {
            let field = match &node.kind {
                pane_core::NodeKind::TextInput(input) => Some((
                    input.placeholder.clone(),
                    input.field.error.clone(),
                    input.field.title.clone(),
                )),
                _ => None,
            };
            if node.key.as_deref() == Some(key) {
                if let Some(field) = field {
                    *into = Some(field);
                }
                return;
            }
            for child in &node.children {
                at(child, key, into);
            }
        }
        let mut found = None;
        at(&shown.tree.root, key, &mut found);
        found.unwrap_or_else(|| panic!("no field {key} in the form"))
    }

    /// The texts the open form view's tree holds.
    fn form_texts(&self, launcher: &Launcher) -> Vec<String> {
        match &launcher.view().screen {
            Screen::DesignedView(view) => {
                let mut texts = Vec::new();
                collect_texts(&view.tree.root, &mut texts);
                texts
            }
            other => panic!("no form view: {other:?}"),
        }
    }

    /// A launcher with this sample's package installed (the color
    /// command's view is a designed one, and only an installed command can
    /// be launched by an item of the package's list), its color command
    /// opened. Text is measured by a stub, so the tree's measured text is
    /// the same in every language and every run.
    fn open_color(&self) -> Launcher {
        let data = tempfile::tempdir().unwrap();
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
        launcher.set_text_measures(std::sync::Arc::new(|_, _| (42., 17.)));
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(self.component.replace('_', "-"));
        block_on(launcher.install_package(&folder));
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        block_on(launcher.set_query("color picker"));
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id.ends_with("#color"))
            .expect("the color command is listed");
        launcher.select(index);
        block_on(launcher.activate_selected());
        assert!(
            matches!(launcher.view().screen, Screen::DesignedView(_)),
            "{:?}",
            launcher.view().screen
        );
        launcher
    }

    /// The number the random item's toast shows, in a runtime of its own.
    fn fresh_random(&self) -> f64 {
        let launcher = self.open();
        let value: f64 = match self.run(&launcher, "random") {
            Status::Result(text) => text.parse().expect("the toast shows a number"),
            other => panic!("the random item shows no number: {other:?}"),
        };
        assert!((0.0..1.0).contains(&value), "{value} is not in [0, 1)");
        value
    }
}

/// Every text a tree holds, plain and in spans.
fn collect_texts(node: &pane_core::Node, into: &mut Vec<String>) {
    if let pane_core::NodeKind::Text(text) = &node.kind {
        match &text.content {
            pane_core::TextContent::Plain(text) => into.push(text.clone()),
            pane_core::TextContent::Spans(spans) => {
                into.extend(spans.iter().map(|span| span.text.clone()));
            }
        }
    }
    for child in &node.children {
        collect_texts(child, into);
    }
}

/// The nodes a tree holds, breadth-first.
fn nodes_of(tree: &pane_core::DesignedTree) -> Vec<&pane_core::Node> {
    fn at<'a>(node: &'a pane_core::Node, into: &mut Vec<&'a pane_core::Node>) {
        into.push(node);
        for child in &node.children {
            at(child, into);
        }
    }
    let mut nodes = Vec::new();
    at(&tree.root, &mut nodes);
    nodes
}

fn opening_shows_the_samples_items(sample: &Sample) {
    let view = sample.open().view();

    assert_eq!(view.title, format!("{} sample", sample.language));
    let items: Vec<(&str, &str)> = view
        .rows
        .iter()
        .map(|row| (row.id.as_str(), row.title.as_str()))
        .collect();
    assert_eq!(items, ITEMS);
    assert_eq!(view.selected, Some(0));
}

fn greeting_shows_the_guests_answer(sample: &Sample) {
    let launcher = sample.open();

    assert_eq!(
        sample.run(&launcher, "greet"),
        Status::Result(format!("Hello from the {} guest", sample.language))
    );
}

fn an_async_wasi_wait_shows_running_until_it_answers(sample: &Sample) {
    let launcher = sample.open();
    launcher.select(1);

    let pending = launcher.activate_selected();
    assert!(matches!(launcher.view().status, Status::Running { .. }));
    block_on(pending);

    assert_eq!(
        shown(&launcher),
        Status::Result(format!("Waited 50 ms inside the {} guest", sample.language))
    );
}

fn a_validation_failure_is_shown_as_an_error(sample: &Sample) {
    let launcher = sample.open();

    assert_eq!(
        sample.run(&launcher, "validate"),
        Status::Error(
            "The extension reported an error: Invalid settings: port must be between 1 and 65535"
                .into()
        )
    );
    // The command keeps working after the error.
    assert!(matches!(sample.run(&launcher, "greet"), Status::Result(_)));
}

/// A callback the sample's list does not name is the guest's error, the
/// same in each SDK; asking to run an item the list lacks is one too.
fn an_unknown_action_is_a_guest_error(sample: &Sample) {
    let runtime = Runtime::start().unwrap();

    let answer = block_on(runtime.handle_event(&sample.path(), "missing", "{}"));

    assert_eq!(
        answer,
        Err(CallError::Guest("unknown action: missing".into()))
    );
    assert_eq!(
        block_on(runtime.run_item(&sample.path(), "missing")),
        Err(CallError::Guest("unknown item: missing".into()))
    );
}

fn separately_started_runtimes_roll_different_numbers(sample: &Sample) {
    // A snapshot that froze its random state would repeat the same number in
    // every fresh instance.
    assert_ne!(sample.fresh_random(), sample.fresh_random());
}

fn one_instance_rolls_a_new_number_each_time(sample: &Sample) {
    let launcher = sample.open();

    let first = sample.run(&launcher, "random");
    let second = sample.run(&launcher, "random");

    assert!(matches!(first, Status::Result(_)), "{first:?}");
    assert_ne!(first, second);
}

fn the_component_imports_only_wasi_0_3(sample: &Sample) {
    let imports = imports(&sample.path());

    assert!(!imports.is_empty());
    // Besides WASI 0.3, only Pane's own interfaces: a JS/TS component lists
    // `pane:extension/settings` whether or not it uses it.
    let other: Vec<&String> = imports
        .iter()
        .filter(|name| !(name.starts_with("wasi:") && name.contains("@0.3.")))
        .filter(|name| !name.starts_with("pane:extension/"))
        .collect();
    assert!(other.is_empty(), "non-WASI 0.3 imports: {other:?}");
}

fn opening_the_form_shows_its_fields(sample: &Sample) {
    let launcher = sample.open_form();
    let view = launcher.view();

    // The form view's title and its form node: every field kind the
    // designed tree offers (#241).
    assert_eq!(view.title, "Greet someone");
    let Screen::DesignedView(shown) = &view.screen else {
        panic!("{:?}", view.screen);
    };
    let fields: Vec<&str> = nodes_of(&shown.tree)
        .into_iter()
        .filter(|node| matches!(&node.kind, pane_core::NodeKind::Form(_)))
        .flat_map(|form| form.children.iter())
        .filter(|node| node.key.is_some())
        .map(|node| match &node.kind {
            pane_core::NodeKind::TextInput(_) => "text-input",
            pane_core::NodeKind::PasswordInput(_) => "password-input",
            pane_core::NodeKind::TextArea(_) => "text-area",
            pane_core::NodeKind::DatePicker(_) => "date-picker",
            pane_core::NodeKind::DateTimePicker(_) => "date-time-picker",
            pane_core::NodeKind::Select(_) => "select",
            pane_core::NodeKind::TagPicker(_) => "tag-picker",
            pane_core::NodeKind::FilePicker(_) => "file-picker",
            pane_core::NodeKind::FolderPicker(_) => "folder-picker",
            pane_core::NodeKind::Checkbox(_) => "checkbox",
            pane_core::NodeKind::Toggle(_) => "toggle",
            _ => "other",
        })
        .collect();
    assert_eq!(
        fields,
        [
            "text-input",
            "password-input",
            "text-area",
            "date-picker",
            "date-time-picker",
            "select",
            "tag-picker",
            "file-picker",
            "folder-picker",
            "checkbox",
            "toggle",
        ]
    );
    // The name's placeholder and title are its field's props, drawn
    // around its control (#241), not texts the tree holds.
    let (placeholder, _, title) = sample.field(&launcher, "name");
    assert_eq!(placeholder.as_deref(), Some("Ada Lovelace"));
    assert_eq!(title.as_deref(), Some("Name"));
    let _ = view;
}

fn a_valid_form_shows_the_guests_answer(sample: &Sample) {
    let launcher = sample.open_form();

    sample.submit(&launcher, "Ada", "morning");
    assert!(
        sample.form_texts(&launcher).contains(&format!(
            "Good morning, Ada, from the {} guest",
            sample.language
        )),
        "{:?}",
        sample.form_texts(&launcher)
    );
    assert!(
        matches!(launcher.view().screen, Screen::DesignedView(_)),
        "{:?}",
        launcher.view().screen
    );
}

fn an_invalid_field_is_marked_and_the_form_stays_open(sample: &Sample) {
    let launcher = sample.open_form();

    sample.submit(&launcher, "   ", "welcome");

    // The field's error, drawn under it (#241); the form view stays open.
    let (_, error, _) = sample.field(&launcher, "name");
    assert_eq!(error.as_deref(), Some("Enter a name"));
    assert!(
        matches!(launcher.view().screen, Screen::DesignedView(_)),
        "{:?}",
        launcher.view().screen
    );
    // A corrected form is accepted.
    sample.submit(&launcher, "Grace", "welcome");
    assert!(
        sample.form_texts(&launcher).contains(&format!(
            "Welcome, Grace, from the {} guest",
            sample.language
        )),
        "{:?}",
        sample.form_texts(&launcher)
    );
}

fn a_too_long_name_is_rejected_by_the_guest(sample: &Sample) {
    let launcher = sample.open_form();

    sample.submit(&launcher, &"x".repeat(41), "hello");

    let (_, error, _) = sample.field(&launcher, "name");
    assert_eq!(error.as_deref(), Some("Use at most 40 characters"));
}

/// Each sample declares one action for Windows only and one for macOS and
/// Linux only. On the system the test runs on, Pane runs the one declared
/// for it and explains the other without calling the guest, and every other
/// action keeps working.
fn a_platform_limited_action_runs_only_on_its_declared_systems(sample: &Sample) {
    let launcher = sample.open();
    let reason = |id: &str| {
        let view = launcher.view();
        let row = view.rows.into_iter().find(|row| row.id == id);
        row.expect("the item is listed").unavailable
    };
    let ran =
        |title: &str| Status::Result(format!("Ran the {title} in the {} guest", sample.language));
    let (available, (unavailable, _), explanation) = platforms::sample_items();

    assert_eq!(reason(available.0), None);
    assert_eq!(
        reason(unavailable),
        Some(Unavailable::OnThisSystem(explanation.clone()))
    );
    assert_eq!(sample.run(&launcher, available.0), ran(available.1));
    assert_eq!(
        sample.run(&launcher, unavailable),
        Status::Error(explanation)
    );
    assert_eq!(launcher.view().screen, Screen::Command);
    assert_eq!(
        sample.run(&launcher, "greet"),
        Status::Result(format!("Hello from the {} guest", sample.language))
    );
}

/// The open color view's canvas: what its node says (its key and its key
/// handler) and what it holds, with the render it was drawn from.
struct DrawnCanvas {
    node_key: String,
    on_key: Option<u32>,
    canvas: Canvas,
}

/// The canvas `tree` draws, when it draws one.
fn canvas_of_tree(tree: &pane_core::DesignedTree) -> DrawnCanvas {
    fn of(node: &Node) -> Option<DrawnCanvas> {
        match &node.kind {
            NodeKind::Canvas(canvas) => Some(DrawnCanvas {
                node_key: node.key.clone().unwrap_or_default(),
                on_key: node.on_key,
                canvas: canvas.clone(),
            }),
            _ => node.children.iter().find_map(of),
        }
    }
    of(&tree.root).expect("the view draws a canvas")
}

/// The open color view's canvas.
fn canvas(launcher: &Launcher) -> Canvas {
    let view = launcher.view();
    let Screen::DesignedView(view) = &view.screen else {
        panic!("a designed view is open, not {:?}", view.screen)
    };
    canvas_of_tree(&view.tree).canvas
}

/// The open color view's value: the chosen color's name and hex code, what
/// its canvas says for assistive technology.
fn color(launcher: &Launcher) -> String {
    canvas(launcher)
        .a11y
        .value
        .clone()
        .expect("the canvas names its value")
}

/// Sends one event to the open color view's canvas — the handler of
/// `kind`, by the callback id its tree named — and returns the color it
/// then shows.
fn send(
    launcher: &Launcher,
    kind: DesignedHandler,
    read: impl FnOnce(&DrawnCanvas) -> Option<u32>,
    payload: &str,
) -> String {
    let view = launcher.view();
    let Screen::DesignedView(view) = &view.screen else {
        panic!("a designed view is open, not {:?}", view.screen)
    };
    let drawn = canvas_of_tree(&view.tree);
    let callback = read(&drawn).expect("the canvas names the handler");
    block_on(launcher.send_designed_seen(
        kind,
        callback,
        (!drawn.node_key.is_empty()).then_some(drawn.node_key.as_str()),
        Some(view.render),
        payload.to_owned(),
    ));
    assert_eq!(launcher.view().status, Status::Idle);
    color(launcher)
}

/// A key event for the open color view's canvas, pressed as `key` spells
/// it.
fn key(key: &str) -> String {
    format!("{{\"key\":\"{key}\"}}")
}

/// The operation a swatch is: a square filled with `fill`.
fn square(x: f32, y: f32, size: f32, fill: Option<Paint>) -> CanvasOp {
    CanvasOp::Rect {
        x,
        y,
        width: size,
        height: size,
        radius: None,
        fill,
        stroke: None,
    }
}

/// A pointer event for the open color view's canvas, at `x`, `y`.
fn pointer(event: &str, x: i32, y: i32) -> String {
    format!("{{\"event\":\"{event}\",\"x\":{x},\"y\":{y}}}")
}

/// The canvas's key handler, which rides its node.
fn on_key(drawn: &DrawnCanvas) -> Option<u32> {
    drawn.on_key
}

fn opening_the_color_view_draws_the_picker(sample: &Sample) {
    let launcher = sample.open_color();

    assert_eq!(launcher.view().title, "Choose a color");
    let canvas = canvas(&launcher);
    assert_eq!(
        (canvas.a11y.role, canvas.a11y.label.as_deref()),
        (Some(CanvasRole::ColorWell), Some("Color"))
    );
    // The frame around the chosen swatch, 8 x 3 swatches, the preview and
    // its hex code: 27 drawing operations, the hex one measured.
    assert_eq!(canvas.ops.len(), 1 + 24 + 2);
    assert_eq!(
        canvas.ops[0],
        square(
            180.,
            36.,
            36.,
            Some(Paint {
                tint: pane_core::Tint::Same(pane_core::Color::Rgba(0xf1f3f5ff)),
                exact: false,
            })
        )
    );
    assert_eq!(
        canvas.ops[1],
        square(
            2.,
            2.,
            32.,
            Some(Paint {
                tint: pane_core::Tint::Same(pane_core::Color::Rgba(0xef9a9aff)),
                exact: true,
            })
        )
    );
    assert_eq!(
        canvas.ops[25],
        square(
            300.,
            2.,
            64.,
            Some(Paint {
                tint: pane_core::Tint::Same(pane_core::Color::Rgba(0x1e88e5ff)),
                exact: true,
            })
        )
    );
    // The hex code, measured: its text sits under the preview, as far
    // down as the stub measures it.
    assert_eq!(
        canvas.ops[26],
        CanvasOp::Text(pane_core::CanvasText {
            x: 300.,
            y: 85.,
            content: "#1E88E5".into(),
            style: Some(pane_core::TextStyle::Caption),
            level: None,
            color: Some(Paint {
                tint: pane_core::Tint::Same(pane_core::Color::Rgba(0xf1f3f5ff)),
                exact: false,
            }),
            size: None,
            weight: None,
        })
    );
}

fn keys_move_the_chosen_color(sample: &Sample) {
    let launcher = sample.open_color();
    let pressed = key;

    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("right")),
        "Purple, #8E24AA"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("down")),
        "Dark purple, #4A148C"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("home")),
        "Dark red, #B71C1C"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("left")),
        "Dark red, #B71C1C"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("end")),
        "Dark pink, #880E4F"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("up")),
        "Pink, #D81B60"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("up")),
        "Light pink, #F48FB1"
    );
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &pressed("up")),
        "Light pink, #F48FB1"
    );
    // The preview shows the chosen color too.
    assert_eq!(
        canvas(&launcher).ops[25],
        square(
            300.,
            2.,
            64.,
            Some(Paint {
                tint: pane_core::Tint::Same(pane_core::Color::Rgba(0xf48fb1ff)),
                exact: true,
            })
        )
    );
}

fn pressing_and_dragging_the_pointer_chooses_swatches(sample: &Sample) {
    let launcher = sample.open_color();
    let down = |drawn: &DrawnCanvas| drawn.canvas.handlers.on_pointer_down;
    let movement = |drawn: &DrawnCanvas| drawn.canvas.handlers.on_pointer_move;
    let up = |drawn: &DrawnCanvas| drawn.canvas.handlers.on_pointer_up;

    assert_eq!(
        send(
            &launcher,
            DesignedHandler::Pointer,
            down,
            &pointer("pointer-down", 10, 10)
        ),
        "Light red, #EF9A9A"
    );
    assert_eq!(
        send(
            &launcher,
            DesignedHandler::Pointer,
            movement,
            &pointer("pointer-move", 80, 80)
        ),
        "Dark yellow, #F57F17"
    );
    // A drag past the grid chooses the nearest swatch.
    assert_eq!(
        send(
            &launcher,
            DesignedHandler::Pointer,
            movement,
            &pointer("pointer-move", -50, 500)
        ),
        "Dark red, #B71C1C"
    );
    send(
        &launcher,
        DesignedHandler::Pointer,
        up,
        &pointer("pointer-up", -50, 500),
    );
    // A move without the drag chooses nothing.
    assert_eq!(
        send(
            &launcher,
            DesignedHandler::Pointer,
            movement,
            &pointer("pointer-move", 200, 40)
        ),
        "Dark red, #B71C1C"
    );
    // A press on the preview, outside the grid, chooses nothing.
    assert_eq!(
        send(
            &launcher,
            DesignedHandler::Pointer,
            down,
            &pointer("pointer-down", 330, 10)
        ),
        "Dark red, #B71C1C"
    );
    assert_eq!(
        send(
            &launcher,
            DesignedHandler::Pointer,
            movement,
            &pointer("pointer-move", 10, 10)
        ),
        "Dark red, #B71C1C"
    );
}

fn each_opened_color_view_starts_afresh(sample: &Sample) {
    let launcher = sample.open_color();
    assert_eq!(
        send(&launcher, DesignedHandler::Key, on_key, &key("right")),
        "Purple, #8E24AA"
    );

    // The root view's own back leaves the command, as leaving its list
    // does: root search afresh, its first row selected. The color command
    // is chosen again, as it was the first time.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    block_on(launcher.set_query("color picker"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id.ends_with("#color"))
        .expect("the color command is listed");
    launcher.select(index);
    block_on(launcher.activate_selected());

    assert_eq!(color(&launcher), "Blue, #1E88E5");
}

fn views_open_at_once_keep_their_own_state(sample: &Sample) {
    let runtime = Runtime::start().unwrap();
    let open = || {
        block_on(runtime.open_designed_view(&sample.path(), "color", &default_launch())).unwrap()
    };
    let ((first, rendered), (second, _)) = (open(), open());

    // The event each view is sent: a key pressed on its canvas, by the
    // callback id its own tree named.
    let pressed = |rendered: &pane_core::DesignedRendered, name: &str| {
        let drawn = canvas_of_tree(&rendered.tree);
        DesignedEvent {
            render: rendered.render,
            key: drawn.node_key,
            callback: drawn.on_key.expect("the canvas takes keys"),
            payload: key(name),
        }
    };
    // One moves right, the other left: each view's own state.
    let first_next = block_on(runtime.designed_view_event(first, pressed(&rendered, "right")));
    let second_next = block_on(runtime.designed_view_event(second, pressed(&rendered, "left")));

    assert_eq!(value_of(&first_next), "Purple, #8E24AA");
    assert_eq!(value_of(&second_next), "Teal, #00897B");
    assert_eq!(block_on(runtime.designed_view_count()), 2);
    runtime.close_designed_view(first);
    assert_eq!(block_on(runtime.designed_view_count()), 1);
    assert_eq!(
        block_on(runtime.designed_view_event(first, pressed(&rendered, "right"))),
        Err(CallError::ViewClosed)
    );
}

/// The launch record every view of these tests opens with: none of them
/// reads it.
fn default_launch() -> pane_core::LaunchRecord {
    pane_core::LaunchRecord::default()
}

/// The value of the canvas the answer `next` drew.
fn value_of(next: &Result<pane_core::DesignedNext, CallError>) -> String {
    let pane_core::DesignedNext::Tree(rendered) = next.as_ref().unwrap() else {
        panic!("the view re-rendered: {next:?}")
    };
    fn of(node: &Node) -> Option<String> {
        match &node.kind {
            NodeKind::Canvas(canvas) => canvas.a11y.value.clone(),
            _ => node.children.iter().find_map(of),
        }
    }
    of(&rendered.tree.root).expect("the view draws a canvas")
}

fn an_unknown_view_is_a_guest_error(sample: &Sample) {
    let runtime = Runtime::start().unwrap();

    let opened = block_on(runtime.open_designed_view(&sample.path(), "missing", &default_launch()));

    assert_eq!(
        opened.map(|(_, rendered)| rendered),
        Err(CallError::Guest("unknown designed view: missing".into()))
    );
    assert_eq!(block_on(runtime.designed_view_count()), 0);
}

fn reverse_typed_into_root_search_lists_the_reversed_text_to_copy(sample: &Sample) {
    // Root results come from installed packages: this sample's package.
    let data = tempfile::tempdir().unwrap();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(sample.component.replace('_', "-"));
    block_on(launcher.install_package(&folder));
    launcher.back();

    block_on(launcher.set_query("reverse Pané 1"));
    // The JavaScript and TypeScript guests answer past the 200 ms budget
    // while their engine starts: the answer merges into the published
    // list within 16 ms of the search's end (#201), which is waited out.
    wait_for_merges(&launcher);

    let view = launcher.view();
    assert_eq!(view.rows[0].title, "1 énaP");
    assert_eq!(
        view.rows[0].subtitle,
        Some(format!("Reversed by the {} guest", sample.language))
    );
    assert_eq!(launcher.selected_copy().as_deref(), Some("1 énaP"));
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Copied 1 énaP to the clipboard".into())
    );
    // Nothing to reverse is no result, not an error.
    block_on(launcher.set_query("reverse   "));
    assert_eq!(launcher.view().rows, []);
}

/// Records the links it is asked to open, so that no browser opens.
#[derive(Default)]
struct RecordedLinks(std::sync::Mutex<Vec<String>>);

impl pane_core::LinkOpener for RecordedLinks {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0.lock().unwrap().push(url.into());
        Ok(())
    }
}

fn a_root_result_opens_a_web_link(sample: &Sample) {
    let data = tempfile::tempdir().unwrap();
    let links = std::sync::Arc::new(RecordedLinks::default());
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_link_opener(links.clone());
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(sample.component.replace('_', "-"));
    block_on(launcher.install_package(&folder));
    launcher.back();

    block_on(launcher.set_query("pane website"));
    // As above: the answer merges within 16 ms of the search's end.
    wait_for_merges(&launcher);

    let view = launcher.view();
    assert_eq!(view.rows[0].title, "Pane's website");
    assert_eq!(
        view.rows[0].subtitle,
        Some(format!("Opened by the {} guest", sample.language))
    );
    assert_eq!(launcher.selected_copy(), None, "it copies nothing");
    block_on(launcher.activate_selected());
    assert_eq!(
        *links.0.lock().unwrap(),
        ["https://github.com/hoangvu12/pane"]
    );
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened https://github.com/hoangvu12/pane".into())
    );
}

/// The commands, forms, views and root results of the sample, one after
/// another in this process, stay under the cap on a guest's memory; the
/// largest memory its instances had is printed for a verify run's log
/// (`.config/nextest.toml` shows it on success).
fn the_guest_stays_under_the_memory_cap(sample: &Sample) {
    greeting_shows_the_guests_answer(sample);
    an_async_wasi_wait_shows_running_until_it_answers(sample);
    one_instance_rolls_a_new_number_each_time(sample);
    a_valid_form_shows_the_guests_answer(sample);
    keys_move_the_chosen_color(sample);
    pressing_and_dragging_the_pointer_chooses_swatches(sample);
    views_open_at_once_keep_their_own_state(sample);
    reverse_typed_into_root_search_lists_the_reversed_text_to_copy(sample);
    a_root_result_opens_a_web_link(sample);

    let peak =
        pane_core::memory_peak(&format!("{}.wasm", sample.component)).expect("the sample ran");
    println!(
        "memory peak of the {} sample through its contract: {:.1} MiB of {} MiB",
        sample.language,
        peak as f64 / (1024.0 * 1024.0),
        GUEST_MEMORY / (1024 * 1024)
    );
    assert!(peak <= GUEST_MEMORY, "{peak} bytes");
}

/// Declares one test per check for each sample.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    opening_shows_the_samples_items,
    greeting_shows_the_guests_answer,
    an_async_wasi_wait_shows_running_until_it_answers,
    a_validation_failure_is_shown_as_an_error,
    an_unknown_action_is_a_guest_error,
    separately_started_runtimes_roll_different_numbers,
    one_instance_rolls_a_new_number_each_time,
    the_component_imports_only_wasi_0_3,
    opening_the_form_shows_its_fields,
    a_valid_form_shows_the_guests_answer,
    an_invalid_field_is_marked_and_the_form_stays_open,
    a_too_long_name_is_rejected_by_the_guest,
    a_platform_limited_action_runs_only_on_its_declared_systems,
    opening_the_color_view_draws_the_picker,
    keys_move_the_chosen_color,
    pressing_and_dragging_the_pointer_chooses_swatches,
    each_opened_color_view_starts_afresh,
    views_open_at_once_keep_their_own_state,
    an_unknown_view_is_a_guest_error,
    reverse_typed_into_root_search_lists_the_reversed_text_to_copy,
    a_root_result_opens_a_web_link,
    the_guest_stays_under_the_memory_cap,
);

/// The names of the component's imports.
fn imports(path: &std::path::Path) -> Vec<String> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .wasm_component_model_async(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_file(&engine, path).unwrap();
    component
        .component_type()
        .imports(&engine)
        .map(|(name, _)| name.to_owned())
        .collect()
}

#[test]
fn the_import_check_detects_a_wasi_0_2_import() {
    // Negative control for `the_component_imports_only_wasi_0_3`.
    let imports = imports(&guest("mixed_p2"));

    assert!(
        imports
            .iter()
            .any(|name| name.starts_with("wasi:cli/stdout@0.2")),
        "{imports:?}"
    );
}
