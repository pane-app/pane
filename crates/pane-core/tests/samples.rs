//! Contract checks every sample command passes alike, whether it is written
//! in Rust, JavaScript or TypeScript: the same items, answers and errors
//! through the launcher's public interface, a native WASI 0.3 async wait,
//! fresh state per instance, a form the guest validates, a color picker the
//! guest draws and changes on keys and pointer input, a root result
//! computed from the query, WASI 0.3-only imports, and memory within the
//! cap Pane puts on each guest.
//!
//! Components come from `cargo xtask guests`; the JavaScript and TypeScript
//! ones are the prebuilt components in `guests/prebuilt/`.

use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{
    CallError, Choice, CommandRegistration, CustomViewRole, FieldKind, FieldValue, FormError,
    FormField, GUEST_MEMORY, Key, Launcher, Point, Rgb, Runtime, Screen, Shape, Status,
    Unavailable, ViewEvent,
};
use wasmtime::component::Component;
use wasmtime::{Config, Engine};

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;

use feedback::shown;
use guests::guest;

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
                search: false,
            }],
        );
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

    /// A launcher with this sample's form opened.
    fn open_form(&self) -> Launcher {
        let launcher = self.open();
        assert_eq!(self.run(&launcher, "form"), Status::Idle);
        assert!(
            matches!(launcher.view().screen, Screen::Form(_)),
            "{:?}",
            launcher.view().screen
        );
        launcher
    }

    /// Fills in the form's name and greeting, submits it and returns the status.
    fn submit(&self, launcher: &Launcher, name: &str, greeting: &str) -> Status {
        launcher.set_field_value("name", name);
        launcher.set_field_value("greeting", greeting);
        block_on(launcher.submit_form());
        launcher.view().status
    }

    /// A launcher with this sample's color picker opened.
    fn open_color(&self) -> Launcher {
        let launcher = self.open();
        assert_eq!(self.run(&launcher, "color"), Status::Idle);
        assert!(
            matches!(launcher.view().screen, Screen::CustomView(_)),
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
    assert_eq!(launcher.view().status, Status::Running);
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
    let view = sample.open_form().view();

    assert_eq!(view.title, "Greet someone");
    let form = view.form().expect("a form");
    assert_eq!(form.submit_label, "Greet");
    let choice = |id: &str, label: &str| Choice {
        id: id.into(),
        label: label.into(),
    };
    assert_eq!(
        form.fields,
        [
            FormField {
                id: "name".into(),
                label: "Name".into(),
                kind: FieldKind::Text {
                    placeholder: Some("Ada Lovelace".into())
                },
                value: String::new(),
                error: None,
            },
            FormField {
                id: "greeting".into(),
                label: "Greeting".into(),
                kind: FieldKind::Choice(vec![
                    choice("hello", "Hello"),
                    choice("morning", "Good morning"),
                    choice("welcome", "Welcome"),
                ]),
                value: "hello".into(),
                error: None,
            },
        ]
    );
}

fn a_valid_form_shows_the_guests_answer(sample: &Sample) {
    let launcher = sample.open_form();

    assert_eq!(
        sample.submit(&launcher, "Ada", "morning"),
        Status::Result(format!(
            "Good morning, Ada, from the {} guest",
            sample.language
        ))
    );
    assert!(
        matches!(launcher.view().screen, Screen::Form(_)),
        "{:?}",
        launcher.view().screen
    );
}

fn an_invalid_field_is_marked_and_the_form_stays_open(sample: &Sample) {
    let launcher = sample.open_form();

    let status = sample.submit(&launcher, "   ", "welcome");

    assert_eq!(status, Status::Error("Name: Enter a name".into()));
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    let fields = &view.form().unwrap().fields;
    assert_eq!(fields[0].error.as_deref(), Some("Enter a name"));
    assert_eq!(fields[1].error, None);
    // The values survive the rejection, and a corrected form is accepted.
    assert_eq!(
        (fields[0].value.as_str(), fields[1].value.as_str()),
        ("   ", "welcome")
    );
    assert_eq!(
        sample.submit(&launcher, "Grace", "welcome"),
        Status::Result(format!(
            "Welcome, Grace, from the {} guest",
            sample.language
        ))
    );
}

fn a_too_long_name_is_rejected_by_the_guest(sample: &Sample) {
    let launcher = sample.open_form();

    let status = sample.submit(&launcher, &"x".repeat(41), "hello");

    assert_eq!(
        status,
        Status::Error("Name: Use at most 40 characters".into())
    );
}

fn an_unknown_choice_is_a_field_error_from_the_guest(sample: &Sample) {
    // The launcher only submits offered choices, so call the guest directly.
    let runtime = Runtime::start().unwrap();
    let values = [("name", "Ada"), ("greeting", "howdy")]
        .map(|(id, value)| FieldValue {
            id: id.into(),
            value: value.into(),
        })
        .to_vec();

    let answer = block_on(runtime.submit_form(&sample.path(), "form", values));

    assert_eq!(
        answer,
        Err(CallError::Form(FormError {
            field: Some("greeting".into()),
            message: "Choose a greeting".into(),
        }))
    );
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

/// The open color picker's value: the chosen color's name and hex code.
fn color(launcher: &Launcher) -> String {
    let view = launcher.view();
    view.custom_view()
        .expect("a view is open")
        .frame
        .value
        .clone()
}

/// Sends `event` to the open view and returns the color it then shows.
fn send(launcher: &Launcher, event: ViewEvent) -> String {
    block_on(launcher.send_view_event(event));
    assert_eq!(launcher.view().status, Status::Idle);
    color(launcher)
}

fn press(launcher: &Launcher, key: Key) -> String {
    send(launcher, ViewEvent::Key(key))
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn opening_the_color_view_draws_the_picker(sample: &Sample) {
    let view = sample.open_color().view();

    assert_eq!(view.title, "Choose a color");
    let custom = view.custom_view().expect("a view is open");
    assert_eq!(
        (custom.label.as_str(), custom.role),
        ("Color", CustomViewRole::ColorWell)
    );
    let frame = &custom.frame;
    assert_eq!(
        (frame.width, frame.height, frame.value.as_str()),
        (376, 108, "Blue, #1E88E5")
    );
    let rect = |x, y, size, fill| Shape::Rect {
        x,
        y,
        width: size,
        height: size,
        fill: Rgb(fill),
    };
    // The frame around the chosen swatch, 8 x 3 swatches, the preview and
    // its hex code.
    assert_eq!(frame.shapes.len(), 1 + 24 + 2);
    assert_eq!(frame.shapes[0], rect(180, 36, 36, 0xf1f3f5));
    assert_eq!(frame.shapes[1], rect(2, 2, 32, 0xef9a9a));
    assert_eq!(frame.shapes[24], rect(254, 74, 32, 0x880e4f));
    assert_eq!(frame.shapes[25], rect(300, 2, 64, 0x1e88e5));
    assert_eq!(
        frame.shapes[26],
        Shape::Text {
            x: 300,
            y: 74,
            content: "#1E88E5".into(),
            color: Rgb(0xf1f3f5),
        }
    );
}

fn keys_move_the_chosen_color(sample: &Sample) {
    let launcher = sample.open_color();

    assert_eq!(press(&launcher, Key::Right), "Purple, #8E24AA");
    assert_eq!(press(&launcher, Key::Down), "Dark purple, #4A148C");
    assert_eq!(press(&launcher, Key::Home), "Dark red, #B71C1C");
    assert_eq!(press(&launcher, Key::Left), "Dark red, #B71C1C");
    assert_eq!(press(&launcher, Key::End), "Dark pink, #880E4F");
    assert_eq!(press(&launcher, Key::Up), "Pink, #D81B60");
    assert_eq!(press(&launcher, Key::Up), "Light pink, #F48FB1");
    assert_eq!(press(&launcher, Key::Up), "Light pink, #F48FB1");
    // The preview shows the chosen color too.
    let frame = launcher.view().custom_view().unwrap().frame.clone();
    assert!(matches!(
        frame.shapes[25],
        Shape::Rect {
            fill: Rgb(0xf48fb1),
            ..
        }
    ));
}

fn pressing_and_dragging_the_pointer_chooses_swatches(sample: &Sample) {
    let launcher = sample.open_color();

    assert_eq!(
        send(&launcher, ViewEvent::PointerDown(at(10, 10))),
        "Light red, #EF9A9A"
    );
    assert_eq!(
        send(&launcher, ViewEvent::PointerMove(at(80, 80))),
        "Dark yellow, #F57F17"
    );
    // A drag past the grid chooses the nearest swatch.
    assert_eq!(
        send(&launcher, ViewEvent::PointerMove(at(-50, 500))),
        "Dark red, #B71C1C"
    );
    send(&launcher, ViewEvent::PointerUp(at(-50, 500)));
    assert_eq!(
        send(&launcher, ViewEvent::PointerMove(at(200, 40))),
        "Dark red, #B71C1C"
    );
    // A press on the preview, outside the grid, chooses nothing.
    assert_eq!(
        send(&launcher, ViewEvent::PointerDown(at(330, 10))),
        "Dark red, #B71C1C"
    );
    assert_eq!(
        send(&launcher, ViewEvent::PointerMove(at(10, 10))),
        "Dark red, #B71C1C"
    );
}

fn each_opened_color_view_starts_afresh(sample: &Sample) {
    let launcher = sample.open_color();
    assert_eq!(press(&launcher, Key::Right), "Purple, #8E24AA");

    launcher.back();
    assert_eq!(launcher.view().screen, Screen::Command);
    block_on(launcher.activate_selected());

    assert_eq!(color(&launcher), "Blue, #1E88E5");
}

fn views_open_at_once_keep_their_own_state(sample: &Sample) {
    let runtime = Runtime::start().unwrap();
    let open = || block_on(runtime.open_view(&sample.path(), "color")).unwrap();
    let ((first, _), (second, _)) = (open(), open());

    let event = ViewEvent::Key(Key::Right);
    let first_frame = block_on(runtime.view_event(first, event)).unwrap();
    let second_frame = block_on(runtime.view_event(second, ViewEvent::Key(Key::Left))).unwrap();

    assert_eq!(first_frame.value, "Purple, #8E24AA");
    assert_eq!(second_frame.value, "Teal, #00897B");
    assert_eq!(block_on(runtime.view_count()), 2);
    runtime.close_view(first);
    assert_eq!(block_on(runtime.view_count()), 1);
    assert_eq!(
        block_on(runtime.view_event(first, event)),
        Err(CallError::ViewClosed)
    );
}

fn an_unknown_view_is_a_guest_error(sample: &Sample) {
    let runtime = Runtime::start().unwrap();

    let opened = block_on(runtime.open_view(&sample.path(), "missing"));

    assert_eq!(
        opened.map(|(_, frame)| frame),
        Err(CallError::Guest("unknown view: missing".into()))
    );
    assert_eq!(block_on(runtime.view_count()), 0);
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
    an_unknown_choice_is_a_field_error_from_the_guest(sample);
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
    an_unknown_choice_is_a_field_error_from_the_guest,
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
