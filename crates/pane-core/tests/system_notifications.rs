//! System notifications (#555, ADR 0053) through the launcher's public
//! interface, with the alerts fixture (a real guest `cargo xtask guests`
//! builds): its activation registers a timer, and each firing — a call
//! no window was shown for, made while the launcher is hidden — posts a
//! notification through `alerts.notify`, writing what it was answered to
//! its content. The notifier is a stand-in that records what reaches it
//! and answers what the test sets, so nothing is shown on the desktop;
//! Windows' own toast is checked by hand (see #555).
//!
//! What reaches the notifier is the package's notification as Pane posts
//! it: its title and message, cut with "…" past 200 and 1,000
//! characters, the package's title as its attribution, and a click that
//! launches the package's command user-initiated with the context given,
//! as `launch` does, asking for the window — or, without a click, shows
//! the launcher. Past 10 a minute the package is told `limit`; a click
//! naming no command of its own, or a context that is not JSON, is
//! `failed`; and a notifier that cannot post (no AppUserModelID Windows
//! knows, notifications turned off, another system) is `not-available`
//! with its reason, as is a launcher given no notifier.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::system_notifications::{Notice, Notifier, NotifyError};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status, WindowPresence};
use tempfile::TempDir;

/// How long an activation, a firing and a launch may take: compiling the
/// guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(20);

const SECOND: Duration = Duration::from_secs(1);

/// The fixture's package title, which its notifications are attributed
/// to.
const TITLE: &str = "Alerts fixture";

/// The notifier standing in for the system's: it records each notice
/// that reaches it and answers what the test set (success by default).
#[derive(Default)]
struct Recording {
    posted: Mutex<Vec<Notice>>,
    refusal: Mutex<Option<NotifyError>>,
}

impl Recording {
    fn posted(&self) -> Vec<Notice> {
        self.posted.lock().unwrap().clone()
    }
}

impl Notifier for Recording {
    fn notify(&self, notice: Notice) -> Result<(), NotifyError> {
        self.posted.lock().unwrap().push(notice);
        match self.refusal.lock().unwrap().clone() {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }
}

/// One test's Pane: its data folder, its source folders, its runtime
/// cache, the clock its timers follow and the stand-in notifier.
struct Pane {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
    clock: Arc<ManualClock>,
    notifier: Arc<Recording>,
}

impl Pane {
    fn new() -> Pane {
        Pane {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
            clock: ManualClock::at(SystemClock.now() + 365 * 86_400_000),
            notifier: Arc::default(),
        }
    }

