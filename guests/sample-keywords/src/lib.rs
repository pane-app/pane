//! A sample of the keywords a command declares in `pane.json` (#197), and
//! of the alternate titles and keywords of the indexed results a command
//! supplies. Two commands share this component:
//!
//! - **Empty the Bin** (`bin`, `"mode": "no-view"`, `"keywords": ["trash",
//!   "rubbish"]`): its keywords find it in root search as its subtitle
//!   would — "trash" lists it although no word of its title or subtitle
//!   holds those letters — and running it empties the bin, saying so.
//! - **Moons** (`moons`, `"mode": "provider"`, `"indexedResults": true`):
//!   a root provider with no row of its own, supplying one indexed
//!   result, "The Moon", found by its title ("moon"), its alternate
//!   title ("luna") as a title is, and its keywords ("satellite",
//!   "rock") as a subtitle is, opening a web address when invoked.
//!
//! Toasts and errors match the JavaScript and TypeScript keywords
//! samples.
#![no_std]

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::indexed::{IndexedAction, IndexedResult, OpenTarget};
use pane_extension::{Command, LaunchRecord, NoCustomView};

struct Keywords;
pane_extension::export!(Keywords);
pane_extension::indexed::export!(Keywords);

impl Command for Keywords {
    type CustomView = NoCustomView;

    /// Empties the bin, saying so. The provider command is never run.
    async fn run(command: String, _launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            "bin" => {
                show_toast(Toast::success("Emptied the bin"));
                Ok(())
            }
            "moons" => Err(String::from("the Moons command only answers root search")),
            other => Err(format!("unknown command: {other}")),
        }
    }
}

impl pane_extension::indexed::Guest for Keywords {
    /// The one result the "Moons" command supplies: found by its title,
    /// its alternate titles and its keywords, opening a web address when
    /// invoked.
    async fn results() -> Result<Vec<IndexedResult>, String> {
        Ok(vec![IndexedResult {
            id: String::from("moon"),
            title: String::from("The Moon"),
            subtitle: Some(String::from("What the keywords sample supplies")),
            alternate_titles: vec![String::from("Luna")],
            keywords: vec![String::from("satellite"), String::from("rock")],
            action: IndexedAction::Open(OpenTarget {
                target: String::from("https://example.com/moon"),
                application: None,
            }),
        }])
    }
}
