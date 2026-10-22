//! The extension runtime's epoch ticker and watchdog wait while no guest
//! call runs (#190), through the launcher's public interface, with the
//! settings sample from `cargo xtask guests`. With nothing in flight both
//! wait, and neither ticks nor looks; a call starting wakes both, they tick
//! and look while it runs, and they wait again once it was answered; a
//! continuing service's cycle, which Pane sends itself, wakes them the same
//! way, with the service sample. Every request sent to the runtime thread
//! counts as in flight (`supervisor`'s `send`), so a search, a schedule's
//! run and closing a view are not checked one by one here. What
//! they do while a call runs (stopping a computing guest after its limit,
//! giving up on a stuck thread, timing schedules and services) is checked,
//! unchanged, by the `unresponsive`, `pausing`, `stopping`, `runtime_crash`,
//! `schedules` and `services` tests; the race between a call starting and
//! the ticker going to wait by `deadlines`' own tests.
//!
//! The hook that reports the timers exists in debug builds only.

#![cfg(debug_assertions)]

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status, Timers};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, to_root};

/// Generous, for a loaded machine.
const LONG: Duration = Duration::from_secs(120);

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

fn timers(runtime: &Runtime) -> Timers {
    runtime.timers().expect("the runtime runs")
}

/// Waits until the runtime's timers are as `done` says, failing with `what`
/// after a long while; what they were then.
fn until(runtime: &Runtime, what: &str, done: impl Fn(&Timers) -> bool) -> Timers {
    let started = Instant::now();
    loop {
        let timers = timers(runtime);
        if done(&timers) {
            return timers;
        }
        assert!(started.elapsed() < LONG, "{what}: {timers:?}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// No call in flight, and both timers waiting for one.
fn waiting(timers: &Timers) -> bool {
    timers.calls == 0 && timers.ticker_waiting && timers.watchdog_waiting
}

#[test]
fn the_runtime_timers_wait_while_no_call_runs_and_wake_for_one() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let runtime = Runtime::start().unwrap();
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
    let folder = package("sample-settings", &sources.path().join("sample-settings"));
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Settings sample".into())
    );

    // Nothing in flight: both wait, and neither ticks nor looks.
    let quiet = until(&runtime, "the timers never waited", waiting);
    thread::sleep(Duration::from_millis(500));
    let still = timers(&runtime);
    assert!(waiting(&still), "{still:?}");
    assert_eq!(
        (still.ticks, still.looks),
        (quiet.ticks, quiet.looks),
        "they ran with no call in flight"
    );

    // Opening the command is a call: each wakes, ticks or looks at least
    // once, and waits again once it was answered.
    to_root(&launcher);
    select_title(&launcher, "Greeting");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
    let opened = until(
        &runtime,
        "the timers did not wait again once the command was open",
        waiting,
    );
    assert!(
        opened.ticks > still.ticks && opened.looks > still.looks,
        "the call did not wake them: {still:?}, then {opened:?}"
    );

    // A call waiting on a clock for 10 seconds is in flight all along: they
    // tick every 10 ms and look every 100 ms meanwhile.
    select_title(&launcher, "Save after waiting");
    let slow = {
        let launcher = launcher.clone();
        thread::spawn(move || block_on(launcher.activate_selected()))
    };
    let running = until(&runtime, "the timers did not wake for the call", |timers| {
        timers.calls > 0 && !timers.ticker_waiting && !timers.watchdog_waiting
    });
    let later = until(
        &runtime,
        "the timers did not tick and look while the call ran",
        |timers| timers.ticks >= running.ticks + 10 && timers.looks >= running.looks + 2,
    );
    assert!(
        later.calls > 0 && !later.ticker_waiting && !later.watchdog_waiting,
        "{later:?}"
    );
    slow.join().unwrap();
    assert_eq!(
        shown(&launcher),
        Status::Result("Saved after waiting 10 seconds".into())
    );

    // Answered: they wait again, and stay waiting.
    let answered = until(
        &runtime,
        "the timers did not wait again once the call was answered",
        waiting,
    );
    thread::sleep(Duration::from_millis(500));
    let after = timers(&runtime);
    assert!(waiting(&after), "{after:?}");
    assert_eq!(
        (after.ticks, after.looks),
        (answered.ticks, answered.looks),
        "they ran after the call was answered"
    );
}

/// How many cycles the Service sample installed from `folder` has run, kept
/// in its content in `data`, or 0 before the first.
fn cycles(data: &Path, folder: &Path) -> u64 {
    let Ok(text) = fs::read_to_string(data.join("extensions/content.json")) else {
        return 0;
    };
    let Ok(file) = serde_json::from_str::<serde_json::Value>(&text) else {
        return 0; // replaced at that moment: read again
    };
    let key = PackageIdentity::local(folder).unwrap().key();
    file["packages"][&key]["cycles"]
        .as_str()
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

/// A continuing service's cycle is a request in flight too, sent by Pane
/// itself rather than by the user: the timers wake for each cycle (the
/// Service sample asks for one a second, by the system's clock here) and
/// wait again between them.
#[test]
fn the_runtime_timers_wake_for_each_service_cycle_and_wait_between_them() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let runtime = Runtime::start().unwrap();
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
    let folder = package("sample-service", &sources.path().join("sample-service"));
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Service sample".into())
    );

    // Between two cycles: both wait.
    let between = until(&runtime, "the timers never waited between cycles", waiting);
    let counted = cycles(data.path(), &folder);
    // The next cycle wakes both: they tick and look.
    let woken = until(
        &runtime,
        "a service cycle did not wake the timers",
        |timers| timers.ticks > between.ticks && timers.looks > between.looks,
    );
    // Answered, they wait again until the cycle after it.
    until(
        &runtime,
        "the timers did not wait again once the cycle was answered",
        |timers| waiting(timers) && timers.ticks >= woken.ticks,
    );
    let started = Instant::now();
    while cycles(data.path(), &folder) <= counted {
        assert!(
            started.elapsed() < LONG,
            "no service cycle ran while the timers woke"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
