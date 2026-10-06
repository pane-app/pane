//! The Clipboard History split view's projection (#102): a narrow,
//! read-only presentation of the clipboard history Pane keeps for its own
//! registered Clipboard History default extension, for the window to draw
//! as a list beside a preview.
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
//! What the window does with the records — which a query and a filter
//! keep, how they group by local day, which one is selected — is the
//! adapter's state over plain functions here ([`ClipboardBrowse`],
//! [`day_of`], [`time_label`], [`copied_line`]), so the rules are the same
//! wherever they are drawn.
//!
//! The operations are the history's existing ones — copy a record again,
//! delete it, turn capture on, pause or resume it — and each revalidates
//! the [reading](ClipboardHistoryView) it was made from first: the same
//! screen, the same verified command, the same generation of the package's
//! code, and a record still kept. A reading the user left, of a package
//! disabled, replaced or uninstalled since, or of a record deleted or
//! expired since, changes nothing it should not. The rest of the history's
//! management (retention, exclusions, clearing, turning off and deleting)
//! stays the command's own list, which the window routes to.

use std::fmt;

use super::{Launcher, Screen, State, Status, owner};
use crate::clipboard::history::PackageHistory;
use crate::clipboard::{self, CaptureState, Commands};
use crate::extension_data::PackageData;
use crate::packages::PackageIdentity;

/// The id of Pane's Clipboard History default extension, and of its one
/// command in its manifest.
pub const CLIPBOARD_HISTORY: &str = "clipboard-history";

/// One kept text, as the split view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardRecord {
    /// Opaque: identifies the record among its package's, for its
    /// operations; never reused.
    pub id: String,
    /// The full stored text.
    pub text: String,
    /// When it was copied, in milliseconds since the Unix epoch.
    pub copied_at: u64,
    /// The program it was copied from, as the system named it (such as
    /// `notepad.exe`), if it did.
    pub source: Option<String>,
}

impl ClipboardRecord {
    /// The record's title: its first line with text, trimmed; empty for
    /// text that is all white space.
    pub fn title(&self) -> &str {
        self.text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("")
    }

