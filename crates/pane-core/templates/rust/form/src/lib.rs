//! __TITLE__, started from `pane-ext`'s `form` template: a command whose
//! item opens a form — a name to type and a greeting to choose — which
//! `submit_form` answers, field problems included. `pane-ext dev` builds
//! it with `cargo build --release --target wasm32-wasip2`, hands each
//! build to the running Pane, and builds it again after each save, so an
//! edit shows at once; see the README beside this file and Pane's
//! development-mode documentation.
//!
//! One component (the WebAssembly file `pane.json` names) serves every
//! command the package declares: Pane passes the command's id with each
//! call, so `render`, `run` and `submit_form` below match on it.
//! `pane-ext new command .` adds one.
#![no_std]

// pane-ext new command adds a command's module here.

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::{
    Choice, Command, Field, FieldKind, FieldValue, Form, FormError, Item, List, NoCustomView,
    TextField,
};

/// The package's own type, whose command the SDK exports.
struct __STRUCT__;

pane_extension::export!(__STRUCT__);

/// The greeting form's choices, by id.
const GREETINGS: [(&str, &str); 3] = [
    ("hello", "Hello"),
    ("morning", "Good morning"),
    ("welcome", "Welcome"),
];

impl Command for __STRUCT__ {
    type CustomView = NoCustomView;

    /// The command's list: the form's item. Choosing it opens the form;
    /// submitting it calls `submit_form` with the item's id.
    async fn render() -> Result<List, String> {
        match pane_extension::commands::current().command.as_str() {
            "__NAME__" => Ok(List::new("__TITLE__").item(
                Item::new("greet", "Greet someone")
                    .subtitle("Fill in a form the command checks and answers")
                    .form(greeting_form()),
            )),
            // pane-ext new command adds a view command's arm here.
            other => Err(format!("unknown command: {other}")),
        }
    }

    /// A command that opens no screen would run here instead; the
    /// `no-view` template starts from one.
    async fn run(command: String, _launch: pane_extension::LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            // pane-ext new command adds a no-view command's arm here.
            other => Err(format!("`{other}` opens a screen; it has no run entry point")),
        }
    }

    /// The form's answer: the chosen greeting and the name, or what was
    /// wrong with a field, shown next to it.
    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        match pane_extension::commands::current().command.as_str() {
            "__NAME__" => submitted(item_id, _values).await,
            // pane-ext new command adds a form command's arm here.
            other => Err(FormError {
                field: None,
                message: format!("`{other}` has no forms: {item_id}"),
            }),
        }
    }
}

/// The item's form: a name to type and a greeting to choose.
fn greeting_form() -> Form {
    Form {
        title: "Greet someone".into(),
        fields: vec![
            Field {
                id: "name".into(),
                label: "Name".into(),
                kind: FieldKind::Text(TextField {
                    placeholder: Some("Ada Lovelace".into()),
                }),
            },
            Field {
                id: "greeting".into(),
                label: "Greeting".into(),
                kind: FieldKind::Choice(
                    GREETINGS
                        .iter()
                        .map(|&(id, label)| Choice {
                            id: id.into(),
                            label: label.into(),
                        })
                        .collect(),
                ),
            },
        ],
        submit_label: "Greet".into(),
    }
}

/// Answers the form: the greeting and the name, or the problem with a
/// field.
async fn submitted(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
    if item_id != "greet" {
        return Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        });
    }
    let value = |id: &str| {
        values
            .iter()
            .find(|field| field.id == id)
            .map_or("", |field| field.value.as_str())
    };
    let name = value("name").trim();
    if name.is_empty() {
        return Err(FormError {
            field: Some("name".into()),
            message: "Enter a name".into(),
        });
    }
    if name.chars().count() > 40 {
        return Err(FormError {
            field: Some("name".into()),
            message: "Use at most 40 characters".into(),
        });
    }
    let Some(&(_, greeting)) = GREETINGS.iter().find(|&&(id, _)| id == value("greeting")) else {
        return Err(FormError {
            field: Some("greeting".into()),
            message: "Choose a greeting".into(),
        });
    };
    Ok(format!("{greeting}, {name}"))
}
