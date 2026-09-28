//! Test fixture: a command that builds and installs, but whose first start
//! fails. Asked for its view the first time, it saves a setting and traps;
//! every later start, such as a Retry, finds the setting and starts.
#![no_std]

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, settings};

/// The settings key recording that a start was attempted.
const ATTEMPTED: &str = "start-attempted";

struct FailingStart;
pane_guest::export!(FailingStart);

impl Guest for FailingStart {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        if settings::get(ATTEMPTED)?.is_none() {
            settings::set(ATTEMPTED, "yes")?;
            panic!("the first start fails");
        }
        Ok(View {
            title: "Started".into(),
            items: vec![Item {
                id: "started".into(),
                title: "Started on a later attempt".into(),
                subtitle: None,
                form: None,
                platforms: None,
                custom_view: None,
            }],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        Ok(format!("ran {item_id}"))
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
