//! `pane-echo`, the helper sample's native helper and the programs
//! sample's system program: a plain program Pane runs for the samples'
//! commands. It reads its input from standard input and answers on
//! standard output, naming the target it was built for (as `pane-target`
//! names it), so the answer shows which of the package's files ran.
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
//!
//! For the programs sample (the modes below read no input unless they say
//! so, and beat and are released in the folder of the program itself, not
//! the working folder):
//!
//! - `--status <code>`: reads its input, writes `out: <input>` to standard
//!   output and `err: <input>` to standard error, and exits with `<code>`;
//! - `--where`: writes its own absolute path;
//! - `--args <arg>...`: writes each argument after `--args` on a line of its
//!   own, in brackets, exactly as it arrived;
//! - `--lines <n>`: writes `progress <k>/<n>` lines, a tenth of a second
//!   apart;
//! - `--echo-lines`: answers each line of its input with `got <line>`
//!   until its input ends;
//! - `--hold <seconds> <name>`: holds for that long, beating in
//!   `pane-echo.<name>.alive` every 20 ms (released early by
//!   `pane-echo.release`), and removes the file when it finishes;
//! - `--parent <seconds>`: starts `pane-echo --hold <seconds> descendant`,
//!   writes `started a descendant`, then holds itself as `parent`;
//! - `--leave <seconds>`: starts `pane-echo --hold <seconds> descendant`,
//!   which keeps its output open, writes `left a descendant` and exits;
//! - `--flood-mib <n>`: writes `<n>` MiB to standard output;
//! - `--context <name>`: writes `in <folder>; <name>=<value>`, the name of
//!   its working folder and the variable's value (`(unset)` without one).

use std::fs::OpenOptions;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use pane_target::Target;

/// Where `--wait` shows that it is still running, in the working folder.
const ALIVE: &str = "pane-echo.alive";

/// What ends `--wait` early, once it appears in the working folder.
const RELEASE: &str = "pane-echo.release";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    // The programs sample's modes, which read no input unless they say so.
    match words.as_slice() {
        ["--status", code] => return status(code),
        ["--where"] => return answer(own_path().display().to_string()),
        ["--args", rest @ ..] => {
            let lines: Vec<String> = rest.iter().map(|arg| format!("[{arg}]\n")).collect();
            return answer(lines.concat());
        }
        ["--lines", count] => return lines(count),
        ["--echo-lines"] => return echo_lines(),
        ["--hold", seconds, name] => {
            return match seconds.parse::<u64>() {
                Ok(seconds) => {
                    hold(Duration::from_secs(seconds), name);
                    ExitCode::SUCCESS
                }
                Err(_) => usage(&format!("--hold needs whole seconds, not {seconds}")),
            };
        }
        ["--parent", seconds] => return parent(seconds),
        ["--leave", seconds] => return leave(seconds),
        ["--flood-mib", count] => return flood_mib(count),
        ["--context", name] => return context(name),
        _ => {}
    }
    let mut input = String::new();
    if let Err(error) = io::stdin().read_to_string(&mut input) {
        eprintln!("pane-echo could not read its input: {error}");
        return ExitCode::from(2);
    }
    match words.as_slice() {
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

/// Explains wrong arguments and exits with code 2.
fn usage(problem: &str) -> ExitCode {
    eprintln!("pane-echo: {problem}");
    ExitCode::from(2)
}

/// Writes `text` to standard output and succeeds.
fn answer(text: String) -> ExitCode {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(2),
    }
}

/// This program's own absolute path.
fn own_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pane-echo"))
}

/// The folder this program is in, where its programs-sample modes beat
/// and are released.
fn own_folder() -> PathBuf {
    own_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `--status <code>`.
fn status(code: &str) -> ExitCode {
    let Ok(code) = code.parse::<u8>() else {
        return usage(&format!("--status needs a code from 0 to 255, not {code}"));
    };
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return ExitCode::from(2);
    }
    let input = input.trim();
    print!("out: {input}");
    eprint!("err: {input}");
    ExitCode::from(code)
}

