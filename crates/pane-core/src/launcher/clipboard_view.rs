//! The Clipboard History split view's projection (#102, #166): a narrow,
//! read-only presentation of the clipboard history Pane keeps for its own
//! registered Clipboard History default extension, for the window to draw
//! as a list beside a preview and the record's Information.
//!
//! Only that command is projected, identified by its verified package and
//! command identity — the default extension `clipboard-history` and its
//! command of the same id, open on its own list screen — never by a title
//! or a row's text. Any other command, a similarly titled one included,
//! keeps the generic rendering of its own list and reaches no other
//! package's records. The projection reads the package's existing history
//! store ([`crate::clipboard`]); it adds no store, no watcher and no
//! capture capability, and it changes nothing by being read.
//!
//! What the window does with the records — which a query and the type
//! dropdown keep, how they group by local day, which one is selected — is
//! the adapter's state over plain functions here ([`ClipboardBrowse`],
//! [`ClipboardKind::of_text`], [`day_of`], [`time_label`],
//! [`information`]), so the rules are the same wherever they are drawn.
//!
//! A record is text (a link and a colour being text Pane recognizes), a
//! copied image — drawn from the PNG the history keeps of it
//! ([`ClipboardImage`]) — or copied files, by their paths (#167).
//!
//! The records are made once per change of the history and shared (#192):
//! the launcher keeps them with the history's count of changes
//! (`HistoryStore::changes`), which a copy kept, a deletion, a choice
//! changed and an expiry move, and every reading made at the same count —
//! each frame the window draws, the Actions panel's — shares the same
//! records ([`ClipboardHistoryView::records`]), copying none of them.
//!
//! The operations are the history's existing ones — copy a record again,
//! delete it, pause or resume recording, keep history for another time,
//! clear it (once the user confirms) — and pasting a record into the
//! application that was in front (#150: Enter), which copies it instead
//! where Pane cannot paste yet. The Actions panel lists them for the
//! selected record ([`ClipboardHistoryView::actions`]); each revalidates
//! the [reading](ClipboardHistoryView) it was made from first: the same
//! screen, the same verified command, the same generation of the package's
//! code, and a record still kept. A reading the user left, of a package
//! disabled, replaced or uninstalled since, or of a record deleted or
//! expired since, changes nothing it should not. The extension's Settings
//! page has the same controls, as preferences whose values are the
//! history's own (see `clipboard_settings`).

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use super::own_actions::COPIED;
use super::{Launcher, Screen, State, Status, owner};
use crate::clipboard::history::PackageHistory;
use crate::clipboard::{self, CaptureState, Commands, program_file_name};
use crate::extension_data::PackageData;
use crate::feedback::{Caller, GivenConfirmation, Hud, ToastStyle};
use crate::icons::{Icon, IconSource};
use crate::packages::PackageIdentity;

/// The id of Pane's Clipboard History default extension, and of its one
/// command in its manifest.
pub const CLIPBOARD_HISTORY: &str = "clipboard-history";

/// The retentions the Actions panel and the Settings page offer, in
/// seconds, with what they are called: 1 hour, 1 day, 7 days, 30 days and
/// 90 days (the host accepts any from 1 minute to 365 days).
pub const RETENTIONS: [(u64, &str); 5] = [
    (3600, "1 Hour"),
    (86_400, "1 Day"),
    (7 * 86_400, "7 Days"),
    (30 * 86_400, "30 Days"),
    (90 * 86_400, "90 Days"),
];

/// What a kept record is, as the type dropdown and the Information say.
/// Links and colours are text Pane recognizes as a URL or a colour value
/// ([`ClipboardKind::of_text`]); an image and files are what the history
/// kept them as (#167).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipboardKind {
    /// Plain text.
    #[default]
    Text,
    /// Text that is one URL (`https://…`, `mailto:…`, `www.…`).
    Link,
    /// Text that is one colour value (`#ff8800`, `rgb(…)`, `hsl(…)`).
    Color,
    /// A copied image, kept as a PNG.
    Image,
    /// Copied files and folders, kept as their paths.
    Files,
}

impl ClipboardKind {
    /// The kind `text` is: a link or a colour when the whole text, trimmed,
    /// is one URL or one colour value; plain text otherwise.
    pub fn of_text(text: &str) -> ClipboardKind {
        let text = text.trim();
        if is_link(text) {
            ClipboardKind::Link
        } else if is_color(text) {
            ClipboardKind::Color
        } else {
            ClipboardKind::Text
        }
    }

    /// Its name in the Information: "Text", "Link", "Color", "Image",
    /// "File".
    pub fn label(self) -> &'static str {
        match self {
            ClipboardKind::Text => "Text",
            ClipboardKind::Link => "Link",
            ClipboardKind::Color => "Color",
            ClipboardKind::Image => "Image",
            ClipboardKind::Files => "File",
        }
    }

    /// Whether a record of this kind is text: plain text, a link or a
    /// colour; an image and files are not.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            ClipboardKind::Text | ClipboardKind::Link | ClipboardKind::Color
        )
    }
}

/// Whether `text` (trimmed) is one URL: a scheme and `//` with something
/// after it (`https://example.com`), a `mailto:` address, or a `www.` host;
/// with no white space anywhere.
fn is_link(text: &str) -> bool {
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = text.to_lowercase();
    if let Some(address) = lower.strip_prefix("mailto:") {
        return address.contains('@');
    }
    if let Some(host) = lower.strip_prefix("www.") {
        return host.contains('.') && !host.starts_with('.') && !host.ends_with('.');
    }
    let Some((scheme, rest)) = lower.split_once("://") else {
        return false;
    };
    let mut letters = scheme.chars();
    letters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && letters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !rest.is_empty()
}

/// Whether `text` (trimmed) is one colour value: a hex colour of 3, 6 or 8
/// digits after `#` (`#rgb`, `#rrggbb`, `#rrggbbaa`; four digits stay
/// text), or a CSS `rgb`, `rgba`, `hsl` or `hsla` function of three or
/// four numbers (with `%` or `deg`, separated by commas, spaces or a
/// slash).
fn is_color(text: &str) -> bool {
    if let Some(hex) = text.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = text.to_lowercase();
    let Some((function, rest)) = lower.split_once('(') else {
        return false;
    };
    if !matches!(function.trim(), "rgb" | "rgba" | "hsl" | "hsla") {
        return false;
    }
    let Some(arguments) = rest.strip_suffix(')') else {
        return false;
    };
    let values: Vec<&str> = arguments
        .split([',', ' ', '/'])
        .filter(|value| !value.is_empty())
        .collect();
    (3..=4).contains(&values.len())
        && values.iter().all(|value| {
            let number = value.trim_end_matches('%').trim_end_matches("deg");
            !number.is_empty() && number.parse::<f64>().is_ok()
        })
}

