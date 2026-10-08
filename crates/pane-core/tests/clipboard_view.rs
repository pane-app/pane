//! The Clipboard History split view's read-only projection (#102, #166),
//! through the launcher's public interface: only Pane's registered
//! Clipboard History default extension, open on its command's own screen,
//! is projected; the projection reads the package's kept text records,
//! newest first, with when and where each was copied, the actual capture
//! state and whether its operations can run. Pane's own Clipboard History
//! records from the first start, with no turn-on (ADR 0042); concealed
//! copies and copies from a disabled application are not recorded. The
//! browse rules the window adapts — the type dropdown, grouping by local
//! day (Today, Yesterday, then dates), the Information, keeping the
//! selection in range — are plain functions over the records. Operations
//! revalidate the reading they were made from: a reading of a screen the
//! user left, of a package that stopped, or of a record no longer kept
//! changes nothing it should not. The Actions panel's entries and the
//! extension's Settings page (its preferences and its Clear History row)
//! reach the same history.
//!
//! The package is the one `cargo xtask guests` assembles in
//! `target/guests/packages/clipboard-history`, acquired as Pane's default
//! extension from an artifact source on 127.0.0.1 (`support/artifacts.rs`);
//! the system's clipboard is a fake that never touches the real one.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::clipboard::{
    CaptureState, ClipboardSystem, Content, CopiedImage, MAX_IMAGE_BYTES, ManualClock, Markers,
    Observation, Sink, Watch,
};
use pane_core::clipboard_view::{
    ClipboardAction, ClipboardBrowse, ClipboardDay, ClipboardFilter, ClipboardImage, ClipboardKind,
    ClipboardRecord, capture_summary, copied_at_label, copied_line, day_of, information,
    time_label,
};
use pane_core::defaults::ArtifactSource;
use pane_core::{
    ConfirmAnswer, DefaultExtension, Launcher, PackageIdentity, Runtime, Screen, Status,
};
use serde_json::Value;
use tempfile::TempDir;

#[path = "support/artifacts.rs"]
mod artifacts;

use artifacts::Artifacts;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/system.rs"]
mod system;

use feedback::RecordingWindow;
use guests::guests;
use system::{Done, RecordingSystem};

const HOUR: i64 = 3_600_000;
/// UTC+7, a local time a day ahead of UTC for part of the day.
const PLUS_7: i64 = 7 * HOUR;
/// 2026-10-05 07:30 UTC: 14:30 on Monday 5 October in UTC+7.
const NOW: u64 = 1_791_185_400_000;

fn record(id: &str, text: &str, copied_at: u64, source: Option<&str>) -> ClipboardRecord {
    ClipboardRecord {
        source: source.map(str::to_owned),
        ..ClipboardRecord::text(id, text, copied_at)
    }
}

fn ids(records: &[&ClipboardRecord]) -> Vec<String> {
    records.iter().map(|record| record.id.clone()).collect()
}

/// Newest first, as the store keeps them.
fn kept() -> Vec<ClipboardRecord> {
    vec![
        // 14:02 local, today.
        record(
            "9",
            "const pane = createPane({\n  blur: 44,\n})",
            1_791_183_720_000,
            Some("Code.exe"),
        ),
        // 00:22 local today, though still 4 October in UTC.
        record("8", "Standup moved to 10:30", 1_791_134_520_000, None),
        // 23:59 local, yesterday.
        record(
            "7",
            "hello@example.com",
            1_791_133_140_000,
            Some("OUTLOOK.EXE"),
        ),
        // 09:00 local on Thursday 1 October.
        record("4", "ssh deploy@10.0.4.12", 1_790_820_000_000, None),
        // 16:12 local on Monday 28 September, a week ago.
        record(
            "2",
            "  \n  Lunch order\nsecond line",
            1_790_586_720_000,
            None,
        ),
    ]
}

#[test]
fn a_record_is_titled_by_its_first_line_with_text() {
    assert_eq!(kept()[4].title(), "Lunch order");
    assert_eq!(kept()[0].title(), "const pane = createPane({");
    assert_eq!(record("1", " \n\t", 0, None).title(), "");
}

#[test]
fn records_group_into_today_yesterday_and_dates_in_local_time() {
    let records = kept();
    let days: Vec<ClipboardDay> = records
        .iter()
        .map(|record| day_of(record.copied_at, NOW, PLUS_7))
        .collect();
    assert!(matches!(
        days[..],
        [
            ClipboardDay::Today,
            ClipboardDay::Today,
            ClipboardDay::Yesterday,
            ClipboardDay::Earlier(_),
            ClipboardDay::Earlier(_)
        ]
    ));
    // In UTC the record copied at 00:22 local is yesterday's.
    assert_eq!(
        day_of(records[1].copied_at, NOW, 0),
        ClipboardDay::Yesterday
    );

    let listing = ClipboardBrowse::default().listing(&records, NOW, PLUS_7);
    let sections: Vec<(&str, usize)> = listing
        .sections
        .iter()
        .map(|section| (section.label.as_str(), section.first))
        .collect();
    // Each earlier day is a section of its own, named by its date.
    assert_eq!(
        sections,
        [
            ("Today", 0),
            ("Yesterday", 2),
            ("Thursday, Oct 1", 3),
            ("Monday, Sep 28", 4)
        ]
    );
    // The store's order is kept, whatever the grouping.
    assert_eq!(ids(&listing.records), ["9", "8", "7", "4", "2"]);
    // A date of another year says which.
    let last_year = [record("1", "old", 1_767_175_200_000, None)];
    let listing = ClipboardBrowse::default().listing(&last_year, NOW, PLUS_7);
    assert_eq!(listing.sections[0].label, "Wednesday, Dec 31, 2025");
}

