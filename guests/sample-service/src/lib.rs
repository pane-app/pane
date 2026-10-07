//! Pane's service sample: a command whose `pane.json` entry declares a
//! continuing service, so Pane runs its cycles while the package is enabled
//! and not paused, without the user asking and at no interval the manifest
//! declares: each cycle answers the status to show and asks Pane when to
//! run the next. The service watches a count of events kept in its
//! content — an item adds one, standing for whatever the service watches —
//! and counts its cycles, in its content for all time and in its instance
//! for this run: the second count is the state of the task the service
//! manages, which lives as long as the code's generation and is dropped
//! with it, so a disable, a reload or a pause ends it and enabling or
//! retrying starts a fresh one. Disabling the package stops the service;
//! enabling it starts it again; a restart starts it again where the
//! package is enabled.
//!
//! Its items stand for the ways a cycle can end, for Pane's checks and for
//! trying them by hand: "Wait on the next cycle" makes the next cycle
//! wait ten seconds, so a disable or reload while it runs stops it (its
//! "finished" is never saved); "Fail the next cycle" answers an error,
//! which never pauses the extension; "Crash the next cycle" traps, and
//! three crashes within five minutes pause it; "Stop responding on the
//! next cycle" computes without waiting for up to a minute, so Pane stops
//! it after five seconds of its own computing and counts that as a crash
//! too; and "Ask for a 0-second cadence" and "Ask for a 31-day cadence"
//! make the next cycle answer cadences beyond Pane's bounds, which it
//! clamps to its 1-second minimum and 30-day maximum.
#![no_std]

use core::sync::atomic::{AtomicU64, Ordering};

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::{
    Command, CustomView, FieldValue, FormError, Item, List, NoCustomView, content, settings,
};

/// The content key holding how many cycles the service has run, ever.
const CYCLES: &str = "cycles";
/// The content key holding how many events the service is watching, added
/// by its "Add an event" item.
const EVENTS: &str = "events";
/// The settings key holding what the next service cycle does: "" (nothing,
/// its usual work), "slow", "fail", "crash", "busy", "fast" or "far". Each
/// is armed once, by its item, and read and cleared by the cycle that does
/// it.
const MODE: &str = "mode";
/// The settings key where a waiting cycle notes how far it got.
const SLOW: &str = "slow";
/// How long a waiting cycle waits, in nanoseconds.
const SLOW_WAIT: u64 = 10_000_000_000;
/// How long a computing cycle computes at most, in nanoseconds: bounded,
/// so that even without Pane stopping it, it ends.
const BUSY_FOR: u64 = 60_000_000_000;
/// The cadence the service asks Pane for, in seconds: how it paces itself
/// until an answer says another.
const EVERY: u64 = 1;
/// The cadence a "0-second" cycle answers: none at all, which Pane clamps
/// to its 1-second minimum, so a service can never busy-loop itself.
const AT_ONCE: u64 = 0;
/// The cadence a "31-day" cycle answers: beyond the 30-day maximum Pane
/// runs a service at, which it clamps to, so a service always runs again.
const TOO_FAR: u64 = 31 * 86_400;

/// How many cycles this instance of the service has run: the state of the
/// task it manages. It lives in the instance's own memory, so it begins
/// again whenever Pane starts the code afresh (a new generation after a
/// disable, a reload, a pause, a restart or a crash) and carries on
/// between cycles while the generation lasts.
static THIS_RUN: AtomicU64 = AtomicU64::new(0);

struct Watching;
pane_guest::export!(Watching);
pane_guest::service::export!(Watching);

/// The count kept in the command's content.
fn counted(key: &str) -> Result<u64, String> {
    content::get(key)?
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "the count is not a number".into())
        })
        .transpose()
        .map(|count| count.unwrap_or(0))
}

