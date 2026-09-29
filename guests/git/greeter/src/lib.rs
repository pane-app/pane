//! Pane's Git-distributed sample (guests/git/greeter): the source of a
//! package whose release revisions carry its built component, so that
//! Pane can install it from a Git repository without building anything.
//! Its answers name the Git repository, so that what runs is visibly the
//! copy Pane fetched rather than another sample.
//!
//! Its command's one item, "Say hello", answers "Hello from the Git
//! repository". `greet` version 1 takes `{"name": "<name>"}` and answers
//! `{"greeting": "Hello, <name>, from the Git repository"}`, or the error
//! "a name is needed".
#![no_std]

use pane_guest::alloc::{format, string::String, string::ToString, vec, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, publish};
use serde_json::{Value, json};

struct Greeter;
pane_guest::export!(Greeter);
pane_guest::publish::export!(Greeter);

impl Guest for Greeter {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        Ok(View {
            title: "Greeter from Git".into(),
            items: vec![Item {
                id: "greet".into(),
                title: "Say hello".into(),
                subtitle: Some("Answer from the Git repository".into()),
                form: None,
                platforms: None,
                custom_view: None,
            }],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        if item_id != "greet" {
            return Err(format!("unknown item: {item_id}"));
        }
        Ok("Hello from the Git repository".into())
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

/// The operation the package publishes: `greet`.
impl publish::Guest for Greeter {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        if operation != "greet" {
            return Err(format!("unknown operation: {operation}"));
        }
        let input: Value = serde_json::from_str(&input).map_err(|error| format!("{error}"))?;
        let name = input.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            return Err("a name is needed".into());
        }
        Ok(json!({ "greeting": format!("Hello, {name}, from the Git repository") }).to_string())
    }
}
