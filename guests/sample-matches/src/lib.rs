//! A sample of a command's `when` and `matches` in `pane.json` (#195): when
//! root search lists a command — with a blank query, only while searching,
//! or both — and what it matches: its title as usual, only URL-like
//! queries, or only path-like queries. Four no-view commands share this
//! component:
//!
//! - **Hear an Address** (`url`, `"matches": "url"`): listed only for a
//!   query that is a typed web address, which root search parses (inferring
//!   `https://` before a bare domain) and sends it as its launch record's
//!   fallback text; never found by its title.
//! - **Hear a Path** (`path`, `"matches": "file-path"`): the same for a
//!   typed path, resolved (`~` to the home folder, `file://` taken off) and
//!   sent as the fallback text.
//! - **Blank Only** (`blank`, `"when": "blank"`): listed only while nothing
//!   is typed, so no query ever finds it by its title.
//! - **Searching Only** (`searching`, `"when": "searching"`): listed only
//!   while something is typed, so the blank query's list does not hold it.
//!
//! Each answers the text it was sent with a toast ("Heard “…”", or "Heard
//! nothing" without any), as Echo (the query sample) does, so what a typed
//! query sent is seen. Toasts and errors match the JavaScript and
//! TypeScript matches samples.
#![no_std]

use pane_extension::alloc::{format, string::String};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, LaunchRecord, LaunchType, NoCustomView};

struct Matches;
pane_extension::export!(Matches);

/// What every command says when it was sent no text.
const NOTHING: &str = "Heard nothing: a typed web address or path is what reaches these commands";

impl Command for Matches {
    type CustomView = NoCustomView;

    /// Shows a toast with the text the command was sent — the address or
    /// path the query was, for a command declared for one (#195). Launched
    /// without text (Enter on its row), it says so; launched in the
    /// background, it shows nothing.
    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            "url" | "path" | "blank" | "searching" => {}
            other => return Err(format!("unknown command: {other}")),
        }
        let heard = match launch.fallback_text {
            Some(text) => format!("Heard “{text}”"),
            None => String::from(NOTHING),
        };
        if launch.launch_type != LaunchType::Background {
            show_toast(Toast::success(heard));
        }
        Ok(())
    }
}
