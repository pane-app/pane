//! The log file in the logs folder: `pane.log`, appended to across
//! starts, each line a UTC time and a message redacted before it is
//! written (see `redact`). Past [`MAX_SIZE`] it becomes `pane.1.log`, the
//! older files shift, and at most [`KEPT`] older files are kept, so the
//! log never holds much more than 12 MiB. One site (a message's fixed
//! text, before its values) writes at most [`LINES_PER_WINDOW`] lines in a
//! [`WINDOW`]; what it says beyond that is held back, and one line says
//! how many similar lines were left out, so a loop of errors cannot fill
//! the disk.
//!
//! Writing never panics and never fails the caller: a file that cannot be
//! opened or written only loses lines, and one that cannot be moved to
//! rotate it keeps its lines, and the older files, until it can.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::redact::Redactor;

/// The log's file in the logs folder.
pub(crate) const FILE: &str = "pane.log";

/// Past this size the log is rotated: 2 MiB.
pub(crate) const MAX_SIZE: u64 = 2 * 1024 * 1024;

/// How many older files rotation keeps, `pane.1.log` (the newest) to
/// `pane.5.log`.
pub(crate) const KEPT: usize = 5;

/// How many lines one site may write in a [`WINDOW`].
pub(crate) const LINES_PER_WINDOW: u32 = 10;

/// The rate limit's window: a minute.
pub(crate) const WINDOW: Duration = Duration::from_secs(60);

/// How many sites the rate limit remembers before it forgets those whose
/// window has passed with nothing held back.
const SITES_KEPT: usize = 512;

/// What tells the log the time: the system's clock, or a test's.
pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// The log of one logs folder.
pub(crate) struct Log {
    folder: PathBuf,
    redactor: Redactor,
    clock: Clock,
    /// Past this size the log is rotated ([`MAX_SIZE`]; a test's own).
    max_size: u64,
    inner: Mutex<Inner>,
}

struct Inner {
    /// `pane.log`, opened for appending; `None` when it could not be.
    file: Option<File>,
    /// Its size as last known.
    size: u64,
    /// The size past which it is rotated next: the log's cap, or, after a
    /// rotation that could not move `pane.log`, its size then plus the cap.
    rotate_at: u64,
    /// What each site wrote in its current window, by its fixed text.
    sites: HashMap<String, Site>,
}

/// One site's current window of the rate limit.
struct Site {
    since: SystemTime,
    written: u32,
    left_out: u32,
}

impl Log {
    /// The log in `folder` (which exists), telling the time by the
    /// system's clock and redacting with `redactor`.
    pub(crate) fn open(folder: &Path, redactor: Redactor) -> Log {
        Log::open_with(folder, redactor, Arc::new(SystemTime::now), MAX_SIZE)
    }

    /// The log in `folder`, telling the time by `clock` and rotated past
    /// `max_size`.
    pub(crate) fn open_with(folder: &Path, redactor: Redactor, clock: Clock, max_size: u64) -> Log {
        let path = folder.join(FILE);
        let file = open_file(&path).ok();
        let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Log {
            folder: folder.to_path_buf(),
            redactor,
            clock,
            max_size,
            inner: Mutex::new(Inner {
                file,
                size,
                rotate_at: max_size,
                sites: HashMap::new(),
            }),
        }
    }

    /// Writes the line each run starts with: Pane's version and its
    /// process.
    pub(crate) fn begin_run(&self, version: &str) {
        let now = (self.clock)();
        let mut inner = self.lock();
        self.append(
            &mut inner,
            now,
            &format!("Pane {version} started (process {})", std::process::id()),
        );
    }

    /// Writes `message`, said at `site` (its fixed text), unless the site
    /// already wrote its [`LINES_PER_WINDOW`] lines in this window; the
    /// first line of a site's next window is preceded by one saying how
    /// many were left out.
    pub(crate) fn write(&self, site: &str, message: &str) {
        let now = (self.clock)();
        let mut inner = self.lock();
        if inner.sites.len() > SITES_KEPT {
            inner
                .sites
                .retain(|_, kept| kept.left_out > 0 || within_window(kept.since, now));
        }
        let fresh = Site {
            since: now,
            written: 0,
            left_out: 0,
        };
        let kept = inner.sites.entry(site.to_owned()).or_insert(fresh);
        let mut summary = None;
        if !within_window(kept.since, now) {
            if kept.left_out > 0 {
                summary = Some(left_out(kept.left_out, site));
            }
            *kept = Site {
                since: now,
                written: 0,
                left_out: 0,
            };
        }
        let allowed = kept.written < LINES_PER_WINDOW;
        if allowed {
            kept.written += 1;
        } else {
            kept.left_out = kept.left_out.saturating_add(1);
        }
        if let Some(summary) = summary {
            self.append(&mut inner, now, &summary);
        }
        if allowed {
            self.append(&mut inner, now, message);
        }
    }

