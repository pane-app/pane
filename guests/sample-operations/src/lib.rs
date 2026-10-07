//! Pane's operations sample in Rust. Its package publishes the operation
//! `greet` (under `operations` in its pane.json), served by
//! [`publish::Guest::run_operation`], and its command calls the `greet`
//! operation of another package, with [`pane_guest::operations::call`]:
//! a form asks for that package's source (its identity, as Pane shows it:
//! `local:` and the folder it was installed from), a name, and whether to
//! ask once or twice at once. Items, titles, results and errors match the
//! JavaScript and TypeScript samples.
//!
//! `greet` version 1 takes `{"name": "<name>"}` and answers
//! `{"greeting": "Hello, <name>, from Rust"}`, or the error "a name is
//! needed".
//!
//! `wait` version 1 shows a call Pane stops: it saves `waiting` as
//! "started" in its settings, waits ten seconds, saves "finished" and
//! answers `{"waited": true}`. Disabling or reloading either package
//! meanwhile stops it; the "wait" item calls it.
#![no_std]

use pane_guest::alloc::{format, string::String, string::ToString, vec, vec::Vec};
use pane_guest::operations::call;
use pane_guest::{
    Choice, Command, CustomView, Field, FieldKind, FieldValue, Form, FormError, Item, List,
    NoCustomView, TextField, publish, settings,
};
use serde_json::{Value, json};

struct Operations;
pane_guest::export!(Operations);
pane_guest::publish::export!(Operations);

/// Calls `greet` version 1 of the package with `source` for `name`, and
/// returns its greeting, or why there is none ("<kind>: <message>").
async fn greet(source: &str, name: &str) -> Result<String, String> {
    let input = json!({ "name": name }).to_string();
    let result = call(source.into(), "greet".into(), 1, input)
        .await
        .map_err(|error| error.explain())?;
    let result: Value = serde_json::from_str(&result).map_err(|error| format!("{error}"))?;
    match result.get("greeting").and_then(Value::as_str) {
        Some(greeting) => Ok(greeting.into()),
        None => Err("the answer has no greeting".into()),
    }
}

/// The form of the "greet" item.
fn greet_form() -> Form {
    let text = |id: &str, label: &str, placeholder: &str| Field {
        id: id.into(),
        label: label.into(),
        kind: FieldKind::Text(TextField {
            placeholder: Some(placeholder.into()),
        }),
    };
    let choice = |id: &str, label: &str| Choice {
        id: id.into(),
        label: label.into(),
    };
    Form {
        title: "Greet through another extension".into(),
        fields: vec![
            text(
                "source",
                "Package source",
                "local:/path/to/sample-operations-js",
            ),
            text("name", "Name", "Rust"),
            Field {
                id: "times".into(),
                label: "Ask".into(),
                kind: FieldKind::Choice(vec![
                    choice("once", "Once"),
                    choice("twice", "Twice at once"),
                ]),
            },
        ],
        submit_label: "Greet".into(),
    }
}

/// The form of the "wait" item.
fn wait_form() -> Form {
    Form {
        title: "Wait in another extension".into(),
        fields: vec![Field {
            id: "source".into(),
            label: "Package source".into(),
            kind: FieldKind::Text(TextField {
                placeholder: Some("local:/path/to/sample-operations-js".into()),
            }),
        }],
        submit_label: "Wait".into(),
    }
}

fn form_error(message: String) -> FormError {
    FormError {
        field: None,
        message,
    }
}

impl Command for Operations {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Call from Rust").items([
            Item::new("greet", "Greet through another extension")
                .subtitle("Calls its greet operation through Pane")
                .form(greet_form()),
            Item::new("wait", "Wait in another extension")
                .subtitle("Calls its wait operation, which takes ten seconds")
                .form(wait_form()),
        ]))
    }

    async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        if item_id != "greet" && item_id != "wait" {
            return Err(form_error(format!("unknown form: {item_id}")));
        }
        let value = |id: &str| {
            values
                .iter()
                .find(|field| field.id == id)
                .map_or("", |field| field.value.as_str())
        };
        let source = value("source").trim();
        if source.is_empty() {
            return Err(FormError {
                field: Some("source".into()),
                message: "Enter the package's source".into(),
            });
        }
        if item_id == "wait" {
            call(source.into(), "wait".into(), 1, "{}".into())
                .await
                .map_err(|error| form_error(error.explain()))?;
            return Ok("Waited in the other extension".into());
        }
        let name = value("name");
        if value("times") == "twice" {
            // Both calls are made at once; Pane serves them one after another.
            let (first, second) = futures::join!(greet(source, name), greet(source, name));
            let (first, second) = (first.map_err(form_error)?, second.map_err(form_error)?);
            return Ok(format!("{first} / {second}"));
        }
        greet(source, name).await.map_err(form_error)
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

/// The operations the package publishes: `greet` and `wait`.
impl publish::Guest for Operations {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        if operation == "wait" {
            settings::set("waiting", "started")?;
            // If Pane stops the call meanwhile, nothing after this runs.
            wasip3::clocks::monotonic_clock::wait_for(10_000_000_000).await;
            settings::set("waiting", "finished")?;
            return Ok(json!({ "waited": true }).to_string());
        }
        if operation != "greet" {
            return Err(format!("unknown operation: {operation}"));
        }
        let input: Value = serde_json::from_str(&input).map_err(|error| format!("{error}"))?;
        let name = input.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            return Err("a name is needed".into());
        }
        Ok(json!({ "greeting": format!("Hello, {name}, from Rust") }).to_string())
    }
}
