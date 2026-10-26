//! Test fixture: a package whose timer posts system notifications (#555,
//! ADR 0053), as an alarm's would, from a call no window was shown for.
//! Its activation entry point registers a timer that fires every second;
//! each firing posts as many notifications as the settings key `count`
//! says (one by default), titled with the key `title` and saying the key
//! `message`, with a click when the key `click` names a command, passing
//! the key `context` as the launch record's context. What each post
//! answered goes in the fixture's content (`answers`, joined by " | ":
//! "posted", or the refusal's kind and reason), beside the count of
//! firings (`fired`), so the tests read both. Its command's list is
//! titled with the context it was launched with, so a click is seen to
//! open it.
#![no_std]

extern crate alloc;

use core::cell::RefCell;

use pane_extension::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_extension::registrations::{self, Events};
use pane_extension::{Command, List, commands, content, settings};

wit_bindgen::generate!({
    path: "../../pane-extension/wit",
    world: "alerts-user",
});

use pane::extension::alerts::{AlertsError, Notification, NotificationClick, notify};

/// The content key counting the timer's firings.
const FIRED: &str = "fired";

/// The content key holding what the last firing's posts answered.
const ANSWERS: &str = "answers";

/// The activation's timer's handle, kept with the instance: dropping it
/// stops the timer.
static TICKING: Ticking = Ticking(RefCell::new(None));

struct Ticking(RefCell<Option<registrations::Timer>>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl Sync for Ticking {}

struct Fixture;
pane_extension::export!(Fixture);
pane_extension::registrations::export_events!(Events);
pane_extension::lifecycle::export!(Fixture);

impl pane_extension::lifecycle::Guest for Fixture {
    /// The activation entry point: registers the timer whose firings post.
    async fn activate() {
        if let Ok(ticking) = registrations::every(1, || async {
            let answers = post();
            content::set(ANSWERS, &answers)?;
            let fired = counted() + 1;
            content::set(FIRED, &fired.to_string())?;
            Ok(())
        }) {
            *TICKING.0.borrow_mut() = Some(ticking);
        }
    }
}

/// The setting `key`, if saved.
fn setting(key: &str) -> Option<String> {
    settings::get(key).ok().flatten()
}

/// The firings counted so far, zero before the first.
fn counted() -> u64 {
    content::get(FIRED)
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// Posts the notifications the settings say, answering what each post
/// answered.
fn post() -> String {
    let count: u32 = setting("count")
        .and_then(|count| count.parse().ok())
        .unwrap_or(1);
    let mut answers = Vec::new();
    for _ in 0..count {
        let notification = Notification {
            title: setting("title").unwrap_or_else(|| "Timer done".into()),
            message: setting("message"),
            click: setting("click").map(|command| NotificationClick {
                command,
                context: setting("context"),
            }),
        };
        answers.push(match notify(&notification) {
            Ok(()) => "posted".to_string(),
            Err(AlertsError::NotAvailable(why)) => format!("not-available: {why}"),
            Err(AlertsError::Limit(why)) => format!("limit: {why}"),
            Err(AlertsError::Failed(why)) => format!("failed: {why}"),
        });
    }
    answers.join(" | ")
}

impl Command for Fixture {
    type DesignedView = pane_extension::view::NoDesignedView;

    async fn render() -> Result<List, String> {
        let launch = commands::current();
        let context = launch.context.unwrap_or_else(|| "no context".into());
        let launch_type = commands::launch_type_name(launch.launch_type);
        Ok(List::new(format!(
            "Alerts fixture: {context}, {launch_type}"
        )))
    }
}
