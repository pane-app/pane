//! A minimal Rust command to develop with Pane's development mode: install
//! this folder, choose "Develop Hello Rust" in Manage extensions, then edit
//! `GREETING` and save. Pane builds the package with `cargo build --release
//! --target wasm32-wasip2` and reloads it while it keeps running; "Say
//! hello" then answers with the new text. See guests/README.md.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

/// What "Say hello" answers.
const GREETING: &str = "Hello from Rust";

struct Hello;
pane_guest::export!(Hello);

/// Runs the action of the item `id`.
async fn act(id: &str) -> Result<String, String> {
    match id {
        "hello" => Ok(GREETING.into()),
        other => Err(format!("unknown item: {other}")),
    }
}

impl Command for Hello {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Hello").item(Item::new("hello", "Say hello").on_action(|| act("hello"))))
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "this command has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("this command has no custom views".into())
    }
}