#[test]
fn times_and_copied_lines_name_the_local_day() {
    let records = kept();
    let labels: Vec<String> = records
        .iter()
        .map(|record| time_label(record.copied_at, NOW, PLUS_7))
        .collect();
    assert_eq!(labels, ["14:02", "00:22", "23:59", "Thu", "Sep 28"]);
    // A year other than this one says which.
    assert_eq!(time_label(1_767_175_200_000, NOW, PLUS_7), "Dec 31, 2025");
    assert_eq!(
        copied_line(&records[0], NOW, PLUS_7),
        "Copied today, 14:02 from Code.exe"
    );
    assert_eq!(
        copied_line(&records[2], NOW, PLUS_7),
        "Copied yesterday, 23:59 from OUTLOOK.EXE"
    );
    assert_eq!(
        copied_line(&records[3], NOW, PLUS_7),
        "Copied on Thursday, 09:00"
    );
    assert_eq!(
        copied_line(&records[4], NOW, PLUS_7),
        "Copied on Sep 28, 16:12"
    );
}

#[test]
fn the_summary_says_what_is_kept_as_it_is() {
    assert_eq!(
        capture_summary(CaptureState::On, None, 7 * 86_400, 0),
        "Recording · kept for 7 days · copies marked private are skipped"
    );
    assert_eq!(
        capture_summary(CaptureState::On, None, 3600, 2),
        "Recording · kept for 1 hour · copies marked private are skipped · 2 applications \
         disabled"
    );
    assert_eq!(
        capture_summary(CaptureState::Paused, None, 3600, 1),
        "Recording is paused · nothing you copy is kept until you resume it"
    );
    assert_eq!(
        capture_summary(CaptureState::Off, None, 3600, 0),
        "Recording is off · nothing you copy is kept until you resume it"
    );
    // A problem is said first, whatever the choice.
    assert_eq!(
        capture_summary(CaptureState::On, Some("Not available here"), 3600, 0),
        "Not available here"
    );
}

#[test]
fn search_keeps_the_store_order_and_matches_text_and_source_ignoring_case() {
    let records = kept();
    let mut browse = ClipboardBrowse {
        query: "  OUTLOOK ".into(),
        ..ClipboardBrowse::default()
    };
    assert_eq!(ids(&browse.listing(&records, NOW, 0).records), ["7"]);
    // Anywhere in the stored text, not only its title.
    browse.query = "BLUR".into();
    assert_eq!(ids(&browse.listing(&records, NOW, 0).records), ["9"]);
    browse.query = "e".into();
    assert_eq!(
        ids(&browse.listing(&records, NOW, 0).records),
        ["9", "8", "7", "4", "2"]
    );
    // The type dropdown: All Types, Text, Images, Files, Links and Colors
    // (#166, #167); every record here is text.
    assert_eq!(
        ClipboardFilter::ALL.map(ClipboardFilter::label),
        ["All Types", "Text", "Images", "Files", "Links", "Colors"]
    );
    browse.query.clear();
    browse.filter = ClipboardFilter::Text;
    assert_eq!(browse.listing(&records, NOW, 0).records.len(), 5);
    browse.query = "nothing like it".into();
    let listing = browse.listing(&records, NOW, 0);
    assert!(listing.records.is_empty() && listing.sections.is_empty());
    assert_eq!(listing.selected, None);
    assert!(listing.selected_record().is_none());
}

/// The type dropdown keeps the records of its kind: links and colours are
/// text recognized as a URL or a colour value, and Text keeps them too;
/// images and files are kinds of their own (#167).
#[test]
fn the_type_dropdown_filters_by_kind() {
    let records = vec![
        ClipboardRecord::image(
            "7",
            ClipboardImage {
                path: PathBuf::from("shot.png"),
                width: 640,
                height: 480,
            },
            1_791_183_840_000,
        ),
        ClipboardRecord::files(
            "6",
            vec![PathBuf::from("/notes/a.txt"), PathBuf::from("/notes/b")],
            1_791_183_780_000,
        ),
        record("5", "https://example.com/docs", 1_791_183_720_000, None),
        record("4", "#ff8800", 1_791_183_660_000, None),
        record("3", "rgb(12, 34, 56)", 1_791_183_600_000, None),
        record("2", "plain words", 1_791_183_540_000, None),
        record("1", "see https://example.com", 1_791_183_480_000, None),
    ];
    let kinds: Vec<ClipboardKind> = records.iter().map(|record| record.kind).collect();
    assert_eq!(
        kinds,
        [
            ClipboardKind::Image,
            ClipboardKind::Files,
            ClipboardKind::Link,
            ClipboardKind::Color,
            ClipboardKind::Color,
            ClipboardKind::Text,
            ClipboardKind::Text
        ]
    );
    let listed = |filter: ClipboardFilter| {
        let browse = ClipboardBrowse {
            filter,
            ..ClipboardBrowse::default()
        };
        ids(&browse.listing(&records, NOW, 0).records)
    };
    assert_eq!(
        listed(ClipboardFilter::All),
        ["7", "6", "5", "4", "3", "2", "1"]
    );
    assert_eq!(listed(ClipboardFilter::Text), ["5", "4", "3", "2", "1"]);
    assert_eq!(listed(ClipboardFilter::Images), ["7"]);
    assert_eq!(listed(ClipboardFilter::Files), ["6"]);
    assert_eq!(listed(ClipboardFilter::Links), ["5"]);
    assert_eq!(listed(ClipboardFilter::Colors), ["4", "3"]);
    // The dropdown's choices, in Raycast's order, named by stable ids.
    let labels: Vec<&str> = ClipboardFilter::ALL
        .into_iter()
        .map(ClipboardFilter::label)
        .collect();
    assert_eq!(
        labels,
        ["All Types", "Text", "Images", "Files", "Links", "Colors"]
    );
    for filter in ClipboardFilter::ALL {
        assert_eq!(ClipboardFilter::from_id(filter.id()), Some(filter));
    }
    // A selection the type hides gives way to the first record it keeps.
    let browse = ClipboardBrowse {
        filter: ClipboardFilter::Colors,
        selected: Some("5".into()),
        ..ClipboardBrowse::default()
    };
    let listing = browse.listing(&records, NOW, 0);
    assert_eq!(listing.selected_record().map(|r| r.id.as_str()), Some("4"));
}

