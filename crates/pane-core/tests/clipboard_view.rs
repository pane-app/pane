//! The Clipboard History split view's read-only projection (#102), through
//! the launcher's public interface: only Pane's registered Clipboard
//! History default extension, open on its command's own screen, is
//! projected; the projection reads the package's kept text records,
//! newest first, with when and where each was copied, the actual capture
//! state and whether its operations can run. The browse rules the window
//! adapts — filtering, grouping by local day, keeping the selection in
//! range — are plain functions over the records. Operations revalidate
//! the reading they were made from: a reading of a screen the user left,
//! of a package that stopped, or of a record no longer kept changes
//! nothing it should not.
//!
//! The package is the one `cargo xtask guests` assembles in
//! `target/guests/packages/clipboard-history`, acquired as Pane's default
//! extension from an artifact source on 127.0.0.1 (`support/artifacts.rs`);
//! the system's clipboard is a fake that never touches the real one.

use std::fs;
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::clipboard::{
    CaptureState, ClipboardSystem, Content, ManualClock, Markers, Observation, Sink, Watch,
};
use pane_core::clipboard_view::{
    ClipboardBrowse, ClipboardDay, ClipboardFilter, ClipboardRecord, capture_summary, copied_line,
    day_of, time_label,
};
use pane_core::defaults::ArtifactSource;
use pane_core::{DefaultExtension, Launcher, PackageIdentity, Runtime, Screen, Status};
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
        id: id.into(),
        text: text.into(),
        copied_at,
        source: source.map(str::to_owned),
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
fn records_group_into_today_yesterday_and_older_in_local_time() {
    let records = kept();
    let days: Vec<ClipboardDay> = records
        .iter()
        .map(|record| day_of(record.copied_at, NOW, PLUS_7))
        .collect();
    assert_eq!(
        days,
        [
            ClipboardDay::Today,
            ClipboardDay::Today,
            ClipboardDay::Yesterday,
            ClipboardDay::Older,
            ClipboardDay::Older
        ]
    );
    // In UTC the record copied at 00:22 local is yesterday's.
    assert_eq!(
        day_of(records[1].copied_at, NOW, 0),
        ClipboardDay::Yesterday
    );

    let listing = ClipboardBrowse::default().listing(&records, NOW, PLUS_7);
    let sections: Vec<(&str, usize)> = listing
        .sections
        .iter()
        .map(|section| (section.day.label(), section.first))
        .collect();
    assert_eq!(sections, [("Today", 0), ("Yesterday", 2), ("Older", 3)]);
    // The store's order is kept, whatever the grouping.
    assert_eq!(ids(&listing.records), ["9", "8", "7", "4", "2"]);
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
        "Text is kept for 7 days · copies marked private are skipped"
    );
    assert_eq!(
        capture_summary(CaptureState::On, None, 3600, 2),
        "Text is kept for 1 hour · copies marked private are skipped · 2 programs excluded"
    );
    assert_eq!(
        capture_summary(CaptureState::Paused, None, 3600, 1),
        "Paused · nothing you copy is kept until you resume"
    );
    assert_eq!(
        capture_summary(CaptureState::Off, None, 3600, 0),
        "Off · nothing you copy is kept until you turn it on"
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
    // Production's filters are All and Text, over text records alone.
    assert_eq!(
        ClipboardFilter::ALL.map(ClipboardFilter::label),
        ["All", "Text"]
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

    fn written(&self) -> Vec<String> {
        self.inner.lock().unwrap().written.clone()
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
    assert_eq!(view.capture, CaptureState::Off);
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
    assert!(
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.title == "Turn on clipboard history"),
        "its generic list is unchanged"
    );

    // A form opened from the command's own list is not the history.
    open(&launcher, COMMAND);
    let retention = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id == "retention")
        .expect("the retention row");
    launcher.select(retention);
    block_on(launcher.activate_selected());
    assert!(launcher.view().form().is_some());
    assert!(launcher.clipboard_history().is_none());
    launcher.back();
    assert!(launcher.clipboard_history().is_some());
}

#[test]
fn the_projection_lists_kept_records_newest_first_with_their_source_and_the_actual_capture() {
    let pane = Pane::new();
    let launcher = pane.start();
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    // Turning history on is the existing capture operation: nothing is
    // kept before it.
    assert!(!pane.clipboard.copy("before", None));
    launcher
        .set_clipboard_capture(&view, CaptureState::On)
        .unwrap();
    assert_eq!(
        launcher.view().status,
        Status::Result("Clipboard history is on".into())
    );
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
        Status::Result("Clipboard history is on again".into())
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
    let view = launcher.clipboard_history().unwrap();
    launcher
        .set_clipboard_capture(&view, CaptureState::On)
        .unwrap();
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
    let view = launcher.clipboard_history().unwrap();
    launcher
        .set_clipboard_capture(&view, CaptureState::On)
        .unwrap();
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

#[test]
fn a_reading_of_a_screen_left_or_a_package_stopped_runs_nothing() {
    let pane = Pane::new();
    let launcher = pane.start();
    open(&launcher, COMMAND);
    let view = launcher.clipboard_history().unwrap();
    launcher
        .set_clipboard_capture(&view, CaptureState::On)
        .unwrap();
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
    // Turning it on is refused with the reason, and nothing changes.
    assert_eq!(
        launcher.set_clipboard_capture(&view, CaptureState::On),
        Err(reason.to_owned())
    );
    assert_eq!(launcher.view().status, Status::Error(reason.into()));
    assert_eq!(
        launcher.clipboard_history().unwrap().capture,
        CaptureState::Off
    );
}
