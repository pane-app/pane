//! Negative control: uses std, so the component imports WASI 0.2 interfaces.
wit_bindgen::generate!({ path: "../../../wit", world: "extension" });

use exports::pane::extension::command::{
    Guest, GuestView, LaunchRecord, Outcome, Rendered, UiEvent, View,
};

struct Mixed;
export!(Mixed);

/// A designed view type that is never opened.
enum NoDesignedView {}

impl GuestView for NoDesignedView {
    async fn render(&self, _context: String) -> Result<Rendered, String> {
        match *self {}
    }

    async fn handle_event(&self, _event: UiEvent) -> Result<Outcome, String> {
        match *self {}
    }
}

impl Guest for Mixed {
    type View = NoDesignedView;

    async fn render(_launch: LaunchRecord) -> Result<String, String> {
        let now = std::time::SystemTime::now();
        eprintln!("listing at {now:?}");
        Ok(
            r#"{"version": 1, "view": {"type": "list", "title": "Mixed", "items": [
            {"id": "x", "title": "x", "actions": [{"onAction": "x"}]}]}}"#
                .into(),
        )
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<String, String> {
        Err(command)
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        Ok(format!("{{\"status\": \"{callback}\"}}"))
    }

    async fn open_view(command: String, _launch: LaunchRecord) -> Result<View, String> {
        Err(command)
    }
}