/// One kept record, as the split view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardRecord {
    /// Opaque: identifies the record among its package's, for its
    /// operations; never reused.
    pub id: String,
    /// The full stored text; for an image its title ("Image (1920×1080)"),
    /// for files their paths, one per line. Shared: a window draws it
    /// without copying it.
    pub text: Arc<str>,
    /// What it is: text, a link, a colour, an image or files.
    pub kind: ClipboardKind,
    /// The image it is, if it is one (#167).
    pub image: Option<ClipboardImage>,
    /// The files it is, by their paths in the order copied, if it is files
    /// (#167); empty otherwise.
    pub files: Vec<PathBuf>,
    /// When it was copied, in milliseconds since the Unix epoch.
    pub copied_at: u64,
    /// The program it was copied from, as the system named it — its path
    /// on Windows (`C:\Windows\notepad.exe`), else its file name or
    /// process name (`notepad.exe`) — if it did.
    pub source: Option<String>,
    /// Why Pane cannot read it on this computer, if it cannot (#130): its
    /// text then says so, and it is neither copied nor pasted, only
    /// deleted or left to expire.
    pub unreadable: Option<String>,
}

/// A kept image, as the split view draws it: the PNG Pane keeps of it
/// (its thumbnail and its preview) and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardImage {
    /// Where its PNG is, in the history's own folder.
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

impl ClipboardRecord {
    /// A record of `text`, its kind recognized from it, with no source.
    pub fn text(id: impl Into<String>, text: impl Into<String>, copied_at: u64) -> Self {
        let text = text.into();
        ClipboardRecord {
            id: id.into(),
            kind: ClipboardKind::of_text(&text),
            text: text.into(),
            image: None,
            files: Vec::new(),
            copied_at,
            source: None,
            unreadable: None,
        }
    }

    /// A record of the image `image`, titled by its size, with no source.
    pub fn image(id: impl Into<String>, image: ClipboardImage, copied_at: u64) -> Self {
        ClipboardRecord {
            id: id.into(),
            text: clipboard::history::image_title(image.width, image.height).into(),
            kind: ClipboardKind::Image,
            image: Some(image),
            files: Vec::new(),
            copied_at,
            source: None,
            unreadable: None,
        }
    }

    /// A record of the files `files`, with no source.
    pub fn files(id: impl Into<String>, files: Vec<PathBuf>, copied_at: u64) -> Self {
        ClipboardRecord {
            id: id.into(),
            text: files
                .iter()
                .map(|file| file.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
            kind: ClipboardKind::Files,
            image: None,
            files,
            copied_at,
            source: None,
            unreadable: None,
        }
    }

    /// The record's title: for text, its first line with text, trimmed
    /// (empty for text that is all white space); for an image "Image
    /// (1920×1080)"; for files the first one's name, with "+2" for two
    /// more.
    pub fn title(&self) -> Cow<'_, str> {
        if let Some(first) = self.files.first() {
            let name = file_name(first);
            return match self.files.len() {
                1 => Cow::Owned(name),
                count => Cow::Owned(format!("{name} +{}", count - 1)),
            };
        }
        Cow::Borrowed(
            self.text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or(""),
        )
    }

    /// How many characters its text has.
    pub fn characters(&self) -> usize {
        self.text.chars().count()
    }

    /// Its image's width and height, if it is an image.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.image.as_ref().map(|image| (image.width, image.height))
    }

    /// The name of the program it was copied from, as the Information's
    /// Source says it: its file name without the extension (`notepad`,
    /// `Code`), if the system named it.
    pub fn source_name(&self) -> Option<String> {
        let source = self.source.as_deref()?;
        let file = program_file_name(source).trim();
        let name = match file.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem,
            _ => file,
        };
        (!name.is_empty()).then(|| name.to_owned())
    }

    /// The program it was copied from, by its full path, where the system
    /// gave one (Windows): its system icon is the Source's icon.
    pub fn source_path(&self) -> Option<PathBuf> {
        let path = PathBuf::from(self.source.as_deref()?);
        path.is_absolute().then_some(path)
    }

    /// Whether `needle` (lowercased, trimmed, not empty) is in the record's
    /// text or its program's file name.
    fn holds(&self, needle: &str) -> bool {
        self.text.to_lowercase().contains(needle)
            || self
                .source
                .as_deref()
                .is_some_and(|source| program_file_name(source).to_lowercase().contains(needle))
    }
}

/// The name of the file or folder at `path`, as a files record's title and
/// its preview name it: its last component, or the whole path for a drive
/// or the root (`C:\`, `/`).
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The clipboard history of the open Clipboard History command, as read
/// now. It also carries the reading's identity — the screen and the
/// package's generation it was read in — which its operations revalidate.
#[derive(Clone)]
pub struct ClipboardHistoryView {
    /// The package that owns the records: Pane's Clipboard History.
    pub owner: PackageIdentity,
    /// The command's own title, as its list names it.
    pub title: String,
    /// The kept records, newest first, none expired: shared by every
    /// reading made while the history did not change (#192).
    pub records: Arc<[ClipboardRecord]>,
    /// Why the history cannot be read now, if it cannot; no records are
    /// listed then.
    pub unreadable: Option<String>,
    /// Now, by the clock the history expires by, in milliseconds since the
    /// Unix epoch.
    pub now: u64,
    /// Whether what is copied is recorded: the actual capture state. Pane's
    /// own Clipboard History records from the first start.
    pub capture: CaptureState,
    /// Why Pane does not watch the clipboard although it should, or cannot
    /// on this system, if so.
    pub problem: Option<String>,
    /// How long each record is kept after it was copied, in seconds.
    pub retention_seconds: u64,
    /// How many applications are disabled: their copies are not recorded.
    pub excluded: usize,
    /// Why a record cannot be copied again now, if it cannot: this Pane
    /// has no clipboard it can write.
    pub copy_unavailable: Option<String>,
    reading: Reading,
}

/// What a reading was made in.
#[derive(Clone)]
struct Reading {
    /// The screen it was read on.
    epoch: u64,
    /// The package's data in the generation it was read in.
    data: PackageData,
    /// The open command's component, for which Clear History asks the
    /// user to confirm.
    component: PathBuf,
}

impl fmt::Debug for ClipboardHistoryView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardHistoryView")
            .field("owner", &self.owner)
            .field("records", &self.records)
            .field("unreadable", &self.unreadable)
            .field("capture", &self.capture)
            .field("problem", &self.problem)
            .field("epoch", &self.reading.epoch)
            .finish_non_exhaustive()
    }
}

/// What the launcher made of Pane's own clipboard history for the view, as
/// it was at a count of the history's changes (#192): made again only once
/// the count moved, and shared by every reading until then.
#[derive(Default)]
pub(super) struct Projected {
    made: Option<Projection>,
    /// How many times the records were made, for tests.
    times: u64,
}

impl Projected {
    /// What was made of `owner`'s history, shared, if it was made from the
    /// store `store` (`HistoryStore::id`) at its count of changes
    /// `changes`.
    fn shared(&self, store: u64, owner: &str, changes: u64) -> Option<Shown> {
        self.made
            .as_ref()
            .filter(|made| made.store == store && made.changes == changes && made.owner == owner)
            .map(|made| made.shown.clone())
    }

