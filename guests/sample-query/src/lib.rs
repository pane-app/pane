//! A command that takes a query, the smallest runnable example of one:
//! Echo answers the text the user sends it from root search, through its
//! alias ("ec hello" when the user gave it the alias "ec") or by choosing it
//! as a fallback for whatever they typed. Pane sends the text only when the
//! user invokes it that way, never while they type. Opened from root search
//! like any command, it lists how to use it.
#![no_std]

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View};

struct Echo;
pane_guest::export!(Echo);
pane_guest::query::export!(Echo);

/// The query Echo answers with an error, to show how a failure looks.
const REFUSED: &str = "fail";

/// The query Echo crashes on, to show how Pane pauses a crashing extension.
const CRASH: &str = "crash";

impl Guest for Echo {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let item = |id: &str, title: &str, subtitle: &str| Item {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View {
            title: "Echo: send it text from root search".into(),
            items: vec![
                item(
                    "alias",
                    "Give Echo an alias in Manage extensions",
                    "Then type the alias, a space and your text in root search",
                ),
                item(
                    "fallback",
                    "Or make Echo a fallback in Manage extensions",
                    "Then type anything in root search and choose Echo below the results",
                ),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "alias" | "fallback" => Ok("Echo answers the text you send it from root search".into()),
            other => Err(format!("unknown item: {other}")),
        }
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "Echo has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("Echo has no custom views".into())
    }
}

impl pane_guest::query::Guest for Echo {
    /// Answers with the text it was sent; "fail" is refused, to show how an
    /// error looks, and "crash" crashes on purpose (three crashes within
    /// five minutes pause the extension).
    async fn run_query(command: String, query: String) -> Result<String, String> {
        if command != "echo" {
            return Err(format!("unknown command: {command}"));
        }
        match query.as_str() {
            REFUSED => Err(format!(
                "Echo refuses “{REFUSED}”, to show how an error looks"
            )),
            CRASH => panic!("Echo crashes on purpose"),
            _ => Ok(format!("Echo heard “{query}”")),
        }
    }
}
