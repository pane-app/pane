//! A minimal Rust command to develop with Pane's development mode: install
//! this folder, choose "Develop Hello Rust" in Manage extensions, then edit
//! `GREETING` and save. Pane builds the package with `cargo build --release
//! --target wasm32-wasip2` and reloads it while it keeps running; "Say
//! hello" then answers with the new text. See guests/README.md.
#![no_std]

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View};

/// What "Say hello" answers.
const GREETING: &str = "Hello from Rust";

struct Hello;
pane_guest::export!(Hello);

impl Guest for Hello {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let item = Item {
            id: "hello".into(),
            title: "Say hello".into(),
            subtitle: None,
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View {
            title: "Hello".into(),
            items: vec![item],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "hello" => Ok(GREETING.into()),
            other => Err(format!("unknown item: {other}")),
        }
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
