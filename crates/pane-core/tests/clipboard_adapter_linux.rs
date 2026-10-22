//! The Linux (X11) clipboard adapter against the real X11 clipboard: it
//! reports each change the test makes with the program that copied, reads
//! a copy no text can be read from as none, a PNG image and a file
//! manager's list of files as what they are (#167), reports an owner that
//! names no program as unknown, puts text, an image and files on it, and
//! reports nothing once its watch is dropped.
//!
//! The test replaces what is on the clipboard, and does not put it back:
//! it runs only where `PANE_TEST_REAL_CLIPBOARD=1` is set and an X11
//! display is reachable, as CI's Linux runner does under Xvfb, never by
//! default on a developer's computer. It uses only text it puts on the
//! clipboard itself (each starting with a prefix of its own), and keeps
//! only reports of that text, or of its own copies no text can be read
//! from; anything else on the clipboard meanwhile is dropped unseen. X11
//! has no formats that mark a copy as not to be kept, so every report's
//! markers are the default, and what a password manager copies can only
//! be kept off by excluding its program. Other systems have no adapter
//! yet (their unavailability is checked in `clipboard.rs`; the session's
//! refusals in `linux.rs`).
#![cfg(target_os = "linux")]

use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pane_core::clipboard::{
    Content, Copied, Markers, Observation, ProgramName, Sink, Skip, Ticket, accept, accept_any,
    testing,
};

/// How long a change may take to be reported: the watcher must ask the
/// owner for the text, which serves it from this test's own thread.
const REPORTED: Duration = Duration::from_secs(5);

/// Passes on the reports of this test's own copies.
struct Ours {
    prefix: String,
    reports: Mutex<mpsc::Sender<Observation>>,
}

impl Sink for Ours {
    fn reading(&self) -> Ticket {
        Ticket::default()
    }

    fn observed(&self, _: Ticket, observation: Observation) {
        // Only this test's own copies are passed on: its text (each
        // starting with its prefix) or one of its own copies no text can
        // be read from. Other programs may use the clipboard while it
        // runs.
        let ours = match &observation.content {
            Content::Text(text) => text.starts_with(&self.prefix),
            // Its own files are named with its prefix.
            Content::Files(files) => files
                .iter()
                .any(|file| file.to_string_lossy().contains(&self.prefix)),
            // Only this test copies images on this quiet display (from an
            // owner that names no program, or as Pane's own write); the
            // test tells its own by their bytes.
            Content::Image(_) => true,
            // Only this test's own copies with no text are reported from
            // an owner that names no program on this quiet display.
            Content::Other | Content::TooLarge => observation.source.is_none(),
            Content::Withheld => false,
        };
        if ours {
            let _ = self.reports.lock().unwrap().send(observation);
        }
    }
}

