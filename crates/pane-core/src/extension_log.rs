//! Each installed package's extension log: what its code writes to standard
//! output and standard error, as lines, with Pane's own messages about the
//! package (its crashes, unresponsive calls, pauses and development builds)
//! interleaved.
//!
//! The runtime hands every guest instance a WASI context whose output goes
//! to [`ExtensionLogs::output`] instead of nowhere. A line may start with
//! an sd-daemon level prefix (`<3>` error, `<4>` warning, `<6>` info, `<7>`
//! debug), which the SDKs write and which is taken off; an untagged line is
//! info on standard output and an error on standard error.
//!
//! Logging is never a hazard for Pane or the disk: a line is cut at
//! [`LINE_LIMIT`], a package writing more than [`LINES_PER_SECOND`] lines
//! loses the rest of that second (Pane notes how many), and writing never
//! blocks the guest. A package not being developed keeps only its most
//! recent lines ([`WINDOW_LINES`], [`WINDOW_BYTES`]), in memory, for
//! diagnostics: nothing is written to disk or sent anywhere, and the lines
//! go when Pane quits. While a package is developed, it keeps more
//! ([`DEVELOPED_LINES`], [`DEVELOPED_BYTES`]), appends every line to a log
//! file of the development session (rotated at [`FILE_LIMIT`], keeping one
//! earlier file) and sends each to whoever follows it.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::source_map::SourceMap;

/// The longest line kept, in bytes: the rest is cut, with a note of how
/// much.
pub const LINE_LIMIT: usize = 4 * 1024;
/// The longest message of Pane's own kept, in bytes: a crash's backtrace
/// may be long.
const PANE_LINE_LIMIT: usize = 16 * 1024;
/// How many lines a package may write in a second; the rest of that second
/// is dropped.
pub const LINES_PER_SECOND: u32 = 1_000;
/// The lines kept for a package not being developed.
pub const WINDOW_LINES: usize = 500;
pub const WINDOW_BYTES: usize = 256 * 1024;
/// The lines kept in memory for a package being developed; its log file
/// has every line.
pub const DEVELOPED_LINES: usize = 5_000;
pub const DEVELOPED_BYTES: usize = 2 * 1024 * 1024;
/// The size at which a development log file is rotated: it becomes
/// `<name>.1`, replacing an earlier one, and a new file is started.
pub const FILE_LIMIT: u64 = 5 * 1024 * 1024;

/// Who wrote a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogSource {
    /// The extension's code, on its standard output or standard error.
    Extension(LogStream),
    /// Pane, about the extension.
    Pane,
}

/// Which of the extension's outputs a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Its name, as the log file writes it.
    pub fn name(self) -> &'static str {
        match self {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// One line of an extension log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub time: SystemTime,
    pub source: LogSource,
    pub level: LogLevel,
    /// The id in `pane.json` of the command the package's code was running
    /// for, when Pane knows it.
    pub command: Option<String>,
    /// The generation of the package's code that wrote it, or that Pane's
    /// message is about: a reload, update or pause starts a new one.
    pub generation: u64,
    pub text: String,
}

impl LogLine {
    /// The line as the log file has it: UTC time, level, who wrote it, the
    /// command, and the text.
    pub fn file_line(&self) -> String {
        let who = match self.source {
            LogSource::Extension(LogStream::Stdout) => "stdout",
            LogSource::Extension(LogStream::Stderr) => "stderr",
            LogSource::Pane => "pane",
        };
        let command = self
            .command
            .as_deref()
            .map(|command| format!(" [{command}]"))
            .unwrap_or_default();
        format!(
            "{} {:<5} {who}{command} {}",
            utc(self.time),
            self.level.name(),
            self.text
        )
    }
}

/// Every package's extension log, shared by the runtime (which writes what
/// guests print and its own messages) and the launcher (which writes its
/// messages, and reads and follows the logs). Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct ExtensionLogs(Arc<Mutex<Logs>>);

#[derive(Default)]
struct Logs {
    /// By package identity key.
    packages: HashMap<String, PackageLog>,
}

