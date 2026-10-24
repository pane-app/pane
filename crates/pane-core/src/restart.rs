//! Restarting Pane into a new version of itself (ADR 0050): starting the
//! program an update just swapped in, and the handoff between the Pane that
//! starts it and the one it starts.
//!
//! **The old process** checks the new program first ([`Relaunch::version`],
//! `<program> --version`), then starts it with `--updated-from <its own
//! version> --wait-pid <its own process id>` ([`Relaunch::start`]) and
//! waits up to [`STARTED_LIMIT`] for it to report that it started
//! ([`wait_for_started`]). The report is a file in Pane's cache folder,
//! named for the old process's id, which both processes know on every
//! system ([`report_started`]). Only then does the old process quit.
//!
//! **The new process** writes that report first thing, before it takes
//! anything, then waits up to [`PREVIOUS_LIMIT`] for the old process to
//! exit ([`wait_for_previous`]): no hotkey, keyboard hook, tray icon, local
//! channel, file index lock or clipboard listener is taken while another
//! Pane holds it. A process that outlives the wait is ended, but only when
//! it still runs one of the programs the update left in the install folder
//! (the old program, under its own name or renamed), so that a process id
//! the system gave to another program meanwhile is never ended.
//!
//! The program is started directly on every system, inheriting the old
//! process's environment: on macOS the bundle's own program
//! (`Pane.app/Contents/MacOS/pane`), as a terminal starts it, rather than
//! through Launch Services, which would start it with its own environment.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::diagnostics::{Liveness, ProcessTable, SystemProcesses};

/// How long the old process waits for the new one to report that it
/// started, before it undoes the update and keeps running.
pub const STARTED_LIMIT: Duration = Duration::from_secs(20);

/// How long the new process waits for the old one to exit before it ends
/// it.
pub const PREVIOUS_LIMIT: Duration = Duration::from_secs(30);

/// How long `<program> --version` may take to answer. A program written a
/// moment ago may first be scanned by the system's malware protection,
/// which can take a few seconds.
const VERSION_LIMIT: Duration = Duration::from_secs(20);

/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(50);

/// The argument naming the version a restart came from.
pub const UPDATED_FROM: &str = "--updated-from";

/// The argument naming the process a restart waits for.
pub const WAIT_PID: &str = "--wait-pid";

/// The argument a restart Pane made by itself, while the user was not
/// using it, adds (ADR 0051): the new process starts with its launcher
/// hidden, as the old one was.
pub const HIDDEN: &str = "--hidden";

/// How Pane checks and starts the program an update swapped in: the
/// system's own processes ([`Native`]), or a test's.
pub trait Relaunch: Send + Sync {
    /// The version `program` reports with `--version` ("Pane 0.2.0" is
    /// `0.2.0`), or why it reported none.
    fn version(&self, program: &Path) -> Result<String, String>;

    /// Starts `program` with `arguments`, apart from this process, so it
    /// keeps running once this one has exited.
    fn start(&self, program: &Path, arguments: &[String]) -> Result<Box<dyn Started>, String>;
}

/// A program [`Relaunch::start`] started.
pub trait Started: Send {
    /// Whether it has exited, as far as this process can tell.
    fn exited(&mut self) -> bool;

    /// Ends it, if it still runs.
    fn end(&mut self);
}

/// The system's own processes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Native;

