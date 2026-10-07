//! Pane's helper sample: a command that runs a native helper its package
//! ships ([`pane_guest::helpers::run`]). The helper, `pane-echo`
//! (`guests/helpers/echo`), is an ordinary program built for each system;
//! the package's `pane.json` names its file for each target under
//! `helpers`, and Pane runs the one for the system it runs on. The command
//! itself stays a WASI 0.3 component.
//!
//! - "Echo through the helper" shows the helper's answer in a toast, which
//!   names the system it was built for.
//! - "Echo after waiting" notes in its settings that it started, has the
//!   helper wait ten seconds, then notes that it finished; disabling or
//!   reloading the package meanwhile ends the helper's process, and it never
//!   finishes (the "started" note is kept).
//! - "Echo within a second" races the same slow helper against a one-second
//!   timer and drops the run when the timer wins: dropping it cancels it,
//!   and Pane ends the process. This is the command's own timeout: Pane sets
//!   none on a helper.
//! - "Echo after a long wait" has the helper wait forty seconds, longer than
//!   the thirty Pane once allowed, then notes in its settings that it
//!   finished and shows the answer; other extensions' calls are served
//!   meanwhile. (Tests end the wait early through the helper's release file
//!   instead of waiting it out.)
//! - "Make the helper fail" and "Run an undeclared helper" show how Pane
//!   explains a failed or missing helper.
#![no_std]

use core::future::Future;
use core::pin::pin;
use core::task::Poll;

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::helpers::{self, HelperError};
use pane_guest::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView, settings};

/// The helper's name in the package's `pane.json`.
const ECHO: &str = "echo";
/// The settings key where "Echo after waiting" notes how far it got.
const WAITING: &str = "helper-wait";
/// The settings key where "Echo after a long wait" notes that it finished.
const LONG_WAIT: &str = "helper-long-wait";
/// How long "Echo within a second" lets the helper run, in nanoseconds.
const LIMIT: u64 = 1_000_000_000;

struct HelperSample;
pane_guest::export!(HelperSample);

/// `<kind>: <message>`, such as "failed: helper `echo` failed (exit code 3)".
fn explain(error: HelperError) -> String {
    format!("{}: {}", error.kind.name(), error.message)
}

/// Runs the echo helper with `args` and `input`.
async fn echo(args: &[&str], input: &str) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).into()).collect();
    helpers::run(ECHO.into(), args, input.into())
        .await
        .map_err(explain)
}

/// Waits for `first` or `second`, whichever finishes first, and drops the
/// other.
async fn race<A, B>(
    first: impl Future<Output = A>,
    second: impl Future<Output = B>,
) -> Result<A, B> {
    let (mut first, mut second) = (pin!(first), pin!(second));
    core::future::poll_fn(|cx| {
        if let Poll::Ready(value) = first.as_mut().poll(cx) {
            return Poll::Ready(Ok(value));
        }
        if let Poll::Ready(value) = second.as_mut().poll(cx) {
            return Poll::Ready(Err(value));
        }
        Poll::Pending
    })
    .await
}

/// Runs the action of the item `item_id` and shows a toast with what
/// [`outcome`] answers; each item's action is this with its id.
async fn act(item_id: &str) -> Result<(), String> {
    let done = outcome(item_id).await?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the item `item_id` does, answering the helper's answer or what
/// became of the run.
async fn outcome(item_id: &str) -> Result<String, String> {
    match item_id {
        "echo" => echo(&[], "hello from Pane").await,
        "wait" => {
            settings::set(WAITING, "started")?;
            // If Pane stops the call meanwhile, the helper's process
            // ends and nothing after this line runs.
            let answer = echo(&["--wait", "10"], "after waiting").await?;
            settings::set(WAITING, "finished")?;
            Ok(answer)
        }
        "limit" => {
            let slow = echo(&["--wait", "10"], "too late");
            let timer = wasip3::clocks::monotonic_clock::wait_for(LIMIT);
            match race(slow, timer).await {
                Ok(answer) => answer,
                // The run was dropped when the timer won: Pane ended the
                // helper's process.
                Err(()) => Ok("Stopped the helper after one second".into()),
            }
        }
        "long" => {
            let answer = echo(&["--wait", "40"], "after a long wait").await?;
            settings::set(LONG_WAIT, "finished")?;
            Ok(answer)
        }
        "fail" => echo(&["--fail"], "").await,
        "undeclared" => helpers::run("absent".into(), Vec::new(), String::new())
            .await
            .map_err(explain),
        other => Err(format!("unknown item: {other}")),
    }
}

impl Command for HelperSample {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let item = |id: &'static str, title: &str, subtitle: &str| {
            Item::new(id, title)
                .subtitle(subtitle)
                .on_action(move || act(id))
        };
        Ok(List::new("Helper sample").items([
            item(
                "echo",
                "Echo through the helper",
                "Runs the package's native helper for this system",
            ),
            item(
                "wait",
                "Echo after waiting",
                "The helper waits 10 seconds; disabling or reloading stops it",
            ),
            item(
                "limit",
                "Echo within a second",
                "Cancels the slow helper after one second",
            ),
            item(
                "long",
                "Echo after a long wait",
                "The helper waits 40 seconds; other extensions answer meanwhile",
            ),
            item(
                "fail",
                "Make the helper fail",
                "The helper exits with an error",
            ),
            item(
                "undeclared",
                "Run an undeclared helper",
                "The package's pane.json declares no helper by that name",
            ),
        ]))
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
