//! The Windows clipboard adapter against the real clipboard: it reports
//! each change the test makes, reads the markers password managers set and
//! then withholds the text, names the program owning the clipboard, puts
//! text on it, and reports nothing once its watch is dropped.
//!
//! The test replaces what is on the clipboard, and does not put it back:
//! it runs only where `PANE_TEST_REAL_CLIPBOARD=1` is set, as CI's Windows
//! runner does, never by default on a developer's computer. It uses only
//! text it puts on the clipboard itself (each starting with a prefix of its
//! own), and keeps only reports of that text, withheld reports of this
//! process's own marked copies, files named with its prefix and 2×2
//! images; anything else on the clipboard meanwhile is dropped unseen. Its marked copies also say `CanIncludeInClipboardHistory`
//! 0 where the check allows, so Windows' own clipboard history (Win+V) keeps
//! them neither, and it never says a copy may be synced
//! (`CanUploadToCloudClipboard` 1). Other systems have no adapter yet
//! (their unavailability is checked in `clipboard.rs`).
//!
//! Copied images (a bitmap alone, as Paint copies one) and files (as File
//! Explorer copies them) are read as what they are, and Pane puts them
//! back so (#167).
//!
//! A command's copies through Pane's system functions (#145) are checked
//! here too: a concealed one carries the markers that keep it out of
//! Pane's own clipboard history and Windows' (Win+V), a plain one none,
//! and what was copied, a file included, reads back. The tests replace the
//! same clipboard, so they run one at a time (`SERIAL`, and a test group
//! of their own under nextest, `.config/nextest.toml`).
#![cfg(target_os = "windows")]

use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pane_core::clipboard::{
    ClipboardSystem, Content, Copied, Markers, Observation, Sink, Skip, Ticket, WindowsClipboard,
    accept, accept_any, program_file_name, testing,
};
use pane_core::system::Clip;

/// Held by each test while it uses the clipboard.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How long a change may take to be reported.
const REPORTED: Duration = Duration::from_secs(5);

const HISTORY: &str = "CanIncludeInClipboardHistory";

/// Passes on the reports of this test's own copies.
struct Ours {
    prefix: String,
    /// This test's program's file name, lowercase.
    program: String,
    reports: Mutex<mpsc::Sender<Observation>>,
}

impl Sink for Ours {
    fn reading(&self) -> Ticket {
        Ticket::default()
    }

    fn observed(&self, _: Ticket, observation: Observation) {
        let from_here = observation
            .source
            .as_deref()
            .is_some_and(|source| program_file_name(source).to_lowercase() == self.program);
        let ours = match &observation.content {
            Content::Text(text) => text.starts_with(&self.prefix),
            // Files this test copies are named with its prefix.
            Content::Files(paths) => {
                !paths.is_empty()
                    && paths.iter().all(|path| {
                        path.file_name()
                            .is_some_and(|name| name.to_string_lossy().starts_with(&self.prefix))
                    })
            }
            // An image has no text to tell it by: this test's are 2×2, or
            // owned by its window. What `write_image` puts on the clipboard
            // has no owner by the time the listener reads it most times (its
            // window goes as the write ends), so is told by its size (#167).
            Content::Image(image) => from_here || (image.width, image.height) == (2, 2),
            // Its text was never read: only this process's own copy can be
            // told apart, by its owner.
            Content::Withheld | Content::TooLarge => from_here,
            Content::Other => false,
        };
        if ours {
            let _ = self.reports.lock().unwrap().send(observation);
        }
    }
}

