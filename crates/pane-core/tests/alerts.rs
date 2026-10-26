//! The alerts sample (ADR 0053, #558) in Rust, JavaScript and TypeScript,
//! real guests `cargo xtask guests` assembles, through the launcher's
//! public interface, with a clock the tests set, the stand-in notifier
//! and the runtime's stand-in sound output, so nothing is shown on the
//! desktop and nothing is heard.
//!
//! "Set Short Alarm" registers a timer for 5 seconds later. When it
//! fires, while the launcher is hidden, the sample rings the alarm until
//! stopped, posts a notification whose click opens "Ringing", and asks
//! for the launcher on "Ringing" (an attention launch) with the context
//! `{"alarm":"Short alarm"}`. Its Dismiss stops the sound; its Snooze
//! stops it and sets the alarm again for a minute later. The three
//! languages pass the same checks.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::feedback::WindowPresence;
use pane_core::system_notifications::{Notice, Notifier, NotifyError};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, StandInSound, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::RecordingWindow;
use rows::{select_title, titles};

/// One language's alerts sample package.
struct Sample {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title, which its notifications are attributed to.
    title: &'static str,
    /// Its commands' titles in root search.
    set: &'static str,
    ringing: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-alerts",
        title: "Rust alerts sample",
        set: "Set Short Alarm",
        ringing: "Ringing",
    },
    Sample {
        package: "sample-alerts-js",
        title: "JavaScript alerts sample",
        set: "Set Short Alarm (JavaScript)",
        ringing: "Ringing (JavaScript)",
    },
    Sample {
        package: "sample-alerts-ts",
        title: "TypeScript alerts sample",
        set: "Set Short Alarm (TypeScript)",
        ringing: "Ringing (TypeScript)",
    },
];

/// How long a guest call, a firing, a launch or a sound starting or
/// stopping may take: compiling the guest once is included; a slow, busy
/// machine is not.
const PROMPTLY: Duration = Duration::from_secs(30);

/// How long after "Set Short Alarm" the alarm goes off.
const ALARM: Duration = Duration::from_secs(5);

/// How long Snooze puts the alarm off.
const SNOOZE: Duration = Duration::from_secs(60);

/// The keyboard and the pointer still for long enough to show an
/// attention launch.
const STILL: Option<Duration> = Some(Duration::from_secs(5));

/// The title "Ringing" shows for the sample's context.
const RINGING_TITLE: &str = "Ringing: Short alarm";

/// The notifier standing in for the system's: it records each notice
/// that reaches it, and posts every one.
#[derive(Default)]
struct Recording {
    posted: Mutex<Vec<Notice>>,
}

impl Recording {
    fn posted(&self) -> Vec<Notice> {
        self.posted.lock().unwrap().clone()
    }
}

impl Notifier for Recording {
    fn notify(&self, notice: Notice) -> Result<(), NotifyError> {
        self.posted.lock().unwrap().push(notice);
        Ok(())
    }
}

/// One test's Pane, with one language's sample installed and its window
/// hidden.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    _cache: TempDir,
    sample: &'static Sample,
    clock: Arc<ManualClock>,
    notifier: Arc<Recording>,
    sound: Arc<StandInSound>,
    runtime: Runtime,
    launcher: Launcher,
    window: Arc<RecordingWindow>,
    identity: PackageIdentity,
}

impl Pane {
    fn new(sample: &'static Sample) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let clock = ManualClock::at(SystemClock.now() + 365 * 86_400_000);
        let notifier = Arc::new(Recording::default());
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        let sound = StandInSound::new();
        runtime.use_stand_in_sound(sound.clone());
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
                .with_clock(clock.clone())
                .with_notifier(notifier.clone());
        let window = RecordingWindow::attach(&launcher);
        let folder = sources.path().join(sample.package);
        copy(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/guests/packages")
                .join(sample.package),
            &folder,
        );
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", sample.title))
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        Pane {
            _sources: sources,
            _data: data,
            _cache: cache,
            sample,
            clock,
            notifier,
            sound,
            runtime,
            launcher,
            window,
            identity,
        }
    }