/// The detail's Information: Source (the program's name, and its path for
/// its icon where the system gave one), Type, Characters (text) or
/// Dimensions (an image), and Copied.
#[test]
fn the_information_says_source_type_characters_and_copied() {
    let records = kept();
    let info = information(&records[0], NOW, PLUS_7);
    assert_eq!(info.source.as_deref(), Some("Code"));
    assert_eq!(info.source_path, None, "a file name alone has no icon");
    assert_eq!(info.kind, "Text");
    assert_eq!(info.characters, Some(records[0].text.chars().count()));
    assert_eq!(info.dimensions, None);
    assert_eq!(info.copied, "Today at 14:02");
    // An image: its Dimensions, no Characters (#167).
    let image = ClipboardRecord::image(
        "11",
        ClipboardImage {
            path: PathBuf::from("shot.png"),
            width: 1920,
            height: 1080,
        },
        NOW,
    );
    let info = information(&image, NOW, PLUS_7);
    assert_eq!(info.kind, "Image");
    assert_eq!(info.dimensions.as_deref(), Some("1920×1080"));
    assert_eq!(info.characters, None);
    assert_eq!(image.title(), "Image (1920×1080)");
    // A program named by its path (Windows) is named by its file, and its
    // path gives its icon.
    let path = if cfg!(windows) {
        r"C:\Windows\System32\notepad.exe"
    } else {
        "/usr/bin/gedit"
    };
    let from_path = record("10", "#abc", NOW, Some(path));
    let info = information(&from_path, NOW, PLUS_7);
    let name = if cfg!(windows) { "notepad" } else { "gedit" };
    assert_eq!(info.source.as_deref(), Some(name));
    assert_eq!(info.source_path, Some(std::path::PathBuf::from(path)));
    assert_eq!(info.kind, "Color");
    assert_eq!(info.characters, Some(4));
    // No source, no Source.
    assert_eq!(information(&records[1], NOW, PLUS_7).source, None);
    // Copied, by local day.
    let copied: Vec<String> = records
        .iter()
        .map(|record| copied_at_label(record.copied_at, NOW, PLUS_7))
        .collect();
    assert_eq!(
        copied,
        [
            "Today at 14:02",
            "Today at 00:22",
            "Yesterday at 23:59",
            "Thursday at 09:00",
            "Sep 28 at 16:12"
        ]
    );
}

#[test]
fn the_selection_stays_in_range_as_the_filter_and_deletion_change_the_list() {
    let mut records = kept();
    let mut browse = ClipboardBrowse::default();
    // Nothing chosen yet: the first record.
    assert_eq!(browse.listing(&records, NOW, 0).selected, Some(0));
    browse.select("7");
    let listing = browse.listing(&records, NOW, 0);
    assert_eq!(listing.selected_record().map(|r| r.id.as_str()), Some("7"));
    // A query that hides it shows the first record it keeps instead.
    browse.query = "ssh".into();
    let listing = browse.listing(&records, NOW, 0);
    assert_eq!(listing.selected_record().map(|r| r.id.as_str()), Some("4"));
    // Down and Up move within what is listed, and stop at its ends.
    browse.query.clear();
    browse.step(&records, 1);
    assert_eq!(browse.selected.as_deref(), Some("4"));
    browse.step(&records, 5);
    assert_eq!(browse.selected.as_deref(), Some("2"));
    browse.step(&records, -10);
    assert_eq!(browse.selected.as_deref(), Some("9"));
    // The selected record deleted (or expired): the first one left.
    browse.select("2");
    records.retain(|record| record.id != "2");
    let listing = browse.listing(&records, NOW, 0);
    assert_eq!(listing.selected_record().map(|r| r.id.as_str()), Some("9"));
    // No records at all: nothing selected, and the keys move nothing.
    records.clear();
    assert_eq!(browse.listing(&records, NOW, 0).selected, None);
    browse.step(&records, 1);
    assert_eq!(browse.listing(&records, NOW, 0).selected, None);
}

// ------------------------------------------------- through the launcher

#[derive(Default)]
struct Clipboard {
    sink: Option<Arc<dyn Sink>>,
    written: Vec<String>,
    /// The images Pane put on the clipboard, as PNGs (#167).
    images: Vec<Vec<u8>>,
    /// The files Pane put on the clipboard, a list per write (#167).
    files: Vec<Vec<PathBuf>>,
}

/// A system clipboard that records what Pane writes, and reports only the
/// changes a test makes; or, with a reason, one Pane cannot watch.
#[derive(Clone, Default)]
struct FakeClipboard {
    inner: Arc<Mutex<Clipboard>>,
    unavailable: Option<String>,
}

struct FakeWatch(Arc<Mutex<Clipboard>>);

impl Drop for FakeWatch {
    fn drop(&mut self) {
        self.0.lock().unwrap().sink = None;
    }
}

impl FakeClipboard {
    /// `text` copied from `source`, if Pane watches; whether it did.
    fn copy(&self, text: &str, source: Option<&str>) -> bool {
        let Some(sink) = self.inner.lock().unwrap().sink.clone() else {
            return false;
        };
        let ticket = sink.reading();
        sink.observed(
            ticket,
            Observation {
                content: Content::Text(text.into()),
                markers: Markers::default(),
                source: source.map(str::to_owned),
            },
        );
        true
    }

