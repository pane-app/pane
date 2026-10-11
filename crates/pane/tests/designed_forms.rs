//! The form a designed tree holds (#241), through the native window's
//! keyboard: the view's opening focus ask focuses its first field, Enter
//! in a single-line field submits the form, a text area's Enter inserts
//! a newline while Ctrl+Enter submits from it, and a refused submission
//! draws the field's error under it, read by assistive technology as the
//! field's description. The tri-lingual samples' "form" command holds
//! the fields; the deeper keyboard coverage — the tab order, the editing
//! keys, the input method, the click paths and the field families — is
//! `window.rs`'s.
//!
//! Components come from `cargo xtask guests`; the JavaScript and
//! TypeScript ones are the prebuilt components in `guests/prebuilt/`.

use std::fs;
use std::path::PathBuf;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, Runtime, Screen};

use tempfile::TempDir;

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// One language's sample package.
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

/// The launcher window with the sample's assembled package installed,
/// the data folder held for the window's life.
fn open_installed<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
) -> (Entity<LauncherWindow>, TempDir, &'a mut VisualTestContext) {
    let data = tempfile::tempdir().unwrap();
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        folder.exists(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    let source = data.path().join(name);
    fs::create_dir_all(&source).unwrap();
    for entry in fs::read_dir(&folder).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), source.join(entry.file_name())).unwrap();
    }
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    futures::executor::block_on(launcher.install_package(&source));
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, data, cx)
}

/// Opens the sample's form command ("Greet someone", the fifth item of
/// its list) with the keyboard: only an installed command can be
/// launched by an item.
fn open_form(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("enter");
    settle(window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = settle(window, cx);
    assert!(
        matches!(view.screen, Screen::DesignedView(_)),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Greet someone");
}

/// The open form's value of field `key`: the keyed state's live text.
fn field_value(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, key: &str) -> String {
    window.update(cx, |window, cx| {
        window
            .designed_field_text(key, cx)
            .expect("the field exists")
    })
}

/// The accessibility tree's nodes, as (role, label, description) each.
fn a11y_nodes(cx: &mut VisualTestContext) -> Vec<(String, String, String)> {
    let json = a11y::a11y(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    nodes
        .values()
        .map(|node| {
            let field = |key: &str| node["aria"][key].as_str().unwrap_or_default().to_owned();
            (field("role"), field("label"), field("description"))
        })
        .collect()
}

/// Whether the answer `text` the form drew is on screen.
fn answer_is(cx: &mut VisualTestContext, text: &str) -> bool {
    a11y_nodes(cx)
        .iter()
        .any(|(role, label, _)| role == "Label" && label == text)
}

/// The form's opening ask focuses its first field: the name, which the
/// tree marks `autoFocus`.
fn the_form_opens_with_its_first_field_focused(cx: &mut TestAppContext, sample: &Sample) {
    let (window, _data, cx) = open_installed(cx, &sample.component.replace('_', "-"));
    open_form(&window, cx);

    assert_eq!(a11y::focused_label(cx).as_deref(), Some("Name"));
    assert!(
        cx.debug_bounds("field-name").is_some(),
        "the name field is drawn"
    );
}

/// Enter in a single-line field submits the form: the name's value, the
/// greeting at its default, and the answer drawn over the form.
fn enter_in_a_single_line_field_submits_the_form(cx: &mut TestAppContext, sample: &Sample) {
    let (window, _data, cx) = open_installed(cx, &sample.component.replace('_', "-"));
    open_form(&window, cx);

    cx.simulate_input("Ada");
    cx.simulate_keystrokes("enter");

    settle(&window, cx);
    assert!(
        answer_is(
            cx,
            &format!("Hello, Ada, from the {} guest", sample.language)
        ),
        "the answer draws over the form"
    );
    assert!(
        matches!(
            cx.read_entity(&window, |window, _| window.launcher().view().screen),
            Screen::DesignedView(_)
        ),
        "the form's view stays open"
    );
}

/// A text area's Enter inserts a newline, and Ctrl+Enter submits from
/// it: the submission carries both lines, not a submit on every Enter.
#[gpui::test]
fn a_text_areas_enter_inserts_a_newline_and_ctrl_enter_submits(cx: &mut TestAppContext) {
    let (window, _data, cx) = open_installed(cx, &RUST.component.replace('_', "-"));
    open_form(&window, cx);

    // The name first, so the submission is accepted; then the notes, two
    // tabs away.
    cx.simulate_input("Ada");
    cx.simulate_keystrokes("tab tab");
    assert_eq!(a11y::focused_label(cx).as_deref(), Some("Notes"));

    cx.simulate_input("one line");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("two");
    assert_eq!(field_value(&window, cx, "notes"), "one line\ntwo");

    // Ctrl+Enter submits from the text area, the answer drawn over the
    // form; the view is still open, so the notes keep their text.
    cx.simulate_keystrokes("ctrl-enter");
    settle(&window, cx);
    assert!(answer_is(cx, "Hello, Ada, from the Rust guest"));
    assert_eq!(field_value(&window, cx, "notes"), "one line\ntwo");
}

/// A refused submission draws the field's error under it, in the next
/// render: under the name field, and read by assistive technology as the
/// field's description.
#[gpui::test]
fn a_refused_submission_draws_the_fields_error_under_it(cx: &mut TestAppContext) {
    let (window, _data, cx) = open_installed(cx, &RUST.component.replace("_", "-"));
    open_form(&window, cx);

    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    let error = cx
        .debug_bounds("field-error-name")
        .expect("the field's error is drawn");
    let name = cx.debug_bounds("field-name").expect("the name field");
    assert!(error.top() >= name.bottom(), "{error:?} under {name:?}");
    let nodes = a11y_nodes(cx);
    assert!(
        nodes.contains(&("TextInput".into(), "Name".into(), "Enter a name".into())),
        "{nodes:?}"
    );
}

/// Declares one window test per check for each sample.
macro_rules! for_each_sample {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(
                #[gpui::test]
                fn $check(cx: &mut gpui::TestAppContext) {
                    super::$check(cx, &super::RUST);
                }
            )*
        }
        mod javascript {
            $(
                #[gpui::test]
                fn $check(cx: &mut gpui::TestAppContext) {
                    super::$check(cx, &super::JAVASCRIPT);
                }
            )*
        }
        mod typescript {
            $(
                #[gpui::test]
                fn $check(cx: &mut gpui::TestAppContext) {
                    super::$check(cx, &super::TYPESCRIPT);
                }
            )*
        }
    };
}

for_each_sample!(
    the_form_opens_with_its_first_field_focused,
    enter_in_a_single_line_field_submits_the_form,
);
