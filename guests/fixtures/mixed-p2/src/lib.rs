//! Negative control: uses std, so the component imports WASI 0.2 interfaces.
wit_bindgen::generate!({ path: "../../../wit", world: "extension" });

use exports::pane::extension::command::{Guest, Item, View};

struct Mixed;
export!(Mixed);

impl Guest for Mixed {
    async fn get_view() -> Result<View, String> {
        let now = std::time::SystemTime::now();
        eprintln!("listing at {now:?}");
        Ok(View {
            title: "Mixed".into(),
            items: vec![Item {
                id: "x".into(),
                title: "x".into(),
                subtitle: None,
            }],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        Ok(item_id)
    }
}