    /// `text` copied from `source`, marked by the application as concealed
    /// (as a password manager marks a password), if Pane watches; whether
    /// it did.
    fn copy_marked(&self, text: &str, source: Option<&str>) -> bool {
        let Some(sink) = self.inner.lock().unwrap().sink.clone() else {
            return false;
        };
        let ticket = sink.reading();
        sink.observed(
            ticket,
            Observation {
                content: Content::Text(text.into()),
                markers: Markers {
                    exclude_from_monitoring: true,
                    ..Markers::default()
                },
                source: source.map(str::to_owned),
            },
        );
        true
    }

    /// `content` (an image or files, #167) copied from `source`, if Pane
    /// watches; whether it did.
    fn copy_content(&self, content: Content, source: Option<&str>) -> bool {
        let Some(sink) = self.inner.lock().unwrap().sink.clone() else {
            return false;
        };
        let ticket = sink.reading();
        sink.observed(
            ticket,
            Observation {
                content,
                markers: Markers::default(),
                source: source.map(str::to_owned),
            },
        );
        true
    }

    fn written(&self) -> Vec<String> {
        self.inner.lock().unwrap().written.clone()
    }

    fn written_images(&self) -> Vec<Vec<u8>> {
        self.inner.lock().unwrap().images.clone()
    }

    fn written_files(&self) -> Vec<Vec<PathBuf>> {
        self.inner.lock().unwrap().files.clone()
    }
}

impl ClipboardSystem for FakeClipboard {
    fn unavailable(&self) -> Option<String> {
        self.unavailable.clone()
    }

    fn watch(&self, sink: Arc<dyn Sink>) -> Result<Watch, String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        self.inner.lock().unwrap().sink = Some(sink);
        Ok(Watch::new(FakeWatch(self.inner.clone())))
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        self.inner.lock().unwrap().written.push(text.into());
        Ok(())
    }

    fn write_image(&self, png: &[u8]) -> Result<(), String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        self.inner.lock().unwrap().images.push(png.to_vec());
        Ok(())
    }

    fn write_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        self.inner.lock().unwrap().files.push(paths.to_vec());
        Ok(())
    }
}

/// The files of the assembled Clipboard History package, by their path in
/// the package.
fn package_files() -> Vec<(String, Vec<u8>)> {
    let folder = guests().join("packages/clipboard-history");
    assert!(
        folder.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .map(|path| {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            (name, fs::read(&path).unwrap())
        })
        .collect();
    files.sort();
    files
}

/// Pane's data location, artifact source, clock and clipboard for one test.
struct Pane {
    data: TempDir,
    artifacts: Artifacts,
    clipboard: FakeClipboard,
    clock: Arc<ManualClock>,
}

impl Pane {
    fn new() -> Pane {
        Pane::with(FakeClipboard::default())
    }

    fn with(clipboard: FakeClipboard) -> Pane {
        let pane = Pane {
            data: tempfile::tempdir().unwrap(),
            artifacts: Artifacts::start(),
            clipboard,
            // 14:02 UTC on 5 October 2026.
            clock: ManualClock::at(1_791_208_920_000),
        };
        let files = package_files();
        let manifest: Value = serde_json::from_slice(
            &files
                .iter()
                .find(|(path, _)| path == "pane.json")
                .expect("the package has a pane.json")
                .1,
        )
        .unwrap();
        let borrowed: Vec<(&str, Vec<u8>)> = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.clone()))
            .collect();
        pane.artifacts.publish(
            "clipboard-history",
            manifest["version"].as_str().unwrap(),
            &borrowed,
        );
        pane
    }

    /// Pane with Clipboard History acquired as its default extension.
    fn start(&self) -> Launcher {
        let launcher = Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_defaults(
            ArtifactSource::local(self.artifacts.url()).unwrap(),
            vec![DefaultExtension {
                id: "clipboard-history".into(),
                title: "Clipboard History".into(),
            }],
        )
        .with_clock(self.clock.clone())
        .with_clipboard(Arc::new(self.clipboard.clone()));
        block_on(launcher.acquire_defaults());
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher
    }

    /// Pane started again on the same data folder, Clipboard History
    /// already installed.
    fn restart(&self) -> Launcher {
        Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_defaults(
            ArtifactSource::local(self.artifacts.url()).unwrap(),
            vec![DefaultExtension {
                id: "clipboard-history".into(),
                title: "Clipboard History".into(),
            }],
        )
        .with_clock(self.clock.clone())
        .with_clipboard(Arc::new(self.clipboard.clone()))
    }
}

/// The id of the default extension's command in root search.
const COMMAND: &str = "default:clipboard-history#clipboard-history";

/// Opens the command whose root row has id `id`, from root search.
fn open(launcher: &Launcher, id: &str) {
    launcher.show_root_search();
    block_on(launcher.set_query("clipboard"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id == id)
        .unwrap_or_else(|| panic!("no row {id} in {:?}", launcher.view().rows));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
}

/// The texts the projection lists, newest first.
fn listed(launcher: &Launcher) -> Vec<String> {
    let view = launcher.clipboard_history().expect("the history is shown");
    view.records.into_iter().map(|record| record.text).collect()
}

#[test]
fn only_the_registered_default_extension_on_its_own_screen_is_projected() {
    let pane = Pane::new();
    let launcher = pane.start();
    // Root search projects nothing.
    assert!(launcher.clipboard_history().is_none());

    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().expect("the history is shown");
    assert_eq!(
        view.owner,
        PackageIdentity::default_extension("clipboard-history")
    );
    // Recording from the first start: no turn-on (ADR 0042).
    assert_eq!(view.capture, CaptureState::On);
    assert!(view.records.is_empty() && view.unreadable.is_none());
    assert_eq!(view.copy_unavailable, None);

    // The same package installed from a folder, titled the same: its
    // command keeps its own list, with no access to another package's
    // records.
    let copy = tempfile::tempdir().unwrap();
    for (path, contents) in package_files() {
        fs::write(copy.path().join(path), contents).unwrap();
    }
    block_on(launcher.install_package(copy.path()));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    let local = PackageIdentity::local(copy.path()).unwrap();
    open(&launcher, &format!("{}#clipboard-history", local.key()));
    assert!(launcher.clipboard_history().is_none());
    // Its generic list: a copy does not record until it is resumed, as
    // only Pane's own Clipboard History records from the first start.
    assert!(
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.title == "Resume Recording"),
        "its generic list is its own"
    );
}

