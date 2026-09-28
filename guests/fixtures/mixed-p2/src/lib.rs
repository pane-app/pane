//! Negative control: uses std, so the component imports WASI 0.2 interfaces.
wit_bindgen::generate!({ path: "../../../wit", world: "extension" });

use exports::pane::extension::command::{
    CustomView, FieldValue, FormError, Frame, Guest, GuestCustomView, Item, View, ViewEvent,
};

struct Mixed;
export!(Mixed);

/// No view is ever opened; the fixture only has to type-check.
struct NoView;

impl GuestCustomView for NoView {
    async fn render(&self) -> Frame {
        unreachable!()
    }

    async fn handle_event(&self, _event: ViewEvent) -> Result<(), String> {
        Ok(())
    }
}

impl Guest for Mixed {
    type CustomView = NoView;

    async fn get_view() -> Result<View, String> {
        let now = std::time::SystemTime::now();
        eprintln!("listing at {now:?}");
        Ok(View {
            title: "Mixed".into(),
            items: vec![Item {
                id: "x".into(),
                title: "x".into(),
                subtitle: None,
                form: None,
                platforms: None,
                custom_view: None,
            }],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        Ok(item_id)
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Ok(item_id)
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(item_id)
    }
}
