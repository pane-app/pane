//! Getting the user's attention outside Pane's window
//! (`pane:extension/alerts`, ADR 0053): a system notification
//! ([`notify`]) and a ringing sound the extension owns ([`ring`]), made
//! for a call no window was shown for, such as a timer's firing
//! ([`crate::registrations`]). Showing the launcher on one of the
//! extension's own commands from such a call is
//! [`commands::launch`](crate::commands::launch), user-initiated (an
//! attention launch).
//!
//! ```ignore
//! use pane_extension::alerts::{self, BundledSound, Notification, Repeat, Sound};
//!
//! alerts::notify(&Notification::new("Tea is ready").message("Steeped for 4 minutes"))?;
//! let ringing = alerts::ring(&Sound::Bundled(BundledSound::Alarm), Repeat::UntilStopped, 100)?;
//! // Keep `ringing` in the component's state; dropping it stops the sound.
//! ```
//!
//! A [`Ringing`] is an owned registration (ADR 0041): dropping it stops
//! the sound at once, and so do the instance holding it going away and
//! the package's code being replaced or disabled, so a crashed or disabled
//! extension never rings forever. There is no `stop`. The command that
//! stops it (a "Ringing" screen's Dismiss) is served by the component that
//! rang, which keeps the handle in its state.
//!
//! Every refusal is an [`AlertsError`], never a reason to pause the
//! extension: `not-available` where Pane has no adapter on this system yet
//! (macOS and Linux, for now), notifications are turned off for Pane or
//! there is no sound output; `limit` past 10 notifications a minute or 2
//! ringing sounds for the package (8 in all); `failed` for a sound file
//! that cannot be read, or a click naming no command of the package.

wit_bindgen::generate!({
    path: "wit",
    world: "alerts-user",
    default_bindings_module: "pane_extension::alerts",
});

pub use pane::extension::alerts::{
    AlertsError, BundledSound, Notification, NotificationClick, Repeat, Ringing, Sound, notify,
    ring,
};

use alloc::string::String;

impl AlertsError {
    /// What it says, for the user: why it is not available here, which
    /// limit refused it, or what went wrong.
    pub fn message(&self) -> &str {
        match self {
            AlertsError::NotAvailable(why) | AlertsError::Limit(why) | AlertsError::Failed(why) => {
                why
            }
        }
    }
}

impl Notification {
    /// A notification titled `title`, with no message, whose click shows
    /// the launcher.
    pub fn new(title: impl Into<String>) -> Notification {
        Notification {
            title: title.into(),
            message: None,
            click: None,
        }
    }

    /// This notification with `message` under its title.
    pub fn message(mut self, message: impl Into<String>) -> Notification {
        self.message = Some(message.into());
        self
    }

    /// This notification opening the package's own command `command` (its
    /// id in `pane.json`) when clicked, user-initiated, with `context`
    /// (JSON text) in its launch record.
    pub fn on_click(mut self, command: impl Into<String>, context: Option<&str>) -> Notification {
        self.click = Some(NotificationClick {
            command: command.into(),
            context: context.map(String::from),
        });
        self
    }
}
