//! Test fixture: a command that starts, but whose list is always refused
//! with an ordinary error the guest returns, as a command that needs the
//! user to sign in first would. That is an expected operation error, not a
//! failure to start.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::{Command, CustomView, FieldValue, FormError, List, NoCustomView};

struct RefusingView;
pane_guest::export!(RefusingView);

impl Command for RefusingView {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Err("sign in first".into())
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