#[derive(Default)]
struct PackageLog {
    lines: VecDeque<LogLine>,
    /// The bytes of text in `lines`.
    bytes: usize,
    /// Set while the package is developed.
    development: Option<DevelopmentLog>,
    followers: Vec<Sender<LogLine>>,
    rate: Rate,
}

/// The log file of a development session.
struct DevelopmentLog {
    path: PathBuf,
    /// Opened at the first line; none once it could not be.
    file: Option<File>,
    written: u64,
    /// Whether opening it failed, so it is not tried for every line.
    failed: bool,
}

/// How many lines the package wrote in the current second.
#[derive(Default)]
struct Rate {
    second: Option<Instant>,
    count: u32,
    /// Lines dropped since Pane last noted it.
    dropped: u64,
    /// The generation the dropped lines came from.
    generation: u64,
}

impl ExtensionLogs {
    fn lock(&self) -> MutexGuard<'_, Logs> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Where a guest instance of the package with identity key `owner`, in
    /// generation `generation`, writes `stream`: lines are tagged with the
    /// command `command` holds when each is written. `map` is the source
    /// map kept beside the component, when one is: what the package writes
    /// has the frames of its stacks mapped back to the sources its bundle
    /// was built from (see `crate::source_map`) as the lines are captured,
    /// which is what a JavaScript or TypeScript development build keeps it
    /// for.
    pub fn output(
        &self,
        owner: &str,
        generation: u64,
        stream: LogStream,
        command: CurrentCommand,
        map: Option<Arc<SourceMap>>,
    ) -> Output {
        Output(Arc::new(OutputTo {
            logs: self.clone(),
            owner: owner.to_owned(),
            generation,
            stream,
            command,
            map,
        }))
    }

    /// Adds a message of Pane's own about the package with identity key
    /// `owner`, about its code of `generation` (0 when it is about none).
    pub fn pane(&self, owner: &str, generation: u64, level: LogLevel, text: &str) {
        let line = LogLine {
            time: SystemTime::now(),
            source: LogSource::Pane,
            level,
            command: None,
            generation,
            text: cut(text, PANE_LINE_LIMIT),
        };
        let mut logs = self.lock();
        let log = logs.packages.entry(owner.to_owned()).or_default();
        log.note_dropped();
        log.push(line);
    }

    /// Adds a line the package's code wrote, of which `cut` more bytes were
    /// not kept, unless it wrote too many this second. Its text after the
    /// level prefix is cut at [`LINE_LIMIT`].
    fn extension(
        &self,
        owner: &str,
        generation: u64,
        stream: LogStream,
        command: Option<String>,
        line: &str,
        cut: usize,
    ) {
        let (level, text) = level_of(stream, line);
        let mut end = text.len().min(LINE_LIMIT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let cut = cut + (text.len() - end);
        let mut text = text[..end].to_owned();
        if cut > 0 {
            // The kept part may end inside a character, which was replaced.
            text.push_str(&format!("… (Pane cut {cut} more bytes of this line)"));
        }
        let mut logs = self.lock();
        let log = logs.packages.entry(owner.to_owned()).or_default();
        if !log.rate.allow(Instant::now(), generation) {
            return;
        }
        log.note_dropped();
        log.push(LogLine {
            time: SystemTime::now(),
            source: LogSource::Extension(stream),
            level,
            command,
            generation,
            text,
        });
    }

    /// The package with identity key `owner` is developed from now on: its
    /// lines are also appended to the log file at `file`, which is started
    /// afresh (an earlier session's goes), and more of them are kept.
    pub fn develop(&self, owner: &str, file: PathBuf) {
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(rotated(&file));
        let mut logs = self.lock();
        let log = logs.packages.entry(owner.to_owned()).or_default();
        log.development = Some(DevelopmentLog {
            path: file,
            file: None,
            written: 0,
            failed: false,
        });
    }

    /// The package with identity key `owner` is no longer developed: its
    /// log file is closed (and kept), its followers are let go, and only
    /// its most recent lines are kept.
    pub fn stop_developing(&self, owner: &str) {
        let mut logs = self.lock();
        if let Some(log) = logs.packages.get_mut(owner) {
            log.development = None;
            log.followers.clear();
            log.trim();
        }
    }

    /// Where the log file of the package with identity key `owner` is,
    /// while it is developed.
    pub fn file(&self, owner: &str) -> Option<PathBuf> {
        let logs = self.lock();
        let development = logs.packages.get(owner)?.development.as_ref()?;
        Some(development.path.clone())
    }

    /// The lines kept for the package with identity key `owner`, oldest
    /// first.
    pub fn lines(&self, owner: &str) -> Vec<LogLine> {
        let mut logs = self.lock();
        let Some(log) = logs.packages.get_mut(owner) else {
            return Vec::new();
        };
        log.note_dropped_after(Instant::now());
        log.lines.iter().cloned().collect()
    }

    /// Each line added to the log of the package with identity key `owner`
    /// from now on, while it is developed: the receiver ends when its
    /// development does.
    pub fn follow(&self, owner: &str) -> Receiver<LogLine> {
        let (sender, receiver) = mpsc::channel();
        let mut logs = self.lock();
        let log = logs.packages.entry(owner.to_owned()).or_default();
        if log.development.is_some() {
            log.followers.push(sender);
        }
        receiver
    }

    /// Forgets the lines kept for the package with identity key `owner`
    /// (not its log file).
    pub fn clear(&self, owner: &str) {
        let mut logs = self.lock();
        if let Some(log) = logs.packages.get_mut(owner) {
            log.lines.clear();
            log.bytes = 0;
        }
    }

    /// Forgets the package with identity key `owner` entirely, as when it
    /// is uninstalled.
    pub fn forget(&self, owner: &str) {
        self.lock().packages.remove(owner);
    }
}

impl PackageLog {
    fn push(&mut self, line: LogLine) {
        if let Some(development) = &mut self.development {
            development.append(&line);
        }
        self.followers
            .retain(|follower| follower.send(line.clone()).is_ok());
        self.bytes += line.text.len();
        self.lines.push_back(line);
        self.trim();
    }

