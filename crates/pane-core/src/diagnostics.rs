//! Pane's own diagnostics and its local crash record (#133).
//!
//! **One diagnostic path.** Every message Pane writes to standard error, in
//! this crate and in the `pane` crate, goes through [`report`] (or the
//! [`diagnostic!`](crate::diagnostic) macro over it): it writes the message
//! to standard error as Pane always did — a developer running Pane from a
//! terminal sees no change — and to Pane's log, where a windowed Windows
//! build's diagnostics would otherwise go nowhere. An extension's own
//! output is not included. Nothing here panics: a diagnostic written from
//! a window procedure or a panic hook only loses its line when it cannot
//! be written.
//!
//! **The log** (`log`) is `pane.log` in the logs folder Pane's binary
//! names (`%LOCALAPPDATA%\Pane\logs`, `~/Library/Logs/Pane`,
//! `$XDG_STATE_HOME/pane/logs`, or `logs` in `PANE_DATA_DIR`), readable by
//! the user only: size-capped and rotated, rate-limited per site, and
//! redacted at its writer (`redact`) — the home folder's path, the user's
//! name and the computer's are replaced before anything is written. Pane's
//! own messages are written so that none carries extension data values,
//! local credentials, clipboard history text, query text or found file
//! names; an address an extension asked for is named by its host only.
//!
//! **Panics** are written to the log (message, location and any captured
//! backtrace) by a panic hook before the default handling.
//!
//! **The marker** (`marker`) lets the next start tell that a run ended
//! unexpectedly, which the launcher shows as one root result and Settings'
//! About page as a notice.
//!
//! Nothing is sent anywhere: no telemetry, no crash upload, no minidump.

use std::cell::Cell;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, Once, OnceLock, PoisonError};

mod log;
mod marker;
mod redact;

pub use marker::{CrashRecord, Liveness, ProcessTable, SystemProcesses};

use log::Log;
pub(crate) use redact::Redactor;

/// Writes one diagnostic through [`report`]: `diagnostic!("Pane could not
/// save {what}: {error}")`, formatted as `eprintln!` formats, whose format
/// string is the message's site for the log's rate limit.
#[macro_export]
macro_rules! diagnostic {
    ($format:literal $($arguments:tt)*) => {
        $crate::diagnostics::report($format, &::std::format!($format $($arguments)*))
    };
}

/// What Pane says after a run ended unexpectedly: the log's line at start,
/// root search's row and the status line, and Settings' About page.
pub const CRASH_NOTICE: &str = "Pane quit unexpectedly last time";

/// The log diagnostics go to, once [`start`] opened it.
static SINK: Mutex<Option<Arc<Log>>> = Mutex::new(None);

thread_local! {
    /// Whether this thread is writing to the log now: a panic inside the
    /// writer must not write to it again.
    static WRITING: Cell<bool> = const { Cell::new(false) };
}

/// Writes `message` to standard error, as Pane always did, and to Pane's
/// log, redacted, unless `site` (the message's fixed text, before its
/// values) already wrote its lines this minute. Never panics.
pub fn report(site: &str, message: &str) {
    {
        use std::io::Write;
        let _ = writeln!(io::stderr().lock(), "{message}");
    }
    to_log(site, message);
}

/// [`report`] for a message whose fixed text is the part before its first
/// `:` or `(`, as the platform adapters' messages are ("Windows refused
/// the tray icon (…); …").
pub fn report_line(message: &str) {
    let site = message.split([':', '(']).next().unwrap_or(message);
    report(site, message);
}

/// `text` redacted as the log redacts what it writes: this computer's home
/// folder's path becomes `~`, the user's name `<user>` and the computer's
/// `<computer>`. For what Pane offers the user to copy into a report, such
/// as the About page's diagnostics.
pub fn redacted(text: &str) -> String {
    static REDACTOR: OnceLock<Redactor> = OnceLock::new();
    REDACTOR.get_or_init(Redactor::of_this_system).redact(text)
}

/// Writes `message` to the log only.
fn to_log(site: &str, message: &str) {
    if let Some(log) = sink() {
        writing(|| log.write(site, message));
    }
}

/// Runs `write`, which takes the log's lock, unless this thread is writing
/// to the log already: a panic inside the writer comes back to the log
/// through the panic hook, and that line is lost rather than the lock taken
/// twice.
fn writing(write: impl FnOnce()) {
    if matches!(WRITING.try_with(|writing| writing.replace(true)), Ok(true)) {
        return;
    }
    write();
    let _ = WRITING.try_with(|writing| writing.set(false));
}

