//! Running the system's own programs for the system functions on macOS and
//! Linux (`open`, `xdg-open`, `gio`, `dbus-send`), never through a shell:
//! each argument reaches the program as it is.

use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long a handler may take to report that it could not open something;
/// one still running then counts as having opened it, as `pane`'s link
/// opener does (`xdg-open` outside a desktop session runs the program
/// itself, and returns only when it exits).
#[cfg_attr(target_os = "macos", allow(dead_code))]
const SETTLE: Duration = Duration::from_secs(3);

/// Runs `program` with `args` to its end: nothing when it succeeds, else
/// what it said on its standard error (or its exit status).
pub(super) fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| not_started(program, &error))?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let said = said.trim();
    Err(if said.is_empty() {
        format!("{program} failed ({})", output.status)
    } else {
        said.lines().last().unwrap_or(said).to_owned()
    })
}

/// Starts `program` with `args` and waits up to [`SETTLE`] for it to report
/// failure; one still running then is left to finish on its own.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) fn start(program: &str, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| not_started(program, &error))?;
    let deadline = Instant::now() + SETTLE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{program} did not open it ({status})")),
            Ok(None) if Instant::now() >= deadline => {
                // Reaped when it exits.
                thread::spawn(move || child.wait());
                return Ok(());
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("{program} failed ({error})")),
        }
    }
}

/// Why `program` did not start, for the user.
fn not_started(program: &str, error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        format!("{program} is not installed")
    } else {
        format!("{program} could not start ({error})")
    }
}
