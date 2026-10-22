//! Pane's Git-distributed sample (guests/git/greeter): the source of a
//! package whose release revisions carry its built component, so that
//! Pane can install it from a Git repository without building anything.
//! What it says names the Git repository, so that what runs is visibly the
//! copy Pane fetched rather than another sample.
//!
//! Its command's one item, "Say hello", shows a toast saying "Hello from
//! the Git repository". `greet` version 1 takes `{"name": "<name>"}` and
//! answers `{"greeting": "Hello, <name>, from the Git repository"}`, or the
//! error "a name is needed".
#![no_std]

use pane_extension::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, Item, List};
use serde_json::{Value, json};

struct Greeter;
pane_extension::export!(Greeter);
pane_extension::publish::export!(Greeter);

/// Runs the action of the item `item_id`: a toast greeting the user.
async fn act(item_id: &str) -> Result<(), String> {
    if item_id != "greet" {
        return Err(format!("unknown item: {item_id}"));
    }
    show_toast(Toast::success("Hello from the Git repository"));
    Ok(())
}

impl Command for Greeter {
    type DesignedView = pane_extension::view::NoDesignedView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Greeter from Git").item(
            Item::new("greet", "Say hello")
                .subtitle("Answer from the Git repository")
                .on_action(|| act("greet")),
        ))
    }
}

/// The operation the package publishes: `greet`.
impl pane_extension::publish::Guest for Greeter {
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