/// The log diagnostics go to: a test's own on this thread, or the one
/// [`start`] opened.
fn sink() -> Option<Arc<Log>> {
    if let Some(log) = test_sink() {
        return Some(log);
    }
    SINK.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[cfg(not(test))]
fn test_sink() -> Option<Arc<Log>> {
    None
}

/// Opens Pane's log in the logs folder `folder` (made, readable by the user
/// only, if it is not there), starts this run's lines with Pane's
/// `version`, sends every diagnostic and panic there from now on, and
/// opens this run's crash record: whether the run before ended
/// unexpectedly, and the marker a clean quit removes. On Linux and macOS a
/// SIGTERM, SIGINT or SIGHUP removes the marker too before it ends Pane as
/// it always did. Pane's binary calls this once, first thing.
pub fn start(folder: &Path, version: &str) -> CrashRecord {
    let _ = create_private_dir(folder);
    let log = Arc::new(Log::open(folder, Redactor::of_this_system()));
    writing(|| log.begin_run(version));
    *SINK.lock().unwrap_or_else(PoisonError::into_inner) = Some(log);
    install_panic_hook();
    let record = CrashRecord::open(folder, version, &SystemProcesses);
    if record.ended_unexpectedly() {
        report(CRASH_NOTICE, CRASH_NOTICE);
    }
    #[cfg(unix)]
    signals::remove_marker_on_signals(record.marker());
    record
}

/// Writes what the log held back, as a clean quit does before Pane ends.
pub(crate) fn finish() {
    if let Some(log) = sink() {
        writing(|| log.finish());
    }
}

/// Has every panic write its message, location and any captured backtrace
/// to the log before the default handling (which still prints it to
/// standard error). Installed once.
fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            to_log("panic", &panic_text(info));
            default(info);
        }));
    });
}

/// What the log says of a panic: its thread, its location, its message and
/// its backtrace when one was captured (`RUST_BACKTRACE`).
fn panic_text(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned());
    let location = match info.location() {
        Some(location) => format!(
            "{}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        ),
        None => "an unknown place".to_owned(),
    };
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    let mut text = format!("Pane panicked in thread '{name}' at {location}: {message}");
    let backtrace = std::backtrace::Backtrace::capture();
    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        text.push_str(&format!("\n{backtrace}"));
    }
    text
}

/// Creates the logs folder, readable by the user only: mode 0700 on Unix,
/// and on Windows a protected DACL for the user and SYSTEM, inherited by
/// its files, as the file index's folder is. A folder that exists keeps
/// its permissions on Windows.
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        if !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        if dir.is_dir() {
            return Ok(());
        }
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match crate::atomic::create_owner_only_dir(dir) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            done => done,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// The signals that end Pane on Linux and macOS, each of which removes the
/// marker first: the system ending the session (SIGTERM, SIGHUP) and the
/// user's Ctrl+C in a terminal (SIGINT) are clean quits.
#[cfg(unix)]
mod signals {
    use std::ffi::CString;
    use std::path::Path;
    use std::sync::OnceLock;

    /// The marker's path, for the handler, which may only call what is
    /// safe in a signal handler.
    static MARKER: OnceLock<CString> = OnceLock::new();

    pub(super) fn remove_marker_on_signals(marker: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = CString::new(marker.as_os_str().as_bytes()) else {
            return;
        };
        if MARKER.set(path).is_err() {
            return;
        }
        let handler = quit as extern "C" fn(libc::c_int) as libc::sighandler_t;
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            // SAFETY: `quit` calls only async-signal-safe functions.
            let before = unsafe { libc::signal(signal, handler) };
            // A signal Pane was started ignoring (`nohup`) stays ignored.
            if before == libc::SIG_IGN {
                // SAFETY: as above.
                unsafe { libc::signal(signal, libc::SIG_IGN) };
            }
        }
    }

    /// Removes the marker, then ends Pane with the signal's own default
    /// action, as it ended before.
    extern "C" fn quit(signal: libc::c_int) {
        if let Some(path) = MARKER.get() {
            // SAFETY: a NUL-terminated path that lives as long as the
            // process; `unlink` is async-signal-safe.
            unsafe { libc::unlink(path.as_ptr()) };
        }
        // SAFETY: `signal` and `raise` are async-signal-safe.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
}

