//! Pane's alerts sample (ADR 0053): a short alarm that gets the user's
//! attention while Pane's window is hidden, through the three things an
//! alarm, a timer or a reminder needs.
//!
//! - "Set Short Alarm" (no-view) registers a timer that fires once, 5
//!   seconds later, and says so in a HUD.
//! - When it fires, from a call no window was shown for, the sample posts
//!   a system notification whose click opens "Ringing", starts the bundled
//!   alarm sound ringing until stopped, and shows the launcher on
//!   "Ringing" (`launch`, user-initiated: an attention launch), passing
//!   the context `{"alarm":"Short alarm"}`.
//! - "Ringing" lists Snooze and Dismiss while the alarm rings. Both drop
//!   the ringing handle, which stops the sound; Snooze also sets the alarm
//!   again for a minute later. The handle lives in this component's state,
//!   so the command that stops it is served by the component that rang:
//!   "Ringing" is a command of the same component.
//!
//! None of the alerts' refusals is a failure of the firing: a notification
//! or a sound Pane cannot give here (another system, notifications turned
//! off, no sound output) is written to the extension log, and the launcher
//! is still shown. The JavaScript and TypeScript alerts samples do the
//! same.
#![no_std]

use core::cell::RefCell;

use pane_extension::alerts::{self, BundledSound, Notification, Repeat, Ringing, Sound};
use pane_extension::alloc::{format, string::String};
use pane_extension::commands::{self, CommandRef};
use pane_extension::feedback::{HudStyle, show_hud};
use pane_extension::registrations::{self, Events, Timer};
use pane_extension::{Command, Item, LaunchRecord, LaunchType, List};

/// The "Ringing" command's id in `pane.json`.
const RINGING: &str = "ringing";

/// How long after "Set Short Alarm" the alarm goes off, in seconds.
const ALARM_SECONDS: u64 = 5;

/// How long Snooze puts the alarm off, in seconds.
const SNOOZE_SECONDS: u64 = 60;

/// The alarm's name, as the notification and "Ringing" say it.
const ALARM: &str = "Short alarm";

/// The context the notification's click and the launch pass "Ringing".
const CONTEXT: &str = r#"{"alarm":"Short alarm"}"#;

/// The alarm's timer while it is set, and its sound while it rings: this
/// instance's own state. Dropping either undoes it.
static TIMER: Held<Timer> = Held(RefCell::new(None));
static SOUND: Held<Ringing> = Held(RefCell::new(None));

/// An owned handle this instance keeps, or none.
struct Held<T>(RefCell<Option<T>>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl<T> Sync for Held<T> {}

impl<T> Held<T> {
    /// Keeps `held`, dropping (undoing) what was kept before.
    fn set(&self, held: Option<T>) {
        *self.0.borrow_mut() = held;
    }

    /// Whether a handle is kept.
    fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }
}

struct Alerts;
pane_extension::export!(Alerts);
pane_extension::registrations::export_events!(Events);

/// Sets the alarm to go off `seconds` from now, replacing one already set.
fn set_alarm(seconds: u64) -> Result<(), String> {
    TIMER.set(None);
    TIMER.set(Some(registrations::after(seconds, go_off)?));
    Ok(())
}

/// The alarm going off: from the timer's firing, a call no window was
/// shown for.
async fn go_off() -> Result<(), String> {
    // The sound, kept until Snooze or Dismiss drops it; a sound still
    // ringing from before is dropped by the new one taking its place.
    SOUND.set(None);
    match alerts::ring(
        &Sound::Bundled(BundledSound::Alarm),
        Repeat::UntilStopped,
        100,
    ) {
        Ok(ringing) => SOUND.set(Some(ringing)),
        Err(error) => pane_extension::warn!("The alarm cannot ring: {}", error.message()),
    }
    let notification = Notification::new(ALARM)
        .message("Snooze or dismiss it in Pane")
        .on_click(RINGING, Some(CONTEXT));
    if let Err(error) = alerts::notify(&notification) {
        pane_extension::warn!(
            "The alarm's notification was not posted: {}",
            error.message()
        );
    }
    // The launcher, shown on "Ringing" once the user can take it.
    let ringing = CommandRef {
        source: None,
        command: RINGING.into(),
    };
    commands::launch(&ringing, LaunchType::UserInitiated, &[], Some(CONTEXT))
}

/// The alarm's name in `launch`'s context, if it names one.
fn alarm_of(launch: &LaunchRecord) -> Option<String> {
    let context: serde_json::Value = serde_json::from_str(launch.context.as_deref()?).ok()?;
    context.get("alarm")?.as_str().map(String::from)
}

impl Command for Alerts {
    type DesignedView = pane_extension::view::NoDesignedView;

    /// "Ringing": Snooze and Dismiss while the alarm rings.
    async fn render() -> Result<List, String> {
        let launch = commands::current();
        let title = match alarm_of(&launch) {
            Some(alarm) => format!("Ringing: {alarm}"),
            None => String::from("Ringing"),
        };
        if !SOUND.is_set() {
            return Ok(List::new(title).item(Item::new("quiet", "Nothing is ringing")));
        }
        Ok(List::new(title).items([
            Item::new("snooze", "Snooze for 1 Minute").on_action(|| async {
                SOUND.set(None);
                set_alarm(SNOOZE_SECONDS)?;
                show_hud("Snoozed for 1 minute", HudStyle::Success);
                Ok(())
            }),
            Item::new("dismiss", "Dismiss").on_action(|| async {
                SOUND.set(None);
                show_hud("Alarm dismissed", HudStyle::Success);
                Ok(())
            }),
        ]))
    }

    /// "Set Short Alarm".
    async fn run(command: String, _launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            "set" => {
                set_alarm(ALARM_SECONDS)?;
                show_hud(
                    &format!("Alarm set for {ALARM_SECONDS} seconds"),
                    HudStyle::Success,
                );
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }
}