    /// Writes, for each site that held lines back in its current window,
    /// how many it left out: what a clean quit says before Pane ends.
    pub(crate) fn finish(&self) {
        let now = (self.clock)();
        let mut inner = self.lock();
        let mut held: Vec<(String, u32)> = inner
            .sites
            .iter_mut()
            .filter(|(_, kept)| kept.left_out > 0)
            .map(|(site, kept)| (site.clone(), std::mem::take(&mut kept.left_out)))
            .collect();
        held.sort();
        for (site, count) in held {
            self.append(&mut inner, now, &left_out(count, &site));
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Appends one line, `message` redacted after the time, rotating the
    /// file first if it passed its size.
    fn append(&self, inner: &mut Inner, now: SystemTime, message: &str) {
        let text = self.redactor.redact(message.trim_end());
        // A message of several lines (a panic's backtrace) keeps them,
        // indented under its time.
        let line = format!("{} {}\n", utc(now), text.replace('\n', "\n  "));
        if inner.size >= inner.rotate_at {
            self.rotate(inner);
        }
        let Some(file) = inner.file.as_mut() else {
            return;
        };
        if file.write_all(line.as_bytes()).is_ok() {
            inner.size = inner.size.saturating_add(line.len() as u64);
        }
    }

    /// Makes `pane.log` the newest older file, shifting the others and
    /// dropping the oldest past [`KEPT`], and starts a new `pane.log`.
    ///
    /// `pane.log` is moved aside first. If it cannot be (on Windows another
    /// program may hold it open without allowing that), no older file is
    /// shifted or dropped: Pane keeps appending to it and tries again once
    /// it has grown by another cap, so a log that cannot move never costs
    /// the older files, nor a rotation for every line.
    fn rotate(&self, inner: &mut Inner) {
        // Closed first: Windows renames an open file only if it was opened
        // to allow it.
        inner.file = None;
        let path = self.folder.join(FILE);
        let aside = self.folder.join(ROTATING);
        let mut moved = fs::rename(&path, &aside).is_ok();
        if moved {
            let _ = fs::remove_file(self.folder.join(older(KEPT)));
            for number in (1..KEPT).rev() {
                let _ = fs::rename(
                    self.folder.join(older(number)),
                    self.folder.join(older(number + 1)),
                );
            }
            // If it cannot become `pane.1.log`, it goes back to be appended
            // to (and if that fails too, a new `pane.log` starts).
            if fs::rename(&aside, self.folder.join(older(1))).is_err()
                && fs::rename(&aside, &path).is_ok()
            {
                moved = false;
            }
        }
        inner.file = open_file(&path).ok();
        inner.size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        inner.rotate_at = if moved {
            self.max_size
        } else {
            inner.size.saturating_add(self.max_size)
        };
    }
}

/// Where `pane.log` is moved while the older files shift.
const ROTATING: &str = "pane.rotating.log";

/// The name of the `number`th older file: `pane.1.log` is the newest.
pub(crate) fn older(number: usize) -> String {
    format!("pane.{number}.log")
}

/// The line saying how many lines `site` left out.
fn left_out(count: u32, site: &str) -> String {
    let lines = match count {
        1 => "line was",
        _ => "lines were",
    };
    format!("{count} similar {lines} left out: {site}")
}

/// Whether `now` is still in the window that began at `since`. A clock
/// that went back counts as the same window.
fn within_window(since: SystemTime, now: SystemTime) -> bool {
    match now.duration_since(since) {
        Ok(elapsed) => elapsed < WINDOW,
        Err(_) => true,
    }
}

/// Opens `path` for appending, creating it readable by the user only.
fn open_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// `time` in UTC, to the millisecond: `2026-10-08T05:06:07.089Z`.
pub(crate) fn utc(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let seconds = since.as_secs();
    let (year, month, day) = civil((seconds / 86_400) as i64);
    let of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        since.subsec_millis()
    )
}

/// The year, month and day of day `days` since 1970-01-01, in the
/// proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock a test moves by hand.
    fn clock_at(start: SystemTime) -> (Clock, Arc<Mutex<SystemTime>>) {
        let now = Arc::new(Mutex::new(start));
        let read = now.clone();
        let clock: Clock = Arc::new(move || *read.lock().unwrap());
        (clock, now)
    }