    /// Pane on this data folder, without a notifier.
    fn launcher(&self) -> Launcher {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        Launcher::with_packages(
            Ok(runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_clock(self.clock.clone())
    }

    /// Pane with the stand-in notifier.
    fn start(&self) -> Launcher {
        self.launcher().with_notifier(self.notifier.clone())
    }

    /// Pane started with the fixture's `settings` saved (before the
    /// start, which reads them), with the stand-in notifier or none, the
    /// fixture installed, the launcher hidden and the fixture's timer
    /// fired once: the launcher, the fixture's folder and what the
    /// firing's posts answered.
    fn fired(&self, settings: &[(&str, &str)], notifier: bool) -> (Launcher, PathBuf, String) {
        let folder = self.fixture(settings);
        let launcher = if notifier {
            self.start()
        } else {
            self.launcher()
        };
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {TITLE}"))
        );
        launcher.set_window_presence(WindowPresence::Hidden);
        let identity = PackageIdentity::local(&folder).unwrap();
        self.until("the activation's timer", true, || {
            launcher.undo_list(&identity).contains(&"timer")
        });
        self.settled(&launcher);
        self.clock.advance(SECOND);
        self.settled(&launcher);
        self.until("the firing's answers", true, || {
            self.content(&folder, "fired").is_some()
        });
        let answers = self.content(&folder, "answers").unwrap_or_default();
        (launcher, folder, answers)
    }

    /// The alerts fixture as a package of this test, its timer started by
    /// its activation entry point, with the settings `settings` saved for
    /// it.
    fn fixture(&self, settings: &[(&str, &str)]) -> PathBuf {
        let folder = self.sources.path().join("alerts");
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/alerts_fixture.wasm");
        assert!(
            fixture.exists(),
            "{} is missing; run `cargo xtask guests`",
            fixture.display()
        );
        fs::create_dir_all(&folder).unwrap();
        fs::copy(&fixture, folder.join("fixture.wasm")).unwrap();
        let manifest = serde_json::json!({
            "manifestVersion": 1,
            "title": TITLE,
            "version": "0.1.0",
            "apiVersion": "0.2",
            "commands": [{ "id": "fixture", "title": TITLE, "component": "fixture.wasm" }],
            "activate": "fixture.wasm",
        });
        fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
        let file = self.data.path().join("extensions/settings.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let identity = PackageIdentity::local(&folder).unwrap();
        let saved: serde_json::Map<String, serde_json::Value> = settings
            .iter()
            .map(|(key, value)| ((*key).to_owned(), serde_json::Value::from(*value)))
            .collect();
        let file_contents = serde_json::json!({
            "version": 1,
            "packages": { identity.key(): saved },
        });
        fs::write(&file, file_contents.to_string()).unwrap();
        folder
    }

    /// The content value `key` the package of `folder` wrote, if any.
    fn content(&self, folder: &Path, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.data.path().join("extensions/content.json")).ok()?;
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        file["packages"][&PackageIdentity::local(folder).unwrap().key()][key]
            .as_str()
            .map(str::to_owned)
    }

    /// Waits until the timers thread looked at every change of the clock
    /// so far, and every firing it started has reported.
    fn settled(&self, launcher: &Launcher) {
        assert!(
            launcher.wait_for_timers(PROMPTLY),
            "the timers did not settle"
        );
    }

    /// Waits until `read` returns `expected`, saying `what` it was.
    fn until<T: PartialEq + std::fmt::Debug>(
        &self,
        what: &str,
        expected: T,
        mut read: impl FnMut() -> T,
    ) {
        let began = Instant::now();
        loop {
            let found = read();
            if found == expected {
                return;
            }
            assert!(
                began.elapsed() < PROMPTLY,
                "waiting for {what}: found {found:?}, expected {expected:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A timer firing while the launcher is hidden posts its notification:
/// the call reaches the host with its title, its message and the
/// package's title as attribution, and opens nothing. Clicking it
/// launches the command it names, user-initiated with its context, and
/// asks for the window, as `launch` does.
#[test]
fn a_timer_firing_while_hidden_posts_and_a_click_opens_the_command() {
    let pane = Pane::new();
    let context = r#"{"timer":"tea"}"#;
    let (launcher, _folder, answers) = pane.fired(
        &[
            ("title", "Tea is ready"),
            ("message", "Steeped for 4 minutes"),
            ("click", "fixture"),
            ("context", context),
        ],
        true,
    );
    assert_eq!(answers, "posted");
    let posted = pane.notifier.posted();
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(posted[0].title, "Tea is ready");
    assert_eq!(posted[0].message.as_deref(), Some("Steeped for 4 minutes"));
    assert_eq!(posted[0].attribution, TITLE);
    // Posting opens nothing and asks for no window.
    assert!(!launcher.take_window_request());
    assert_ne!(launcher.view().screen, Screen::Command);

    (posted[0].clicked)();
    assert!(launcher.wait_for_launches(PROMPTLY), "the click's launch");
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(
        view.title,
        format!("Alerts fixture: {context}, user-initiated")
    );
    assert!(launcher.take_window_request());
}

/// A notification without a click shows the launcher when clicked, as a
/// second start of Pane does, and opens no command.
#[test]
fn a_click_without_a_command_shows_the_launcher() {
    let pane = Pane::new();
    let (launcher, _folder, answers) = pane.fired(&[("title", "Stand up")], true);
    assert_eq!(answers, "posted");
    let posted = pane.notifier.posted();
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(posted[0].message, None);
    assert!(!launcher.take_window_request());
    (posted[0].clicked)();
    assert!(launcher.take_window_request());
    assert_ne!(launcher.view().screen, Screen::Command);
}

/// A title past 200 characters and a message past 1,000 are cut with "…"
/// rather than refused.
#[test]
fn overlong_text_is_cut_with_an_ellipsis() {
    let pane = Pane::new();
    let title = "T".repeat(300);
    let message = "m".repeat(1_500);
    let (_launcher, _folder, answers) =
        pane.fired(&[("title", &title), ("message", &message)], true);
    assert_eq!(answers, "posted");
    let posted = pane.notifier.posted();
    assert_eq!(posted[0].title, format!("{}…", "T".repeat(199)));
    assert_eq!(
        posted[0].message.as_deref(),
        Some(format!("{}…", "m".repeat(999)).as_str())
    );
}

/// Past 10 notifications in a minute, the package is told the limit, and
/// the one refused never reaches the system.
#[test]
fn past_ten_a_minute_the_package_is_told_the_limit() {
    let pane = Pane::new();
    let (_launcher, _folder, answers) = pane.fired(&[("count", "11")], true);
    let answers: Vec<&str> = answers.split(" | ").collect();
    assert_eq!(answers.len(), 11, "{answers:?}");
    assert!(
        answers[..10].iter().all(|answer| *answer == "posted"),
        "{answers:?}"
    );
    assert_eq!(
        answers[10],
        "limit: Alerts fixture posted 10 notifications in the last minute, the most Pane posts \
         for an extension"
    );
    assert_eq!(pane.notifier.posted().len(), 10);
}

/// A click naming no command of the package, or passing a context that
/// is not JSON, is refused as `failed`, and nothing is posted.
#[test]
fn a_click_naming_no_command_of_its_own_fails() {
    let pane = Pane::new();
    let (_launcher, _folder, answers) = pane.fired(&[("click", "elsewhere")], true);
    assert_eq!(
        answers,
        "failed: a notification's click opens a command of Alerts fixture's own, and it has \
         no command `elsewhere`"
    );
    assert!(pane.notifier.posted().is_empty());

    let pane = Pane::new();
    let (_launcher, _folder, answers) =
        pane.fired(&[("click", "fixture"), ("context", "{not json")], true);
    assert!(
        answers.starts_with("failed: the context is not JSON"),
        "{answers}"
    );
    assert!(pane.notifier.posted().is_empty());
}

/// A notifier that cannot post tells the extension why, as
/// `not-available`, and so does a launcher given no notifier: never a
/// trap, and the extension keeps running.
#[test]
fn a_notification_that_cannot_show_says_why() {
    let pane = Pane::new();
    *pane.notifier.refusal.lock().unwrap() = Some(NotifyError::NotAvailable(
        "Windows does not know Pane's AppUserModelID".into(),
    ));
    let (launcher, folder, answers) = pane.fired(&[], true);
    assert_eq!(
        answers,
        "not-available: Windows does not know Pane's AppUserModelID"
    );
    // The firing after it runs as well: the package was not paused.
    pane.clock.advance(SECOND);
    pane.settled(&launcher);
    pane.until("the next firing", Some("2".to_owned()), || {
        pane.content(&folder, "fired")
    });

    let pane = Pane::new();
    let (_launcher, _folder, answers) = pane.fired(&[], false);
    assert_eq!(
        answers,
        "not-available: This Pane posts no system notifications for extensions"
    );
}
