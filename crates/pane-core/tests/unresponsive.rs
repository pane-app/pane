//! Recovering from an extension that stops responding (#18), through the
//! launcher's public interface, with real guests from `cargo xtask guests`.
//!
//! - The settings sample's "Stop responding" computes without waiting for
//!   up to a minute, in Rust, JavaScript and TypeScript. Pane stops the call
//!   after the compute limit, says so, keeps its saved data and counts it
//!   towards pausing the package: the third time pauses it, with Retry and
//!   the details. Meanwhile the launcher keeps answering the user, and the
//!   calculator, another extension, answers as soon as the call is stopped.
//! - A runtime thread stuck outside any guest (a fault injected in debug
//!   builds) is first said to be not responding yet, then given up on:
//!   nothing is named or paused, Manage extensions says the runtime stopped
//!   responding, a fresh thread runs the next call, and the stuck one, once
//!   it returns, saves nothing more. One that carries on before Pane gives
//!   up on it is left running.
//!
//! The runtime's limits are shortened ([`Limits`]), so nothing waits for
//! the real ones; waits are on what Pane shows, with generous deadlines.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, Limits, PackageIdentity, Row, Runtime, RuntimeFailure, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles, to_root};

const COMMAND: &str = "Greeting";
const BUSY: &str = "Stop responding";

/// Generous, for a loaded machine.
const LONG: Duration = Duration::from_secs(120);

/// The limits these tests run with: a guest call may compute for two
/// seconds; a runtime thread making no progress is said to be not
/// responding yet after one, and given up on after three.
fn limits() -> Limits {
    Limits {
        compute: Duration::from_secs(2),
        warn: Duration::from_secs(1),
        unresponsive: Duration::from_secs(3),
    }
}

/// A settings sample package: the same command in each language.
struct Fixture {
    package: &'static str,
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-settings",
    title: "Settings sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-settings-js",
    title: "JavaScript settings sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-settings-ts",
    title: "TypeScript settings sample",
};

/// Copies the assembled package `name` into `folder`.
fn package(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
    }
    folder.to_path_buf()
}

struct Pane {
    _sources: TempDir,
    data: TempDir,
    runtime: Runtime,
    launcher: Launcher,
    identity: PackageIdentity,
}

impl Pane {
    /// Pane with the settings sample of `fixture` and the calculator
    /// installed.
    fn new(fixture: &Fixture) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        runtime.set_limits(limits());
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
        let folder = package(fixture.package, &sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        let calculator = package("calculator", &sources.path().join("calculator"));
        block_on(launcher.install_package(&calculator));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        Pane {
            identity: PackageIdentity::local(&folder).unwrap(),
            _sources: sources,
            data,
            runtime,
            launcher,
        }
    }

    /// What the sample saved in its settings under `key`.
    fn saved(&self, key: &str) -> Option<String> {
        let path = self.data.path().join("extensions/settings.json");
        let text = fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["packages"][self.identity.key()][key]
            .as_str()
            .map(str::to_owned)
    }

