//! Test fixture: a command that builds and installs, but whose first start
//! fails. Asked for its list the first time, it saves a setting and traps;
//! every later start, such as a Retry, finds the setting and starts.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView, settings};

/// The settings key recording that a start was attempted.
const ATTEMPTED: &str = "start-attempted";

struct FailingStart;
pane_guest::export!(FailingStart);

/// Runs the action `id`: shows a toast saying that it ran.
async fn act(id: &str) -> Result<(), String> {
    show_toast(Toast::success(format!("ran {id}")));
    Ok(())
}

impl Command for FailingStart {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        if settings::get(ATTEMPTED)?.is_none() {
            settings::set(ATTEMPTED, "yes")?;
            panic!("the first start fails");
        }
        Ok(List::new("Started")
            .item(Item::new("started", "Started on a later attempt").on_action(|| act("started"))))
    }

    /// A callback no item names runs as an action of that id too.
    async fn run_search_result(id: String) -> Result<(), String> {
        act(&id).await
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
