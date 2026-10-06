//! The macOS (pasteboard) clipboard adapter against the real pasteboard:
//! it reports each change the test makes, withholds the text of a copy
//! marked the way a password manager marks it, reads a copy no text can be
//! read from as none, puts text on it, and reports nothing once its watch
//! is dropped.
//!
//! The test replaces what is on the pasteboard, and does not put it back:
//! it runs only where `PANE_TEST_REAL_CLIPBOARD=1` is set, as CI's macOS
//! runner does, never by default on a developer's computer. It uses only
//! text it puts on the pasteboard itself (each starting with a prefix of
//! its own), and keeps only reports of that text, of its own concealed
//! copy, or of its own copies no text can be read from; anything else on
//! the pasteboard meanwhile is dropped unseen. A copy with the concealed
//! type `org.nspasteboard.ConcealedType` is withheld like a password
//! manager's marker on Windows, with the text never read; and the
//! pasteboard never names the program that copied, so every report's
//! source is unknown and no excluded program ever matches. The command's
//! availability per system is checked in `clipboard.rs`'s tests, and the
//! marker decision in `macos.rs`'s unit tests.
//!
//! A command's copies through Pane's system functions (#145) are checked
//! here too: a concealed one carries the concealed type that keeps it out
//! of Pane's own clipboard history and other clipboard managers, a plain
//! one none, and what was copied, a file included, reads back. The tests
//! replace the same pasteboard, so they run one at a time (`SERIAL`).
#![cfg(target_os = "macos")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pane_core::clipboard::{
    Content, Markers, Observation, ProgramName, Sink, Skip, Ticket, accept, testing,
};
use pane_core::system::Clip;

/// Held by each test while it uses the pasteboard.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How long a change may take to be reported: the watcher looks at the
/// pasteboard's change count only now and then.
const REPORTED: Duration = Duration::from_secs(5);

/// Passes on the reports of this test's own copies.
struct Ours {
    prefix: String,
    /// Set by the test while it is about to make its own concealed copy,
    /// whose text is never read: the pasteboard names no program, so only
    /// the test's own timing tells its copy from another program's.
    concealed: Arc<AtomicBool>,
    reports: Mutex<mpsc::Sender<Observation>>,
}

impl Sink for Ours {
    fn reading(&self) -> Ticket {
        Ticket::default()
    }

    fn observed(&self, _: Ticket, observation: Observation) {
        // Only this test's own copies are passed on: its text (each
        // starting with its prefix) or the concealed copy it is about to
        // make. A copy no text can be read from cannot be told apart from
        // another program's, the pasteboard naming no program: on this
        // quiet runner's pasteboard, only this test's own image is
        // reported as no text. Other programs may use the pasteboard
        // while it runs.
        let ours = match &observation.content {
            Content::Text(text) => text.starts_with(&self.prefix),
            Content::Withheld => self.concealed.swap(false, Ordering::SeqCst),
            Content::Other => true,
        };
        if ours {
            let _ = self.reports.lock().unwrap().send(observation);
        }
    }
}