    /// Keeps `made`, in place of what was made before.
    fn keep(&mut self, made: Projection) {
        self.times += 1;
        self.made = Some(made);
    }

    /// Lets go of what was made: the view closed.
    fn forget(&mut self) {
        self.made = None;
    }
}

/// The view's records and the history's choices, at a count of changes.
struct Projection {
    /// The store it was read from (`HistoryStore::id`): each store counts
    /// its changes from 0.
    store: u64,
    /// The owner whose history it is.
    owner: String,
    /// The history's count of changes it was made at
    /// (`HistoryStore::changes`).
    changes: u64,
    shown: Shown,
}

/// What a reading shows of the history: cloning it shares the records.
#[derive(Clone)]
struct Shown {
    records: Arc<[ClipboardRecord]>,
    unreadable: Option<String>,
    capture: CaptureState,
    retention_seconds: u64,
    excluded: usize,
}

impl Shown {
    /// No records, and why: the history cannot be read.
    fn unreadable(reason: String) -> Shown {
        Shown::without(Vec::<ClipboardRecord>::new().into(), reason)
    }

    /// No records, and why: the package's code stopped. Read again at
    /// every reading, so its records are one list shared by them all, and
    /// the window, which redraws once the records are another list, does
    /// not redraw for it.
    fn refused(reason: String) -> Shown {
        static NONE: OnceLock<Arc<[ClipboardRecord]>> = OnceLock::new();
        let records = NONE.get_or_init(|| Vec::<ClipboardRecord>::new().into());
        Shown::without(records.clone(), reason)
    }

    /// `records`, holding none, and why.
    fn without(records: Arc<[ClipboardRecord]>, reason: String) -> Shown {
        let history = PackageHistory::default();
        Shown {
            records,
            unreadable: Some(reason),
            capture: history.capture,
            retention_seconds: history.retention(),
            excluded: 0,
        }
    }
}

impl Projection {
    /// The records of `owner`'s history `history` (read at `changes`),
    /// whose images are where `store` keeps them.
    fn of(
        owner: &str,
        changes: u64,
        history: Result<PackageHistory, String>,
        store: &crate::clipboard::history::HistoryStore,
    ) -> Projection {
        let history = match history {
            Ok(history) => history,
            Err(reason) => {
                return Projection {
                    store: store.id(),
                    owner: owner.to_owned(),
                    changes,
                    shown: Shown::unreadable(reason),
                };
            }
        };
        let records = history
            .items
            .iter()
            .map(|item| {
                // An image's PNG is where the store keeps it (#167); an
                // item that cannot be read (#130) is shown as its
                // explanation.
                let image = item.image.as_ref().and_then(|image| {
                    item.unreadable.is_none().then(|| ClipboardImage {
                        path: store.image_path(owner, &image.digest),
                        width: image.width,
                        height: image.height,
                    })
                });
                let kind = if image.is_some() {
                    ClipboardKind::Image
                } else if !item.files.is_empty() {
                    ClipboardKind::Files
                } else {
                    ClipboardKind::of_text(&item.text)
                };
                ClipboardRecord {
                    id: item.id.to_string(),
                    kind,
                    text: item.text.as_str().into(),
                    image,
                    files: item.files.clone(),
                    copied_at: item.copied_at,
                    source: item.source.clone(),
                    unreadable: item.unreadable.clone(),
                }
            })
            .collect();
        Projection {
            store: store.id(),
            owner: owner.to_owned(),
            changes,
            shown: Shown {
                records,
                unreadable: None,
                capture: history.capture,
                retention_seconds: history.retention(),
                excluded: history.excluded.len(),
            },
        }
    }
}

/// The Actions panel's section over the history's own actions.
const HISTORY_SECTION: &str = "Clipboard History";
/// The Actions panel's section over the retentions.
const KEEP_SECTION: &str = "Keep History For";
/// The Actions panel's section over the way to the Settings page.
const SETTINGS_SECTION: &str = "Settings";

impl ClipboardHistoryView {
    /// The record with `id`, if this reading lists it.
    pub fn record(&self, id: &str) -> Option<&ClipboardRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    /// One line on what is recorded now, as it actually is (see
    /// [`capture_summary`]).
    pub fn summary(&self) -> String {
        capture_summary(
            self.capture,
            self.problem.as_deref(),
            self.retention_seconds,
            self.excluded,
        )
    }

    /// What the Actions panel lists with the record `selected` (by id;
    /// `None` with none selected): the record's own actions — Paste, Copy
    /// to Clipboard, Delete Entry — then the history's — Pause Recording
    /// (or Resume Recording) and Clear History… — then Keep History For,
    /// one entry per retention offered (the one in force cannot be chosen
    /// again), then Disabled Applications…, which opens the extension's
    /// Settings page. Nothing is listed without a working operation
    /// behind it: a record's actions only with one selected, Copy
    /// unavailable where this Pane cannot write the clipboard, Clear
    /// History only with records kept, and the history's actions only while
    /// it can be read.
    pub fn actions(&self, selected: Option<&str>) -> Vec<ClipboardActionItem> {
        let item =
            |action: ClipboardAction, label: &str, available: bool, section: Option<&str>| {
                ClipboardActionItem {
                    action,
                    label: label.to_owned(),
                    available,
                    destructive: matches!(
                        action,
                        ClipboardAction::Delete | ClipboardAction::ClearHistory
                    ),
                    section: section.map(str::to_owned),
                }
            };
        let mut items = Vec::new();
        if selected.is_some_and(|id| self.record(id).is_some()) {
            items.push(item(ClipboardAction::Paste, "Paste", true, None));
            items.push(item(
                ClipboardAction::Copy,
                "Copy to Clipboard",
                self.copy_unavailable.is_none(),
                None,
            ));
            items.push(item(ClipboardAction::Delete, "Delete Entry", true, None));
        }
        if self.unreadable.is_some() {
            return items;
        }
        items.push(match self.capture {
            CaptureState::On => item(
                ClipboardAction::PauseRecording,
                "Pause Recording",
                true,
                Some(HISTORY_SECTION),
            ),
            // Where Pane cannot watch the clipboard, resuming is refused
            // with the reason: it is offered, to say so.
            CaptureState::Paused | CaptureState::Off => item(
                ClipboardAction::ResumeRecording,
                "Resume Recording",
                true,
                Some(HISTORY_SECTION),
            ),
        });
        items.push(item(
            ClipboardAction::ClearHistory,
            "Clear History…",
            !self.records.is_empty(),
            Some(HISTORY_SECTION),
        ));
        for (seconds, label) in RETENTIONS {
            let current = seconds == self.retention_seconds;
            items.push(ClipboardActionItem {
                action: ClipboardAction::KeepFor(seconds),
                label: if current {
                    format!("{label} (current)")
                } else {
                    label.to_owned()
                },
                available: !current,
                destructive: false,
                section: Some(KEEP_SECTION.to_owned()),
            });
        }
        items.push(item(
            ClipboardAction::DisabledApplications,
            "Disabled Applications…",
            true,
            Some(SETTINGS_SECTION),
        ));
        items
    }
}

