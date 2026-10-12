//! The form a designed tree holds (#241), at the Launcher seam: a
//! submission carries the values of every field kind — the text a field
//! edits, the state a checkbox or toggle is in, the tags or paths a
//! picker chose — a validation error is the field's own in the next
//! render, and a `remember` field's last submitted value is kept as the
//! package's settings and prefilled onto the field the next time a view
//! opens holding it. The tri-lingual samples' "form" command answers
//! through an SDK, so its submission is checked by what the guest draws
//! back; the designed fixture's form is written by hand and draws the
//! payload Pane sent, so every field kind's value is observable as it
//! left Pane.
//!
//! Components come from `cargo xtask guests`; the JavaScript and
//! TypeScript ones are the prebuilt components in `guests/prebuilt/`.

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{FormValue, Launcher, Node, NodeKind, Runtime, Screen};

use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::select_title;

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

/// The values of every field kind, as a submission carries them, keyed by
/// the fields' keys: a text field's, a password field's, a text area's, a
/// date field's, a date and time field's and a dropdown's texts, a tag
/// picker's and a folder picker's lists, a file picker's path, and a
/// checkbox's and a toggle's states. The samples' and the fixture's forms
/// share their fields' keys, so one set serves both.
fn every_kind() -> Vec<(String, FormValue)> {
    vec![
        ("name".to_owned(), FormValue::Text("Ada".into())),
        ("secret".to_owned(), FormValue::Text("hunter2".into())),
        ("notes".to_owned(), FormValue::Text("two lines".into())),
        ("day".to_owned(), FormValue::Text("2026-01-31".into())),
        ("at".to_owned(), FormValue::Text("2026-01-31 14:05".into())),
        ("greeting".to_owned(), FormValue::Text("morning".into())),
        (
            "tags".to_owned(),
            FormValue::List(vec!["friend".into(), "colleague".into()]),
        ),
        ("file".to_owned(), FormValue::Text("/tmp/todo.txt".into())),
        (
            "folder".to_owned(),
            FormValue::List(vec!["/tmp/a".into(), "/tmp/b".into()]),
        ),
        ("updates".to_owned(), FormValue::On(true)),
        ("quiet".to_owned(), FormValue::On(false)),
    ]
}

/// The submission's payload as Pane writes it: the values, keyed by the
/// fields' keys, as JSON — what the designed fixture draws back.
const EVERY_KIND_PAYLOAD: &str = concat!(
    "{\"values\":{\"name\":\"Ada\",\"secret\":\"hunter2\",\"notes\":\"two lines\",",
    "\"day\":\"2026-01-31\",\"at\":\"2026-01-31 14:05\",\"greeting\":\"morning\",",
    "\"tags\":[\"friend\",\"colleague\"],\"file\":\"/tmp/todo.txt\",",
    "\"folder\":[\"/tmp/a\",\"/tmp/b\"],\"updates\":true,\"quiet\":false}}"
);

/// One test's Pane: its data folder (held, so the package's settings
/// survive between openings) and the launcher, with the assembled package
/// `package` installed from `target/guests/packages`.
struct Pane {
    _data: TempDir,
    launcher: Launcher,
}

impl Pane {
    fn new(package: &str) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let launcher =
            Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"));
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(package);
        assert!(
            folder.exists(),
            "{} is missing; run `cargo xtask guests`",
            folder.display()
        );
        let source = data.path().join(package);
        fs::create_dir_all(&source).unwrap();
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), source.join(entry.file_name())).unwrap();
        }
        block_on(launcher.install_package(&source));
        assert!(matches!(
            launcher.view().status,
            pane_core::Status::Result(_)
        ));
        Pane {
            _data: data,
            launcher,
        }
    }

    /// Returns to root search and opens the command titled `title`,
    /// searched for as `query` names it.
    fn open(&mut self, query: &str, title: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(query));
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
        assert!(
            matches!(self.launcher.view().screen, Screen::DesignedView(_)),
            "{:?}",
            self.launcher.view().screen
        );
    }

    /// Submits the form the open designed view holds with `values` and
    /// waits for the tree its answer draws.
    fn submit(&self, values: Vec<(String, FormValue)>) {
        let submitting = self.launcher.submit_designed_form(None, values);
        block_on(submitting);
    }

    /// The open designed view's tree.
    fn tree(&self) -> pane_core::DesignedTree {
        match &self.launcher.view().screen {
            Screen::DesignedView(view) => view.tree.clone(),
            other => panic!("no designed view: {other:?}"),
        }
    }

    /// Every text the open designed view's tree holds.
    fn texts(&self) -> Vec<String> {
        let mut texts = Vec::new();
        collect_texts(&self.tree().root, &mut texts);
        texts
    }
}