/// `--lines <n>`.
fn lines(count: &str) -> ExitCode {
    let Ok(count) = count.parse::<u32>() else {
        return usage(&format!("--lines needs a count, not {count}"));
    };
    let mut out = io::stdout().lock();
    for line in 1..=count {
        if writeln!(out, "progress {line}/{count}")
            .and_then(|()| out.flush())
            .is_err()
        {
            return ExitCode::from(2);
        }
        if line < count {
            thread::sleep(Duration::from_millis(100));
        }
    }
    ExitCode::SUCCESS
}

/// `--echo-lines`.
fn echo_lines() -> ExitCode {
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            return ExitCode::from(2);
        };
        if writeln!(out, "got {}", line.trim_end_matches('\r'))
            .and_then(|()| out.flush())
            .is_err()
        {
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// `--parent <seconds>`.
fn parent(seconds: &str) -> ExitCode {
    let Ok(held) = seconds.parse::<u64>() else {
        return usage(&format!("--parent needs whole seconds, not {seconds}"));
    };
    if let Err(code) = descendant(seconds, "started a descendant") {
        return code;
    }
    hold(Duration::from_secs(held), "parent");
    ExitCode::SUCCESS
}

/// `--leave <seconds>`: what it starts outlives it.
fn leave(seconds: &str) -> ExitCode {
    if seconds.parse::<u64>().is_err() {
        return usage(&format!("--leave needs whole seconds, not {seconds}"));
    }
    match descendant(seconds, "left a descendant") {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

/// Starts `pane-echo --hold <seconds> descendant`, which keeps this
/// program's output open, and writes `said` once it beats.
fn descendant(seconds: &str, said: &str) -> Result<(), ExitCode> {
    let alive = own_folder().join("pane-echo.descendant.alive");
    let length = || std::fs::metadata(&alive).map_or(0, |metadata| metadata.len());
    let before = length();
    // The descendant keeps this program's output open, as a program a tool
    // starts in turn often does.
    let started = Command::new(own_path())
        .args(["--hold", seconds, "descendant"])
        .stdin(Stdio::null())
        .spawn();
    if let Err(error) = started {
        return Err(usage(&format!("could not start a descendant: {error}")));
    }
    // It says so once the descendant runs: once it has beaten.
    let waiting = Instant::now();
    while length() <= before && waiting.elapsed() < Duration::from_secs(10) {
        thread::sleep(Duration::from_millis(5));
    }
    let mut out = io::stdout().lock();
    if writeln!(out, "{said}").and_then(|()| out.flush()).is_err() {
        return Err(ExitCode::from(2));
    }
    Ok(())
}

/// `--flood-mib <n>`.
fn flood_mib(count: &str) -> ExitCode {
    let Ok(count) = count.parse::<usize>() else {
        return usage(&format!("--flood-mib needs a count, not {count}"));
    };
    let block = vec![b'x'; 1 << 20];
    let mut out = io::stdout().lock();
    for _ in 0..count {
        if out.write_all(&block).is_err() {
            // Pane stopped reading.
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// `--context <name>`.
fn context(name: &str) -> ExitCode {
    let folder = std::env::current_dir()
        .ok()
        .and_then(|folder| {
            folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let value = std::env::var(name).unwrap_or_else(|_| "(unset)".into());
    answer(format!("in {folder}; {name}={value}"))
}

/// Holds for `duration`, or until [`RELEASE`] appears beside the program,
/// beating in `pane-echo.<name>.alive` beside it meanwhile.
fn hold(duration: Duration, name: &str) {
    let folder = own_folder();
    beat(
        duration,
        &folder.join(format!("pane-echo.{name}.alive")),
        &folder.join(RELEASE),
    );
}

/// Waits for `duration`, or until [`RELEASE`] appears, beating in [`ALIVE`]
/// meanwhile.
fn wait(duration: Duration) {
    beat(duration, Path::new(ALIVE), Path::new(RELEASE));
}

/// Waits for `duration`, or until `release` exists, appending a byte to
/// `alive_path` every 20 ms meanwhile, and removes it when done.
fn beat(duration: Duration, alive_path: &Path, release: &Path) {
    let started = Instant::now();
    let mut alive = OpenOptions::new()
        .create(true)
        .append(true)
        .open(alive_path)
        .ok();
    while started.elapsed() < duration && !release.exists() {
        if let Some(file) = &mut alive {
            let _ = file.write_all(b".");
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(alive);
    let _ = std::fs::remove_file(alive_path);
}