/// The acceptance check of #166: a fresh data folder records the first
/// copy, with nothing turned on, before the command was ever opened.
#[test]
fn a_fresh_data_folder_records_the_first_copy_with_no_turn_on() {
    let pane = Pane::new();
    let launcher = pane.start();
    // Watching as soon as the default extension is installed.
    assert!(pane.clipboard.copy("the first copy", Some("notepad.exe")));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["the first copy"]);
    let file: Value = serde_json::from_str(
        &fs::read_to_string(pane.data.path().join("extensions/clipboard-history.json")).unwrap(),
    )
    .unwrap();
    let own = PackageIdentity::default_extension("clipboard-history").key();
    assert_eq!(file["packages"][&own]["items"][0]["text"], "the first copy");

    // Disabling the extension still stops all observation.
    let identity = PackageIdentity::default_extension("clipboard-history");
    block_on(launcher.set_enabled(&identity, false));
    assert!(!pane.clipboard.copy("while disabled", None));
    block_on(launcher.set_enabled(&identity, true));
    assert!(pane.clipboard.copy("enabled again", None));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["enabled again", "the first copy"]);

    // Paused, it stays paused after a restart: the file says so.
    let view = launcher.clipboard_history().unwrap();
    launcher
        .set_clipboard_capture(&view, CaptureState::Paused)
        .unwrap();
    drop(launcher);
    let launcher = pane.restart();
    pane.clipboard.copy("after the restart", None);
    open(&launcher, COMMAND);
    assert_eq!(
        launcher.clipboard_history().unwrap().capture,
        CaptureState::Paused
    );
    assert_eq!(listed(&launcher), ["enabled again", "the first copy"]);
}

/// Concealed copies are skipped, as before, and copies from an application
/// the user disabled (Disabled Applications on the Settings page) are not
/// recorded: the host honours both before recording.
#[test]
fn concealed_copies_and_copies_from_a_disabled_application_are_not_recorded() {
    let pane = Pane::new();
    let launcher = pane.start();
    let identity = PackageIdentity::default_extension("clipboard-history");
    let keepass = if cfg!(windows) {
        r"C:\Program Files\KeePass Password Safe 2\KeePass.exe"
    } else {
        "/usr/bin/KeePass.exe"
    };
    assert!(pane.clipboard.copy_marked("hunter2", Some("1Password.exe")));
    block_on(launcher.set_preference(&identity, "disabledApplications", Some("KeePass.exe")))
        .unwrap();
    assert!(pane.clipboard.copy("from keepass", Some(keepass)));
    assert!(
        pane.clipboard
            .copy("KEEPASS by its file", Some("keepass.EXE"))
    );
    assert!(pane.clipboard.copy("from notepad", Some("notepad.exe")));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["from notepad"]);
    // The Settings page reads the list from the history.
    let preferences = launcher.preferences_of(&identity).expect("its preferences");
    let disabled = preferences
        .fields
        .iter()
        .find(|field| field.key == "disabledApplications")
        .expect("Disabled Applications");
    assert_eq!(disabled.value.as_deref(), Some("keepass.exe"));
    assert_eq!(
        disabled.preference.kind,
        pane_core::PreferenceKind::Applications
    );
    assert_eq!(launcher.clipboard_history().unwrap().excluded, 1);
    // Cleared, KeePass's copies are recorded again.
    block_on(launcher.set_preference(&identity, "disabledApplications", Some(""))).unwrap();
    assert!(pane.clipboard.copy("from keepass again", Some(keepass)));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["from keepass again", "from notepad"]);
}

/// Pause/Resume Recording, Keep History For and Clear History are reachable
/// from the extension's Settings page: its preferences are the history's
/// own state, and its card's Clear history row asks, then clears.
#[test]
fn the_settings_page_pauses_keeps_and_clears_the_history() {
    let pane = Pane::new();
    let launcher = pane.start();
    let identity = PackageIdentity::default_extension("clipboard-history");
    let value = |key: &str| {
        launcher
            .preferences_of(&identity)
            .expect("its preferences")
            .fields
            .into_iter()
            .find(|field| field.key == key)
            .and_then(|field| field.value)
    };
    assert_eq!(value("keepHistoryFor").as_deref(), Some("604800"));
    assert_eq!(value("pauseRecording").as_deref(), Some("false"));
    assert_eq!(value("disabledApplications"), None);

    // Pause Recording.
    block_on(launcher.set_preference(&identity, "pauseRecording", Some("true"))).unwrap();
    assert_eq!(value("pauseRecording").as_deref(), Some("true"));
    assert!(!pane.clipboard.copy("while paused", None));
    block_on(launcher.set_preference(&identity, "pauseRecording", Some("false"))).unwrap();
    assert!(pane.clipboard.copy("kept", None));

    // Keep History For: 1 hour, older items deleted at once.
    pane.clock.advance(std::time::Duration::from_secs(7200));
    assert!(pane.clipboard.copy("newer", None));
    block_on(launcher.set_preference(&identity, "keepHistoryFor", Some("3600"))).unwrap();
    assert_eq!(value("keepHistoryFor").as_deref(), Some("3600"));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["newer"]);
    assert_eq!(
        launcher.clipboard_history().unwrap().retention_seconds,
        3600
    );
    // Not an option: refused, and nothing changes.
    assert!(block_on(launcher.set_preference(&identity, "keepHistoryFor", Some("5"))).is_err());

    // Clear history, from the card: asked first, then cleared; recording
    // goes on.
    let row = format!("clear-clipboard-history:{}", identity.key());
    assert!(
        launcher.extension_list().rows.iter().any(|r| r.id == row),
        "the card's Clear history"
    );
    launcher.manage_extensions();
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|r| r.id == row)
        .unwrap();
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Confirm { .. }));
    let clear = launcher
        .view()
        .rows
        .iter()
        .position(|r| r.title == "Clear history")
        .unwrap();
    launcher.select(clear);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Deleted 1 kept item of Clipboard History".into())
    );
    assert!(pane.clipboard.copy("after clearing", None));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["after clearing"]);
}

