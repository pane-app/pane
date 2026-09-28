//! Test fixture: a guest whose actions fail in each way the host must report.
#![no_std]

use pane_guest::alloc::{string::String, vec};
use pane_guest::{Guest, Item, View};

struct Faulty;
pane_guest::export!(Faulty);

fn item(id: &str) -> Item {
    Item {
        id: id.into(),
        title: id.into(),
        subtitle: None,
    }
}

impl Guest for Faulty {
    async fn get_view() -> Result<View, String> {
        Ok(View {
            title: "Faulty".into(),
            items: vec![item("ok"), item("error"), item("trap")],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "error" => Err("the guest refused".into()),
            "trap" => panic!("guest trap"),
            _ => Ok("fine".into()),
        }
    }
}