    /// Drops the oldest lines beyond what the package may keep.
    fn trim(&mut self) {
        let (lines, bytes) = match self.development {
            Some(_) => (DEVELOPED_LINES, DEVELOPED_BYTES),
            None => (WINDOW_LINES, WINDOW_BYTES),
        };
        while self.lines.len() > lines || (self.bytes > bytes && self.lines.len() > 1) {
            let Some(line) = self.lines.pop_front() else {
                break;
            };
            self.bytes -= line.text.len();
        }
    }

    /// Notes how many lines were dropped, if the second they were dropped
    /// in is over.
    fn note_dropped_after(&mut self, now: Instant) {
        if self
            .rate
            .second
            .is_some_and(|second| now.duration_since(second) >= Duration::from_secs(1))
        {
            self.note_dropped();
        }
    }

    /// Notes how many lines were dropped since the last note, if any were.
    fn note_dropped(&mut self) {
        let dropped = std::mem::take(&mut self.rate.dropped);
        if dropped == 0 {
            return;
        }
        let line = LogLine {
            time: SystemTime::now(),
            source: LogSource::Pane,
            level: LogLevel::Warn,
            command: None,
            generation: self.rate.generation,
            text: format!(
                "Pane dropped {dropped} {} the extension wrote: it wrote more than \
                 {LINES_PER_SECOND} lines in a second",
                if dropped == 1 { "line" } else { "lines" }
            ),
        };
        self.push(line);
    }
}

impl Rate {
    /// Whether a line written `now` is kept: no more than
    /// [`LINES_PER_SECOND`] in each second.
    fn allow(&mut self, now: Instant, generation: u64) -> bool {
        let current = self
            .second
            .is_some_and(|second| now.duration_since(second) < Duration::from_secs(1));
        if !current {
            self.second = Some(now);
            self.count = 0;
        }
        if self.count >= LINES_PER_SECOND {
            self.dropped += 1;
            self.generation = generation;
            return false;
        }
        self.count += 1;
        true
    }
}

impl DevelopmentLog {
    fn append(&mut self, line: &LogLine) {
        if self.failed {
            return;
        }
        let text = line.file_line() + "\n";
        if self.file.is_some() && self.written + text.len() as u64 > FILE_LIMIT {
            self.file = None;
            if std::fs::rename(&self.path, rotated(&self.path)).is_err() {
                let _ = std::fs::remove_file(&self.path);
            }
            self.written = 0;
        }
        if self.file.is_none() {
            match open(&self.path) {
                Ok(file) => self.file = Some(file),
                Err(_) => {
                    self.failed = true;
                    return;
                }
            }
        }
        if let Some(file) = &mut self.file
            && file.write_all(text.as_bytes()).is_ok()
        {
            self.written += text.len() as u64;
        }
    }
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Where the log file at `path` goes when it is rotated.
pub fn rotated(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    path.with_file_name(name)
}

/// The command a guest instance's call is running for, as the runtime last
/// set it; read when a line is written. Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct CurrentCommand(Arc<Mutex<Option<String>>>);

impl CurrentCommand {
    pub fn set(&self, command: Option<String>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = command;
    }