/// Runs the action of the item `item_id` and shows a toast with what
/// [`outcome`] answers; each item's action is this with its id.
async fn act(item_id: &str) -> Result<(), String> {
    let done = outcome(item_id)?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the action of the item `item_id` does, answering what it changed.
fn outcome(item_id: &str) -> Result<String, String> {
    match item_id {
        "add" => {
            let events = counted(EVENTS)? + 1;
            content::set(EVENTS, &format!("{events}"))?;
            Ok(format!("Added event {events}; the next cycle reports it"))
        }
        "slow" => {
            settings::set(MODE, "slow")?;
            Ok("The next cycle will wait 10 seconds".into())
        }
        "fail" => {
            settings::set(MODE, "fail")?;
            Ok("The next cycle will answer an error".into())
        }
        "crash" => {
            settings::set(MODE, "crash")?;
            Ok("The next cycle will crash".into())
        }
        "busy" => {
            settings::set(MODE, "busy")?;
            Ok("The next cycle will stop responding".into())
        }
        "fast" => {
            settings::set(MODE, "fast")?;
            Ok("The next cycle will answer 0 seconds".into())
        }
        "far" => {
            settings::set(MODE, "far")?;
            Ok("The next cycle will answer 31 days".into())
        }
        other => Err(format!("unknown item: {other}")),
    }
}

impl Command for Watching {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let cycles = counted(CYCLES)?;
        let events = counted(EVENTS)?;
        let item = |id: &'static str, title: &str, subtitle: &str| {
            Item::new(id, title)
                .subtitle(subtitle)
                .on_action(move || act(id))
        };
        Ok(
            List::new(format!("Watching: {events} events ({cycles} cycles)")).items([
                item(
                    "add",
                    "Add an event",
                    "What the service watches; the next cycle reports it",
                ),
                item(
                    "slow",
                    "Wait on the next cycle",
                    "The next cycle waits 10 seconds; disabling or reloading stops it",
                ),
                item(
                    "fail",
                    "Fail the next cycle",
                    "The next cycle answers an error, which never pauses the extension",
                ),
                item(
                    "crash",
                    "Crash the next cycle",
                    "The next cycle crashes on purpose; three crashes within five minutes \
                     pause the extension",
                ),
                item(
                    "busy",
                    "Stop responding on the next cycle",
                    "The next cycle computes without waiting; Pane stops it after 5 seconds, \
                     counted as a crash",
                ),
                item(
                    "fast",
                    "Ask for a 0-second cadence",
                    "The next cycle answers 0; Pane runs it no sooner than its 1-second minimum",
                ),
                item(
                    "far",
                    "Ask for a 31-day cadence",
                    "The next cycle answers 31 days; Pane clamps it to its 30-day maximum",
                ),
            ]),
        )
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "The service sample has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("The service sample has no custom views".into())
    }
}

impl pane_guest::service::Guest for Watching {
    /// One cycle of the service: it counts itself (in its content for all
    /// time, in its instance for this run) and answers the status to show
    /// and when to run the next. The mode an item armed makes this cycle
    /// wait, answer an error, crash or stop responding instead.
    async fn run_cycle(command: String) -> Result<pane_guest::service::Cycle, String> {
        if command != "watching" {
            return Err(format!("unknown command: {command}"));
        }
        // One more cycle: counted before anything else, so a cycle Pane
        // stops on the way still counts as having begun.
        let cycles = counted(CYCLES)? + 1;
        content::set(CYCLES, &format!("{cycles}"))?;
        let this_run = THIS_RUN.fetch_add(1, Ordering::Relaxed) + 1;
        let mode = settings::get(MODE)?.unwrap_or_default();
        if !mode.is_empty() {
            settings::set(MODE, "")?;
        }
        let status = |waited: bool, next: u64| {
            let events = counted(EVENTS)?;
            let wait = if waited {
                ", after waiting 10 seconds"
            } else {
                ""
            };
            Ok(pane_guest::service::Cycle {
                status: format!(
                    "Watching: {events} events (cycle {cycles}, {this_run} this run){wait}"
                ),
                next_seconds: next,
            })
        };
        match mode.as_str() {
            "slow" => {
                settings::set(SLOW, "started")?;
                // The guest suspends here; if Pane stops the cycle meanwhile,
                // nothing after this line runs.
                wasip3::clocks::monotonic_clock::wait_for(SLOW_WAIT).await;
                settings::set(SLOW, "finished")?;
                status(true, EVERY)
            }
            "fail" => Err("The service sample refuses, to show how an error looks".into()),
            // The cycle is counted, then the panic traps the guest: Pane
            // reports a crash, not an error the extension answered with.
            "crash" => panic!("crashed on purpose"),
            // The cycle is counted, then it computes without awaiting
            // anything: the guest never yields to Pane by itself, so Pane
            // stops it after its computing limit and counts it towards
            // pausing the package, as a crash.
            "busy" => {
                let now = wasip3::clocks::monotonic_clock::now;
                let end = now() + BUSY_FOR;
                while now() < end {}
                status(false, EVERY)
            }
            // Cadences beyond Pane's bounds, clamped by it: a service that
            // asks for no wait at all still runs no sooner than every
            // second, and one that asks for more than 30 days still runs
            // again.
            "fast" => status(false, AT_ONCE),
            "far" => status(false, TOO_FAR),
            _ => status(false, EVERY),
        }
    }
}
