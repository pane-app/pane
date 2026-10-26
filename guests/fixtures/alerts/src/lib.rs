//! Test fixture: a package that uses `pane:extension/alerts` (ADR 0053):
//! it rings sounds, keeping each handle in its instance, and its timer
//! posts system notifications.
//!
//! Ringing (#556): the tests end its code each way it ends and check that
//! the sound stopped. Its own settings say what "Ring the file" plays: the
//! key `file` the path (absolute or relative to the package's folder),
//! `repeat` how often ("once", "until-stopped", or a number of times) and
//! `volume` how loud (0 to 100). Items: "Ring the alarm" rings the bundled
//! alarm until stopped; "Ring the file" rings what the settings say; "Ring
//! the system sound" rings the system's notification sound once; "Drop the
//! sounds" drops every handle held; "Ring and crash" rings the alarm, then
//! traps; "Ring and spin" rings the alarm, then computes without end, so
//! Pane stops it as unresponsive. Each answers a toast: how many sounds
//! the instance holds, or the refusal it met, which it also writes to its
//! content (`rang`), where the smokes read it. Root search's query "ring
//! and wait" rings the alarm, then waits 30 seconds before answering, so a
//! test stops the call meanwhile by asking something else.
//!
//! Notifications (#555): a manifest that names the fixture's activation
//! entry point has it register a timer that fires every second, as an
//! alarm's would, from a call no window was shown for. Each firing posts
//! as many notifications as the settings key `count` says (one by
//! default), titled with the key `title` and saying the key `message`,
//! with a click when the key `click` names a command, passing the key
//! `context` as the launch record's context. What each post answered goes
//! in the fixture's content (`answers`, joined by " | ": "posted", or the
//! refusal's kind and reason), beside the count of firings (`fired`). Its
//! command's list is titled with the context it was launched with, so a
//! click is seen to open it.
#![no_std]

extern crate alloc;

use core::cell::RefCell;

use pane_extension::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::registrations::{self, Events};
use pane_extension::root::WallTime;
use pane_extension::{Command, List, commands, content, settings};

use pane_extension::alerts::{
    AlertsError, BundledSound, Notification, NotificationClick, Repeat, Ringing, Sound, notify,
    ring,
};

/// The handles this instance holds: dropping one stops its sound.
static HELD: Held = Held(RefCell::new(Vec::new()));

struct Held(RefCell<Vec<Ringing>>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl Sync for Held {}

/// The content key counting the timer's firings.
const FIRED: &str = "fired";

/// The content key holding what the last firing's posts answered.
const ANSWERS: &str = "answers";

/// The content key holding what the last item answered.
const RANG: &str = "rang";

/// The activation's timer's handle, kept with the instance: dropping it
/// stops the timer.
static TICKING: Ticking = Ticking(RefCell::new(None));

struct Ticking(RefCell<Option<registrations::Timer>>);

// SAFETY: as `Held`.
unsafe impl Sync for Ticking {}

struct Fixture;
pane_extension::export!(Fixture);
pane_extension::root::export!(Fixture);
pane_extension::registrations::export_events!(Events);
pane_extension::lifecycle::export!(Fixture);

/// The refusal `error` as the toast says it: its case, then why.
fn refused(error: AlertsError) -> String {
    match error {
        AlertsError::NotAvailable(why) => format!("refused: not-available: {why}"),
        AlertsError::Limit(why) => format!("refused: limit: {why}"),
        AlertsError::Failed(why) => format!("refused: failed: {why}"),
    }
}

/// Rings `sound` as `repeat` says at `volume`, keeping the handle, and
/// answers how many the instance holds, or the refusal.
fn ring_and_keep(sound: Sound, repeat: Repeat, volume: u8) -> String {
    match ring(&sound, repeat, volume) {
        Ok(ringing) => {
            let mut held = HELD.0.borrow_mut();
            held.push(ringing);
            format!("Ringing {} sounds", held.len())
        }
        Err(error) => refused(error),
    }
}

/// The alarm, until stopped, at full volume.
fn ring_the_alarm() -> String {
    ring_and_keep(
        Sound::Bundled(BundledSound::Alarm),
        Repeat::UntilStopped,
        100,
    )
}

/// The settings value `key`, if set.
fn setting(key: &str) -> Option<String> {
    settings::get(key).ok().flatten()
}

/// What the item `item_id` does, answered as a toast's text.
fn outcome(item_id: &str) -> Result<String, String> {
    match item_id {
        "alarm" => Ok(ring_the_alarm()),
        "file" => {
            let repeat = match setting("repeat").as_deref() {
                None | Some("once") => Repeat::Once,
                Some("until-stopped") => Repeat::UntilStopped,
                Some(times) => Repeat::Times(times.parse().map_err(|_| "a bad `repeat`")?),
            };
            let volume = setting("volume")
                .and_then(|volume| volume.parse().ok())
                .unwrap_or(100);
            Ok(ring_and_keep(
                Sound::File(setting("file").unwrap_or_default()),
                repeat,
                volume,
            ))
        }
        "system" => Ok(ring_and_keep(Sound::System, Repeat::Once, 100)),
        "drop" => {
            HELD.0.borrow_mut().clear();
            Ok("Dropped the sounds".into())
        }
        "crash" => {
            ring_the_alarm();
            panic!("the fixture traps while its sound rings, as the test asked");
        }
        "spin" => {
            ring_the_alarm();
            loop {
                core::hint::spin_loop();
            }
        }
        _ => Err(format!("unknown item: {item_id}")),
    }
}

/// Runs the item `item_id` and shows what it answered, also writing it to
/// the content key [`RANG`], where the smokes read it.
async fn act(item_id: &str) -> Result<(), String> {
    let answer = outcome(item_id)?;
    content::set(RANG, &answer)?;
    show_toast(Toast::success(answer));
    Ok(())
}

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
        let item = |id: &'static str, title: &str| {
            pane_extension::Item::new(id, title).on_action(move || act(id))
        };
        Ok(
            List::new(format!("Alerts fixture: {context}, {launch_type}"))
                .item(item("alarm", "Ring the alarm"))
                .item(item("file", "Ring the file"))
                .item(item("system", "Ring the system sound"))
                .item(item("drop", "Drop the sounds"))
                .item(item("crash", "Ring and crash"))
                .item(item("spin", "Ring and spin")),
        )
    }
}

impl pane_extension::root::Guest for Fixture {
    async fn results_for(
        query: String,
        _at: WallTime,
    ) -> Result<Vec<pane_extension::root::RootResult>, String> {
        if query == "ring and wait" {
            ring_the_alarm();
            wasip3::clocks::monotonic_clock::wait_for(30_000_000_000).await;
        }
        Ok(Vec::new())
    }
}
