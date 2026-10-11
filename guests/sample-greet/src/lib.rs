//! Pane's greet capability provider sample in Rust. Its package provides
//! the capability `pane-samples:greet@1` (under `provides` in its
//! pane.json), served by [`publish::Guest::run_operation`] through the same
//! export that serves published operations; its command only says what it
//! provides. The JavaScript and TypeScript samples (guests/sample-greet-js
//! and guests/sample-greet-ts) provide the same capability, each answering
//! with its own language's name, so a consumer installed with any of them
//! works without naming any of them.
//!
//! The capability's `greet` operation takes `{"name": "<name>"}` and answers
//! `{"greeting": "Hello, <name>, from Rust"}`, or the error "a name is
//! needed". Pane calls it with the operation qualified by its capability —
//! `pane-samples:greet@1/greet` — so one component can tell a capability
//! call from a call by identity to an operation of the same name; nothing
//! else is served.
#![no_std]

use pane_extension::alloc::{format, string::String, string::ToString};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, CustomView, Item, List, NoCustomView, publish};
use serde_json::{Value, json};

/// The capability this package provides, with the operation Pane qualifies
/// by it.
const CAPABILITY: &str = "pane-samples:greet@1";
const GREET: &str = "pane-samples:greet@1/greet";

struct Greeter;
pane_extension::export!(Greeter);
pane_extension::publish::export!(Greeter);

/// The item's action: what the item says this package provides.
async fn about() -> Result<(), String> {
    show_toast(Toast::success(format!(
        "Provides {CAPABILITY}: other extensions call its greet operation through Pane"
    )));
    Ok(())
}

/// The one item: what this package provides.
fn about_item() -> Item {
    Item::new("about", "What this provides")
        .subtitle("The pane-samples:greet@1 capability, for other extensions")
        .on_action(about)
}

impl Command for Greeter {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Rust greet provider").items([about_item()]))
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

impl publish::Guest for Greeter {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        if operation != GREET {
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