/// Every text a tree holds, plain and in spans.
fn collect_texts(node: &Node, into: &mut Vec<String>) {
    if let NodeKind::Text(text) = &node.kind {
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

/// The nodes a tree holds.
fn nodes_of(tree: &pane_core::DesignedTree) -> Vec<&Node> {
    fn at<'a>(node: &'a Node, into: &mut Vec<&'a Node>) {
        into.push(node);
        for child in &node.children {
            at(child, into);
        }
    }
    let mut nodes = Vec::new();
    at(&tree.root, &mut nodes);
    nodes
}

/// The field `key` of the open designed view's tree, by its key.
fn field<'a>(tree: &'a pane_core::DesignedTree, key: &str) -> &'a Node {
    nodes_of(tree)
        .into_iter()
        .find(|node| node.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no field {key:?} in the tree"))
}

/// The sample's form command opened ("Greet someone"), from its installed
/// package: only an installed command can be launched.
fn sample_form(sample: &Sample) -> Pane {
    let mut pane = Pane::new(&sample.component.replace('_', "-"));
    pane.open("greet someone", "Greet someone");
    pane
}

/// The designed fixture's form command opened ("Designed form fixture"):
/// a form written by hand against the contract, not by an SDK.
fn fixture_form() -> Pane {
    let mut pane = Pane::new("designed");
    pane.open("", "Designed form fixture");
    pane
}

/// A submission carrying every field kind's value reaches the guest and
/// is answered: the whole payload parses, the greeting it carries is the
/// one the answer names, and the view stays open with the answer.
fn every_field_kind_submits_its_value_through_the_sample(sample: &Sample) {
    let pane = sample_form(sample);
    pane.submit(every_kind());

    let texts = pane.texts();
    assert!(
        texts.contains(&format!(
            "Good morning, Ada, from the {} guest",
            sample.language
        )),
        "{texts:?}"
    );
    assert!(
        matches!(pane.launcher.view().screen, Screen::DesignedView(_)),
        "the form's view stays open"
    );
}

/// The submission's payload, as the designed fixture draws it back: every
/// field kind's value, exactly as Pane sent it.
#[test]
fn the_designed_fixture_draws_the_values_of_every_field_kind() {
    let pane = fixture_form();
    pane.submit(every_kind());

    let texts = pane.texts();
    let sent = format!("Sent: 31 on form: {EVERY_KIND_PAYLOAD}");
    assert!(texts.contains(&sent), "{texts:?}");
}

/// The designed fixture's form offers every field kind, keyed and
/// titled, with the name asking to be remembered: what the hand-written
/// tree names, checked before its submissions are.
#[test]
fn the_designed_fixtures_form_offers_every_field_kind() {
    let pane = fixture_form();
    let tree = pane.tree();

    let kinds: Vec<&str> = nodes_of(&tree)
        .into_iter()
        .filter(|node| node.key.is_some())
        .map(|node| match &node.kind {
            NodeKind::Form(_) => "form",
            NodeKind::TextInput(_) => "text-input",
            NodeKind::PasswordInput(_) => "password-input",
            NodeKind::TextArea(_) => "text-area",
            NodeKind::DatePicker(_) => "date-picker",
            NodeKind::DateTimePicker(_) => "date-time-picker",
            NodeKind::Select(_) => "select",
            NodeKind::TagPicker(_) => "tag-picker",
            NodeKind::FilePicker(_) => "file-picker",
            NodeKind::FolderPicker(_) => "folder-picker",
            NodeKind::Checkbox(_) => "checkbox",
            NodeKind::Toggle(_) => "toggle",
            _ => "other",
        })
        .collect();
    // The keyed nodes end with the button that returns to the counter.
    assert_eq!(
        kinds,
        [
            "form",
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
            "other",
        ]
    );

    // The form's submission and submit button; the name's placeholder and
    // remember; the folder picker choosing several paths; the checkbox
    // starting on.
    let NodeKind::Form(form) = &field(&tree, "form").kind else {
        unreachable!("the form node")
    };
    assert_eq!(form.submit_label.as_deref(), Some("Send"));
    assert!(form.on_submit.is_some(), "the form submits");
    let NodeKind::TextInput(name) = &field(&tree, "name").kind else {
        unreachable!("the name field")
    };
    assert_eq!(name.placeholder.as_deref(), Some("Ada Lovelace"));
    assert!(name.field.remember, "the name is remembered");
    let NodeKind::FolderPicker(folder) = &field(&tree, "folder").kind else {
        unreachable!("the folder field")
    };
    assert!(folder.multiple, "the folder picker chooses several");
    let NodeKind::Checkbox(updates) = &field(&tree, "updates").kind else {
        unreachable!("the updates field")
    };
    assert!(updates.checked, "the checkbox starts on");
}