impl Relaunch for Native {
    fn version(&self, program: &Path) -> Result<String, String> {
        let mut command = Command::new(program);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("it could not be started: {error}"))?;
        // The answer is a line: it fits in the pipe, so waiting for the
        // exit before reading cannot block the program.
        let deadline = Instant::now() + VERSION_LIMIT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "it did not answer `--version` within {} seconds",
                        VERSION_LIMIT.as_secs()
                    ));
                }
                Err(error) => return Err(format!("it could not be waited for: {error}")),
            }
        };
        let mut answer = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut answer);
        }
        if !status.success() {
            return Err(format!("`--version` failed ({status})"));
        }
        reported_version(&answer).ok_or_else(|| format!("`--version` answered `{}`", answer.trim()))
    }

    fn start(&self, program: &Path, arguments: &[String]) -> Result<Box<dyn Started>, String> {
        let mut command = Command::new(program);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_window(&mut command);
        // A process group of its own, so that a signal sent to this one's
        // group (Ctrl+C in the terminal it was started from) does not reach
        // the Pane that replaces it.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let child = command
            .spawn()
            .map_err(|error| format!("it could not be started: {error}"))?;
        Ok(Box::new(NativeChild(child)))
    }
}

/// Starts `command` with no window of its own on Windows (Pane's program
/// is a windowed one and makes none anyway; the flag keeps a console from
/// ever being made for it).
fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(::windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// A program the system started.
struct NativeChild(Child);

impl Started for NativeChild {
    fn exited(&mut self) -> bool {
        !matches!(self.0.try_wait(), Ok(None))
    }

    fn end(&mut self) {
        if !self.exited() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// The version a program's `--version` answer names: `Pane <version>`.
fn reported_version(answer: &str) -> Option<String> {
    let version = answer.trim().strip_prefix("Pane ")?.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

/// The arguments the new program is started with: the version it was
/// updated from, the process it waits for, and, for a restart Pane made by
/// itself, that it starts hidden.
pub fn arguments(updated_from: &str, waited: u32, hidden: bool) -> Vec<String> {
    let mut arguments = vec![
        UPDATED_FROM.to_owned(),
        updated_from.to_owned(),
        WAIT_PID.to_owned(),
        waited.to_string(),
    ];
    if hidden {
        arguments.push(HIDDEN.to_owned());
    }
    arguments
}

/// The file in `folder` the process a restart started writes, as the old
/// process `waited` waits for it.
fn report_file(folder: &Path, waited: u32) -> PathBuf {
    folder.join(format!("restart-{waited}.started"))
}

/// Reports to the process `waited`, which started this one, that this one
/// has started: a file in `folder` (Pane's cache folder) named for it,
/// holding this process's id. Written first thing, before this process
/// waits for the other to exit ([`wait_for_previous`]).
pub fn report_started(folder: &Path, waited: u32) -> std::io::Result<()> {
    fs::create_dir_all(folder)?;
    fs::write(
        report_file(folder, waited),
        std::process::id().to_string().as_bytes(),
    )
}

/// Waits up to `limit` for `started`, which this process (`own`) started,
/// to report that it started (see [`report_started`]), looking in
/// `folder`. The report is removed once it is read. `Err` with why not:
/// the program exited first, or said nothing in time.
pub fn wait_for_started(
    folder: &Path,
    own: u32,
    started: &mut dyn Started,
    limit: Duration,
) -> Result<(), String> {
    let report = report_file(folder, own);
    let deadline = Instant::now() + limit;
    loop {
        if report.is_file() {
            let _ = fs::remove_file(&report);
            return Ok(());
        }
        if started.exited() {
            // It may have reported just before it exited.
            if report.is_file() {
                let _ = fs::remove_file(&report);
                return Ok(());
            }
            return Err("it exited as it started".into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "it did not report that it started within {} seconds",
                limit.as_secs()
            ));
        }
        std::thread::sleep(POLL);
    }
}

/// Removes a report left in `folder` for this process (`own`), from a
/// restart that came too late to be read.
pub fn forget_report(folder: &Path, own: u32) {
    let _ = fs::remove_file(report_file(folder, own));
}

/// What became of the process a restart waited for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Previous {
    /// It exited within the wait.
    Exited,
    /// It still ran at the end of the wait, running one of the old
    /// programs, and was ended.
    Ended,
    /// It still ran at the end of the wait, running something else (its
    /// id was given to another program), and was left alone.
    LeftAlone,
}

/// Waits up to `limit` for the process `pid` to exit, as the new process of
/// a restart does before it takes anything the old one held. A process
/// still running at the end of the wait is ended only when the program it
/// runs is one of `programs` (the old program, under its own name and
/// renamed out of the new one's way).
pub fn wait_for_previous(pid: u32, programs: &[PathBuf], limit: Duration) -> Previous {
    wait_for_previous_in(&SystemProcesses, pid, programs, limit)
}

fn wait_for_previous_in(
    processes: &dyn ProcessTable,
    pid: u32,
    programs: &[PathBuf],
    limit: Duration,
) -> Previous {
    let deadline = Instant::now() + limit;
    loop {
        if processes.look_up(pid) == Liveness::Ended {
            return Previous::Exited;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(POLL);
    }
    let runs_old = platform::image(pid)
        .is_some_and(|image| programs.iter().any(|program| same_file(&image, program)));
    if !runs_old {
        return Previous::LeftAlone;
    }
    platform::end(pid);
    // It is given a moment to go, so what it held is free once this
    // returns.
    let deadline = Instant::now() + Duration::from_secs(5);
    while processes.look_up(pid) != Liveness::Ended && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    Previous::Ended
}

/// Whether two paths name the same file: compared as written, with
/// Windows' letter case ignored there.
fn same_file(image: &Path, program: &Path) -> bool {
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (image, program) = (canonical(image), canonical(program));
    if cfg!(windows) {
        image.to_string_lossy().to_lowercase() == program.to_string_lossy().to_lowercase()
    } else {
        image == program
    }
}

#[cfg(windows)]
mod platform {
    use std::path::PathBuf;

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
    use ::windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, TerminateProcess,
    };
    use ::windows::core::PWSTR;

    /// The program the process `pid` runs, as the system names it now.
    pub(super) fn image(pid: u32) -> Option<PathBuf> {
        // SAFETY: plain values; the handle is closed below.
        let handle: HANDLE =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut name = [0u16; 1024];
        let mut length = name.len() as u32;
        // SAFETY: `name` is writable for `length` units.
        let queried = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(name.as_mut_ptr()),
                &mut length,
            )
        };
        // SAFETY: opened above.
        let _ = unsafe { CloseHandle(handle) };
        queried.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(
            &name[..length as usize],
        )))
    }

    /// Ends the process `pid`.
    pub(super) fn end(pid: u32) {
        // SAFETY: plain values; the handle is closed below.
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }) else {
            return;
        };
        // SAFETY: a handle opened for terminating, closed once here.
        unsafe {
            let _ = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::PathBuf;

    /// The program the process `pid` runs, as the system names it now (a
    /// renamed program is named by its new name).
    pub(super) fn image(pid: u32) -> Option<PathBuf> {
        let image = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        // A program removed or replaced since it started is named with
        // " (deleted)" after its path.
        let text = image.to_string_lossy();
        Some(PathBuf::from(
            text.strip_suffix(" (deleted)").unwrap_or(&text).to_owned(),
        ))
    }

    /// Asks the process `pid` to end, as the session's end does: Pane
    /// quits cleanly on it.
    pub(super) fn end(pid: u32) {
        if let Ok(pid) = libc::pid_t::try_from(pid) {
            // SAFETY: a plain system call on a process id.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    /// The program the process `pid` runs, as the system names it.
    pub(super) fn image(pid: u32) -> Option<PathBuf> {
        let pid = libc::c_int::try_from(pid).ok()?;
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: `path` is writable for its length.
        let length =
            unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        if length <= 0 {
            return None;
        }
        path.truncate(length as usize);
        Some(PathBuf::from(OsString::from_vec(path)))
    }

    /// Asks the process `pid` to end, as the session's end does: Pane
    /// quits cleanly on it.
    pub(super) fn end(pid: u32) {
        if let Ok(pid) = libc::pid_t::try_from(pid) {
            // SAFETY: a plain system call on a process id.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use std::path::PathBuf;

    /// A system whose processes Pane does not read: no process is ended.
    pub(super) fn image(_pid: u32) -> Option<PathBuf> {
        None
    }

    pub(super) fn end(_pid: u32) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn a_version_is_read_from_pane_s_answer() {
        assert_eq!(reported_version("Pane 0.2.0\n").as_deref(), Some("0.2.0"));
        assert_eq!(
            reported_version("  Pane 99.0.0 ").as_deref(),
            Some("99.0.0")
        );
        for answer in ["", "Pane", "Pane ", "pane 0.2.0", "0.2.0"] {
            assert_eq!(reported_version(answer), None, "{answer}");
        }
    }

    #[test]
    fn the_arguments_name_the_old_version_and_the_process_waited_for() {
        assert_eq!(
            arguments("0.1.0", 42, false),
            ["--updated-from", "0.1.0", "--wait-pid", "42"]
        );
        assert_eq!(
            arguments("0.1.0", 42, true),
            ["--updated-from", "0.1.0", "--wait-pid", "42", "--hidden"]
        );
    }

    /// A started program a test drives: whether it has exited, and whether
    /// it was ended.
    #[derive(Default)]
    struct Fake {
        exited: bool,
        ended: bool,
    }

    impl Started for Fake {
        fn exited(&mut self) -> bool {
            self.exited || self.ended
        }

        fn end(&mut self) {
            self.ended = true;
        }
    }

    #[test]
    fn the_report_of_the_started_program_is_read_and_removed() {
        let folder = tempfile::tempdir().unwrap();
        let mut started = Fake::default();
        // Nothing reported: the wait ends without it.
        let silent = wait_for_started(folder.path(), 7, &mut started, Duration::from_millis(100));
        assert!(silent.unwrap_err().contains("did not report"));
        // A report for another process is not this one's.
        fs::write(report_file(folder.path(), 8), b"9").unwrap();
        assert!(
            wait_for_started(folder.path(), 7, &mut started, Duration::from_millis(100)).is_err()
        );
        // This one's is read, and goes.
        report_started(folder.path(), 7).unwrap();
        wait_for_started(folder.path(), 7, &mut started, Duration::from_secs(5)).unwrap();
        assert!(!report_file(folder.path(), 7).exists());
        // A program that exited without reporting is not waited for.
        let mut gone = Fake {
            exited: true,
            ..Fake::default()
        };
        let exited = wait_for_started(folder.path(), 7, &mut gone, Duration::from_secs(30));
        assert_eq!(exited.unwrap_err(), "it exited as it started");
    }

    /// A process table where the listed processes run until a test ends
    /// them.
    #[derive(Default)]
    struct Table(Mutex<Vec<u32>>);

    impl ProcessTable for Table {
        fn look_up(&self, process: u32) -> Liveness {
            if self.0.lock().unwrap().contains(&process) {
                Liveness::Running { started: None }
            } else {
                Liveness::Ended
            }
        }
    }

    #[test]
    fn a_process_that_exited_is_not_waited_for() {
        let table = Table::default();
        assert_eq!(
            wait_for_previous_in(&table, 4242, &[], Duration::from_secs(30)),
            Previous::Exited
        );
    }

    #[test]
    fn a_process_running_another_program_is_left_alone() {
        // This test's own process runs past the wait, and runs the test
        // program, not Pane's: it is left alone.
        let own = std::process::id();
        let table = Table(Mutex::new(vec![own]));
        let programs = [
            PathBuf::from("/nowhere/pane"),
            PathBuf::from("/nowhere/pane.old"),
        ];
        let started = Instant::now();
        assert_eq!(
            wait_for_previous_in(&table, own, &programs, Duration::from_millis(200)),
            Previous::LeftAlone
        );
        assert!(started.elapsed() >= Duration::from_millis(200));
    }
}