/// One action the Actions panel offers in the Clipboard History view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardAction {
    /// Pastes the selected record (Enter).
    Paste,
    /// Copies the selected record again.
    Copy,
    /// Deletes the selected record.
    Delete,
    /// Stops recording what is copied until it is resumed.
    PauseRecording,
    /// Records what is copied again.
    ResumeRecording,
    /// Deletes every kept record, once the user confirms.
    ClearHistory,
    /// Keeps each record this many seconds after it was copied.
    KeepFor(u64),
    /// Opens the extension's Settings page, where the applications whose
    /// copies are not recorded are chosen (the window does).
    DisabledApplications,
}

/// One entry of the Actions panel in the Clipboard History view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardActionItem {
    pub action: ClipboardAction,
    /// What the entry says.
    pub label: String,
    /// Whether it can run now.
    pub available: bool,
    /// Whether it deletes something: drawn in the destructive style.
    pub destructive: bool,
    /// The label of its section; the record's own actions have none.
    pub section: Option<String>,
}

/// One line on what clipboard history records: `problem`, if there is
/// one; otherwise whether recording is off, paused or on, and on for how
/// long (`retention_seconds`) and with how many applications disabled
/// (`excluded`). It promises nothing the history does not do: copies an
/// application marks as concealed are skipped, and disabled applications
/// are counted.
pub fn capture_summary(
    capture: CaptureState,
    problem: Option<&str>,
    retention_seconds: u64,
    excluded: usize,
) -> String {
    if let Some(problem) = problem {
        return problem.to_owned();
    }
    match capture {
        CaptureState::Off => {
            "Recording is off · nothing you copy is kept until you resume it".into()
        }
        CaptureState::Paused => {
            "Recording is paused · nothing you copy is kept until you resume it".into()
        }
        CaptureState::On => {
            let kept = format!(
                "Recording · kept {} · copies marked private are skipped",
                span(retention_seconds)
            );
            match excluded {
                0 => kept,
                1 => format!("{kept} · 1 application disabled"),
                excluded => format!("{kept} · {excluded} applications disabled"),
            }
        }
    }
}

/// "for 7 days", "for 1 hour".
fn span(seconds: u64) -> String {
    let (count, one, many) = match seconds {
        _ if seconds.is_multiple_of(86_400) => (seconds / 86_400, "day", "days"),
        _ if seconds.is_multiple_of(3600) => (seconds / 3600, "hour", "hours"),
        _ => (seconds / 60, "minute", "minutes"),
    };
    if count == 1 {
        format!("for 1 {one}")
    } else {
        format!("for {count} {many}")
    }
}

/// What pasting `record` puts on the clipboard for the application in front
/// to paste: its text, or its one file. An image or several files have no
/// [`Clip`](crate::system::Clip) the system pastes yet: `None`, and Paste
/// copies them as what they are instead (#167), as where Pane cannot paste.
fn pasted_clip(record: &ClipboardRecord) -> Option<crate::system::Clip> {
    match (&record.image, record.files.as_slice()) {
        (Some(_), _) => None,
        (None, []) => Some(crate::system::Clip::Text(record.text.to_string())),
        (None, [file]) => Some(crate::system::Clip::File(file.clone())),
        (None, _) => None,
    }
}

/// "1 kept item", "3 kept items".
pub(super) fn kept_items(count: usize) -> String {
    if count == 1 {
        "1 kept item".into()
    } else {
        format!("{count} kept items")
    }
}

impl Launcher {
    /// The clipboard history the split view shows, while Pane's registered
    /// Clipboard History command is open on its own list and its package
    /// runs; `None` on every other screen and for every other command.
    /// Read-only: see the module documentation. Its records are made again
    /// only once the history changed; until then every reading shares them
    /// and copies nothing of the history (#192).
    pub fn clipboard_history(&self) -> Option<ClipboardHistoryView> {
        let mut state = self.lock();
        let Some((identity, data)) = self.verified_clipboard(&state) else {
            // The view closed (the window asks as the screen changes): what
            // was made for it is let go of, and made again once it opens.
            state.clipboard_records.forget();
            return None;
        };
        let epoch = state.screen_epoch;
        let title = state.view.title.clone();
        let component = state.open.clone()?;
        drop(state);
        let (problem, copy_unavailable) = match &self.clipboard {
            Some(capture) => (capture.problem(), capture.system().unavailable()),
            None => {
                let unavailable = clipboard::none().unavailable();
                (unavailable.clone(), unavailable)
            }
        };
        let owner = data.owner();
        let (now, shown) = match data.clipboard_history() {
            Ok(store) => {
                let changes = store.changes();
                let kept = self
                    .lock()
                    .clipboard_records
                    .shared(store.id(), owner, changes);
                let shown = match kept {
                    Some(shown) => shown,
                    None => {
                        // Read, and made, off the launcher's lock.
                        let (changes, history) = store.get_counted(owner);
                        let made = Projection::of(owner, changes, history, store);
                        let shown = made.shown.clone();
                        self.lock().clipboard_records.keep(made);
                        shown
                    }
                };
                (store.now(), shown)
            }
            // Code that stopped reads nothing: nothing is kept of it.
            Err(refusal) => (0, Shown::refused(refusal)),
        };
        Some(ClipboardHistoryView {
            owner: identity,
            title,
            records: shown.records,
            unreadable: shown.unreadable,
            now,
            capture: shown.capture,
            problem,
            retention_seconds: shown.retention_seconds,
            excluded: shown.excluded,
            copy_unavailable,
            reading: Reading {
                epoch,
                data,
                component,
            },
        })
    }