#[test]
fn the_watcher_reports_this_tests_changes_until_dropped() {
    if std::env::var("PANE_TEST_REAL_CLIPBOARD").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_CLIPBOARD=1 to let it replace the pasteboard");
        return;
    }
    let _serial = serial();
    let clipboard = pane_core::clipboard::native();
    assert_eq!(
        clipboard.unavailable(),
        None,
        "macOS can watch the pasteboard"
    );
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let prefix = format!("pane-clipboard-test-{}-{nanos}-", std::process::id());
    let (sender, reports) = mpsc::channel::<Observation>();
    // The sink's only reference goes to the watch, so the channel closes
    // when the watch's thread ends; the flag is shared so the test can
    // tell the sink to expect its own concealed copy.
    let concealed = Arc::new(AtomicBool::new(false));
    let watch = clipboard
        .watch(Arc::new(Ours {
            prefix: prefix.clone(),
            concealed: concealed.clone(),
            reports: Mutex::new(sender),
        }))
        .expect("macOS can watch the pasteboard");
    // The next report matching `wanted`; a change can be reported more
    // than once, so an earlier one reported again is skipped.
    let next = |what: &str, wanted: &dyn Fn(&Observation) -> bool| loop {
        let report = reports
            .recv_timeout(REPORTED)
            .unwrap_or_else(|_| panic!("{what} is reported"));
        if wanted(&report) {
            return report;
        }
    };
    let text =
        |text: String| move |report: &Observation| report.content == Content::Text(text.clone());

    // Plain text: the report has the text, no marker (nothing was set) and
    // no source (the pasteboard never names the program that copied).
    let plain_text = format!("{prefix}plain ✓");
    testing::set_text(&plain_text, false).unwrap();
    let plain = next("plain text", &text(plain_text.clone()));
    assert_eq!(accept(&plain, &[]), Ok(plain_text.as_str()));
    assert_eq!(plain.markers, Markers::default());
    assert_eq!(plain.source, None);
    // An unknown owner is never excluded, so no program's name keeps the
    // copy out, not even the test's own process.
    assert_eq!(
        accept(&plain, &[ProgramName::parse("KeePass").unwrap()]),
        Ok(plain_text.as_str())
    );

    // A copy marked the way a password manager marks it
    // (org.nspasteboard.ConcealedType): the text is withheld, never read.
    concealed.store(true, Ordering::SeqCst);
    testing::set_text(&format!("{prefix}secret"), true).unwrap();
    let concealed = next("a concealed copy", &|report| {
        report.content == Content::Withheld
    });
    assert_eq!(accept(&concealed, &[]), Err(Skip::Marked));
    assert_eq!(
        concealed.markers,
        Markers {
            exclude_from_monitoring: true,
            ..Markers::default()
        }
    );
    assert_eq!(concealed.source, None);

    // A copy no text can be read from (an image, say): kept as no text,
    // never as withheld, and never excluded.
    testing::set_target("public.png", format!("{prefix}image").as_bytes()).unwrap();
    let other = next("an image", &|report| report.content == Content::Other);
    assert_eq!(accept(&other, &[]), Err(Skip::NotText));
    assert_eq!(other.markers, Markers::default());
    assert_eq!(other.source, None);

    // Writing is a change too, reported like any other.
    let written = format!("{prefix}written");
    clipboard.write_text(&written).unwrap();
    let copy = next("written text", &text(written.clone()));
    assert_eq!(accept(&copy, &[]), Ok(written.as_str()));
    assert_eq!(copy.source, None);

    // Once the watch is dropped, nothing more is reported.
    drop(watch);
    while reports.try_recv().is_ok() {}
    testing::set_text(&format!("{prefix}after"), false).unwrap();
    // The sink went with the watch, so the channel is closed and empty.
    assert!(matches!(
        reports.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

/// Passes on the reports of text starting with `prefix`, and of copies
/// whose text is withheld: on this quiet runner's pasteboard, only this
/// test's own concealed copy is.
struct Prefixed {
    prefix: String,
    reports: Mutex<mpsc::Sender<Observation>>,
}

impl Sink for Prefixed {
    fn reading(&self) -> Ticket {
        Ticket::default()
    }

    fn observed(&self, _: Ticket, observation: Observation) {
        let ours = match &observation.content {
            Content::Text(text) => text.starts_with(&self.prefix),
            Content::Withheld => true,
            Content::Other => false,
        };
        if ours {
            let _ = self.reports.lock().unwrap().send(observation);
        }
    }
}

#[test]
fn a_commands_concealed_copy_is_marked_and_skipped_and_what_it_copied_reads_back() {
    if std::env::var("PANE_TEST_REAL_CLIPBOARD").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_CLIPBOARD=1 to let it replace the pasteboard");
        return;
    }
    let _serial = serial();
    let system = pane_core::system::native();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let prefix = format!("pane-system-test-{}-{nanos}-", std::process::id());
    let (sender, reports) = mpsc::channel::<Observation>();
    let watch = pane_core::clipboard::native()
        .watch(Arc::new(Prefixed {
            prefix: prefix.clone(),
            reports: Mutex::new(sender),
        }))
        .expect("macOS can watch the pasteboard");
    let next = |what: &str, wanted: &dyn Fn(&Observation) -> bool| loop {
        let report = reports
            .recv_timeout(REPORTED)
            .unwrap_or_else(|_| panic!("{what} is reported"));
        if wanted(&report) {
            return report;
        }
    };

    // Concealed: the concealed type rides along, so Pane's history
    // withholds it, its text never read.
    let secret = format!("{prefix}secret");
    system
        .copy(&Clip::Text(secret.clone()), true)
        .expect("the concealed copy is made");
    let concealed = next("the concealed copy", &|report| {
        report.content == Content::Withheld
    });
    assert_eq!(
        concealed.markers,
        Markers {
            exclude_from_monitoring: true,
            ..Markers::default()
        }
    );
    assert_eq!(accept(&concealed, &[]), Err(Skip::Marked));
    // It is on the pasteboard all the same, for pasting.
    assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(secret))));

    // Plain: no marker, and kept.
    let plain = format!("{prefix}plain ✓");
    system
        .copy(&Clip::Text(plain.clone()), false)
        .expect("the plain copy is made");
    let copied = next("the plain copy", &|report| {
        report.content == Content::Text(plain.clone())
    });
    assert_eq!(copied.markers, Markers::default());
    assert_eq!(accept(&copied, &[]), Ok(plain.as_str()));
    assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(plain))));

    // A file, as Finder copies one (its file URL), reads back as the file.
    let file = std::env::current_exe().unwrap();
    system
        .copy(&Clip::File(file.clone()), false)
        .expect("the file is copied");
    assert_eq!(system.read_clipboard(), Ok(Some(Clip::File(file))));
    drop(watch);
}
