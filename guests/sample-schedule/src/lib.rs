//! Pane's schedule sample: a command whose `pane.json` entry declares a
//! schedule, so Pane runs its "Count" action every interval while the
//! package is enabled and not paused, without the user asking. Each run
//! adds one to a count kept in its content and answers the new count, so
//! the run is visible in its saved data and on its screen. Disabling the
//! package stops the schedule; enabling it starts it again, and the
//! interval restarts. A restart schedules again whatever the manifest
//! declares, without replaying work that fell due while Pane was stopped.
//!
//! Its other items stand for the ways a scheduled run can end, for
//! Pane's checks and for trying them by hand: "Run slowly" waits ten
//! seconds, so a disable or reload while it runs stops it (its "finished"
//! is never saved); "Answer an error" answers an error, which never pauses
//! the extension; "Crash" traps, and three crashes within five minutes
//! pause it; "Stop responding" computes without waiting for up to a
//! minute, so Pane stops it after five seconds of its own computing and
//! counts that as a crash too.
#![no_std]

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{
    CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, content, settings,
};

/// The content key holding how many runs the command counted.
const COUNT: &str = "count";
/// The settings key where "Run slowly" notes how far it got.
const SLOW: &str = "slow";
/// How long "Run slowly" waits, in nanoseconds.
const SLOW_WAIT: u64 = 10_000_000_000;
/// How long "Stop responding" computes at most, in nanoseconds: bounded,
/// so that even without Pane stopping it, it ends.
const BUSY_FOR: u64 = 60_000_000_000;

struct Counting;
pane_guest::export!(Counting);

/// The count kept in the command's content.
fn count() -> Result<u64, String> {
    content::get(COUNT)?
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "the count is not a number".into())
        })
        .transpose()
        .map(|count| count.unwrap_or(0))
}

impl Guest for Counting {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let runs = count()?;
        let item = |id: &str, title: &str, subtitle: &str| Item {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View {
            title: format!("Ran {runs} times"),
            items: vec![
                item(
                    "count",
                    "Run now",
                    "What the schedule runs; it adds one to the count and answers it",
                ),
                item(
                    "slow",
                    "Run slowly",
                    "Waits 10 seconds, then answers; disabling or reloading stops it",
                ),
                item(
                    "refuse",
                    "Answer an error",
                    "An error the extension answers with never pauses it",
                ),
                item(
                    "crash",
                    "Crash",
                    "Crashes on purpose; three crashes within five minutes pause the extension",
                ),
                item(
                    "busy",
                    "Stop responding",
                    "Computes without waiting for up to a minute; Pane stops it after 5 seconds, \
                     counted as a crash",
                ),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "count" | "slow" => {
                // One more run: counted before anything else, so a run Pane
                // stops on the way still counts as having begun.
                let runs = count()? + 1;
                content::set(COUNT, &format!("{runs}"))?;
                if item_id == "slow" {
                    settings::set(SLOW, "started")?;
                    // The guest suspends here; if Pane stops the call meanwhile,
                    // nothing after this line runs.
                    wasip3::clocks::monotonic_clock::wait_for(SLOW_WAIT).await;
                    settings::set(SLOW, "finished")?;
                    return Ok(format!("Ran {runs} times, after waiting 10 seconds"));
                }
                Ok(format!("Ran {runs} times"))
            }
            "refuse" => Err("The schedule sample refuses, to show how an error looks".into()),
            // The run is counted, then the panic traps the guest: Pane
            // reports a crash, not an error the extension answered with.
            "crash" => {
                let runs = count()? + 1;
                content::set(COUNT, &format!("{runs}"))?;
                panic!("crashed on purpose");
            }
            // The run is counted, then it computes without awaiting
            // anything: the guest never yields to Pane by itself, so Pane
            // stops it after its computing limit and counts it towards
            // pausing the package, as a crash.
            "busy" => {
                let runs = count()? + 1;
                content::set(COUNT, &format!("{runs}"))?;
                let now = wasip3::clocks::monotonic_clock::now;
                let end = now() + BUSY_FOR;
                while now() < end {}
                Ok(format!("Ran {runs} times"))
            }
            other => Err(format!("unknown item: {other}")),
        }
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "The schedule sample has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("The schedule sample has no custom views".into())
    }
}