    /// How many times the Clipboard History view's records were made from
    /// the history (#192), for tests: drawing the view again with nothing
    /// changed makes none.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn clipboard_records_made(&self) -> u64 {
        self.lock().clipboard_records.times
    }

    /// The icon of the program or file at `path` (a record's
    /// [`ClipboardRecord::source_path`], or one of its
    /// [`ClipboardRecord::files`], #167) as the Information's Source, a
    /// files record's row and its preview show it now: the system's icon
    /// once Pane extracted it (#142), which it starts doing now if it has
    /// not; a neutral placeholder until then, and for good if the system
    /// has none.
    pub fn clipboard_source_icon(&self, path: &std::path::Path) -> Icon {
        let icon = Icon {
            source: IconSource::File(path.to_path_buf()),
            tint: None,
            mask: None,
            fallback: None,
            tooltip: None,
        };
        let loads = self.lock().icon_loads.clone();
        loads.want(None, &icon);
        loads.shown(None, &icon)
    }

    /// Puts the record `id` of `view` on the clipboard again, through the
    /// history's existing copy, once `view` is revalidated (see the module
    /// documentation), then closes the window and says "Copied to
    /// Clipboard" in a HUD, as every Copy action does. `Err` says why
    /// nothing was copied, in the status too; the window then stays.
    pub fn copy_clipboard_record(
        &self,
        view: &ClipboardHistoryView,
        id: &str,
    ) -> Result<(), String> {
        self.clipboard_operation(view, |commands| {
            commands.copy(id).map(|()| COPIED.to_owned())
        })?;
        if let Some(mut state) = self.lock_if_current(view.reading.epoch) {
            state.view.status = Status::Idle;
        }
        self.show_hud(Hud {
            title: COPIED.into(),
            style: ToastStyle::Success,
        });
        Ok(())
    }

    /// Pastes the record `id` of `view` into the application that was in
    /// front before Pane, once `view` is revalidated, closing Pane's window
    /// first; where Pane cannot paste on this system yet (#125), copies it
    /// through the history's existing copy instead, and a HUD says so
    /// ([`crate::system::PASTE_FALLBACK`]). Await the returned future for
    /// the paste, which the system makes off the calling thread. A stale
    /// reading or a record no longer kept pastes nothing and says why in
    /// the status.
    pub fn paste_clipboard_record(
        &self,
        view: &ClipboardHistoryView,
        id: &str,
    ) -> impl Future<Output = ()> + Send + 'static {
        let epoch = view.reading.epoch;
        // Read again: the record must still be kept, on the screen read.
        let now = self
            .clipboard_history()
            .filter(|now| now.reading.epoch == epoch && now.owner == view.owner);
        let clip = match &now {
            None => Err("That clipboard history is no longer shown".to_owned()),
            Some(now) => match now.record(id) {
                None => Err("That item is no longer kept".to_owned()),
                // One that cannot be read on this computer (#130) says why.
                Some(ClipboardRecord {
                    unreadable: Some(why),
                    ..
                }) => Err(why.clone()),
                Some(record) => Ok(pasted_clip(record)),
            },
        };
        {
            let mut state = self.lock();
            if state.screen_epoch == epoch {
                state.view.status = match &clip {
                    Err(why) => Status::Error(why.clone()),
                    // Running until it is pasted, or copied instead.
                    Ok(_) => Status::Running { since: Instant::now() },
                };
            }
        }
        let data = view.reading.data.clone();
        let capture = self.clipboard.clone();
        let id = id.to_owned();
        let launcher = self.clone();
        async move {
            let Ok(clip) = clip else {
                return;
            };
            let copy = move || {
                Commands {
                    data: &data,
                    capture,
                }
                .copy(&id)
            };
            launcher.paste_or_copy(epoch, clip, Box::new(copy)).await;
        }
    }

    /// Deletes the record `id` of `view` through the history's existing
    /// delete, once `view` is revalidated; a record no longer kept is
    /// refused. The outcome shows as the status.
    pub fn delete_clipboard_record(
        &self,
        view: &ClipboardHistoryView,
        id: &str,
    ) -> Result<(), String> {
        self.clipboard_operation(view, |commands| {
            match commands.delete(&[id.to_owned()])? {
                0 => Err("That item is no longer kept".to_owned()),
                _ => Ok("Deleted the kept item".to_owned()),
            }
        })
    }

    /// Pauses or resumes recording — the history's existing capture choice
    /// — once `view` is revalidated: [`CaptureState::Paused`] pauses it,
    /// [`CaptureState::On`] resumes it, and [`CaptureState::Off`], which
    /// Pane's own view no longer offers, turns it off. Resuming where this
    /// Pane cannot watch the clipboard is refused with the reason. The
    /// outcome shows as the status.
    pub fn set_clipboard_capture(
        &self,
        view: &ClipboardHistoryView,
        capture: CaptureState,
    ) -> Result<(), String> {
        let done = match capture {
            CaptureState::On => "Recording resumed",
            CaptureState::Paused => "Recording paused",
            CaptureState::Off => "Recording is off",
        };
        self.clipboard_operation(view, |commands| {
            commands.set_capture(capture).map(|()| done.to_owned())
        })
    }

    /// Keeps each record of `view`'s history `seconds` after it was copied
    /// — the history's existing retention — once `view` is revalidated;
    /// records already older are deleted at once, and the outcome says how
    /// many.
    pub fn set_clipboard_retention(
        &self,
        view: &ClipboardHistoryView,
        seconds: u64,
    ) -> Result<(), String> {
        self.clipboard_operation(view, |commands| {
            let before = commands.status()?.items;
            commands.set_retention(seconds)?;
            let after = commands.status()?.items;
            let kept = format!("History is kept {}", span(seconds));
            Ok(match before.saturating_sub(after) {
                0 => kept,
                deleted => format!("{kept}; deleted {} older", kept_items(deleted)),
            })
        })
    }

    /// Clear History: asks the user to confirm over the view (the
    /// launcher's confirmation, as a command's `feedback.confirm` is
    /// shown), then deletes every record of `view`'s history through the
    /// history's existing clear, once `view` is revalidated; recording does
    /// not change. Await the returned future for the answer; dismissed, it
    /// changes nothing. `Err` says why nothing was cleared.
    pub fn clear_clipboard_history(
        &self,
        view: &ClipboardHistoryView,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let shown = {
            let state = self.lock();
            state.screen_epoch == view.reading.epoch
                && self
                    .verified_clipboard(&state)
                    .is_some_and(|(id, _)| id == view.owner)
        };
        let asking = shown.then(|| {
            self.ask_to_confirm(
                &Caller {
                    component: view.reading.component.clone(),
                    command: Some(CLIPBOARD_HISTORY.to_owned()),
                    windowed: true,
                },
                GivenConfirmation {
                    title: "Clear Clipboard History?".into(),
                    message: Some(format!(
                        "This deletes the {} you copied. It cannot be undone.",
                        kept_items(view.records.len())
                    )),
                    primary: "Clear History".into(),
                    destructive: true,
                    dismiss: None,
                    remember: None,
                },
            )
        });
        let launcher = self.clone();
        let view = view.clone();
        async move {
            let Some(asking) = asking else {
                return Err("That clipboard history is no longer shown".to_owned());
            };
            if !asking.await? {
                return Ok(());
            }
            launcher.clipboard_operation(&view, |commands| {
                commands
                    .clear()
                    .map(|deleted| format!("Deleted {}", kept_items(deleted)))
            })
        }
    }

    /// Runs `operation` on the history `view` was read from, if `view` is
    /// still the reading on screen: the same screen, still the verified
    /// command of the same package. The generation it was read in, and the
    /// record, are checked by the history's own operations, which refuse
    /// stopped code and records no longer kept. The outcome shows as the
    /// status of that screen; a stale reading changes no status.
    fn clipboard_operation(
        &self,
        view: &ClipboardHistoryView,
        operation: impl FnOnce(&Commands<'_>) -> Result<String, String>,
    ) -> Result<(), String> {
        {
            let state = self.lock();
            let current = state.screen_epoch == view.reading.epoch
                && self
                    .verified_clipboard(&state)
                    .is_some_and(|(id, _)| id == view.owner);
            if !current {
                return Err("That clipboard history is no longer shown".into());
            }
        }
        // Off the launcher's lock: the history writes its file, and the
        // system's clipboard may take a moment.
        let commands = Commands {
            data: &view.reading.data,
            capture: self.clipboard.clone(),
        };
        let outcome = operation(&commands);
        let mut state = self.lock();
        if state.screen_epoch == view.reading.epoch {
            state.view.status = match &outcome {
                Ok(done) => Status::Result(done.clone()),
                Err(why) => Status::Error(why.clone()),
            };
        }
        outcome.map(|_| ())
    }

    /// The owner and the data of Pane's registered Clipboard History
    /// command, if it is the command open on its own list screen and its
    /// package runs: the package is the default extension
    /// [`CLIPBOARD_HISTORY`], and the open component serves exactly its
    /// command of that id.
    fn verified_clipboard(&self, state: &State) -> Option<(PackageIdentity, PackageData)> {
        if !matches!(state.view.screen, Screen::Command) {
            return None;
        }
        let component = state.open.as_ref()?;
        let package = owner(&state.packages, component)?;
        if package.identity != PackageIdentity::default_extension(CLIPBOARD_HISTORY)
            || !state.runs(package)
        {
            return None;
        }
        let commands = package.commands();
        let mut serving = commands
            .iter()
            .filter(|command| command.component == *component);
        let command = serving.next()?;
        if serving.next().is_some() || command.manifest_id() != CLIPBOARD_HISTORY {
            return None;
        }
        let data = self.installation.as_ref()?.data.owned_by(&package.identity);
        Some((package.identity.clone(), data))
    }
}

