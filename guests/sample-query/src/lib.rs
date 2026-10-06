//! A command that takes a query, the smallest runnable example of one:
//! Echo answers the text the user sends it from root search, through its
//! alias ("ec hello" when the user gave it the alias "ec") or by choosing it
//! as a fallback for whatever they typed. Pane sends the text only when the
//! user invokes it that way, never while they type, as the fallback text of
//! its launch record. Echo is a no-view command (`"mode": "no-view"`): it
//! opens no screen, and root search stays as it was while its answer shows.
#![no_std]

use pane_guest::alloc::{format, string::String};
use pane_guest::{Command, LaunchRecord, NoCustomView};

struct Echo;
pane_guest::export!(Echo);

/// The text Echo answers with an error, to show how a failure looks.
const REFUSED: &str = "fail";

/// The text Echo crashes on, to show how Pane pauses a crashing extension.
const CRASH: &str = "crash";

impl Command for Echo {
    type CustomView = NoCustomView;

    /// Answers with the text it was sent; "fail" is refused, to show how an
    /// error looks, and "crash" crashes on purpose (three crashes within
    /// five minutes pause the extension). Launched without text (Enter on
    /// its row), it says how to send it some.
    async fn run(command: String, launch: LaunchRecord) -> Result<String, String> {
        if command != "echo" {
            return Err(format!("unknown command: {command}"));
        }
        let Some(text) = launch.fallback_text else {
            return Ok(NOTHING.into());
        };
        match text.as_str() {
            REFUSED => Err(format!(
                "Echo refuses “{REFUSED}”, to show how an error looks"
            )),
            CRASH => panic!("Echo crashes on purpose"),
            _ => Ok(format!("Echo heard “{text}”")),
        }
    }
}

/// What Echo answers when it was sent no text.
const NOTHING: &str = "Echo heard nothing: give it an alias or make it a fallback in Manage \
                       extensions, then send it text from root search";