    /// Chooses the root search row titled `title`.
    fn choose(&self, title: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(""));
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
    }

    /// The titles of the HUDs shown since this was last asked.
    fn huds(&self) -> Vec<String> {
        let huds = self.window.huds();
        self.window.take();
        huds.into_iter().map(|hud| hud.title).collect()
    }

    /// The user runs "Set Short Alarm", and the launcher is hidden: the
    /// sample's timer is registered, nothing rings yet.
    fn set_alarm(&self) {
        self.choose(self.sample.set);
        until("the alarm's timer", || {
            self.launcher.undo_list(&self.identity).contains(&"timer")
        });
        assert_eq!(self.huds(), ["Alarm set for 5 seconds"]);
        self.launcher.set_window_presence(WindowPresence::Hidden);
        self.settled();
        assert_eq!(self.sound.playing(), 0);
        assert!(self.notifier.posted().is_empty());
    }

    /// The clock moves on by `by`, and every timer it made due has fired.
    fn advance(&self, by: Duration) {
        self.clock.advance(by);
        self.settled();
    }

    /// Waits until the timers thread looked at every change of the clock
    /// so far, and every firing it started has reported.
    fn settled(&self) {
        assert!(
            self.launcher.wait_for_timers(PROMPTLY),
            "the timers did not settle"
        );
    }

    /// The alarm goes off: the sound rings, owned by the package, and the
    /// launcher waits to be shown on "Ringing".
    fn goes_off(&self) {
        until("the alarm to ring", || {
            self.sound.playing() == 1 && self.sound.frames().iter().all(|frames| *frames > 0)
        });
        assert_eq!(
            self.launcher
                .undo_list(&self.identity)
                .iter()
                .filter(|what| **what == "ringing sound")
                .count(),
            1
        );
        until("the attention launch", || self.launcher.attention_waiting());
        // The firing opens nothing by itself while the window is hidden.
        assert_ne!(self.launcher.view().screen, Screen::Command);
    }

    /// The window asks to show the waiting launch, the user still: the
    /// launcher shows "Ringing" with the alarm's context and the focus.
    fn shows_ringing(&self) {
        assert!(self.launcher.attend(STILL), "the attention launch is shown");
        assert!(self.launcher.wait_for_launches(PROMPTLY));
        assert!(
            self.launcher.take_window_request(),
            "the window is asked for"
        );
        assert!(
            self.launcher.take_focus_request(),
            "with the keyboard focus"
        );
        self.launcher.set_window_presence(WindowPresence::Shown);
        let view = self.launcher.view();
        assert_eq!(view.screen, Screen::Command, "{view:?}");
        assert_eq!(view.title, RINGING_TITLE);
        assert_eq!(titles(&self.launcher), ["Snooze for 1 Minute", "Dismiss"]);
    }

    /// The notification the alarm posted, the `count`th: its title,
    /// message and attribution.
    fn notified(&self, count: usize) -> Notice {
        let posted = self.notifier.posted();
        assert_eq!(posted.len(), count, "{posted:?}");
        let notice = posted[count - 1].clone();
        assert_eq!(notice.title, "Short alarm");
        assert_eq!(
            notice.message.as_deref(),
            Some("Snooze or dismiss it in Pane")
        );
        assert_eq!(notice.attribution, self.sample.title);
        notice
    }

    /// The user chooses `item` on "Ringing".
    fn choose_on_ringing(&self, item: &str) {
        select_title(&self.launcher, item);
        block_on(self.launcher.activate_selected());
    }

    /// The sound stopped: no stream is open, nothing more is written, the
    /// runtime holds no ringing and the undo list none.
    fn assert_quiet(&self) {
        until("the sound to stop", || self.sound.playing() == 0);
        let written = self.sound.frames();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(self.sound.frames(), written, "the sound went on");
        until("the ringing to be let go", || {
            self.runtime.ringing_sounds() == 0
        });
        assert!(
            !self
                .launcher
                .undo_list(&self.identity)
                .contains(&"ringing sound"),
            "{:?}",
            self.launcher.undo_list(&self.identity)
        );
    }
}