/// Which records the type dropdown keeps: All Types, Text, Images, Files,
/// Links or Colors (#166, #167). Links and colours are text, so Text keeps
/// them too; an image or files are not text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipboardFilter {
    #[default]
    All,
    Text,
    Images,
    Files,
    Links,
    Colors,
}

impl ClipboardFilter {
    /// The filters, in the dropdown's order.
    pub const ALL: [ClipboardFilter; 6] = [
        ClipboardFilter::All,
        ClipboardFilter::Text,
        ClipboardFilter::Images,
        ClipboardFilter::Files,
        ClipboardFilter::Links,
        ClipboardFilter::Colors,
    ];

    /// The dropdown's label for it.
    pub fn label(self) -> &'static str {
        match self {
            ClipboardFilter::All => "All Types",
            ClipboardFilter::Text => "Text",
            ClipboardFilter::Images => "Images",
            ClipboardFilter::Files => "Files",
            ClipboardFilter::Links => "Links",
            ClipboardFilter::Colors => "Colors",
        }
    }

    /// Its stable id, as the dropdown names its choice: "all", "text",
    /// "images", "files", "links", "colors".
    pub fn id(self) -> &'static str {
        match self {
            ClipboardFilter::All => "all",
            ClipboardFilter::Text => "text",
            ClipboardFilter::Images => "images",
            ClipboardFilter::Files => "files",
            ClipboardFilter::Links => "links",
            ClipboardFilter::Colors => "colors",
        }
    }

    /// The filter whose [`ClipboardFilter::id`] is `id`.
    pub fn from_id(id: &str) -> Option<ClipboardFilter> {
        ClipboardFilter::ALL
            .into_iter()
            .find(|filter| filter.id() == id)
    }

    /// Whether the filter keeps `record`.
    pub fn keeps(self, record: &ClipboardRecord) -> bool {
        match self {
            ClipboardFilter::All => true,
            ClipboardFilter::Text => record.kind.is_text(),
            ClipboardFilter::Images => record.kind == ClipboardKind::Image,
            ClipboardFilter::Files => record.kind == ClipboardKind::Files,
            ClipboardFilter::Links => record.kind == ClipboardKind::Link,
            ClipboardFilter::Colors => record.kind == ClipboardKind::Color,
        }
    }
}

/// The local day a record was copied, relative to today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardDay {
    Today,
    Yesterday,
    /// An earlier day: its local day number (days since 1970-01-01, local).
    Earlier(i64),
}

impl ClipboardDay {
    /// The day's section label, today being local day `today`: "Today",
    /// "Yesterday", or the date ("Thursday, Oct 1"; "Wednesday, Dec 31,
    /// 2025" for another year than today's).
    pub fn label(self, today: i64) -> String {
        match self {
            ClipboardDay::Today => "Today".into(),
            ClipboardDay::Yesterday => "Yesterday".into(),
            ClipboardDay::Earlier(day) => format!("{}, {}", weekday(day), date(day, today)),
        }
    }
}

/// A run of listed records copied on one local day: from `first` up to
/// the next section's `first` (or the end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardSection {
    pub day: ClipboardDay,
    /// The section's label: "Today", "Yesterday", "Thursday, Oct 1".
    pub label: String,
    /// The index of the run's first record in the listing.
    pub first: usize,
}

/// The split view's own state over the records: the query typed, the type
/// chosen in the dropdown and the record chosen (by id). The window
/// adapter owns it; its listing follows the rules below whatever the
/// records are now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardBrowse {
    pub query: String,
    pub filter: ClipboardFilter,
    /// The record chosen, by id; the listing selects the first record
    /// instead while this one is not listed (filtered out, deleted,
    /// expired).
    pub selected: Option<String>,
}

/// What the split view lists now (see [`ClipboardBrowse::listing`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardListing<'a> {
    /// The records the query and filter keep, in the store's newest-first
    /// order.
    pub records: Vec<&'a ClipboardRecord>,
    /// Their local days, as runs.
    pub sections: Vec<ClipboardSection>,
    /// The selected record's index in `records`; `None` only with none
    /// listed.
    pub selected: Option<usize>,
}

impl<'a> ClipboardListing<'a> {
    /// The selected record, which the preview shows and the primary action
    /// pastes.
    pub fn selected_record(&self) -> Option<&'a ClipboardRecord> {
        self.selected.map(|index| self.records[index])
    }
}

impl ClipboardBrowse {
    /// Chooses the record `id`.
    pub fn select(&mut self, id: impl Into<String>) {
        self.selected = Some(id.into());
    }

    /// What `records` (newest first) list under this state: those whose
    /// text or source holds the trimmed query, ignoring case, and the
    /// filter keeps, in their order, grouped into runs by the local day
    /// they were copied (`now` and the local time's offset from UTC, in
    /// milliseconds); and the chosen record selected while it is listed,
    /// else the first.
    pub fn listing<'a>(
        &self,
        records: &'a [ClipboardRecord],
        now: u64,
        offset_ms: i64,
    ) -> ClipboardListing<'a> {
        let listed = self.visible(records);
        let today = local_day(now, offset_ms);
        let mut sections: Vec<ClipboardSection> = Vec::new();
        for (index, record) in listed.iter().enumerate() {
            let day = day_of(record.copied_at, now, offset_ms);
            if sections.last().is_none_or(|section| section.day != day) {
                sections.push(ClipboardSection {
                    day,
                    label: day.label(today),
                    first: index,
                });
            }
        }
        let selected = self.position(&listed);
        ClipboardListing {
            records: listed,
            sections,
            selected,
        }
    }

    /// Moves the selection `delta` records through what is listed now,
    /// stopping at either end; nothing moves with none listed.
    pub fn step(&mut self, records: &[ClipboardRecord], delta: isize) {
        let listed = self.visible(records);
        let Some(index) = self.position(&listed) else {
            return;
        };
        let last = listed.len() - 1;
        let next = index.saturating_add_signed(delta).min(last);
        self.selected = Some(listed[next].id.clone());
    }

    fn visible<'a>(&self, records: &'a [ClipboardRecord]) -> Vec<&'a ClipboardRecord> {
        let needle = self.query.trim().to_lowercase();
        records
            .iter()
            .filter(|record| self.filter.keeps(record))
            .filter(|record| needle.is_empty() || record.holds(&needle))
            .collect()
    }

    fn position(&self, listed: &[&ClipboardRecord]) -> Option<usize> {
        if listed.is_empty() {
            return None;
        }
        let chosen = self
            .selected
            .as_ref()
            .and_then(|id| listed.iter().position(|record| record.id == *id));
        Some(chosen.unwrap_or(0))
    }
}

