//! System notifications (`notify` of `wit/alerts.wit`, ADR 0053, #555):
//! a notification an extension posts through the system's own
//! notification service, from any call and with no window needed, such
//! as a timer's firing. It is Pane's, under Pane's name and icon, with
//! the extension's title as its attribution, silent, and plain text cut
//! with "…" past [`MAX_TITLE`] and [`MAX_MESSAGE`] characters, never
//! refused for its length.
//!
//! The launcher reaches the system through one trait, [`Notifier`],
//! given to it as the open windows are
//! ([`crate::Launcher::with_notifier`]): the app passes [`native`],
//! tests a recording stand-in, and a launcher given none answers with
//! [`none`]'s refusal. The launcher checks the click and the package's
//! limit ([`PER_MINUTE`] a minute, [`Posted`]) before it asks the
//! notifier, and builds what a click does: it launches the package's
//! command through the launch path a user-initiated `launch` takes, or
//! shows the launcher as a second start does.
//!
//! Only the Windows half is Windows' ([`windows`]): a toast through WinRT's
//! `ToastNotificationManager`, under Pane's AppUserModelID ([`APP_ID`]).
//! Elsewhere [`native`] answers that notifications are not available
//! there yet (#559), which is not a failure.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{adopt_app_id, register_app_id};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Pane's AppUserModelID on Windows: the id the system knows Pane's
/// notifications by, set on Pane's process, carried by the setup's Start
/// menu shortcut and registered for the user (see `windows`). A debug
/// build has its own, so it never takes over the installed Pane's
/// notifications or their settings.
pub const APP_ID: &str = if cfg!(debug_assertions) {
    "PaneApp.Pane.Development"
} else {
    RELEASE_APP_ID
};

/// A release build's [`APP_ID`], the one the setup's shortcut carries.
const RELEASE_APP_ID: &str = "PaneApp.Pane";

/// The name the system shows Pane's notifications under, with [`APP_ID`].
pub const APP_NAME: &str = if cfg!(debug_assertions) {
    "Pane (development)"
} else {
    "Pane"
};

/// The characters of a title past which it is cut with "…".
pub const MAX_TITLE: usize = 200;

/// The characters of a message past which it is cut with "…".
pub const MAX_MESSAGE: usize = 1_000;

/// The notifications a package may post in a minute; one more answers
/// `limit`. The system throttles too, but a bug in a 1-second timer is
/// refused by Pane, not by Windows.
pub const PER_MINUTE: usize = 10;

/// A notification as a command gives it (`notification` in the WIT),
/// before Pane cuts its texts and checks its click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GivenNotification {
    pub title: String,
    pub message: Option<String>,
    pub click: Option<GivenClick>,
}

/// What a command asks a click on its notification to open: one of its
/// package's own commands, by its id in `pane.json`, with the context
/// passed in its launch record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GivenClick {
    pub command: String,
    pub context: Option<String>,
}

/// What clicking a notification does, built by the launcher: launches the
/// package's command, or shows the launcher. Called on whatever thread the
/// system delivers the click on.
pub type Clicked = Arc<dyn Fn() + Send + Sync>;

/// A notification as the notifier posts it: its texts already cut, and
/// what its click does.
#[derive(Clone)]
pub struct Notice {
    pub title: String,
    pub message: Option<String>,
    /// The package's title, shown as the notification's attribution.
    pub attribution: String,
    pub clicked: Clicked,
}

impl std::fmt::Debug for Notice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Notice")
            .field("title", &self.title)
            .field("message", &self.message)
            .field("attribution", &self.attribution)
            .finish_non_exhaustive()
    }
}

/// Why a notification was not posted (`alerts-error` in the WIT).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyError {
    /// It cannot be posted here, and nothing went wrong in the extension:
    /// no adapter on this system, notifications turned off for Pane or
    /// for the system, or no AppUserModelID Windows knows. Says what.
    NotAvailable(String),
    /// The package posted [`PER_MINUTE`] in the last minute.
    Limit(String),
    /// It went wrong: a click naming no command of the package, a context
    /// that is not JSON within the limit, or the system refusing it.
    Failed(String),
}

/// The system's notification service, as the launcher posts to it. A
/// post is short: it answers once the system took the notification, not
/// once the user saw it.
pub trait Notifier: Send + Sync + 'static {
    fn notify(&self, notice: Notice) -> Result<(), NotifyError>;
}

/// This system's notifications: Windows' toasts on Windows; elsewhere one
/// that answers that notifications are not available there yet.
pub fn native() -> Arc<dyn Notifier> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::Toasts::new(APP_ID))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let here = crate::platform::Platform::current().map_or_else(
            || format!("this system ({})", std::env::consts::OS),
            |platform| platform.to_string(),
        );
        Arc::new(Unavailable(format!(
            "System notifications are not available on {here} yet"
        )))
    }
}

/// The notifier of a launcher given none: it posts nothing, saying why.
pub fn none() -> Arc<dyn Notifier> {
    Arc::new(Unavailable(
        "This Pane posts no system notifications for extensions".into(),
    ))
}

/// Posts nothing, saying why.
struct Unavailable(String);

impl Notifier for Unavailable {
    fn notify(&self, _notice: Notice) -> Result<(), NotifyError> {
        Err(NotifyError::NotAvailable(self.0.clone()))
    }
}

