//! Pane's dependencies sample in Rust: a package that uses other packages'
//! operations and says so in its pane.json, under `dependencies`:
//!
//! - `greeter`, required: the JavaScript operations sample, from the folder
//!   `../sample-operations-js` beside this package's. Installing this
//!   package installs it too if it is missing.
//! - `rust-greeter`, optional: the Rust operations sample, from
//!   `../sample-operations`. Pane never installs it with this package; the
//!   sample uses it when the user installed it, and says how to get it
//!   when not.
//!
//! The code calls each by that id, never by the folder it was installed
//! from, which only Pane knows: `call("greeter", "greet", 1, input)`. Each
//! item calls `greet` version 1 with `{"name": "Pane"}`, the version the
//! manifest names for it.
#![no_std]

use pane_guest::alloc::{format, string::String, string::ToString, vec, vec::Vec};
use pane_guest::operations::{CallErrorKind, call};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View};
use serde_json::{Value, json};

struct Dependencies;
pane_guest::export!(Dependencies);

/// Calls `greet` version 1 of the dependency with id `dependency` for
/// "Pane", and returns its greeting.
async fn greet(dependency: &str) -> Result<String, pane_guest::operations::CallError> {
    let input = json!({ "name": "Pane" }).to_string();
    let result = call(dependency.into(), "greet".into(), 1, input).await?;
    let result: Value = serde_json::from_str(&result).unwrap_or(Value::Null);
    Ok(result
        .get("greeting")
        .and_then(Value::as_str)
        .unwrap_or("the answer has no greeting")
        .into())
}

fn item(id: &str, title: &str, subtitle: &str) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        subtitle: Some(subtitle.into()),
        form: None,
        platforms: None,
        custom_view: None,
    }
}

impl Guest for Dependencies {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        Ok(View {
            title: "Greet through dependencies".into(),
            items: vec![
                item(
                    "required",
                    "Greet through the required greeter",
                    "The JavaScript operations sample, installed with this one",
                ),
                item(
                    "optional",
                    "Greet through the optional greeter",
                    "The Rust operations sample, if you installed it",
                ),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "required" => greet("greeter").await.map_err(|error| error.explain()),
            "optional" => match greet("rust-greeter").await {
                Ok(greeting) => Ok(greeting),
                // An optional dependency may be missing: say how to get it
                // rather than fail.
                Err(error) if error.kind == CallErrorKind::NotFound => Ok(
                    "The optional Rust greeter is not installed; install the Rust operations \
                     sample to use it"
                        .into(),
                ),
                Err(error) => Err(error.explain()),
            },
            _ => Err(format!("unknown item: {item_id}")),
        }
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        })
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}