/// The Actions panel's entries in the view (#166): the record's Paste,
/// Copy and Delete, then Pause or Resume Recording, Clear History (asked
/// first), Keep History For and Disabled Applications.
#[test]
fn the_actions_panel_lists_and_runs_the_historys_actions() {
    let pane = Pane::new();
    let launcher = pane.start();
    let window = RecordingWindow::attach(&launcher);
    launcher.set_window_presence(pane_core::WindowPresence::Shown);
    pane.clipboard.copy("one", None);
    pane.clipboard.copy("two", None);
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    let id = view.records[0].id.clone();
    let labels = |view: &pane_core::clipboard_view::ClipboardHistoryView, id: Option<&str>| {
        view.actions(id)
            .into_iter()
            .map(|item| (item.label, item.available))
            .collect::<Vec<_>>()
    };
    let shown = |label: &str, available: bool| (label.to_owned(), available);
    assert_eq!(
        labels(&view, Some(&id)),
        [
            shown("Paste", true),
            shown("Copy to Clipboard", true),
            shown("Delete Entry", true),
            shown("Pause Recording", true),
            shown("Clear History…", true),
            shown("1 Hour", true),
            shown("1 Day", true),
            shown("7 Days (current)", false),
            shown("30 Days", true),
            shown("90 Days", true),
            shown("Disabled Applications…", true),
        ]
    );
    // Without a record selected, only the history's.
    assert_eq!(labels(&view, None)[0], shown("Pause Recording", true));
    let actions = view.actions(Some(&id));
    assert!(
        actions
            .iter()
            .any(|item| item.action == ClipboardAction::ClearHistory && item.destructive)
    );

    // Pause, then Resume.
    launcher
        .set_clipboard_capture(&view, CaptureState::Paused)
        .unwrap();
    assert_eq!(
        launcher.view().status,
        Status::Result("Recording paused".into())
    );
    let view = launcher.clipboard_history().unwrap();
    assert_eq!(labels(&view, None)[0], shown("Resume Recording", true));
    launcher
        .set_clipboard_capture(&view, CaptureState::On)
        .unwrap();

    // Keep History For.
    let view = launcher.clipboard_history().unwrap();
    launcher.set_clipboard_retention(&view, 86_400).unwrap();
    assert_eq!(
        launcher.view().status,
        Status::Result("History is kept for 1 day".into())
    );
    assert_eq!(
        launcher.clipboard_history().unwrap().retention_seconds,
        86_400
    );

    // Clear History asks first; dismissed, it keeps everything.
    let view = launcher.clipboard_history().unwrap();
    let cleared = launcher.clear_clipboard_history(&view);
    let asked = launcher.confirmation().expect("Clear History asks first");
    assert_eq!(asked.title, "Clear Clipboard History?");
    assert!(asked.destructive);
    launcher.answer_confirmation(asked.id, ConfirmAnswer::Dismissed, false);
    block_on(cleared).unwrap();
    assert_eq!(listed(&launcher), ["two", "one"]);
    // Confirmed, it deletes every record; recording goes on.
    let view = launcher.clipboard_history().unwrap();
    let cleared = launcher.clear_clipboard_history(&view);
    let asked = launcher.confirmation().expect("asked again");
    launcher.answer_confirmation(asked.id, ConfirmAnswer::Confirmed, false);
    block_on(cleared).unwrap();
    assert_eq!(
        launcher.view().status,
        Status::Result("Deleted 2 kept items".into())
    );
    assert!(listed(&launcher).is_empty());
    assert!(pane.clipboard.copy("three", None));
    assert_eq!(listed(&launcher), ["three"]);
    drop(window);
}

#[test]
fn the_projection_lists_kept_records_newest_first_with_their_source_and_the_actual_capture() {
    let pane = Pane::new();
    let launcher = pane.start();
    open(&launcher, COMMAND);
    // Recording from the first start: no turn-on.
    assert!(pane.clipboard.copy("first", Some("notepad.exe")));
    pane.clock.advance(std::time::Duration::from_secs(60));
    assert!(pane.clipboard.copy("second\nline", None));

    let view = launcher.clipboard_history().unwrap();
    assert_eq!(view.capture, CaptureState::On);
    let texts: Vec<&str> = view.records.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, ["second\nline", "first"]);
    assert_eq!(view.records[1].source.as_deref(), Some("notepad.exe"));
    assert_eq!(view.records[1].copied_at, 1_791_208_920_000);
    assert_eq!(view.records[0].copied_at, 1_791_208_980_000);
    assert_eq!(view.now, 1_791_208_980_000);
    assert_eq!(view.retention_seconds, 7 * 86_400);

    // Pausing and resuming change the actual state, and what is kept.
    launcher
        .set_clipboard_capture(&view, CaptureState::Paused)
        .unwrap();
    assert!(!pane.clipboard.copy("while paused", None));
    let view = launcher.clipboard_history().unwrap();
    assert_eq!(view.capture, CaptureState::Paused);
    launcher
        .set_clipboard_capture(&view, CaptureState::On)
        .unwrap();
    assert_eq!(
        launcher.view().status,
        Status::Result("Recording resumed".into())
    );
    assert_eq!(listed(&launcher), ["second\nline", "first"]);
    // Records expire after the retention, and the projection says so.
    pane.clock
        .advance(std::time::Duration::from_secs(7 * 86_400));
    assert!(listed(&launcher).is_empty());
}