    /// Waits until the sample saved `value` under `key`.
    fn until_saved(&self, key: &str, value: &str) {
        let started = Instant::now();
        while self.saved(key).as_deref() != Some(value) {
            assert!(started.elapsed() < LONG, "{key} was not saved as {value}");
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn row(launcher: &Launcher, title: &str) -> Row {
    launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
}

/// Opens the command and selects its item titled `item`.
fn open_at(launcher: &Launcher, item: &str) {
    to_root(launcher);
    select_title(launcher, COMMAND);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
    select_title(launcher, item);
}

/// Opens the command, runs its item titled `item` and returns the outcome:
/// its toast, or the status line.
fn run(launcher: &Launcher, item: &str) -> Status {
    open_at(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// Root search lists the calculator's answer to `6 * 7`.
fn calculator_answers(launcher: &Launcher) -> bool {
    to_root(launcher);
    block_on(launcher.set_query("6 * 7"));
    let answered = titles(launcher).iter().any(|title| title == "42");
    block_on(launcher.set_query(""));
    answered
}

fn a_guest_that_stops_responding_is_stopped_and_paused_the_third_time(fixture: &Fixture) {
    let pane = Pane::new(fixture);
    let launcher = &pane.launcher;
    let title = fixture.title;
    assert_eq!(
        run(launcher, "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );
    assert!(calculator_answers(launcher));

    // The first time, the user leaves the command and keeps working while
    // the guest computes: with no compute limit to speak of, the guest's
    // call ends only when Pane lowers it.
    pane.runtime.set_limits(Limits {
        compute: Duration::from_secs(3600),
        ..limits()
    });
    open_at(launcher, BUSY);
    let busy = {
        let launcher = launcher.clone();
        thread::spawn(move || block_on(launcher.activate_selected()))
    };
    pane.until_saved("busy", "started");
    manage(launcher);
    assert!(
        !busy.is_finished(),
        "Manage extensions waited for the guest"
    );
    to_root(launcher);
    // The call is stopped once the limit applies, and the calculator's
    // answer, which waited behind it, comes.
    pane.runtime.set_limits(limits());
    assert!(calculator_answers(launcher));
    busy.join().unwrap();
    assert_eq!(pane.saved("busy").as_deref(), Some("started"));

    // The second time, its error says what happened.
    let second = error(run(launcher, BUSY));
    assert!(
        second.starts_with("The extension stopped responding: it computed for 2 seconds"),
        "{second}"
    );
    assert!(row_is_runnable(launcher));

    // The third time pauses it.
    let toast = error(run(launcher, BUSY));
    assert!(
        toast.starts_with(&format!(
            "{title} stopped responding 3 times within 5 minutes and is paused"
        )),
        "{toast}"
    );
    assert!(!row_is_runnable(launcher));
    // It never finished, and nothing ran it again.
    assert_eq!(pane.saved("busy").as_deref(), Some("started"));
    assert_eq!(
        pane.saved("greeting-style").as_deref(),
        Some("formal"),
        "saved data is kept"
    );
    // The other extension still answers.
    assert!(calculator_answers(launcher));

    // Manage extensions explains it, with Retry.
    manage(launcher);
    let subtitle = row(launcher, title).subtitle.unwrap();
    assert!(
        subtitle.starts_with("Enabled · Paused after not responding"),
        "{subtitle}"
    );
    select_title(launcher, &format!("Why {title} is paused"));
    block_on(launcher.activate_selected());
    let view = launcher.view();
    let details = view.details();
    assert_eq!(
        details[0],
        format!("{title} stopped responding 3 times within 5 minutes.")
    );
    assert!(
        details.iter().any(|line| line.starts_with(
            "Stopped responding 3 times within 5 minutes; the last time: The extension stopped \
             responding: it computed for 2 seconds"
        )),
        "{details:?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Started {title}"))
    );
    assert_eq!(
        run(launcher, "Greet me"),
        Status::Result("Good day to you".into())
    );
}

/// Whether root search lists the command as runnable, not paused.
fn row_is_runnable(launcher: &Launcher) -> bool {
    to_root(launcher);
    row(launcher, COMMAND).unavailable.is_none()
}

#[test]
fn a_rust_guest_that_stops_responding_is_stopped_and_paused_the_third_time() {
    a_guest_that_stops_responding_is_stopped_and_paused_the_third_time(&RUST);
}

#[test]
fn a_javascript_guest_that_stops_responding_is_stopped_and_paused_the_third_time() {
    a_guest_that_stops_responding_is_stopped_and_paused_the_third_time(&JAVASCRIPT);
}

#[test]
fn a_typescript_guest_that_stops_responding_is_stopped_and_paused_the_third_time() {
    a_guest_that_stops_responding_is_stopped_and_paused_the_third_time(&TYPESCRIPT);
}

/// Faults are injected in debug builds only.
#[cfg(debug_assertions)]
#[test]
fn a_runtime_that_stops_responding_is_replaced_naming_no_extension() {
    use pane_core::{Fault, RuntimeStatus};

    let pane = Pane::new(&RUST);
    let launcher = &pane.launcher;
    open_at(launcher, "Save after waiting");
    let slow = {
        let launcher = launcher.clone();
        thread::spawn(move || block_on(launcher.activate_selected()))
    };
    pane.until_saved("slow-save", "started");

    pane.runtime.inject(Fault::Hang);

    slow.join().unwrap();
    assert!(matches!(
        pane.runtime.status(),
        RuntimeStatus::Restarted {
            failure: RuntimeFailure::Unresponsive,
            ..
        }
    ));
    let status = error(launcher.view().status);
    assert!(status.contains("stopped responding"), "{status}");
    assert_eq!(pane.runtime.abandoned_threads(), 1);

    // Nothing is paused or named; Manage extensions says what happened.
    assert!(row_is_runnable(launcher));
    manage(launcher);
    assert_eq!(
        row(launcher, "Why the extension runtime stopped")
            .subtitle
            .as_deref(),
        Some("Restarted after not responding · The error and its diagnostics")
    );
    assert_eq!(
        row(launcher, RUST.title)
            .subtitle
            .as_deref()
            .map(|s| s.starts_with("Enabled")),
        Some(true)
    );
    select_title(launcher, "Why the extension runtime stopped");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    let details = view.details();
    assert!(
        details[0].contains("made no progress for 3 seconds, so Pane gave up on it"),
        "{details:?}"
    );
    assert!(
        details
            .iter()
            .any(|line| line.contains("none is named or paused")),
        "{details:?}"
    );
    assert!(launcher.view().rows.is_empty(), "it was restarted");

    // A fresh thread runs the next call; the calculator answers too.
    assert_eq!(
        run(launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
    to_root(launcher);
    assert!(calculator_answers(launcher));

    // The stuck thread returns, and saves nothing: the guest's wait went
    // with it.
    pane.runtime.inject(Fault::Release);
    let released = Instant::now();
    while pane.runtime.abandoned_threads() > 0 {
        assert!(released.elapsed() < LONG, "the stuck thread did not end");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(pane.saved("slow-save").as_deref(), Some("started"));
    assert_eq!(pane.saved("greeting-style").as_deref(), Some("casual"));
}

/// A runtime thread that makes no progress for a while is said to be not
/// responding yet; once it carries on, before Pane gave up on it, the
/// status line says what it said before, nothing was stopped, and the call
/// it held answers.
#[cfg(debug_assertions)]
#[test]
fn a_runtime_not_responding_yet_says_so_and_is_left_running_when_it_carries_on() {
    use pane_core::{Fault, RuntimeStatus};

    let pane = Pane::new(&RUST);
    // Never given up on here.
    pane.runtime.set_limits(Limits {
        unresponsive: Duration::from_secs(3600),
        ..limits()
    });
    let launcher = &pane.launcher;
    open_at(launcher, "Save after waiting");
    let slow = {
        let launcher = launcher.clone();
        thread::spawn(move || block_on(launcher.activate_selected()))
    };
    pane.until_saved("slow-save", "started");
    let before = launcher.view().status;

    pane.runtime.inject(Fault::Hang);
    let started = Instant::now();
    let not_yet = loop {
        if let Status::Progress(text) = launcher.view().status {
            break text;
        }
        assert!(started.elapsed() < LONG, "{:?}", launcher.view().status);
        thread::sleep(Duration::from_millis(5));
    };
    assert!(
        not_yet.starts_with("Pane's extension runtime is not responding yet"),
        "{not_yet}"
    );

    pane.runtime.inject(Fault::Release);
    while launcher.view().status != before && !slow.is_finished() {
        assert!(started.elapsed() < LONG, "{:?}", launcher.view().status);
        thread::sleep(Duration::from_millis(5));
    }
    slow.join().unwrap();
    assert_eq!(
        shown(launcher),
        Status::Result("Saved after waiting 10 seconds".into())
    );
    assert_eq!(pane.saved("slow-save").as_deref(), Some("finished"));
    assert_eq!(pane.runtime.status(), RuntimeStatus::Running);
    assert_eq!(pane.runtime.abandoned_threads(), 0);
}
