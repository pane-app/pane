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
/// SIGTERM, SIGINT or SIGHUP quits cleanly too ([`quit_cleanly_on_signals`])
/// before it ends Pane as it always did. Pane's binary calls this once,
/// first thing.
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

/// Has `quit` run, on a thread of its own, when SIGTERM, SIGINT or SIGHUP
/// ends Pane on Linux or macOS, before the marker goes and Pane ends with
/// the signal's own action: Pane's binary passes the launcher's clean quit
/// (`Launcher::quit_cleanly`), which writes what clipboard history keeps
/// waiting in a batch (#192). Given at most two seconds; past them, or
/// before this was called, the marker is removed directly. Only the first
/// call counts. Does nothing elsewhere: Windows' session end reaches the
/// app's quit hooks.
pub fn quit_cleanly_on_signals(quit: impl FnOnce() + Send + 'static) {
    #[cfg(unix)]
    signals::on_quit(Box::new(quit));
    #[cfg(not(unix))]
    let _ = quit;
}

/// The signals that end Pane on Linux and macOS, each of which quits
/// cleanly first: the system ending the session (SIGTERM, SIGHUP) and the
/// user's Ctrl+C in a terminal (SIGINT) are clean quits.
///
/// The handler does only what is safe in a signal handler: it writes one
/// byte to a pipe, which wakes the thread `pane-signals`, and returns. That
/// thread runs the clean quit (`quit_cleanly_on_signals`) on a thread of
/// its own, waits for it at most `QUIT_LIMIT`, removes the marker whether
/// it ended or not, restores the signal's default action and raises the
/// signal again, so that Pane ends as it always did. A signal received
/// meanwhile changes nothing. Should the pipe or the thread not exist, the
/// handler itself removes the marker and ends Pane at once, as before.
#[cfg(unix)]
mod signals {
    use std::ffi::CString;
    use std::io;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
    use std::time::Duration;

    /// The longest a signal waits for the clean quit before Pane ends.
    pub(super) const QUIT_LIMIT: Duration = Duration::from_secs(2);

    /// The marker's path, for the handler, which may only call what is
    /// safe in a signal handler.
    static MARKER: OnceLock<CString> = OnceLock::new();

    /// The pipe's end the handler writes to, while `pane-signals` waits on
    /// the other; -1 otherwise.
    static PIPE: AtomicI32 = AtomicI32::new(-1);

    /// Whether a signal was received already.
    static PASSED: AtomicBool = AtomicBool::new(false);

    /// The signal passed on to `pane-signals`.
    static RECEIVED: AtomicI32 = AtomicI32::new(0);

    /// What a signal runs before Pane ends (see `quit_cleanly_on_signals`).
    static QUIT: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);

    pub(super) fn on_quit(quit: Box<dyn FnOnce() + Send>) {
        let mut kept = QUIT.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.is_none() {
            *kept = Some(quit);
        }
    }

    pub(super) fn remove_marker_on_signals(marker: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = CString::new(marker.as_os_str().as_bytes()) else {
            return;
        };
        if MARKER.set(path).is_err() {
            return;
        }
        listen();
        let handler = received as extern "C" fn(libc::c_int) as libc::sighandler_t;
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            // SAFETY: `received` calls only async-signal-safe functions.
            let before = unsafe { libc::signal(signal, handler) };
            // A signal Pane was started ignoring (`nohup`) stays ignored.
            if before == libc::SIG_IGN {
                // SAFETY: as above.
                unsafe { libc::signal(signal, libc::SIG_IGN) };
            }
        }
    }

    /// Makes the pipe and starts `pane-signals`, which waits on it; on
    /// failure the handler ends Pane itself.
    fn listen() {
        let mut ends: [libc::c_int; 2] = [-1, -1];
        // SAFETY: `ends` has room for the two descriptors `pipe` writes.
        if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
            return;
        }
        let [reading, writing] = ends;
        for descriptor in ends {
            // Not inherited by the programs Pane starts.
            // SAFETY: a descriptor this function just opened.
            unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        // The handler never blocks on it.
        // SAFETY: as above.
        unsafe { libc::fcntl(writing, libc::F_SETFL, libc::O_NONBLOCK) };
        let started = std::thread::Builder::new()
            .name("pane-signals".into())
            .spawn(move || wait(reading));
        if started.is_ok() {
            PIPE.store(writing, Ordering::SeqCst);
        } else {
            // SAFETY: descriptors this function opened, used by nothing.
            unsafe {
                libc::close(reading);
                libc::close(writing);
            }
        }
    }

    /// `pane-signals`: waits for the handler's byte, then quits cleanly and
    /// ends Pane with the signal received.
    fn wait(reading: libc::c_int) {
        let mut byte = 0u8;
        loop {
            // SAFETY: reads at most one byte into `byte`.
            let read = unsafe { libc::read(reading, (&raw mut byte).cast(), 1) };
            if read == 1 {
                break;
            }
            if read < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            // The pipe failed: the handler ends Pane itself from now on.
            PIPE.store(-1, Ordering::SeqCst);
            return;
        }
        let signal = RECEIVED.load(Ordering::SeqCst);
        let quit = QUIT.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(quit) = quit {
            let (done, ended) = mpsc::channel();
            let quitting = std::thread::Builder::new()
                .name("pane-quit".into())
                .spawn(move || {
                    quit();
                    let _ = done.send(());
                });
            if quitting.is_ok() {
                // Past the limit (a lock held by a thread that hangs), Pane
                // ends anyway: what was written stays.
                let _ = ended.recv_timeout(QUIT_LIMIT);
            }
        }
        end(signal);
    }

    /// The handler: passes the signal on to `pane-signals`, once; ends Pane
    /// at once if it cannot (see the module).
    extern "C" fn received(signal: libc::c_int) {
        let errno = errno();
        // SAFETY: this thread's `errno`, when known.
        let saved = (!errno.is_null()).then(|| unsafe { *errno });
        // A signal after one was passed on (the session's end sends
        // SIGTERM and SIGHUP together) leaves it to end Pane, within
        // `QUIT_LIMIT`.
        let passed = PASSED.swap(true, Ordering::SeqCst);
        let writing = PIPE.load(Ordering::SeqCst);
        if !passed && writing >= 0 {
            RECEIVED.store(signal, Ordering::SeqCst);
            let byte = 1u8;
            // SAFETY: writes one byte from `byte`; `write` is
            // async-signal-safe.
            let written = unsafe { libc::write(writing, (&raw const byte).cast(), 1) };
            if written != 1 {
                end(signal);
            }
        } else if !passed {
            end(signal);
        }
        // The interrupted code finds `errno` as it left it.
        if let Some(saved) = saved {
            // SAFETY: as above.
            unsafe { *errno = saved };
        }
    }

    /// Removes the marker, then ends Pane with `signal`'s own default
    /// action, as it ended before. Async-signal-safe.
    fn end(signal: libc::c_int) {
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

    /// Where this thread's `errno` is, which the handler leaves as it found
    /// it; null where unknown.
    #[cfg(target_os = "linux")]
    fn errno() -> *mut libc::c_int {
        // SAFETY: returns this thread's `errno`; async-signal-safe.
        unsafe { libc::__errno_location() }
    }

    #[cfg(target_os = "macos")]
    fn errno() -> *mut libc::c_int {
        // SAFETY: as above.
        unsafe { libc::__error() }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn errno() -> *mut libc::c_int {
        std::ptr::null_mut()
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