    fn get(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// One of a guest instance's outputs, as WASI hands it to the guest.
#[derive(Clone)]
pub(crate) struct Output(Arc<OutputTo>);

struct OutputTo {
    logs: ExtensionLogs,
    owner: String,
    generation: u64,
    stream: LogStream,
    command: CurrentCommand,
    /// The source map of the component the instance runs, when one is kept
    /// beside it: a development build of JavaScript or TypeScript.
    map: Option<Arc<SourceMap>>,
}

impl wasmtime_wasi::cli::IsTerminal for Output {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl wasmtime_wasi::cli::StdoutStream for Output {
    fn async_stream(&self) -> Box<dyn tokio::io::AsyncWrite + Send + Sync> {
        Box::new(Lines {
            to: self.0.clone(),
            partial: Vec::new(),
            cut: 0,
        })
    }
}

/// The bytes of a line kept before its level prefix is read: the limit,
/// and room for the prefix.
const KEPT_BYTES: usize = LINE_LIMIT + 3;

/// Splits what a guest writes into lines. Each WASI write stream has its
/// own, so a line two streams write at once is not mixed; a line left
/// unfinished when its stream ends is kept as it is.
struct Lines {
    to: Arc<OutputTo>,
    partial: Vec<u8>,
    /// How many bytes of the current line were cut.
    cut: usize,
}

impl Lines {
    fn take(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while let Some(end) = rest.iter().position(|&byte| byte == b'\n') {
            self.add(&rest[..end]);
            self.finish();
            rest = &rest[end + 1..];
        }
        self.add(rest);
    }

    fn add(&mut self, bytes: &[u8]) {
        let room = KEPT_BYTES.saturating_sub(self.partial.len());
        let kept = bytes.len().min(room);
        self.partial.extend_from_slice(&bytes[..kept]);
        self.cut += bytes.len() - kept;
    }

    fn finish(&mut self) {
        let mut bytes = std::mem::take(&mut self.partial);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        let text = String::from_utf8_lossy(&bytes);
        // A JavaScript or TypeScript command's stacks name its bundle; a
        // map beside the component turns them back into its sources as
        // the line is captured, so every reader of the log shows them so.
        let text = match &self.to.map {
            Some(map) => Cow::Owned(map.map_frames(&text)),
            None => text,
        };
        let cut = std::mem::take(&mut self.cut);
        let to = &self.to;
        to.logs.extension(
            &to.owner,
            to.generation,
            to.stream,
            to.command.get(),
            &text,
            cut,
        );
    }
}

impl Drop for Lines {
    fn drop(&mut self) {
        if !self.partial.is_empty() || self.cut > 0 {
            self.finish();
        }
    }
}

impl tokio::io::AsyncWrite for Lines {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.take(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// The level a line written to `stream` has, and its text without the
/// level prefix the SDKs write.
fn level_of(stream: LogStream, line: &str) -> (LogLevel, &str) {
    let bytes = line.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'<' && bytes[2] == b'>' && bytes[1].is_ascii_digit() {
        let level = match bytes[1] {
            b'0'..=b'3' => LogLevel::Error,
            b'4' => LogLevel::Warn,
            b'5' | b'6' => LogLevel::Info,
            _ => LogLevel::Debug,
        };
        return (level, &line[3..]);
    }
    match stream {
        LogStream::Stdout => (LogLevel::Info, line),
        LogStream::Stderr => (LogLevel::Error, line),
    }
}

/// `text`, cut at `limit` bytes with a note of how much was cut.
fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}… (Pane cut {} more bytes of this line)",
        &text[..end],
        text.len() - end
    )
}

/// `time` as UTC, in RFC 3339 with milliseconds.
fn utc(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since.as_secs() as i64;
    let (day, second) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's `civil_from_days`.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}:{:02}:{:02}.{:03}Z",
        second / 3600,
        second / 60 % 60,
        second % 60,
        since.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(logs: &ExtensionLogs, stream: LogStream, bytes: &[u8]) {
        let output = logs.output("p", 1, stream, CurrentCommand::default(), None);
        let mut lines = Lines {
            to: output.0.clone(),
            partial: Vec::new(),
            cut: 0,
        };
        lines.take(bytes);
    }

    fn texts(logs: &ExtensionLogs) -> Vec<(LogLevel, String)> {
        logs.lines("p")
            .into_iter()
            .map(|line| (line.level, line.text))
            .collect()
    }

    #[test]
    fn lines_are_split_and_levelled() {
        let logs = ExtensionLogs::default();
        write(&logs, LogStream::Stdout, b"plain\r\n<4>careful\n<7>deta");
        write(&logs, LogStream::Stderr, b"bad\n");
        assert_eq!(
            texts(&logs),
            vec![
                (LogLevel::Info, "plain".into()),
                (LogLevel::Warn, "careful".into()),
                (LogLevel::Debug, "deta".into()),
                (LogLevel::Error, "bad".into()),
            ]
        );
    }

    #[test]
    fn a_long_line_is_cut() {
        let logs = ExtensionLogs::default();
        let long = vec![b'x'; LINE_LIMIT + 10];
        write(&logs, LogStream::Stdout, &long);
        let text = &logs.lines("p")[0].text;
        assert!(text.starts_with(&"x".repeat(LINE_LIMIT)));
        assert!(
            text.ends_with("(Pane cut 10 more bytes of this line)"),
            "{text}"
        );
    }

    #[test]
    fn the_window_keeps_the_most_recent_lines() {
        let logs = ExtensionLogs::default();
        for n in 0..WINDOW_LINES + 5 {
            logs.pane("p", 1, LogLevel::Info, &n.to_string());
        }
        let lines = logs.lines("p");
        assert_eq!(lines.len(), WINDOW_LINES);
        assert_eq!(lines[0].text, "5");
    }

    #[test]
    fn a_development_log_file_is_rotated_at_its_limit() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("extension.log");
        let logs = ExtensionLogs::default();
        logs.develop("p", file.clone());
        let text = "y".repeat(PANE_LINE_LIMIT);
        let lines = FILE_LIMIT as usize / PANE_LINE_LIMIT + 2;
        for _ in 0..lines {
            logs.pane("p", 1, LogLevel::Info, &text);
        }
        let earlier = std::fs::metadata(rotated(&file)).unwrap().len();
        let current = std::fs::metadata(&file).unwrap().len();
        assert!(
            earlier <= FILE_LIMIT && earlier > FILE_LIMIT / 2,
            "{earlier}"
        );
        assert!(current > 0 && current < FILE_LIMIT / 2, "{current}");

        // A new session starts afresh; stopping keeps the file.
        logs.develop("p", file.clone());
        assert!(!rotated(&file).exists() && !file.exists());
        logs.pane("p", 1, LogLevel::Info, "kept");
        logs.stop_developing("p");
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .ends_with(" pane kept\n")
        );
    }

    #[test]
    fn a_second_keeps_no_more_than_the_limit() {
        let mut rate = Rate::default();
        let start = Instant::now();
        let kept = (0..LINES_PER_SECOND + 5)
            .filter(|_| rate.allow(start, 1))
            .count();
        assert_eq!(kept, LINES_PER_SECOND as usize);
        assert_eq!(rate.dropped, 5);
        // The next second starts afresh.
        assert!(rate.allow(start + Duration::from_secs(1), 1));
    }

    #[test]
    fn utc_is_rfc_3339() {
        let time = UNIX_EPOCH + Duration::from_millis(1_760_000_000_123);
        assert_eq!(utc(time), "2025-10-09T08:53:20.123Z");
    }
}