#[test]
fn the_watcher_reports_this_tests_changes_until_dropped() {
    if std::env::var("PANE_TEST_REAL_CLIPBOARD").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_CLIPBOARD=1 to let it replace the clipboard");
        return;
    }
    let clipboard = pane_core::clipboard::native();
    if let Some(reason) = clipboard.unavailable() {
        eprintln!("skipped: {reason}");
        return;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let prefix = format!("pane-clipboard-test-{}-{nanos}-", std::process::id());
    let program = std::env::current_exe().unwrap();
    let program = program
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    let (sender, reports) = mpsc::channel::<Observation>();
    let watch = clipboard
        .watch(std::sync::Arc::new(Ours {
            prefix: prefix.clone(),
            reports: Mutex::new(sender),
        }))
        .expect("X11 can watch the clipboard");
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

    // Plain text, owned by a window of this test's process, which says its
    // process: the report names this test's program, and X11 has no
    // markers, so nothing is withheld.
    let plain_text = format!("{prefix}plain ✓");
    let owner = testing::set_text(&plain_text).unwrap();
    let plain = next("plain text", &text(plain_text.clone()));
    drop(owner);
    assert_eq!(accept(&plain, &[]), Ok(plain_text.as_str()));
    assert_eq!(plain.markers, Markers::default());
    assert_eq!(
        plain.source.as_deref().map(str::to_lowercase),
        Some(program.clone())
    );
    // The reported program is the one an exclusion matches, as on Windows:
    // excluding it by its file name keeps the copy out, another program's
    // name keeps it.
    let own = ProgramName::parse(&program).unwrap();
    assert_eq!(
        accept(&plain, std::slice::from_ref(&own)),
        Err(Skip::Excluded(own))
    );
    assert_eq!(
        accept(&plain, &[ProgramName::parse("another-program").unwrap()]),
        Ok(plain_text.as_str())
    );

    // A copy no text can be read from (an image, say), owned by a window
    // that names no program: kept as no text, never as withheld, and its
    // owner is unknown, so it is never excluded.
    let image = format!("{prefix}image");
    let _image_owner = testing::set_target("image/png", image.as_bytes()).unwrap();
    let other = next("an image", &|report| report.content == Content::Other);
    assert_eq!(accept(&other, &[]), Err(Skip::NotText));
    assert_eq!(other.markers, Markers::default());
    assert_eq!(other.source, None);

    // #167: an image copied as a PNG is read as that image, which Pane's
    // own Clipboard History keeps.
    let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
    let png = pane_core::icons::encode_png(2, 1, &rgba).unwrap();
    let _png_owner = testing::set_target("image/png", &png).unwrap();
    let image = next(
        "a PNG image",
        &|report| matches!(&report.content, Content::Image(image) if image.png == png),
    );
    match &image.content {
        Content::Image(image) => assert_eq!((image.width, image.height), (2, 1)),
        other => panic!("{other:?}"),
    }
    assert!(matches!(accept_any(&image, &[]), Ok(Copied::Image(_))));
    assert_eq!(accept(&image, &[]), Err(Skip::NotText));

    // Files, as a file manager copies them (`text/uri-list`): every path,
    // in order.
    let folder = tempfile::tempdir().unwrap();
    let files = [
        folder.path().join(format!("{prefix}a b.txt")),
        folder.path().join(format!("{prefix}c")),
    ];
    let list: String = files
        .iter()
        .map(|file| {
            format!(
                "file://{}\r\n",
                file.display().to_string().replace(' ', "%20")
            )
        })
        .collect();
    let _files_owner = testing::set_target("text/uri-list", list.as_bytes()).unwrap();
    let copied = next("copied files", &|report| {
        report.content == Content::Files(files.to_vec())
    });
    assert!(matches!(
        accept_any(&copied, &[]),
        Ok(Copied::Files(listed)) if listed == files.as_slice()
    ));

    // Pane puts an image and files back as what they are: its own writes
    // are reported as such.
    clipboard.write_image(&png).unwrap();
    next(
        "the written image",
        &|report| matches!(&report.content, Content::Image(image) if image.png == png),
    );
    clipboard.write_files(&files).unwrap();
    next("the written files", &|report| {
        report.content == Content::Files(files.to_vec())
    });

    // Writing is a change too, reported with its owner, Pane's process.
    let written = format!("{prefix}written");
    clipboard.write_text(&written).unwrap();
    let copy = next("written text", &text(written.clone()));
    assert_eq!(copy.source.as_deref().map(str::to_lowercase), Some(program));
    assert_eq!(accept(&copy, &[]), Ok(written.as_str()));

    // Once the watch is dropped, nothing more is reported.
    drop(watch);
    while reports.try_recv().is_ok() {}
    let _owner = testing::set_text(&format!("{prefix}after")).unwrap();
    // The sink went with the watch, so the channel is closed and empty.
    assert!(matches!(
        reports.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

/// `clipboard::native` on Linux without a display to watch through: the
/// honest reason, checked here where none is reachable and skipped where
/// one is (CI runs the test above under Xvfb).
#[test]
fn without_a_display_linux_says_why_the_clipboard_cannot_be_watched() {
    let display = std::env::var("DISPLAY").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if !display.is_empty() || !wayland.is_empty() {
        eprintln!("skipped: this session has a display");
        return;
    }
    let reason = pane_core::clipboard::native()
        .unavailable()
        .expect("no adapter without a display");
    assert_eq!(
        reason,
        "Not available: Pane is not running on an X11 or Wayland display"
    );
}
