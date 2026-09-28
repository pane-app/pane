//! Pane's settings sample: a command whose choice Pane keeps between runs.
//! The chosen greeting style is saved with [`pane_guest::settings`], so it
//! survives restarting Pane and disabling and re-enabling the package.
#![no_std]

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, settings};

/// The settings key holding the chosen greeting style.
const STYLE: &str = "greeting-style";

struct Greeting;
pane_guest::export!(Greeting);

impl Guest for Greeting {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let title = match settings::get(STYLE)? {
            Some(style) => format!("Greeting: {style}"),
            None => "Greeting".into(),
        };
        let item = |id: &str, title: &str, subtitle: &str| Item {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View {
            title,
            items: vec![
                item(
                    "formal",
                    "Use a formal greeting",
                    "Saved in Pane's settings",
                ),
                item(
                    "casual",
                    "Use a casual greeting",
                    "Saved in Pane's settings",
                ),
                item("greet", "Greet me", "Answer in the saved style"),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "formal" | "casual" => {
                settings::set(STYLE, &item_id)?;
                Ok(format!("Saved the {item_id} greeting"))
            }
            "greet" => match settings::get(STYLE)?.as_deref() {
                Some("formal") => Ok("Good day to you".into()),
                Some("casual") => Ok("Hi there".into()),
                _ => Err("No greeting style is saved yet; choose one first".into()),
            },
            other => Err(format!("unknown item: {other}")),
        }
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
