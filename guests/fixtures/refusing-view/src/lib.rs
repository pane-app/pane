//! Test fixture: a command that starts, but whose view is always refused
//! with an ordinary error the guest returns, as a command that needs the
//! user to sign in first would. That is an expected operation error, not a
//! failure to start.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, NoCustomView, View};

struct RefusingView;
pane_guest::export!(RefusingView);

impl Guest for RefusingView {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        Err("sign in first".into())
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
