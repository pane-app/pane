//! Pane's operations sample in Rust. Its package publishes the operation
//! `greet` (under `operations` in its pane.json), served by
//! [`publish::Guest::run_operation`], and its command calls the `greet` operation the
//! JavaScript and TypeScript operations samples publish, with
//! [`pane_guest::operations::call`]. Items, titles, results and errors match
//! those samples.
//!
//! `greet` version 1 takes `{"name": "<name>"}` and answers
//! `{"greeting": "Hello, <name>, from Rust"}`, or the error "a name is
//! needed".
#![no_std]

use pane_guest::alloc::{format, string::String, string::ToString, vec, vec::Vec};
use pane_guest::operations::call;
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, publish};
use serde_json::{Value, json};

/// The packages this command calls, by source: relative to this package's
/// own source folder, so the samples work side by side wherever they are.
const JAVASCRIPT: &str = "local:../sample-operations-js";
const TYPESCRIPT: &str = "local:../sample-operations-ts";
const MISSING: &str = "local:../no-such-extension";

struct Operations;
pane_guest::export!(Operations);
pane_guest::publish::export!(Operations);

/// Calls `greet` version 1 of the package with `source` for `name`, and
/// returns its greeting, or why there is none.
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

impl Guest for Operations {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let item = |id: &str, title: &str| Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View {
            title: "Call from Rust".into(),
            items: vec![
                item("ask-js", "Ask JavaScript to greet"),
                item("ask-ts", "Ask TypeScript to greet"),
                item("ask-empty", "Ask JavaScript with no name"),
                item("ask-missing", "Ask an extension that is not installed"),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        let (source, answerer, name) = match item_id.as_str() {
            "ask-js" => (JAVASCRIPT, "JavaScript", "Rust"),
            "ask-ts" => (TYPESCRIPT, "TypeScript", "Rust"),
            "ask-empty" => (JAVASCRIPT, "JavaScript", ""),
            "ask-missing" => (MISSING, "Nobody", "Rust"),
            other => return Err(format!("unknown item: {other}")),
        };
        let greeting = greet(source, name).await?;
        Ok(format!("{answerer} answered: {greeting}"))
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

/// The operations the package publishes: `greet`.
impl publish::Guest for Operations {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
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