/// The Information the detail pane shows under the selected record's
/// preview, as Raycast's does: where it was copied from, what it is, how
/// long a text is or how large an image is, and when it was copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardInformation {
    /// The program it was copied from, by name (`notepad`), if the system
    /// named it.
    pub source: Option<String>,
    /// That program's full path, where the system gave one: its icon is
    /// drawn beside the name ([`Launcher::clipboard_source_icon`]).
    pub source_path: Option<PathBuf>,
    /// "Text", "Link", "Color", "Image", "File".
    pub kind: &'static str,
    /// How many characters it has, for text (a link and a colour too);
    /// `None` for an image or files.
    pub characters: Option<usize>,
    /// An image's size, "1920×1080"; `None` for anything else (#167).
    pub dimensions: Option<String>,
    /// When it was copied: "Today at 14:02", "Yesterday at 23:59",
    /// "Thursday at 09:00", "Sep 28 at 16:12".
    pub copied: String,
}

/// The Information of `record`, now being `now` at `offset_ms` from UTC.
pub fn information(record: &ClipboardRecord, now: u64, offset_ms: i64) -> ClipboardInformation {
    ClipboardInformation {
        source: record.source_name(),
        source_path: record.source_path(),
        kind: record.kind.label(),
        characters: record.kind.is_text().then(|| record.characters()),
        dimensions: record
            .dimensions()
            .map(|(width, height)| format!("{width}×{height}")),
        copied: copied_at_label(record.copied_at, now, offset_ms),
    }
}

/// When something was copied, as the Information says it: "Today at
/// 14:02", "Yesterday at 23:59", "Thursday at 09:00" within the week
/// before, else "Sep 28 at 16:12" ("Dec 31, 2025 at 09:00" for another
/// year).
pub fn copied_at_label(copied_at: u64, now: u64, offset_ms: i64) -> String {
    let (day, today) = (local_day(copied_at, offset_ms), local_day(now, offset_ms));
    let time = clock_time(copied_at, offset_ms);
    match today - day {
        ..=0 => format!("Today at {time}"),
        1 => format!("Yesterday at {time}"),
        2..=6 => format!("{} at {time}", weekday(day)),
        _ => format!("{} at {time}", date(day, today)),
    }
}

const DAY_MS: i64 = 86_400_000;

/// The local day number of `at` (milliseconds since the Unix epoch) at
/// `offset_ms` from UTC: days since 1970-01-01, local.
pub(crate) fn local_day(at: u64, offset_ms: i64) -> i64 {
    local_ms(at, offset_ms).div_euclid(DAY_MS)
}

/// `at` (milliseconds since the Unix epoch) as local milliseconds.
fn local_ms(at: u64, offset_ms: i64) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX) + offset_ms
}

/// The local day `copied_at` falls on, relative to `now`'s, at `offset_ms`
/// from UTC. A time after today (a clock set back) counts as today.
pub fn day_of(copied_at: u64, now: u64, offset_ms: i64) -> ClipboardDay {
    let day = local_day(copied_at, offset_ms);
    match local_day(now, offset_ms) - day {
        ..=0 => ClipboardDay::Today,
        1 => ClipboardDay::Yesterday,
        _ => ClipboardDay::Earlier(day),
    }
}

/// The local time of day of `at`, "14:02".
pub(crate) fn clock_time(at: u64, offset_ms: i64) -> String {
    let minutes = local_ms(at, offset_ms).rem_euclid(DAY_MS) / 60_000;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The weekday of local day `day`: 1970-01-01 was a Thursday.
fn weekday(day: i64) -> &'static str {
    WEEKDAYS[usize::try_from((day + 3).rem_euclid(7)).unwrap_or(0)]
}

/// The (year, month 1-12, day 1-31) of local day `day`, by the proleptic
/// Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn civil(day: i64) -> (i64, usize, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, usize::try_from(month).unwrap_or(1), date)
}

/// The month's short name, the day of the month and the year of local
/// day `day`: ("Sep", 28, 2026).
pub(crate) fn month_and_day(day: i64) -> (&'static str, i64, i64) {
    let (year, month, date) = civil(day);
    (MONTHS[month - 1], date, year)
}

/// "Sep 28", or "Dec 31, 2025" for another year than `now`'s.
fn date(day: i64, today: i64) -> String {
    let (year, month, date) = civil(day);
    let name = MONTHS[month - 1];
    if year == civil(today).0 {
        format!("{name} {date}")
    } else {
        format!("{name} {date}, {year}")
    }
}

/// When a record was copied, as its row says it: the local time for today
/// and yesterday ("14:02"), the weekday for the week before ("Thu"), else
/// the date ("Sep 28").
pub fn time_label(copied_at: u64, now: u64, offset_ms: i64) -> String {
    let (day, today) = (local_day(copied_at, offset_ms), local_day(now, offset_ms));
    match today - day {
        ..=1 => clock_time(copied_at, offset_ms),
        2..=6 => weekday(day)[..3].to_owned(),
        _ => date(day, today),
    }
}

/// When and where `record` was copied, in one line: "Copied today, 14:02
/// from notepad.exe", "Copied on Thursday, 09:00"; the source (its
/// program's file name) only when the system named it.
pub fn copied_line(record: &ClipboardRecord, now: u64, offset_ms: i64) -> String {
    let (day, today) = (
        local_day(record.copied_at, offset_ms),
        local_day(now, offset_ms),
    );
    let time = clock_time(record.copied_at, offset_ms);
    let when = match today - day {
        ..=0 => format!("today, {time}"),
        1 => format!("yesterday, {time}"),
        2..=6 => format!("on {}, {time}", weekday(day)),
        _ => format!("on {}, {time}", date(day, today)),
    };
    match &record.source {
        Some(source) => format!("Copied {when} from {}", program_file_name(source)),
        None => format!("Copied {when}"),
    }
}