    /// Whether `needle` (lowercased, trimmed, not empty) is in the record's
    /// text or its source.
    fn holds(&self, needle: &str) -> bool {
        self.text.to_lowercase().contains(needle)
            || self
                .source
                .as_ref()
                .is_some_and(|source| source.to_lowercase().contains(needle))
    }
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
    /// The kept records, newest first, none expired.
    pub records: Vec<ClipboardRecord>,
    /// Why the history cannot be read now, if it cannot; no records are
    /// listed then.
    pub unreadable: Option<String>,
    /// Now, by the clock the history expires by, in milliseconds since the
    /// Unix epoch.
    pub now: u64,
    /// Whether what is copied is kept: the actual capture state.
    pub capture: CaptureState,
    /// Why Pane does not watch the clipboard although it should, or cannot
    /// on this system, if so.
    pub problem: Option<String>,
    /// How long each record is kept after it was copied, in seconds.
    pub retention_seconds: u64,
    /// How many programs are excluded.
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

impl ClipboardHistoryView {
    /// The record with `id`, if this reading lists it.
    pub fn record(&self, id: &str) -> Option<&ClipboardRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    /// One line on what is kept now, as it actually is (see
    /// [`capture_summary`]).
    pub fn summary(&self) -> String {
        capture_summary(
            self.capture,
            self.problem.as_deref(),
            self.retention_seconds,
            self.excluded,
        )
    }
}

/// One line on what clipboard history keeps: `problem`, if there is one;
/// otherwise whether history is off, paused or on, and on for how long
/// (`retention_seconds`) and with how many programs `excluded`. It
/// promises nothing the history does not do: copies an application marks
/// as not to be kept are skipped, and excluded programs are counted.
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
        CaptureState::Off => "Off · nothing you copy is kept until you turn it on".into(),
        CaptureState::Paused => "Paused · nothing you copy is kept until you resume".into(),
        CaptureState::On => {
            let kept = format!(
                "Text is kept {} · copies marked private are skipped",
                span(retention_seconds)
            );
            match excluded {
                0 => kept,
                1 => format!("{kept} · 1 program excluded"),
                excluded => format!("{kept} · {excluded} programs excluded"),
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

impl Launcher {
    /// The clipboard history the split view shows, while Pane's registered
    /// Clipboard History command is open on its own list and its package
    /// runs; `None` on every other screen and for every other command.
    /// Read-only: see the module documentation.
    pub fn clipboard_history(&self) -> Option<ClipboardHistoryView> {
        let state = self.lock();
        let (identity, data) = self.verified_clipboard(&state)?;
        let epoch = state.screen_epoch;
        let title = state.view.title.clone();
        drop(state);
        let (problem, copy_unavailable) = match &self.clipboard {
            Some(capture) => (capture.problem(), capture.system().unavailable()),
            None => {
                let unavailable = clipboard::none().unavailable();
                (unavailable.clone(), unavailable)
            }
        };
        let (history, now, unreadable) = match data.clipboard_history() {
            Ok(store) => match store.get(data.owner()) {
                Ok(history) => (history, store.now(), None),
                Err(reason) => (PackageHistory::default(), store.now(), Some(reason)),
            },
            Err(refusal) => (PackageHistory::default(), 0, Some(refusal)),
        };
        let records = history
            .items
            .iter()
            .map(|item| ClipboardRecord {
                id: item.id.to_string(),
                text: item.text.clone(),
                copied_at: item.copied_at,
                source: item.source.clone(),
            })
            .collect();
        Some(ClipboardHistoryView {
            owner: identity,
            title,
            records,
            unreadable,
            now,
            capture: history.capture,
            problem,
            retention_seconds: history.retention(),
            excluded: history.excluded.len(),
            copy_unavailable,
            reading: Reading { epoch, data },
        })
    }

    /// Puts the record `id` of `view` on the clipboard again, through the
    /// history's existing copy, once `view` is revalidated (see the module
    /// documentation). The outcome shows as the status; `Err` says why
    /// nothing was copied.
    pub fn copy_clipboard_record(
        &self,
        view: &ClipboardHistoryView,
        id: &str,
    ) -> Result<(), String> {
        self.clipboard_operation(view, |commands| {
            commands
                .copy(id)
                .map(|()| "Copied to the clipboard".to_owned())
        })
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

    /// Turns capture on, pauses or resumes it, or turns it off — the
    /// history's existing capture choice — once `view` is revalidated.
    /// Turning it on where this Pane cannot watch the clipboard is refused
    /// with the reason. The outcome shows as the status.
    pub fn set_clipboard_capture(
        &self,
        view: &ClipboardHistoryView,
        capture: CaptureState,
    ) -> Result<(), String> {
        let done = match (capture, view.capture) {
            (CaptureState::On, CaptureState::Paused) => "Clipboard history is on again",
            (CaptureState::On, _) => "Clipboard history is on",
            (CaptureState::Paused, _) => "Clipboard history is paused",
            (CaptureState::Off, _) => "Clipboard history is off",
        };
        self.clipboard_operation(view, |commands| {
            commands.set_capture(capture).map(|()| done.to_owned())
        })
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

/// Which records the split view's tabs keep. Pane keeps text alone, so
/// both keep every record; the reference's links, images and colors are
/// not kinds Pane keeps or guesses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipboardFilter {
    #[default]
    All,
    Text,
}

impl ClipboardFilter {
    /// The filters, in the tabs' order.
    pub const ALL: [ClipboardFilter; 2] = [ClipboardFilter::All, ClipboardFilter::Text];

    /// The tab's label.
    pub fn label(self) -> &'static str {
        match self {
            ClipboardFilter::All => "All",
            ClipboardFilter::Text => "Text",
        }
    }

    /// Whether the filter keeps `record`: every kept record is text.
    fn keeps(self, _record: &ClipboardRecord) -> bool {
        match self {
            ClipboardFilter::All | ClipboardFilter::Text => true,
        }
    }
}

/// The local day a record was copied, relative to today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardDay {
    Today,
    Yesterday,
    Older,
}

impl ClipboardDay {
    /// The day's section label.
    pub fn label(self) -> &'static str {
        match self {
            ClipboardDay::Today => "Today",
            ClipboardDay::Yesterday => "Yesterday",
            ClipboardDay::Older => "Older",
        }
    }
}

/// A run of listed records copied on one local day: from `first` up to
/// the next section's `first` (or the end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardSection {
    pub day: ClipboardDay,
    /// The index of the run's first record in the listing.
    pub first: usize,
}

/// The split view's own state over the records: the query typed, the tab
/// chosen and the record chosen (by id). The window adapter owns it; its
/// listing follows the rules below whatever the records are now.
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
    /// copies.
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
        let mut sections: Vec<ClipboardSection> = Vec::new();
        for (index, record) in listed.iter().enumerate() {
            let day = day_of(record.copied_at, now, offset_ms);
            if sections.last().is_none_or(|section| section.day != day) {
                sections.push(ClipboardSection { day, first: index });
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
    match local_day(now, offset_ms) - local_day(copied_at, offset_ms) {
        ..=0 => ClipboardDay::Today,
        1 => ClipboardDay::Yesterday,
        _ => ClipboardDay::Older,
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

/// When and where `record` was copied, as the split view's footer says
/// it: "Copied today, 14:02 from notepad.exe", "Copied on Thursday, 09:00";
/// the source only when the system named it.
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
        Some(source) => format!("Copied {when} from {source}"),
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