/// Copies the files of `from` into the new folder `to`.
fn copy(from: &Path, to: &Path) {
    assert!(
        from.exists(),
        "{} is missing; run `cargo xtask guests`",
        from.display()
    );
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

/// Waits until `done`, saying `what` it waited for.
fn until(what: &str, mut done: impl FnMut() -> bool) {
    let began = Instant::now();
    while !done() {
        assert!(began.elapsed() < PROMPTLY, "waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// The alarm goes off once its 5 seconds are up, not before: it rings,
/// posts its notification and shows the launcher on "Ringing" with its
/// context; Dismiss stops the sound. Clicking the notification afterwards
/// opens "Ringing" with the same context, at once.
#[test]
fn the_alarm_rings_notifies_and_shows_ringing_and_dismiss_stops_it() {
    for sample in &SAMPLES {
        let pane = Pane::new(sample);
        pane.set_alarm();
        pane.advance(ALARM - Duration::from_secs(1));
        assert_eq!(pane.sound.playing(), 0, "{}: rang early", sample.title);
        assert!(pane.notifier.posted().is_empty());

        pane.advance(Duration::from_secs(1));
        pane.goes_off();
        let notice = pane.notified(1);
        pane.shows_ringing();

        pane.choose_on_ringing("Dismiss");
        pane.assert_quiet();
        assert_eq!(pane.huds(), ["Alarm dismissed"]);
        // Nothing rings again: the alarm is not set any more.
        pane.advance(SNOOZE);
        assert_eq!(pane.sound.playing(), 0);
        assert_eq!(pane.notifier.posted().len(), 1);

        // The notification's click opens "Ringing" with the alarm's
        // context, now that nothing rings.
        pane.launcher.set_window_presence(WindowPresence::Hidden);
        (notice.clicked)();
        assert!(
            pane.launcher.wait_for_launches(PROMPTLY),
            "the click's launch"
        );
        let view = pane.launcher.view();
        assert_eq!(view.screen, Screen::Command, "{}: {view:?}", sample.title);
        assert_eq!(view.title, RINGING_TITLE);
        assert_eq!(titles(&pane.launcher), ["Nothing is ringing"]);
        assert!(pane.launcher.take_window_request());

        // Opened from root search, it has no context to name the alarm by.
        pane.choose(sample.ringing);
        assert!(pane.launcher.wait_for_launches(PROMPTLY));
        assert_eq!(pane.launcher.view().title, "Ringing");
        assert_eq!(titles(&pane.launcher), ["Nothing is ringing"]);
    }
}

/// Snooze stops the sound and sets the alarm again: a minute later it
/// rings, notifies and shows "Ringing" again, and Dismiss then stops it.
#[test]
fn snooze_stops_the_sound_and_rings_again_a_minute_later() {
    for sample in &SAMPLES {
        let pane = Pane::new(sample);
        pane.set_alarm();
        pane.advance(ALARM);
        pane.goes_off();
        pane.notified(1);
        pane.shows_ringing();

        pane.choose_on_ringing("Snooze for 1 Minute");
        pane.assert_quiet();
        assert_eq!(pane.huds(), ["Snoozed for 1 minute"]);
        pane.launcher.set_window_presence(WindowPresence::Hidden);
        // The user left "Ringing": the next attention launch may show.
        while !matches!(pane.launcher.view().screen, Screen::Root { .. }) {
            pane.launcher.back();
        }

        pane.advance(SNOOZE - Duration::from_secs(1));
        assert_eq!(pane.sound.playing(), 0, "{}: rang early", sample.title);
        pane.advance(Duration::from_secs(1));
        pane.goes_off();
        pane.notified(2);
        pane.shows_ringing();
        pane.choose_on_ringing("Dismiss");
        pane.assert_quiet();
    }
}

/// The handle lives in the instance that rang: disabling the package ends
/// its generation, which stops the sound and drops the launch still
/// waiting, without the user doing anything.
#[test]
fn disabling_the_package_stops_the_alarm() {
    for sample in &SAMPLES {
        let pane = Pane::new(sample);
        pane.set_alarm();
        pane.advance(ALARM);
        pane.goes_off();
        block_on(pane.launcher.set_enabled(&pane.identity, false));
        pane.assert_quiet();
        assert!(
            !pane.launcher.attention_waiting(),
            "{}: the waiting launch is dropped",
            sample.title
        );
    }
}