/// The local time's offset from UTC, in milliseconds, as the system's time
/// zone settings give it; 0 where the system does not say. On Linux and
/// macOS it is the offset in force at `at` (milliseconds since the Unix
/// epoch); on Windows (`FileTimeToLocalFileTime`) the offset in force now,
/// so a day boundary near a daylight-saving change can fall an hour off for
/// items copied before it.
pub fn local_offset_ms(at: u64) -> i64 {
    platform_offset(at).unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn platform_offset(at: u64) -> Option<i64> {
    use ::windows::Win32::Foundation::FILETIME;
    use ::windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    // A FILETIME counts 100ns intervals since 1601-01-01.
    const FROM_1601_MS: u64 = 11_644_473_600_000;
    let ticks = at.checked_add(FROM_1601_MS)?.checked_mul(10_000)?;
    let utc = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = FILETIME::default();
    // SAFETY: both point to FILETIMEs that live for the call.
    unsafe { FileTimeToLocalFileTime(&utc, &mut local) }.ok()?;
    let local_ticks = (u64::from(local.dwHighDateTime) << 32) | u64::from(local.dwLowDateTime);
    let difference = i64::try_from(local_ticks).ok()? - i64::try_from(ticks).ok()?;
    Some(difference / 10_000)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn platform_offset(at: u64) -> Option<i64> {
    let seconds = libc::time_t::try_from(at / 1000).ok()?;
    // SAFETY: an all-zero `tm` is valid, and `localtime_r` writes only it.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to values that live for the call.
    if unsafe { libc::localtime_r(&seconds, &mut local) }.is_null() {
        return None;
    }
    // `tm_gmtoff` is a C long: 64 bits here, 32 on other targets.
    #[allow(clippy::unnecessary_cast)]
    let seconds_east = local.tm_gmtoff as i64;
    Some(seconds_east * 1000)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_offset(_at: u64) -> Option<i64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #192: while the package's code stopped, every reading shares one
    /// list of no records, so the window's watch, which compares the
    /// lists, does not redraw every second.
    #[test]
    fn readings_refused_share_their_records() {
        let first = Shown::refused("stopped".into());
        let second = Shown::refused("stopped".into());
        assert!(Arc::ptr_eq(&first.records, &second.records));
        assert!(first.records.is_empty());
        assert_eq!(second.unreadable.as_deref(), Some("stopped"));
    }

    /// #192: what was made of one store's history is not shared with a
    /// reading of another's at the same count of changes, and goes once
    /// the view closes.
    #[test]
    fn records_are_shared_only_for_the_store_they_were_made_from() {
        let mut projected = Projected::default();
        projected.keep(Projection {
            store: 1,
            owner: "own".into(),
            changes: 3,
            shown: Shown::unreadable("none".into()),
        });
        assert!(projected.shared(1, "own", 3).is_some());
        assert!(projected.shared(2, "own", 3).is_none());
        assert!(projected.shared(1, "other", 3).is_none());
        assert!(projected.shared(1, "own", 4).is_none());
        projected.forget();
        assert!(projected.shared(1, "own", 3).is_none());
    }

    #[test]
    fn links_and_colors_are_recognized_from_the_whole_text() {
        for link in [
            "https://example.com/a?b=c",
            "  http://localhost:8080  ",
            "ftp://files.example.org",
            "mailto:hello@example.com",
            "www.example.com",
            "vscode://file/c:/a.txt",
        ] {
            assert_eq!(ClipboardKind::of_text(link), ClipboardKind::Link, "{link}");
        }
        for color in [
            "#fff",
            "#FF8800",
            "#ff880080",
            "rgb(255, 136, 0)",
            "rgba(255 136 0 / 50%)",
            "hsl(30deg 100% 50%)",
            " HSLA(30, 100%, 50%, 0.5) ",
        ] {
            assert_eq!(
                ClipboardKind::of_text(color),
                ClipboardKind::Color,
                "{color}"
            );
        }
        for text in [
            "see https://example.com",
            "https://",
            "www.",
            "#ggg",
            "#ff88",
            "#12345",
            "rgb(1, 2)",
            "rgb(a, b, c)",
            "hello",
            "mailto:nobody",
            "",
        ] {
            assert_eq!(ClipboardKind::of_text(text), ClipboardKind::Text, "{text}");
        }
    }

    #[test]
    fn a_source_is_named_by_its_programs_file_name() {
        let mut record = ClipboardRecord::text("1", "x", 0);
        assert_eq!(record.source_name(), None);
        assert_eq!(record.source_path(), None);
        record.source = Some("notepad.exe".into());
        assert_eq!(record.source_name().as_deref(), Some("notepad"));
        assert_eq!(record.source_path(), None, "a file name alone is no path");
        let path = if cfg!(windows) {
            r"C:\Program Files\Microsoft VS Code\Code.exe"
        } else {
            "/usr/share/code/code"
        };
        record.source = Some(path.into());
        let name = if cfg!(windows) { "Code" } else { "code" };
        assert_eq!(record.source_name().as_deref(), Some(name));
        assert_eq!(record.source_path(), Some(PathBuf::from(path)));
        assert!(record.holds(&name.to_lowercase()));
        assert!(
            !record.holds("program files"),
            "only the file name is searched"
        );
    }

    /// #167: an image is titled and measured by its size, files by the
    /// first one's name and how many more; neither counts characters, and
    /// a search finds them by their title or paths.
    #[test]
    fn images_and_files_are_titled_by_size_and_first_name() {
        let image = ClipboardRecord::image(
            "1",
            ClipboardImage {
                path: PathBuf::from("a.png"),
                width: 1920,
                height: 1080,
            },
            0,
        );
        assert_eq!(image.kind, ClipboardKind::Image);
        assert_eq!(image.title(), "Image (1920×1080)");
        let info = information(&image, 0, 0);
        assert_eq!(info.kind, "Image");
        assert_eq!(info.dimensions.as_deref(), Some("1920×1080"));
        assert_eq!(info.characters, None);
        assert!(image.holds("image"));

        let one = ClipboardRecord::files("2", vec![PathBuf::from("/notes/report.pdf")], 0);
        assert_eq!(one.kind, ClipboardKind::Files);
        assert_eq!(one.title(), "report.pdf");
        let three = ClipboardRecord::files(
            "3",
            vec![
                PathBuf::from("/notes/report.pdf"),
                PathBuf::from("/notes/b.txt"),
                PathBuf::from("/notes/photos"),
            ],
            0,
        );
        assert_eq!(three.title(), "report.pdf +2");
        assert!(three.holds("photos"));
        let info = information(&three, 0, 0);
        assert_eq!(
            (info.kind, info.characters, info.dimensions),
            ("File", None, None)
        );
        assert_eq!(file_name(Path::new("/")), "/");

        // Paste puts text and one file on the clipboard; an image or
        // several files are copied instead.
        assert_eq!(
            pasted_clip(&one),
            Some(crate::system::Clip::File(PathBuf::from(
                "/notes/report.pdf"
            )))
        );
        assert_eq!(pasted_clip(&three), None);
        assert_eq!(pasted_clip(&image), None);
        assert_eq!(
            pasted_clip(&ClipboardRecord::text("4", "hi", 0)),
            Some(crate::system::Clip::Text("hi".into()))
        );
    }
}