/// `text` cut to at most `limit` characters, its last one "…" when it was
/// longer.
pub(crate) fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// The notifications each package posted in the last minute, by package
/// identity key, oldest first: what [`PER_MINUTE`] is counted against.
#[derive(Default)]
pub(crate) struct Posted {
    times: HashMap<String, VecDeque<u64>>,
}

impl Posted {
    /// Counts a post by `package` at `now` (milliseconds) and answers
    /// `true`, or answers `false` when it already posted [`PER_MINUTE`] in
    /// the minute before `now`.
    pub(crate) fn admit(&mut self, package: &str, now: u64) -> bool {
        let times = self.times.entry(package.to_owned()).or_default();
        while times
            .front()
            .is_some_and(|&posted| now.saturating_sub(posted) >= 60_000)
        {
            times.pop_front();
        }
        if times.len() >= PER_MINUTE {
            return false;
        }
        times.push_back(now);
        true
    }
}

/// The toast's XML for `notice`, as Windows reads it: a generic toast
/// with the title, the message and the attribution as plain text, and no
/// sound (the ringing sound's job). Written on every system, so its
/// escaping is tested everywhere.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn toast_xml(notice: &Notice) -> String {
    let mut texts = format!("<text>{}</text>", escaped(&notice.title));
    if let Some(message) = &notice.message {
        texts.push_str(&format!("<text>{}</text>", escaped(message)));
    }
    texts.push_str(&format!(
        "<text placement=\"attribution\">{}</text>",
        escaped(&notice.attribution)
    ));
    format!(
        "<toast><visual><binding template=\"ToastGeneric\">{texts}</binding></visual>\
         <audio silent=\"true\"/></toast>"
    )
}

/// `text` as XML character data and attribute text.
fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            // XML 1.0 has no way to write the other control characters.
            character if character.is_control() && !matches!(character, '\t' | '\n' | '\r') => {
                escaped.push(' ')
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(title: &str, message: Option<&str>) -> Notice {
        Notice {
            title: title.into(),
            message: message.map(str::to_owned),
            attribution: "Timers".into(),
            clicked: Arc::new(|| {}),
        }
    }

    #[test]
    fn overlong_text_is_cut_with_an_ellipsis_to_the_limit() {
        assert_eq!(cut("Timer done", MAX_TITLE), "Timer done");
        let exact = "x".repeat(MAX_TITLE);
        assert_eq!(cut(&exact, MAX_TITLE), exact);
        let long = "é".repeat(MAX_TITLE + 1);
        let cut_title = cut(&long, MAX_TITLE);
        assert_eq!(cut_title.chars().count(), MAX_TITLE);
        assert!(cut_title.ends_with("é…"));
        let message = cut(&"y".repeat(5_000), MAX_MESSAGE);
        assert_eq!(message.chars().count(), MAX_MESSAGE);
        assert!(message.ends_with('…'));
    }

    #[test]
    fn a_package_posts_ten_a_minute() {
        let mut posted = Posted::default();
        for second in 0..PER_MINUTE as u64 {
            assert!(posted.admit("local:timers", second * 1_000));
        }
        // The eleventh within the minute is refused, and not counted.
        assert!(!posted.admit("local:timers", 30_000));
        // Another package has a minute of its own.
        assert!(posted.admit("local:pomodoro", 30_000));
        // A minute after the first, one more may go.
        assert!(posted.admit("local:timers", 60_000));
        assert!(!posted.admit("local:timers", 60_500));
        assert!(posted.admit("local:timers", 61_000));
    }

    #[test]
    fn the_toast_is_silent_plain_text_with_the_attribution() {
        assert_eq!(
            toast_xml(&notice("Tea <ready>", Some("Steeped & \"done\""))),
            "<toast><visual><binding template=\"ToastGeneric\">\
             <text>Tea &lt;ready&gt;</text>\
             <text>Steeped &amp; &quot;done&quot;</text>\
             <text placement=\"attribution\">Timers</text>\
             </binding></visual><audio silent=\"true\"/></toast>"
        );
        let bare = toast_xml(&notice("Up\u{7}", None));
        assert!(bare.contains("<text>Up </text><text placement"), "{bare}");
    }

    /// The setup's Start menu shortcut carries a release build's id, and
    /// the setup registers it, so its uninstall removes the registration.
    #[test]
    fn the_setup_carries_the_app_id() {
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/setup-windows.iss"),
        )
        .unwrap();
        let shortcut = script
            .lines()
            .find(|line| line.starts_with(r#"Name: "{userprograms}\Pane""#))
            .expect("the setup's Start menu shortcut");
        assert!(
            shortcut.ends_with(&format!("AppUserModelID: \"{RELEASE_APP_ID}\"")),
            "{shortcut}"
        );
        assert!(
            script.lines().any(|line| line.starts_with(&format!(
                r#"Root: HKCU; Subkey: "Software\Classes\AppUserModelId\{RELEASE_APP_ID}""#
            )) && line.ends_with("Flags: uninsdeletekey")),
            "the setup registers {RELEASE_APP_ID}"
        );
    }

    #[test]
    fn a_launcher_without_a_notifier_says_so() {
        assert_eq!(
            none().notify(notice("Timer done", None)),
            Err(NotifyError::NotAvailable(
                "This Pane posts no system notifications for extensions".into()
            ))
        );
    }
}
