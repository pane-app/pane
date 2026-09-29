//! The system's handlers for web links and files, which open quicklinks and
//! other `open-url` results, and the files of `open-file` results, with the
//! commands the `open` crate names for this system: `/usr/bin/open` on
//! macOS; PowerShell's `Start-Process` (then `explorer.exe`) on Windows;
//! `xdg-open` (then `gio`, `gnome-open`, `kde-open`) on Linux. None runs
//! through a shell. On Windows the target reaches PowerShell in an
//! environment variable, not on its command line, and an existing path (a
//! file Pane checked) opens with `Invoke-Item -LiteralPath`, a web link with
//! `Start-Process`.
//!
//! A handler that is still running after [`SETTLE`] counts as having opened
//! the link or file: `xdg-open` outside a desktop session runs the program
//! itself and returns only when it exits, so waiting for it would leave Pane
//! "running" as long as the browser or viewer is open.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use pane_core::LinkOpener;

/// How long a handler may take to report that it could not open a link.
const SETTLE: Duration = Duration::from_secs(3);

/// Opens web links with the system's default handler, normally the default
/// browser, and files with the handler for their type, and explains a
/// missing handler or a refusal.
pub struct SystemLinks;

/// What is being opened, for the user's messages.
#[derive(Clone, Copy)]
enum Opening {
    Link,
    File,
}

impl Opening {
    /// "web links" or "this kind of file".
    fn what(self) -> &'static str {
        match self {
            Opening::Link => "web links",
            Opening::File => "this kind of file",
        }
    }
}

impl LinkOpener for SystemLinks {
    fn open(&self, url: &str) -> Result<(), String> {
        open_with_system(url.as_ref(), Opening::Link)
    }

    fn open_file(&self, path: &Path) -> Result<(), String> {
        // The macOS and Windows smokes record the file instead of opening
        // it, since any program that opened it would be the user's own;
        // nothing else sets this, and a release build has no such hook.
        #[cfg(debug_assertions)]
        if let Some(log) = std::env::var_os("PANE_TEST_OPEN_FILE_LOG") {
            let line = format!("{}\n", path.display());
            return std::fs::write(&log, line)
                .map_err(|error| format!("the smoke's record could not be written ({error})"));
        }
        open_with_system(path.as_os_str(), Opening::File)
    }
}

/// Opens `target` with the first of the system's handlers that starts.
fn open_with_system(target: &OsStr, kind: Opening) -> Result<(), String> {
    let mut missing = None;
    for mut command in open::commands(target) {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(child) => return settle(&command, child, kind),
            // Not installed: try the next handler.
            Err(error) => missing = Some(error),
        }
    }
    Err(match missing {
        Some(error) if error.kind() != io::ErrorKind::NotFound => {
            format!("the system could not start its handler ({error})")
        }
        _ => format!("no program to open {} is installed", kind.what()),
    })
}

/// Waits up to [`SETTLE`] for `child`, the handler `command` started, to
/// report failure; one still running then is left to finish on its own.
fn settle(command: &Command, mut child: Child, kind: Opening) -> Result<(), String> {
    let deadline = Instant::now() + SETTLE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(explain(command, status, kind)),
            Ok(None) if Instant::now() >= deadline => {
                // Reaped when it exits.
                thread::spawn(move || child.wait());
                return Ok(());
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("the system's handler failed ({error})")),
        }
    }
}

/// Why the handler `command` exited with `status`, for the user.
fn explain(command: &Command, status: ExitStatus, kind: Opening) -> String {
    // xdg-open documents exit status 3 for "a required tool could not be
    // found" and 4 for "the action failed".
    if command.get_program() == "xdg-open" {
        match status.code() {
            Some(3) => {
                return format!(
                    "no program to open {} is set up (xdg-open found none)",
                    kind.what()
                );
            }
            Some(4) => {
                return match kind {
                    Opening::Link => "the browser or link handler refused or failed to open it",
                    Opening::File => {
                        "the program for this kind of file refused or failed to open it"
                    }
                }
                .into();
            }
            _ => {}
        }
    }
    let handler = match kind {
        Opening::Link => "link handler",
        Opening::File => "handler for files",
    };
    format!("the system's {handler} did not open it ({status})")
}