#[test]
fn the_listener_reports_this_tests_changes_with_their_markers_until_dropped() {
    if std::env::var("PANE_TEST_REAL_CLIPBOARD").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_CLIPBOARD=1 to let it replace the clipboard");
        return;
    }
    let _serial = serial();
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
    let watch = WindowsClipboard
        .watch(std::sync::Arc::new(Ours {
            prefix: prefix.clone(),
            program: program.clone(),
            reports: Mutex::new(sender),
        }))
        .expect("Windows can watch the clipboard");
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

    // Plain text, owned by a window of this test's process.
    let plain_text = format!("{prefix}plain");
    let owner = testing::set_text(&plain_text, &[]).unwrap();
    let plain = next("plain text", &text(plain_text.clone()));
    drop(owner);
    assert_eq!(accept(&plain, &[]), Ok(plain_text.as_str()));
    assert_eq!(plain.markers, Markers::default());
    // The owner is named by its program's full path (#166), whose file
    // name is this test's.
    let source = plain.source.clone().expect("the owner is named");
    assert!(
        std::path::Path::new(&source).is_absolute(),
        "{source} is a full path"
    );
    assert_eq!(program_file_name(&source).to_lowercase(), program);

    // Each marker a password manager sets is read, and withholds the text.
    let hidden = (HISTORY, 0);
    let marked: [(&[(&str, u32)], Markers); 4] = [
        (
            &[("ExcludeClipboardContentFromMonitorProcessing", 0)],
            Markers {
                exclude_from_monitoring: true,
                ..Markers::default()
            },
        ),
        (
            &[("Clipboard Viewer Ignore", 0), hidden],
            Markers {
                exclude_from_monitoring: true,
                include_in_history: Some(false),
                ..Markers::default()
            },
        ),
        (
            &[hidden],
            Markers {
                include_in_history: Some(false),
                ..Markers::default()
            },
        ),
        (
            &[("CanUploadToCloudClipboard", 0), hidden],
            Markers {
                include_in_history: Some(false),
                upload_to_cloud: Some(false),
                ..Markers::default()
            },
        ),
    ];
    for (formats, markers) in marked {
        let _owner = testing::set_text(&format!("{prefix}secret"), formats).unwrap();
        let secret = next(&format!("{formats:?}"), &|report| {
            report.content == Content::Withheld && report.markers == markers
        });
        assert!(!secret.markers.allow(), "{formats:?}");
        assert_eq!(accept(&secret, &[]), Err(Skip::Marked));
    }
    // Saying it may be kept in clipboard history (which does not sync it)
    // keeps it.
    let allowed_text = format!("{prefix}allowed");
    let _owner = testing::set_text(&allowed_text, &[(HISTORY, 1)]).unwrap();
    let allowed = next("allowed text", &text(allowed_text));
    assert_eq!(allowed.markers.include_in_history, Some(true));
    assert_eq!(allowed.markers.upload_to_cloud, None);

    // Writing is a change too.
    let written = format!("{prefix}written ✓");
    WindowsClipboard.write_text(&written).unwrap();
    next("written text", &text(written));

    // #167: a bitmap alone, as Paint copies a picture, is read as an image
    // (made into a PNG), which Pane's own Clipboard History keeps.
    let owner = testing::set_bitmap(&bitmap_24()).unwrap();
    let bitmap = next(
        "a copied bitmap",
        &|report| matches!(&report.content, Content::Image(image) if (image.width, image.height) == (2, 2)),
    );
    drop(owner);
    assert!(matches!(accept_any(&bitmap, &[]), Ok(Copied::Image(_))));
    assert_eq!(accept(&bitmap, &[]), Err(Skip::NotText));
    // Pane puts a kept image back as an image: its PNG, as it was kept.
    let png = match &bitmap.content {
        Content::Image(image) => image.png.clone(),
        other => panic!("{other:?}"),
    };
    WindowsClipboard.write_image(&png).unwrap();
    next(
        "the written image",
        &|report| matches!(&report.content, Content::Image(image) if image.png == png),
    );

    // Files, as File Explorer copies them (`CF_HDROP`): every path, in
    // order; Pane puts kept files back as files.
    let folder = tempfile::tempdir().unwrap();
    let files = vec![
        folder.path().join(format!("{prefix}a b.txt")),
        folder.path().join(format!("{prefix}c")),
    ];
    WindowsClipboard.write_files(&files).unwrap();
    let copied = next("the written files", &|report| {
        report.content == Content::Files(files.clone())
    });
    assert!(matches!(
        accept_any(&copied, &[]),
        Ok(Copied::Files(listed)) if listed == files.as_slice()
    ));

    // Once the watch is dropped, nothing more is reported.
    drop(watch);
    while reports.try_recv().is_ok() {}
    let _owner = testing::set_text(&format!("{prefix}after"), &[hidden]).unwrap();
    // The sink went with the watch, so the channel is closed and empty.
    assert!(matches!(
        reports.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

/// A 2×2 bitmap (`BITMAPINFOHEADER`, 24 bits a pixel, bottom row first,
/// each row padded to 4 bytes): red, green over blue, white.
fn bitmap_24() -> Vec<u8> {
    let mut dib = Vec::new();
    for value in [40u32, 2, 2] {
        dib.extend_from_slice(&value.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&24u16.to_le_bytes());
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]);
    dib.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
    dib
}

/// Passes on every report, whoever copied: the system functions' copies
/// are owned by a window that is gone by the time the listener reads them.
struct Everything(Mutex<mpsc::Sender<Observation>>);

impl Sink for Everything {
    fn reading(&self) -> Ticket {
        Ticket::default()
    }

    fn observed(&self, _: Ticket, observation: Observation) {
        let _ = self.0.lock().unwrap().send(observation);
    }
}

#[test]
fn a_commands_concealed_copy_is_marked_and_skipped_and_what_it_copied_reads_back() {
    if std::env::var("PANE_TEST_REAL_CLIPBOARD").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_CLIPBOARD=1 to let it replace the clipboard");
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
    let watch = WindowsClipboard
        .watch(Arc::new(Everything(Mutex::new(sender))))
        .expect("Windows can watch the clipboard");
    let next = |what: &str, wanted: &dyn Fn(&Observation) -> bool| loop {
        let report = reports
            .recv_timeout(REPORTED)
            .unwrap_or_else(|_| panic!("{what} is reported"));
        if wanted(&report) {
            return report;
        }
    };

    // Concealed: monitors must not look at it, and neither Windows'
    // clipboard history nor its cloud clipboard keeps it; Pane's history
    // withholds it, its text never read.
    let concealed_markers = Markers {
        exclude_from_monitoring: true,
        include_in_history: Some(false),
        upload_to_cloud: Some(false),
    };
    let secret = format!("{prefix}secret");
    system
        .copy(&Clip::Text(secret.clone()), true)
        .expect("the concealed copy is made");
    let concealed = next("the concealed copy", &|report| {
        report.content == Content::Withheld && report.markers == concealed_markers
    });
    assert_eq!(accept(&concealed, &[]), Err(Skip::Marked));
    // It is on the clipboard all the same, for pasting.
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

    // A file, as File Explorer copies one, reads back as the file.
    let file = std::env::current_exe().unwrap();
    system
        .copy(&Clip::File(file.clone()), false)
        .expect("the file is copied");
    assert_eq!(system.read_clipboard(), Ok(Some(Clip::File(file))));
    drop(watch);
}
