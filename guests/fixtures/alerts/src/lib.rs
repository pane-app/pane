//! Test fixture: a package that rings sounds through
//! `pane:extension/alerts` (ADR 0053), keeping each handle in its instance,
//! so the tests can end its code each way it ends and check that the
//! sound stopped. Its own settings say what "Ring the file" plays: the key
//! `file` the path (absolute or relative to the package's folder),
//! `repeat` how often ("once", "until-stopped", or a number of times) and
//! `volume` how loud (0 to 100).
//!
//! Items: "Ring the alarm" rings the bundled alarm until stopped; "Ring the
//! file" rings what the settings say; "Ring the system sound" rings the
//! system's notification sound once; "Drop the sounds" drops every handle
//! held; "Ring and crash" rings the alarm, then traps; "Ring and spin"
//! rings the alarm, then computes without end, so Pane stops it as
//! unresponsive. Each answers a toast: how many sounds the instance holds,
//! or the refusal it met. Root search's query "ring and wait" rings the
//! alarm, then waits 30 seconds before answering, so a test stops the
//! call meanwhile by asking something else.
#![no_std]

extern crate alloc;

use core::cell::RefCell;

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::root::WallTime;
use pane_extension::{Command, List, settings};

mod bindings {
    wit_bindgen::generate!({
        path: "../../pane-extension/wit",
        world: "alerts-user",
    });
}

use bindings::pane::extension::alerts::{AlertsError, BundledSound, Repeat, Ringing, Sound, ring};

/// The handles this instance holds: dropping one stops its sound.
static HELD: Held = Held(RefCell::new(Vec::new()));

struct Held(RefCell<Vec<Ringing>>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl Sync for Held {}

struct Fixture;
pane_extension::export!(Fixture);
pane_extension::root::export!(Fixture);

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

/// Runs the item `item_id` and shows what it answered.
async fn act(item_id: &str) -> Result<(), String> {
    show_toast(Toast::success(outcome(item_id)?));
    Ok(())
}

impl Command for Fixture {
    type DesignedView = pane_extension::view::NoDesignedView;

    async fn render() -> Result<List, String> {
        let item = |id: &'static str, title: &str| {
            pane_extension::Item::new(id, title).on_action(move || act(id))
        };
        Ok(List::new("Alerts fixture")
            .item(item("alarm", "Ring the alarm"))
            .item(item("file", "Ring the file"))
            .item(item("system", "Ring the system sound"))
            .item(item("drop", "Drop the sounds"))
            .item(item("crash", "Ring and crash"))
            .item(item("spin", "Ring and spin")))
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