#[cfg(test)]
thread_local! {
    /// A test's own log for diagnostics written on its thread, so tests
    /// that run at once never share one.
    static TEST_SINK: std::cell::RefCell<Option<Arc<Log>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_sink() -> Option<Arc<Log>> {
    TEST_SINK
        .try_with(|sink| sink.borrow().clone())
        .ok()
        .flatten()
}

/// Sends the diagnostics written on this thread to a log in `folder`
/// (redacted with `redactor`) until the returned guard is dropped.
#[cfg(test)]
pub(crate) fn capture_into(folder: &Path, redactor: Redactor) -> CaptureGuard {
    let opened = Arc::new(Log::open(folder, redactor));
    TEST_SINK.with(|sink| *sink.borrow_mut() = Some(opened));
    CaptureGuard(folder.join(log::FILE))
}

/// Ends a test's capture when dropped; [`CaptureGuard::text`] reads what
/// it wrote.
#[cfg(test)]
pub(crate) struct CaptureGuard(std::path::PathBuf);

#[cfg(test)]
impl CaptureGuard {
    /// The log's text so far.
    pub(crate) fn text(&self) -> String {
        std::fs::read_to_string(&self.0).unwrap_or_default()
    }
}

#[cfg(test)]
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let _ = TEST_SINK.try_with(|sink| sink.borrow_mut().take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diagnostic_reaches_the_log_through_the_one_path() {
        let folder = tempfile::tempdir().unwrap();
        let captured = capture_into(folder.path(), Redactor::none());
        let error = "the disk is full";
        crate::diagnostic!("Pane could not save the record: {error}");
        report_line("Pane could not read the clipboard: it is busy");
        let text = captured.text();
        assert!(
            text.contains("Pane could not save the record: the disk is full"),
            "{text}"
        );
        assert!(
            text.contains("Pane could not read the clipboard: it is busy"),
            "{text}"
        );
    }

    /// A platform adapter's message keeps its reason out of its site, so
    /// the rate limit holds back a repeated failure whatever its reason.
    #[test]
    fn a_platform_messages_site_ends_before_its_reason() {
        let folder = tempfile::tempdir().unwrap();
        let captured = capture_into(folder.path(), Redactor::none());
        for number in 0..12 {
            report_line(&format!(
                "Windows refused the tray icon (error {number}); it is tried again"
            ));
        }
        let text = captured.text();
        assert_eq!(
            text.matches("Windows refused the tray icon").count(),
            super::log::LINES_PER_WINDOW as usize,
            "{text}"
        );
    }

    #[test]
    fn a_panic_is_written_to_the_log_with_its_message_and_location() {
        let folder = tempfile::tempdir().unwrap();
        let captured = capture_into(folder.path(), Redactor::none());
        install_panic_hook();
        let line = line!() + 1;
        let caught = std::panic::catch_unwind(|| panic!("the test's own panic {}", 42));
        assert!(caught.is_err());
        let text = captured.text();
        assert!(
            text.contains("Pane panicked in thread '") && text.contains("the test's own panic 42"),
            "{text}"
        );
        assert!(
            text.contains(&format!("diagnostics.rs:{line}:")),
            "the location: {text}"
        );
    }

    /// Every message Pane writes to standard error goes through this
    /// module: no other source file of either crate writes there itself.
    #[test]
    fn nothing_else_writes_to_standard_error() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut offenders = Vec::new();
        for crate_dir in ["pane-core/src", "pane/src"] {
            let mut folders = vec![root.join(crate_dir)];
            while let Some(folder) = folders.pop() {
                for entry in std::fs::read_dir(&folder).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        folders.push(path);
                        continue;
                    }
                    if path.extension().is_none_or(|extension| extension != "rs")
                        || path.ends_with("diagnostics.rs")
                    {
                        continue;
                    }
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (number, line) in text.lines().enumerate() {
                        let code = line.trim_start();
                        if code.starts_with("//") {
                            continue;
                        }
                        if ["eprintln!", "eprint!", "stderr()"]
                            .iter()
                            .any(|writes| code.contains(writes))
                        {
                            offenders.push(format!("{}:{}", path.display(), number + 1));
                        }
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "write through `diagnostic!`: {offenders:#?}"
        );
    }
}