/// An empty name is refused by the fixture's form: the field's error is
/// in the tree the next render draws, the form staying open, and a
/// corrected submission clears it.
#[test]
fn an_empty_submission_marks_the_fields_error_from_the_next_render() {
    let pane = fixture_form();
    let mut refused = every_kind();
    refused[0].1 = FormValue::Text(String::new());
    pane.submit(refused);

    let tree = pane.tree();
    let NodeKind::TextInput(name) = &field(&tree, "name").kind else {
        unreachable!("the name field")
    };
    assert_eq!(name.field.error.as_deref(), Some("Enter a name"));
    assert!(
        matches!(pane.launcher.view().screen, Screen::DesignedView(_)),
        "the form stays open"
    );

    // A corrected submission clears the error.
    pane.submit(every_kind());
    let tree = pane.tree();
    let NodeKind::TextInput(name) = &field(&tree, "name").kind else {
        unreachable!("the name field")
    };
    assert_eq!(name.field.error, None);
}

/// A `remember` field's last submitted value is prefilled onto the field
/// the next time the form opens — the value a starting point, never an
/// instruction — while a field that does not ask is left as the tree
/// drew it. The sample's name is the remembered field.
fn a_remembered_field_is_prefilled_when_the_form_opens_again(sample: &Sample) {
    let mut pane = sample_form(sample);
    let mut values = every_kind();
    values[0].1 = FormValue::Text("Grace".into());
    pane.submit(values);

    // Leave the form and open it again: the name carries the remembered
    // value, the secret stays as the tree drew it.
    pane.launcher.back();
    pane.open("greet someone", "Greet someone");
    let tree = pane.tree();
    let NodeKind::TextInput(name) = &field(&tree, "name").kind else {
        unreachable!("the name field")
    };
    assert_eq!(name.value, "Grace");
    let NodeKind::PasswordInput(secret) = &field(&tree, "secret").kind else {
        unreachable!("the secret field")
    };
    assert_eq!(secret.value, "");
}

/// The designed fixture's remembered name is prefilled as the samples'
/// is, the hand-written tree saying `remember` as an SDK's does.
#[test]
fn the_fixtures_remembered_field_is_prefilled_too() {
    let mut pane = fixture_form();
    let mut values = every_kind();
    values[0].1 = FormValue::Text("Grace".into());
    pane.submit(values);

    pane.launcher.back();
    pane.open("", "Designed form fixture");
    let tree = pane.tree();
    let NodeKind::TextInput(name) = &field(&tree, "name").kind else {
        unreachable!("the name field")
    };
    assert_eq!(name.value, "Grace");
    // A field the tree does not mark `remember` is left as it drew it.
    let NodeKind::TextArea(notes) = &field(&tree, "notes").kind else {
        unreachable!("the notes field")
    };
    assert_eq!(notes.value, "");
}

/// Declares one test per sample.
macro_rules! for_each_sample {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::RUST);
                }
            )*
        }
        mod javascript {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::JAVASCRIPT);
                }
            )*
        }
        mod typescript {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::TYPESCRIPT);
                }
            )*
        }
    };
}

for_each_sample!(
    every_field_kind_submits_its_value_through_the_sample,
    a_remembered_field_is_prefilled_when_the_form_opens_again,
);