#[test]
fn copy_and_delete_run_the_existing_operations_on_a_record_still_kept() {
    let pane = Pane::new();
    let launcher = pane.start();
    let window = RecordingWindow::attach(&launcher);
    open(&launcher, COMMAND);
    pane.clipboard.copy("keep me", None);
    pane.clipboard.copy("delete me", None);
    let view = launcher.clipboard_history().unwrap();
    let (newest, oldest) = (view.records[0].id.clone(), view.records[1].id.clone());

    window.take();
    launcher.copy_clipboard_record(&view, &oldest).unwrap();
    assert_eq!(pane.clipboard.written(), ["keep me"]);
    // As every Copy action: the window closes, and a HUD says so.
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(window.hides(), 1, "the window closes");
    let huds: Vec<String> = window.huds().into_iter().map(|hud| hud.title).collect();
    assert_eq!(huds, ["Copied to Clipboard"]);

    launcher.delete_clipboard_record(&view, &newest).unwrap();
    assert_eq!(
        launcher.view().status,
        Status::Result("Deleted the kept item".into())
    );
    assert_eq!(listed(&launcher), ["keep me"]);
    // The stale reading still names the deleted record: it is gone, and
    // neither copying nor deleting it does anything.
    let error = launcher.copy_clipboard_record(&view, &newest).unwrap_err();
    assert_eq!(error, "That item is no longer kept");
    assert_eq!(launcher.view().status, Status::Error(error));
    assert!(launcher.delete_clipboard_record(&view, &newest).is_err());
    assert_eq!(pane.clipboard.written(), ["keep me"]);
}

/// Enter in the split view (#150): Paste, through the recording system,
/// closing the window first; where Pane cannot paste yet, the history's own
/// copy, with a HUD saying so.
#[test]
fn paste_pastes_a_record_or_copies_it_where_paste_is_not_available() {
    let pane = Pane::new();
    let system = Arc::new(RecordingSystem::default());
    let launcher = pane.start().with_system(system.clone());
    let window = RecordingWindow::attach(&launcher);
    open(&launcher, COMMAND);
    pane.clipboard.copy("older", None);
    pane.clipboard.copy("newer", None);
    let view = launcher.clipboard_history().unwrap();
    let oldest = view.records[1].id.clone();

    // Not available here yet: copied through the history, and said so.
    block_on(launcher.paste_clipboard_record(&view, &oldest));
    assert_eq!(pane.clipboard.written(), ["older"]);
    assert!(system.take().is_empty(), "nothing was pasted");
    let huds: Vec<String> = window.huds().into_iter().map(|hud| hud.title).collect();
    assert_eq!(huds, ["Copied — paste is not available here yet"]);
    window.take();

    // Where it can, the window closes and the record is pasted, its copy
    // concealed: the history keeps nothing new.
    system.support_paste();
    launcher.set_window_presence(pane_core::WindowPresence::Shown);
    let view = launcher.clipboard_history().unwrap();
    let newest = view.records[0].id.clone();
    let text = view.records[0].text.clone();
    block_on(launcher.paste_clipboard_record(&view, &newest));
    assert_eq!(
        system.take(),
        [
            Done::Copied {
                clip: pane_core::system::Clip::Text(text.clone()),
                concealed: true,
            },
            Done::Pasted(Some(pane_core::system::Clip::Text(text))),
        ]
    );
    assert_eq!(window.hides(), 1);
    assert_eq!(pane.clipboard.written(), ["older"]);

    // A record no longer kept pastes nothing, and says why.
    launcher.delete_clipboard_record(&view, &newest).unwrap();
    block_on(launcher.paste_clipboard_record(&view, &newest));
    assert!(system.take().is_empty());
}

/// A copied image of `width` × `height` red pixels, as an adapter reports
/// it.
fn copied_image(width: u32, height: u32) -> CopiedImage {
    let pixels: Vec<u8> = [255, 0, 0, 255].repeat((width * height) as usize);
    let png = pane_core::icons::encode_png(width, height, &pixels).unwrap();
    CopiedImage::from_png(png).unwrap()
}

