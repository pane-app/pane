//! The calculator's date and time words: "now", "time", "today", "date",
//! "tomorrow" and "yesterday" answer at once with the local date or time
//! in the layout Pane's own dates use ("Jun 1, 2025", "14:23"), so a
//! query about the moment needs no unit, no arithmetic and no time zone.
//!
//! The moment is Pane's: the clock root search's dates are shown by,
//! passed with the query, and the local time's offset from UTC with it —
//! so the answer is the local one, and a test can hold the clock still.
//! The words are lowercase, as the calculator's language is; a moment
//! after the query was asked is the answer's, not the guest's, reading
//! of the clock.
//!
//! The answer copies as ISO 8601 (a date for a date word, the whole
//! moment otherwise, in the local zone) and as a Unix timestamp (the
//! moment's seconds, a date word's at its local midnight).

use pane_extension::alloc::format;
use pane_extension::alloc::string::String;

use pane_extension::root::WallTime;

/// A day in milliseconds.
const DAY: i64 = 86_400_000;

/// The months' short names, as Pane's dates show them.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The word a query is, if it is one of the date and time words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Word {
    /// The local date and time.
    Now,
    /// The local time.
    Time,
    /// The local date.
    Today,
    /// The day after.
    Tomorrow,
    /// The day before.
    Yesterday,
}

/// The word `query` is, if it is one of the date and time words.
pub(super) fn word(query: &str) -> Option<Word> {
    Some(match query {
        "now" => Word::Now,
        "time" => Word::Time,
        "today" | "date" => Word::Today,
        "tomorrow" => Word::Tomorrow,
        "yesterday" => Word::Yesterday,
        _ => return None,
    })
}

/// What a word answers: what is shown, and what is copied as ISO 8601 and
/// as a Unix timestamp.
pub(super) struct Answer {
    pub(super) shown: String,
    pub(super) iso: String,
    pub(super) unix: String,
}

/// What `word` answers at `at`.
pub(super) fn answer(word: Word, at: &WallTime) -> Answer {
    // The local time, as milliseconds: the epoch plus the offset, in 128
    // bits so no clock value can overflow the day arithmetic.
    let local = i128::from(at.milliseconds) + i128::from(at.offset);
    let day = i64::try_from(local.div_euclid(i128::from(DAY))).unwrap_or(0);
    let time = i64::try_from(local.rem_euclid(i128::from(DAY))).unwrap_or(0);
    match word {
        Word::Now => Answer {
            shown: format!("{}, {}", date_of(day), clock(time)),
            iso: format!("{}T{}{}", iso_date(day), stamp(time), zone(at.offset)),
            unix: unix(at.milliseconds / 1000),
        },
        Word::Time => Answer {
            shown: clock(time),
            iso: format!("{}T{}{}", iso_date(day), stamp(time), zone(at.offset)),
            unix: unix(at.milliseconds / 1000),
        },
        // A date word answers the day, offset from today; its timestamp
        // is the day's local midnight.
        Word::Today => dated(day, at.offset),
        Word::Tomorrow => dated(day + 1, at.offset),
        Word::Yesterday => dated(day - 1, at.offset),
    }
}

/// The day `day` answers with: its date, its ISO 8601 date and its local
/// midnight as a Unix timestamp.
fn dated(day: i64, offset: i64) -> Answer {
    Answer {
        shown: date_of(day),
        iso: iso_date(day),
        unix: unix(u64::try_from((day * DAY - offset) / 1000).unwrap_or(0)),
    }
}

/// The date of the day `day` days since 1970-01-01, "Jun 1, 2025".
fn date_of(day: i64) -> String {
    let (year, month, date) = civil(day);
    format!("{} {date}, {year}", MONTHS[month - 1])
}

/// The same date as ISO 8601 writes it, "2025-06-01".
fn iso_date(day: i64) -> String {
    let (year, month, date) = civil(day);
    format!("{year:04}-{month:02}-{date:02}")
}

/// The time of day of `time` (milliseconds since local midnight),
/// "14:23".
fn clock(time: i64) -> String {
    let minutes = time / 60_000;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The time of day of `time` (milliseconds since local midnight) with
/// seconds, "14:23:45".
fn stamp(time: i64) -> String {
    let seconds = time / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The zone of an offset from UTC in milliseconds, as ISO 8601 writes it:
/// "Z" for UTC, "+05:30" otherwise (an offset's seconds, if it has any,
/// are not shown).
fn zone(offset: i64) -> String {
    if offset == 0 {
        return "Z".into();
    }
    let minutes = offset / 60_000;
    let (sign, minutes) = if minutes < 0 {
        ("-", -minutes)
    } else {
        ("+", minutes)
    };
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `seconds` since the Unix epoch, as a timestamp is written.
fn unix(seconds: u64) -> String {
    format!("{seconds}")
}

/// The (year, month 1-12, day) of the day `day` days since 1970-01-01, by
/// the proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`).
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
