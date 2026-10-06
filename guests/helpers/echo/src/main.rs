//! `pane-echo`, the helper sample's native helper: a plain program Pane
//! runs for the sample's command. It reads its input from standard input
//! and answers on standard output, naming the target it was built for (as
//! `pane-target` names it), so the answer shows which of the package's files
//! ran.
//!
//! - no arguments: answers `Echoed "<input>" on <system> <processor>`;
//! - `--wait <seconds>`: waits that long first, so that stopping it (by
//!   cancelling, disabling or reloading) can be seen. While it waits it
//!   appends a byte to `pane-echo.alive` in its working folder every 20 ms,
//!   and removes the file when it finishes, so a check can see that a
//!   stopped helper no longer runs without trusting a process id. It stops
//!   waiting early once a file `pane-echo.release` appears in that folder,
//!   so a test stands in for the clock instead of waiting the time out;
//! - `--fail`: writes an explanation to standard error and exits with code 3;
//! - `--flood`: writes more output than Pane passes back (2 MiB).

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use pane_target::Target;

/// Where `--wait` shows that it is still running, in the working folder.
const ALIVE: &str = "pane-echo.alive";

/// What ends `--wait` early, once it appears in the working folder.
const RELEASE: &str = "pane-echo.release";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = String::new();
    if let Err(error) = io::stdin().read_to_string(&mut input) {
        eprintln!("pane-echo could not read its input: {error}");
        return ExitCode::from(2);
    }
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => {}
        ["--wait", seconds] => match seconds.parse::<u64>() {
            Ok(seconds) => wait(Duration::from_secs(seconds)),
            Err(_) => {
                eprintln!("pane-echo: --wait needs a whole number of seconds, not {seconds}");
                return ExitCode::from(2);
            }
        },
        ["--fail"] => {
            eprintln!("pane-echo was asked to fail");
            return ExitCode::from(3);
        }
        ["--flood"] => {
            let line = [b'x'; 1024];
            let mut out = io::stdout().lock();
            for _ in 0..2048 {
                if out.write_all(&line).is_err() {
                    break;
                }
            }
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("pane-echo: unknown arguments {other:?}");
            return ExitCode::from(2);
        }
    }
    let system = Target::current().map_or_else(
        || format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        |target| target.to_string(),
    );
    print!("Echoed \"{}\" on {system}", input.trim());
    ExitCode::SUCCESS
}

/// Waits for `duration`, or until [`RELEASE`] appears, beating in [`ALIVE`]
/// meanwhile.
fn wait(duration: Duration) {
    let started = Instant::now();
    let mut alive = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ALIVE)
        .ok();
    while started.elapsed() < duration && !std::path::Path::new(RELEASE).exists() {
        if let Some(file) = &mut alive {
            let _ = file.write_all(b".");
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(alive);
    let _ = std::fs::remove_file(ALIVE);
}