/// #167: a copied image and copied files are kept beside text — the image
/// as a PNG in the history's own folder, the files as their paths — under
/// the same rules (a disabled application's are not), listed with their
/// kinds, titles and the image's file; Copy and Paste put them back as
/// what they were; an oversized copy is not kept; and an image expires,
/// its PNG with it.
#[test]
fn copied_images_and_files_are_kept_and_put_back_as_what_they_were() {
    let pane = Pane::new();
    let system = Arc::new(RecordingSystem::default());
    let launcher = pane.start().with_system(system.clone());
    let window = RecordingWindow::attach(&launcher);
    let identity = PackageIdentity::default_extension("clipboard-history");
    let image = copied_image(3, 2);
    let files = vec![
        pane.data.path().join("report.pdf"),
        pane.data.path().join("photos"),
    ];
    assert!(
        pane.clipboard
            .copy_content(Content::Image(image.clone()), Some("mspaint.exe"))
    );
    assert!(
        pane.clipboard
            .copy_content(Content::Files(files.clone()), Some("explorer.exe"))
    );
    // Oversized, or too large to read: skipped.
    let huge = CopiedImage {
        png: vec![0; MAX_IMAGE_BYTES + 1],
        width: 1,
        height: 1,
    };
    assert!(pane.clipboard.copy_content(Content::Image(huge), None));
    assert!(pane.clipboard.copy_content(Content::TooLarge, None));
    // A disabled application's image is not kept either.
    block_on(launcher.set_preference(&identity, "disabledApplications", Some("Snip.exe"))).unwrap();
    assert!(
        pane.clipboard
            .copy_content(Content::Image(copied_image(1, 1)), Some("snip.exe"))
    );

    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    let kinds: Vec<ClipboardKind> = view.records.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [ClipboardKind::Files, ClipboardKind::Image]);
    let (listed_files, listed_image) = (&view.records[0], &view.records[1]);
    assert_eq!(listed_files.title(), "report.pdf +1");
    assert_eq!(listed_files.files, files);
    assert_eq!(listed_image.title(), "Image (3×2)");
    let kept = listed_image.image.clone().expect("an image record");
    assert_eq!((kept.width, kept.height), (3, 2));
    // The PNG is kept in the history's own folder, beside its file.
    assert!(
        kept.path
            .starts_with(pane.data.path().join("extensions/clipboard-images"))
    );
    assert_eq!(fs::read(&kept.path).unwrap(), image.png);
    // The dropdown keeps each by its kind.
    let browse = ClipboardBrowse {
        filter: ClipboardFilter::Images,
        ..ClipboardBrowse::default()
    };
    assert_eq!(
        ids(&browse.listing(&view.records, view.now, 0).records),
        std::slice::from_ref(&listed_image.id)
    );

    // Copy puts each back as what it was.
    launcher
        .copy_clipboard_record(&view, &listed_image.id)
        .unwrap();
    assert_eq!(
        pane.clipboard.written_images(),
        std::slice::from_ref(&image.png)
    );
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    let files_id = view.records[0].id.clone();
    launcher.copy_clipboard_record(&view, &files_id).unwrap();
    assert_eq!(pane.clipboard.written_files(), std::slice::from_ref(&files));
    assert!(pane.clipboard.written().is_empty(), "no text was written");

    // Paste: where the system can paste, files and an image have no clip
    // it pastes yet, so they are copied as what they are instead, and
    // said so.
    system.support_paste();
    window.take();
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    let image_id = view
        .records
        .iter()
        .find(|record| record.kind == ClipboardKind::Image)
        .unwrap()
        .id
        .clone();
    block_on(launcher.paste_clipboard_record(&view, &image_id));
    assert!(system.take().is_empty(), "nothing was pasted");
    assert_eq!(pane.clipboard.written_images().len(), 2);
    let huds: Vec<String> = window.huds().into_iter().map(|hud| hud.title).collect();
    assert_eq!(huds, ["Copied — paste is not available here yet"]);

    // Deleted, its PNG goes with it; expired, the same.
    let kept_path = kept.path.clone();
    open(&launcher, COMMAND);
    assert!(
        pane.clipboard
            .copy_content(Content::Image(copied_image(4, 4)), None)
    );
    let view = launcher.clipboard_history().unwrap();
    let newest = view.records[0].image.clone().expect("the new image");
    assert!(newest.path.is_file());
    launcher
        .delete_clipboard_record(&view, &view.records[0].id)
        .unwrap();
    assert!(!newest.path.exists(), "a deleted image's PNG is deleted");
    assert!(kept_path.is_file());
    pane.clock
        .advance(std::time::Duration::from_secs(7 * 86_400));
    assert!(listed(&launcher).is_empty());
    // The background sweep can remove the records before it finishes
    // deleting their PNGs. Wait for that sweep before checking the disk.
    assert!(
        launcher.wait_for_clipboard_expiry(std::time::Duration::from_secs(300)),
        "the expiry thread never swept"
    );
    assert!(!kept_path.exists(), "an expired image's PNG is deleted");
    drop(window);
}

#[test]
fn a_reading_of_a_screen_left_or_a_package_stopped_runs_nothing() {
    let pane = Pane::new();
    let launcher = pane.start();
    open(&launcher, COMMAND);
    pane.clipboard.copy("kept", None);
    let view = launcher.clipboard_history().unwrap();
    let id = view.records[0].id.clone();

    // Left for root search: the reading is stale and changes nothing, not
    // even the status of the screen now shown.
    launcher.back();
    assert!(launcher.view().query().is_some());
    let status = launcher.view().status;
    assert!(launcher.copy_clipboard_record(&view, &id).is_err());
    assert!(launcher.delete_clipboard_record(&view, &id).is_err());
    assert!(
        launcher
            .set_clipboard_capture(&view, CaptureState::Off)
            .is_err()
    );
    assert_eq!(launcher.view().status, status);
    assert!(pane.clipboard.written().is_empty());

    // Opened again, then the package disabled: its command closes, and the
    // reading made before can no longer act.
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    let identity = PackageIdentity::default_extension("clipboard-history");
    block_on(launcher.set_enabled(&identity, false));
    assert!(launcher.clipboard_history().is_none());
    assert!(launcher.copy_clipboard_record(&view, &id).is_err());
    assert!(pane.clipboard.written().is_empty());
    // Its records stay kept for when it is enabled again.
    block_on(launcher.set_enabled(&identity, true));
    open(&launcher, COMMAND);
    assert_eq!(listed(&launcher), ["kept"]);
}

#[test]
fn a_clipboard_pane_cannot_watch_says_why_and_offers_no_copy() {
    let reason = "Not available: this desktop gives Pane no clipboard";
    let pane = Pane::with(FakeClipboard {
        unavailable: Some(reason.into()),
        ..FakeClipboard::default()
    });
    let launcher = pane.start();
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().expect("the history is shown");
    assert_eq!(view.problem.as_deref(), Some(reason));
    assert_eq!(view.copy_unavailable.as_deref(), Some(reason));
    assert_eq!(view.summary(), reason);
    // Paused, resuming is refused with the reason, and nothing changes.
    launcher
        .set_clipboard_capture(&view, CaptureState::Paused)
        .unwrap();
    let view = launcher.clipboard_history().unwrap();
    assert_eq!(
        launcher.set_clipboard_capture(&view, CaptureState::On),
        Err(reason.to_owned())
    );
    assert_eq!(launcher.view().status, Status::Error(reason.into()));
    assert_eq!(
        launcher.clipboard_history().unwrap().capture,
        CaptureState::Paused
    );
}