    fn start() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_791_417_600)
    }

    fn lines(folder: &Path, file: &str) -> Vec<String> {
        fs::read_to_string(folder.join(file))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn times_are_utc_to_the_millisecond() {
        let time = start() + Duration::from_millis(((5 * 60 + 6) * 60 + 7) * 1000 + 89);
        assert_eq!(utc(time), "2026-10-08T05:06:07.089Z");
        assert_eq!(utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            utc(UNIX_EPOCH + Duration::from_secs(1_709_251_199)),
            "2024-02-29T23:59:59.000Z"
        );
        assert_eq!(
            utc(UNIX_EPOCH + Duration::from_secs(951_868_800)),
            "2000-03-01T00:00:00.000Z"
        );
    }

    #[test]
    fn each_line_has_its_time_and_a_run_starts_with_the_version() {
        let folder = tempfile::tempdir().unwrap();
        let (clock, _) = clock_at(start());
        let log = Log::open_with(folder.path(), Redactor::none(), clock, MAX_SIZE);
        log.begin_run("1.2.3");
        log.write("site", "Pane could not do it:\nfirst\nsecond");
        let lines = lines(folder.path(), FILE);
        assert_eq!(
            lines,
            [
                format!(
                    "2026-10-08T00:00:00.000Z Pane 1.2.3 started (process {})",
                    std::process::id()
                ),
                "2026-10-08T00:00:00.000Z Pane could not do it:".to_owned(),
                "  first".to_owned(),
                "  second".to_owned(),
            ]
        );
    }

    #[test]
    fn the_log_rotates_at_its_size_and_keeps_at_most_five_older_files() {
        let folder = tempfile::tempdir().unwrap();
        let (clock, _) = clock_at(start());
        // A small cap stands for 2 MiB: each line here is 64 bytes or so.
        let log = Log::open_with(folder.path(), Redactor::none(), clock, 200);
        for number in 0..60 {
            // A site per line, so the rate limit holds nothing back.
            log.write(
                &format!("site {number}"),
                &format!("line {number:03} of the rotation test, padded out"),
            );
        }
        let mut names: Vec<String> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "pane.1.log",
                "pane.2.log",
                "pane.3.log",
                "pane.4.log",
                "pane.5.log",
                "pane.log"
            ]
        );
        // No file grew much past the cap: each was rotated once it passed
        // it, by at most one line.
        for name in &names {
            let size = fs::metadata(folder.path().join(name)).unwrap().len();
            assert!(size < 200 + 80, "{name} is {size} bytes");
        }
        // The newest line is in pane.log, the ones before it in pane.1.log,
        // and the oldest lines are gone.
        let current = lines(folder.path(), FILE);
        assert!(current.last().unwrap().contains("line 059"));
        let newest_older = lines(folder.path(), &older(1));
        let current_first = &current[0];
        assert_eq!(
            line_number(newest_older.last().unwrap()) + 1,
            line_number(current_first),
            "{newest_older:?} then {current_first}"
        );
        let all: Vec<String> = names
            .iter()
            .flat_map(|name| lines(folder.path(), name))
            .collect();
        assert!(!all.iter().any(|line| line.contains("line 000")));

        // A new run over the same folder appends, and rotates the same way.
        drop(log);
        let (clock, _) = clock_at(start());
        let log = Log::open_with(folder.path(), Redactor::none(), clock, 200);
        for number in 60..80 {
            log.write(
                &format!("site {number}"),
                &format!("line {number:03} of the rotation test, padded out"),
            );
        }
        let count = fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(count, 1 + KEPT, "never more than five older files");
    }

    /// A `pane.log` that cannot be moved (on Windows, another program
    /// holding it open) costs no older file, and is not tried again for
    /// every line.
    #[test]
    fn a_log_that_cannot_move_keeps_the_older_files() {
        let folder = tempfile::tempdir().unwrap();
        for number in 1..=KEPT {
            fs::write(
                folder.path().join(older(number)),
                format!("older {number}\n"),
            )
            .unwrap();
        }
        // A folder where `pane.log` would be moved makes the move fail on
        // every system.
        let blocker = folder.path().join(ROTATING);
        fs::create_dir(&blocker).unwrap();
        fs::write(blocker.join("held"), "held").unwrap();
        let (clock, _) = clock_at(start());
        let log = Log::open_with(folder.path(), Redactor::none(), clock, 200);
        for number in 0..20 {
            log.write(
                &format!("site {number}"),
                &format!("line {number:03} of the rotation test, padded out"),
            );
        }
        for number in 1..=KEPT {
            assert_eq!(
                fs::read_to_string(folder.path().join(older(number))).unwrap(),
                format!("older {number}\n"),
                "pane.{number}.log is untouched"
            );
        }
        let current = lines(folder.path(), FILE);
        assert_eq!(current.len(), 20, "every line is kept in pane.log");

        // Once it can move, it rotates as before: four lines (some 270
        // bytes) pass the next try, and the lines it held become the newest
        // older file, just before the one that was `pane.1.log`.
        fs::remove_dir_all(&blocker).unwrap();
        for number in 20..24 {
            log.write(
                &format!("site {number}"),
                &format!("line {number:03} of the rotation test, padded out"),
            );
        }
        let older_files: Vec<String> = (1..=KEPT)
            .map(|number| fs::read_to_string(folder.path().join(older(number))).unwrap())
            .collect();
        let held = older_files
            .iter()
            .position(|text| text.contains("line 000"))
            .unwrap();
        assert_eq!(older_files[held + 1], "older 1\n", "{older_files:#?}");
        assert!(!folder.path().join(ROTATING).exists());
    }

    /// The number in a line the rotation test wrote.
    fn line_number(line: &str) -> u32 {
        let at = line.find("line ").unwrap() + "line ".len();
        line[at..at + 3].parse().unwrap()
    }

    #[test]
    fn an_eleventh_line_from_one_site_in_a_minute_is_held_back_and_counted() {
        let folder = tempfile::tempdir().unwrap();
        let (clock, now) = clock_at(start());
        let log = Log::open_with(folder.path(), Redactor::none(), clock, MAX_SIZE);
        let site = "Pane could not save {what}: {error}";
        for number in 0..15 {
            log.write(site, &format!("Pane could not save item {number}: no room"));
            // Another site is not held back by this one.
            if number == 12 {
                log.write("other", "another message");
            }
        }
        let written = lines(folder.path(), FILE);
        let saved = written
            .iter()
            .filter(|line| line.contains("Pane could not save item"))
            .count();
        assert_eq!(saved, 10, "{written:#?}");
        assert!(written.join("\n").contains("another message"));
        assert!(!written.iter().any(|line| line.contains("item 10")));

        // Within the minute: still held back.
        *now.lock().unwrap() = start() + Duration::from_secs(59);
        log.write(site, "Pane could not save item 15: no room");
        assert_eq!(lines(folder.path(), FILE).len(), written.len());

        // The next minute: one line says how many were left out, then the
        // site writes again.
        *now.lock().unwrap() = start() + Duration::from_secs(61);
        log.write(site, "Pane could not save item 16: no room");
        let written = lines(folder.path(), FILE);
        let tail = &written[written.len() - 2..];
        assert!(
            tail[0].ends_with(&format!("6 similar lines were left out: {site}")),
            "{tail:?}"
        );
        assert!(tail[1].ends_with("Pane could not save item 16: no room"));
    }

    #[test]
    fn a_clean_finish_says_what_was_held_back() {
        let folder = tempfile::tempdir().unwrap();
        let (clock, _) = clock_at(start());
        let log = Log::open_with(folder.path(), Redactor::none(), clock, MAX_SIZE);
        for _ in 0..11 {
            log.write("loop", "the same failure");
        }
        log.finish();
        let written = lines(folder.path(), FILE);
        assert!(
            written
                .last()
                .unwrap()
                .ends_with("1 similar line was left out: loop"),
            "{written:?}"
        );
        // Said once.
        log.finish();
        assert_eq!(lines(folder.path(), FILE).len(), written.len());
    }

    #[test]
    fn what_is_written_is_redacted_first() {
        let folder = tempfile::tempdir().unwrap();
        let (clock, _) = clock_at(start());
        let redactor = Redactor::new(
            Some(Path::new("/home/alice")),
            &["alice".to_owned()],
            &["alice-laptop".to_owned()],
        );
        let log = Log::open_with(folder.path(), redactor, clock, MAX_SIZE);
        log.write(
            "site",
            "Pane could not read /home/alice/notes on ALICE-LAPTOP for Alice",
        );
        let written = lines(folder.path(), FILE).join("\n");
        assert!(
            written.ends_with("Pane could not read ~/notes on <computer> for <user>"),
            "{written}"
        );
        assert!(!written.to_lowercase().contains("alice"));
    }
}
