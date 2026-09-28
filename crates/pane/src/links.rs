//! The system's handler for web links, which opens quicklinks and other
//! `open-url` results with the commands the `open` crate names for this
//! system: `/usr/bin/open` on macOS; PowerShell's `Start-Process` (then
//! `explorer.exe`) on Windows; `xdg-open` (then `gio`, `gnome-open`,
//! `kde-open`) on Linux.
//!
//! A handler that is still running after [`SETTLE`] counts as having opened
//! the link: `xdg-open` outside a desktop session runs the browser itself
//! and returns only when it exits, so waiting for it would leave Pane
//! "running" as long as the browser is open.

use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use pane_core::LinkOpener;

/// How long a handler may take to report that it could not open a link.
const SETTLE: Duration = Duration::from_secs(3);

/// Opens web links with the system's default handler, normally the default
/// browser, and explains a missing handler or a refusal.
pub struct SystemLinks;

impl LinkOpener for SystemLinks {
    fn open(&self, url: &str) -> Result<(), String> {
        let mut missing = None;
        for mut command in open::commands(url) {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            match command.spawn() {
                Ok(child) => return settle(&command, child),
                // Not installed: try the next handler.
                Err(error) => missing = Some(error),
            }
        }
        Err(match missing {
            Some(error) if error.kind() != io::ErrorKind::NotFound => {
                format!("the system could not start its link handler ({error})")
            }
            _ => "no program to open web links is installed".into(),
        })
    }
}

/// Waits up to [`SETTLE`] for `child`, the handler `command` started, to
/// report failure; one still running then is left to finish on its own.
fn settle(command: &Command, mut child: Child) -> Result<(), String> {
    let deadline = Instant::now() + SETTLE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(explain(command, status)),
            Ok(None) if Instant::now() >= deadline => {
                // Reaped when it exits.
                thread::spawn(move || child.wait());
                return Ok(());
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("the system's link handler failed ({error})")),
        }
    }
}

/// Why the handler `command` exited with `status`, for the user.
fn explain(command: &Command, status: ExitStatus) -> String {
    // xdg-open documents exit status 3 for "a required tool could not be
    // found" and 4 for "the action failed".
    if command.get_program() == "xdg-open" {
        match status.code() {
            Some(3) => {
                return "no program to open web links is set up (xdg-open found none)".into();
            }
            Some(4) => return "the browser or link handler refused or failed to open it".into(),
            _ => {}
        }
    }
    format!("the system's link handler did not open it ({status})")
}
