//! A command that takes a query, the smallest runnable example of one:
//! Echo shows a toast with the text the user sends it from root search,
//! through its alias ("ec hello" when the user gave it the alias "ec") or by
//! choosing it as a fallback for whatever they typed. Pane sends the text
//! only when the user invokes it that way, never while they type, as the
//! fallback text of its launch record. Echo is a no-view command
//! (`"mode": "no-view"`): it opens no screen, and root search stays as it
//! was while its toast shows.
#![no_std]

use pane_guest::alloc::{format, string::String};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::{Command, LaunchRecord, LaunchType, NoCustomView};

struct Echo;
pane_guest::export!(Echo);

/// The text Echo answers with an error, to show how a failure looks.
const REFUSED: &str = "fail";

/// The text Echo crashes on, to show how Pane pauses a crashing extension.
const CRASH: &str = "crash";

impl Command for Echo {
    type CustomView = NoCustomView;

    /// Shows a toast with the text it was sent; "fail" is refused, to show
    /// how an error looks, and "crash" crashes on purpose (three crashes
    /// within five minutes pause the extension). Launched without text
    /// (Enter on its row), it says how to send it some. Launched in the
    /// background, it shows nothing.
    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        if command != "echo" {
            return Err(format!("unknown command: {command}"));
        }
        let heard = match launch.fallback_text.as_deref() {
            None => NOTHING.into(),
            Some(REFUSED) => {
                return Err(format!(
                    "Echo refuses “{REFUSED}”, to show how an error looks"
                ));
            }
            Some(CRASH) => panic!("Echo crashes on purpose"),
            Some(text) => format!("Echo heard “{text}”"),
        };
        if launch.launch_type != LaunchType::Background {
            show_toast(Toast::success(heard));
        }
        Ok(())
    }
}

/// What Echo says when it was sent no text.
const NOTHING: &str = "Echo heard nothing: give it an alias or make it a fallback in Manage \
                       extensions, then send it text from root search";
