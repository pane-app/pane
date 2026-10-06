//! Scheduled work through the launcher's public interface, with the
//! Schedule sample in Rust, JavaScript and TypeScript, real guests `cargo
//! xtask guests` assembles, and a clock the tests set: a command whose
//! `pane.json` entry declares a schedule runs its item's action every
//! interval while its package is enabled and not paused — installation alone activates nothing until a
//! run is due, and nothing of other packages; the answer of a run shows in
//! a toast on the command's screen, and an error the extension answers with
//! shows as a failure toast; disabling stops a run still pending, without replaying the ticks
//! that passed meanwhile, and its late answer is discarded; a restart
//! schedules again whatever the manifest declares, from a full interval,
//! without replaying work that fell due while Pane was stopped; and a run
//! that traps is a crash like any action, so three within five minutes
//! pause the package and stop the schedule, and a run that computes
//! without yielding is stopped by Pane and counted the same way. An
//! impossible schedule in the manifest is refused before anything is
//! installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::{Launcher, Limits, PackageIdentity, Runtime, Screen, Status, Unavailable};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles, to_root};

/// One language's Schedule sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    /// Its package's title.
    title: &'static str,
    /// Its command's title in root search.
    command: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-schedule",
    component: "sample_schedule.wasm",
    title: "Schedule sample",
    command: "Counting",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-schedule-js",
    component: "sample_schedule_js.wasm",
    title: "JavaScript schedule sample",
    command: "Counting (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-schedule-ts",
    component: "sample_schedule_ts.wasm",
    title: "TypeScript schedule sample",
    command: "Counting (TypeScript)",
};

/// How long the scheduler and a run may take: compiling the guest once is
/// included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(8);

/// How long the "Run slowly" item waits after it began: within this, a
/// second run cannot have begun while the first still runs (the count,
/// which every run raises when it begins, proves it).
const SLOW_RUNS: Duration = Duration::from_secs(10);

const SECOND: Duration = Duration::from_secs(1);

/// One test's Pane: its data folder (outliving restarts), its source
/// folders and the clock its schedules follow.
struct Pane {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
    /// Pane's clock for scheduled work and clipboard expiry, which moves
    /// only when a test advances it. It starts a year ahead of the
    /// system's, which the launcher uses before it is given this one.
    clock: Arc<ManualClock>,
}

impl Pane {
    fn new() -> Pane {
        Pane {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
            clock: ManualClock::at(SystemClock.now() + 365 * 86_400_000),
        }
    }

    /// Starts Pane on this data folder, as after a restart: a fresh runtime
    /// (keeping compiled code in the cache, as Pane's does) and the clock
    /// this test moves.
    fn start(&self) -> Started {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        let launcher = Launcher::with_packages(
            Ok(runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_clock(self.clock.clone());
        Started { runtime, launcher }
    }

    /// Starts Pane with `fixture`'s sample installed, its schedule
    /// running `item` every `every` seconds.
    fn installed(
        &self,
        fixture: &Fixture,
        item: &str,
        every: u64,
    ) -> (Started, PackageIdentity, PathBuf) {
        let started = self.start();
        let folder = self.package(fixture, item, every);
        block_on(started.launcher.install_package(&folder));
        assert_eq!(
            started.launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        // Let the scheduler see the installed package before the clock
        // moves: the interval begins when it sees the code may run.
        self.settled(&started);
        (started, identity, folder)
    }

    /// The assembled sample of `fixture` copied into a source folder of
    /// this test, its command's schedule set to run `item` every `every`
    /// seconds.
    fn package(&self, fixture: &Fixture, item: &str, every: u64) -> PathBuf {
        let folder = self.sources.path().join(fixture.package);
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(fixture.package);
        assert!(
            assembled.exists(),
            "{} is missing; run `cargo xtask guests`",
            assembled.display()
        );
        fs::create_dir_all(&folder).unwrap();
        let manifest = fs::read_to_string(assembled.join("pane.json")).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        manifest["commands"][0]["schedule"] =
            serde_json::json!({ "everySeconds": every, "item": item });
        fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
        fs::copy(
            assembled.join(fixture.component),
            folder.join(fixture.component),
        )
        .unwrap();
        folder
    }

    /// The settings sample, installed to stand for an unrelated package
    /// that declares no scheduled work.
    fn settings_package(&self) -> PathBuf {
        let folder = self.sources.path().join("settings");
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/sample-settings");
        fs::create_dir_all(&folder).unwrap();
        for file in ["pane.json", "sample_settings.wasm"] {
            fs::copy(assembled.join(file), folder.join(file)).unwrap();
        }
        folder
    }

    /// How many runs the Schedule sample has counted, kept in its content,
    /// or `None` before the first.
    fn runs(&self, folder: &Path) -> Option<u64> {
        let text = fs::read_to_string(self.content_file()).ok()?;
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let key = PackageIdentity::local(folder).unwrap().key();
        file["packages"][&key]["count"]
            .as_str()
            .and_then(|count| count.parse().ok())
    }

    /// What "Run slowly" saved last: "started", "finished" or nothing.
    fn slow_save(&self) -> Option<String> {
        let text = fs::read_to_string(self.settings_file()).ok()?;
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"slow\": \"{progress}\"")))
            .map(str::to_owned)
    }

    fn content_file(&self) -> PathBuf {
        self.data.path().join("extensions/content.json")
    }

    fn settings_file(&self) -> PathBuf {
        self.data.path().join("extensions/settings.json")
    }

    /// Waits until the scheduler has looked at every change of the clock
    /// and of the packages so far, and every run it started has reported.
    fn settled(&self, started: &Started) {
        assert!(
            started.launcher.wait_for_schedules(PROMPTLY),
            "the scheduler did not settle"
        );
    }

    /// Waits until `read` returns `expected`, saying `what` it was.
    fn until<T: PartialEq + std::fmt::Debug>(
        &self,
        what: &str,
        expected: T,
        mut read: impl FnMut() -> T,
    ) {
        let began = Instant::now();
        loop {
            let found = read();
            if found == expected {
                return;
            }
            assert!(
                began.elapsed() < SLOW_RUNS + PROMPTLY,
                "waiting for {what}: found {found:?}, expected {expected:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A started Pane: its launcher and the runtime it runs extensions with.
struct Started {
    runtime: Runtime,
    launcher: Launcher,
}

impl Started {
    /// Whether any component of the installed package `identity` runs.
    fn is_running(&self, identity: &PackageIdentity) -> bool {
        let location = self
            .launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == *identity)
            .unwrap()
            .location;
        block_on(self.runtime.running())
            .iter()
            .any(|component| component.starts_with(&location))
    }
}

fn row(launcher: &Launcher, title: &str) -> pane_core::Row {
    launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
}

/// Opens the sample's command from root search.
fn open(launcher: &Launcher, command: &str) {
    to_root(launcher);
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
}

/// Activates the extension manager's row titled `title`, returning the
/// outcome.
fn press(launcher: &Launcher, title: &str) -> Status {
    manage(launcher);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    launcher.view().status
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// Whether root search lists the command as paused rather than runnable.
fn is_paused(launcher: &Launcher, command: &str) -> bool {
    to_root(launcher);
    row(launcher, command).unavailable.is_some()
}

fn installation_activates_nothing_until_a_run_is_due_and_only_its_own_package(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "count", 2);
    // An unrelated package that declares no scheduled work.
    let settings = pane.settings_package();
    block_on(started.launcher.install_package(&settings));
    let settings_identity = PackageIdentity::local(&settings).unwrap();

    // Before the first interval, nothing runs: installation alone activates
    // no executable code.
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), None);
    assert!(!started.is_running(&identity));
    assert!(!started.is_running(&settings_identity));

    // The interval passes: the scheduled item's action runs, activating its
    // own package and no other.
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));
    assert!(started.is_running(&identity));
    assert!(!started.is_running(&settings_identity));

    // Each further interval runs it again.
    pane.clock.advance(2 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(2));
}

fn the_answer_of_a_scheduled_run_shows_on_the_command_screen(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture, "count", 2);

    open(&started.launcher, fixture.command);
    pane.clock.advance(2 * SECOND);
    pane.settled(&started);
    assert_eq!(
        shown(&started.launcher),
        Status::Result("Ran 1 times".into())
    );
    assert_eq!(started.launcher.view().screen, Screen::Command);
    assert_eq!(pane.runs(&folder), Some(1));
    // The command's list was asked for again once the run answered, as after
    // an action the user chose, so it shows what the run did.
    assert_eq!(started.launcher.view().title, "Ran 1 times");

    // Opened again, it shows the same.
    open(&started.launcher, fixture.command);
    assert_eq!(started.launcher.view().title, "Ran 1 times");
}

fn an_error_a_scheduled_run_answers_with_shows_as_one(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, _) = pane.installed(fixture, "refuse", 1);

    open(&started.launcher, fixture.command);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        shown(&started.launcher),
        Status::Error(
            "The extension reported an error: The schedule sample refuses, to show how an error looks".into()
        )
    );
    // An error the extension answers with never pauses it.
    assert!(!is_paused(&started.launcher, fixture.command));
}

fn a_disabled_package_is_never_scheduled_and_enabling_starts_it_again(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "count", 2);
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);

    // Ticks pass while the package is disabled: none of them runs, and
    // nothing of the package is activated.
    pane.clock.advance(10 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), None);
    assert!(!started.is_running(&identity));

    // Enabling it starts the schedule again, from a full interval.
    block_on(started.launcher.set_enabled(&identity, true));
    pane.settled(&started);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), None);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));
}

fn disabling_stops_a_pending_run_and_discards_its_answer(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "slow", 2);

    pane.clock.advance(2 * SECOND);
    pane.until("the run to begin", Some(1), || pane.runs(&folder));
    pane.until(
        "the run to save “started”",
        Some("started".into()),
        || pane.slow_save(),
    );

    // Disabling the package ends its generation, which stops the run: it
    // never saves "finished".
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
    assert_eq!(pane.runs(&folder), Some(1));
    // The disable's own outcome stays: the stopped run's late answer is
    // discarded, not shown over it.
    assert_eq!(
        started.launcher.view().status,
        Status::Result(format!("Disabled {}", fixture.title))
    );

    // Ticks pass while the package is disabled: no further run.
    pane.clock.advance(10 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
}

fn ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "slow", 1);

    pane.clock.advance(SECOND);
    pane.until("the run to begin", Some(1), || pane.runs(&folder));

    // Three more ticks fall due while the first run waits: no second run
    // is asked for, so the count, which every run raises when it begins,
    // stays at one.
    pane.clock.advance(3 * SECOND);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(pane.runs(&folder), Some(1));

    // Once the first run answers, the ticks that passed meanwhile are one
    // run, at the next tick.
    pane.until("the coalesced run to begin", Some(2), || pane.runs(&folder));
    // Stopping the package ends that run too, so it saves no "finished"
    // of its own: whatever the first run left stands, and no third run
    // begins.
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(2));
    pane.clock.advance(10 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(2));
}

fn a_scheduled_run_does_not_block_the_launcher(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "slow", 2);

    pane.clock.advance(2 * SECOND);
    pane.until("the run to begin", Some(1), || pane.runs(&folder));
    pane.until(
        "the run to save “started”",
        Some("started".into()),
        || pane.slow_save(),
    );

    // While the run waits inside the guest, on its own thread, the
    // launcher answers the user at once.
    let began = Instant::now();
    block_on(started.launcher.set_query("anything"));
    started.launcher.back();
    assert!(began.elapsed() < SECOND, "took {:?}", began.elapsed());
    assert!(matches!(
        started.launcher.view().screen,
        Screen::Root { .. }
    ));
    // Ending the package's generation (disabling it) stops the pending
    // run, which saves nothing more.
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
}

fn a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped(
    fixture: &Fixture,
) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture, "count", 2);
    pane.clock.advance(2 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));
    drop(started);

    // Ten intervals pass while Pane is stopped: none of them is replayed
    // when it starts again.
    pane.clock.advance(20 * SECOND);
    let restarted = pane.start();
    pane.settled(&restarted);
    assert_eq!(pane.runs(&folder), Some(1));

    // The manifest's declaration survives the restart: the schedule runs
    // again, from a full interval.
    pane.clock.advance(SECOND);
    pane.settled(&restarted);
    assert_eq!(pane.runs(&folder), Some(1));
    pane.clock.advance(SECOND);
    pane.settled(&restarted);
    assert_eq!(pane.runs(&folder), Some(2));
}

fn a_package_disabled_before_a_restart_is_not_scheduled_after_it(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "count", 2);
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    drop(started);

    // The disabled state survives the restart, and its schedule with it:
    // nothing runs.
    pane.clock.advance(10 * SECOND);
    let restarted = pane.start();
    pane.settled(&restarted);
    assert_eq!(pane.runs(&folder), None);
    // Enabling it on the restarted Pane starts the schedule.
    block_on(restarted.launcher.set_enabled(&identity, true));
    pane.settled(&restarted);
    pane.clock.advance(SECOND);
    pane.settled(&restarted);
    assert_eq!(pane.runs(&folder), None);
    pane.clock.advance(SECOND);
    pane.settled(&restarted);
    assert_eq!(pane.runs(&folder), Some(1));
}

fn reloading_stops_a_pending_run_and_the_replacement_schedules_again(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "slow", 2);

    pane.clock.advance(2 * SECOND);
    pane.until("the run to begin", Some(1), || pane.runs(&folder));
    pane.until(
        "the run to save “started”",
        Some("started".into()),
        || pane.slow_save(),
    );

    // Reloading the package replaces its code, which ends the old code's
    // generation: the pending run stops and never saves "finished".
    block_on(started.launcher.reload(&identity));
    pane.settled(&started);
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
    // The reload's own outcome stays: the stopped run's late answer is
    // discarded, not shown over it.
    assert_eq!(
        started.launcher.view().status,
        Status::Result(format!("Reloaded {}", fixture.title))
    );

    // The replacement's schedule runs again, its interval restarting with
    // the new code. Its run is the slow one again: it begins, and stopping
    // the package's generation ends it too.
    pane.clock.advance(2 * SECOND);
    pane.until("the replacement's run to begin", Some(2), || {
        pane.runs(&folder)
    });
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(2));
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
}

fn uninstalling_stops_the_schedule(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "count", 2);
    pane.clock.advance(2 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));

    // Uninstalling ends the package's generation, with its schedule.
    manage(&started.launcher);
    select_title(&started.launcher, &format!("Uninstall {}", fixture.title));
    block_on(started.launcher.activate_selected());
    select_title(&started.launcher, "Uninstall and keep saved data");
    block_on(started.launcher.activate_selected());
    assert_eq!(
        started.launcher.view().status,
        Status::Result(format!(
            "Uninstalled {}; its settings and content are kept",
            fixture.title
        ))
    );
    pane.clock.advance(10 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(1));
    let location = started
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity)
        .map(|package| package.location);
    assert!(location.is_none());
    assert!(block_on(started.runtime.running()).is_empty());
}

fn three_scheduled_crashes_pause_the_package_and_stop_the_schedule(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "crash", 1);

    // A scheduled run that traps is a crash of the package, like an action
    // the user ran: the first two are reported; the third pauses it.
    for expected in [1, 2] {
        pane.clock.advance(SECOND);
        pane.settled(&started);
        assert_eq!(pane.runs(&folder), Some(expected));
        assert!(!is_paused(&started.launcher, fixture.command));
    }
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(3));
    let reason = match row(&started.launcher, fixture.command).unavailable {
        Some(Unavailable::Paused(reason)) => reason,
        other => panic!("expected a pause, got {other:?}"),
    };
    assert_eq!(
        reason,
        format!(
            "{} is paused after an error; retry it in Manage extensions",
            fixture.title
        )
    );

    // The pause ends the generation, with the schedule: ticks pass and
    // none of them runs.
    pane.clock.advance(5 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(3));
    assert!(!started.is_running(&identity));

    // Retrying it starts the code again in a new generation, with its
    // schedule and its crash count afresh.
    assert_eq!(
        press(&started.launcher, &format!("Retry {}", fixture.title)),
        Status::Result(format!("Started {}", fixture.title))
    );
    pane.settled(&started);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(4));
    assert!(!is_paused(&started.launcher, fixture.command));
}

fn a_scheduled_run_that_stops_responding_is_counted_as_a_crash(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture, "busy", 1);
    // A guest call may compute for half a second before Pane stops it
    // where it yields, as #18's unresponsive tests shorten the limit.
    started.runtime.set_limits(Limits {
        compute: Duration::from_millis(500),
        warn: Duration::from_secs(10),
        unresponsive: Duration::from_secs(30),
    });

    // A scheduled run that computes without waiting is stopped by Pane and
    // counted as a crash of the package, as an action's is: the third
    // within five minutes pauses it, which ends the schedule.
    for expected in [1, 2] {
        pane.clock.advance(SECOND);
        pane.settled(&started);
        assert_eq!(pane.runs(&folder), Some(expected));
        assert!(!is_paused(&started.launcher, fixture.command));
    }
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(3));
    assert!(is_paused(&started.launcher, fixture.command));
    let toast = error(started.launcher.view().status.clone());
    assert!(
        toast.starts_with(&format!(
            "{} stopped responding 3 times within 5 minutes and is paused",
            fixture.title
        )),
        "{toast}"
    );

    // The extension list says how it paused, and ticks that pass while it
    // is paused run nothing.
    manage(&started.launcher);
    let subtitle = row(&started.launcher, fixture.title).subtitle.unwrap();
    assert!(
        subtitle.starts_with("Enabled · Paused after not responding"),
        "{subtitle}"
    );
    pane.clock.advance(5 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.runs(&folder), Some(3));
    assert!(!started.is_running(&identity));
}

fn an_impossible_schedule_is_refused_before_anything_is_installed(fixture: &Fixture) {
    let pane = Pane::new();
    let cases = [
        (
            serde_json::json!({ "everySeconds": 0, "item": "count" }),
            "Invalid pane.json: the schedule of command `counting` has `everySeconds` 0, \
             below the 1-second minimum",
        ),
        (
            serde_json::json!({ "everySeconds": 31 * 86_400, "item": "count" }),
            "Invalid pane.json: the schedule of command `counting` has `everySeconds` 2678400, \
             above the 2592000-second maximum",
        ),
        (
            serde_json::json!({ "everySeconds": 60, "item": "" }),
            "Invalid pane.json: the schedule of command `counting` names no `item`; name the \
             item whose action the schedule runs, or make the command no-view (\"mode\": \
             \"no-view\") to have the schedule run the command itself",
        ),
        (
            serde_json::json!({ "everySeconds": 60 }),
            "Invalid pane.json: the schedule of command `counting` names no `item`; name the \
             item whose action the schedule runs, or make the command no-view (\"mode\": \
             \"no-view\") to have the schedule run the command itself",
        ),
        (
            serde_json::json!({ "everySeconds": 60, "item": "count", "at": "9:00" }),
            "Invalid pane.json: unknown field `at`, expected `everySeconds` or `item`",
        ),
        (
            serde_json::json!({ "everySeconds": 60, "item": "x".repeat(257) }),
            "Invalid pane.json: the `item` of the schedule of command `counting` is longer than \
             256 characters",
        ),
    ];
    for (schedule, message) in cases {
        let started = pane.start();
        let folder = pane
            .sources
            .path()
            .join(format!("{}-refused", fixture.package));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(fixture.package);
        let manifest = fs::read_to_string(assembled.join("pane.json")).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        manifest["commands"][0]["schedule"] = schedule.clone();
        fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
        fs::copy(
            assembled.join(fixture.component),
            folder.join(fixture.component),
        )
        .unwrap();
        block_on(started.launcher.install_package(&folder));
        assert_eq!(
            error(started.launcher.view().status.clone()),
            message,
            "for {schedule}"
        );
        // Nothing was installed, and nothing is scheduled.
        assert!(started.launcher.packages().is_empty());
        pane.clock.advance(60 * SECOND);
        pane.settled(&started);
    }
}

#[test]
fn installation_activates_nothing_until_a_run_is_due_and_only_its_own_package_in_rust() {
    installation_activates_nothing_until_a_run_is_due_and_only_its_own_package(&RUST);
}

#[test]
fn installation_activates_nothing_until_a_run_is_due_and_only_its_own_package_in_javascript() {
    installation_activates_nothing_until_a_run_is_due_and_only_its_own_package(&JAVASCRIPT);
}

#[test]
fn installation_activates_nothing_until_a_run_is_due_and_only_its_own_package_in_typescript() {
    installation_activates_nothing_until_a_run_is_due_and_only_its_own_package(&TYPESCRIPT);
}

#[test]
fn the_answer_of_a_scheduled_run_shows_on_the_command_screen_in_rust() {
    the_answer_of_a_scheduled_run_shows_on_the_command_screen(&RUST);
}

#[test]
fn the_answer_of_a_scheduled_run_shows_on_the_command_screen_in_javascript() {
    the_answer_of_a_scheduled_run_shows_on_the_command_screen(&JAVASCRIPT);
}

#[test]
fn the_answer_of_a_scheduled_run_shows_on_the_command_screen_in_typescript() {
    the_answer_of_a_scheduled_run_shows_on_the_command_screen(&TYPESCRIPT);
}

#[test]
fn an_error_a_scheduled_run_answers_with_shows_as_one_in_rust() {
    an_error_a_scheduled_run_answers_with_shows_as_one(&RUST);
}

#[test]
fn an_error_a_scheduled_run_answers_with_shows_as_one_in_javascript() {
    an_error_a_scheduled_run_answers_with_shows_as_one(&JAVASCRIPT);
}

#[test]
fn an_error_a_scheduled_run_answers_with_shows_as_one_in_typescript() {
    an_error_a_scheduled_run_answers_with_shows_as_one(&TYPESCRIPT);
}

#[test]
fn a_disabled_package_is_never_scheduled_and_enabling_starts_it_again_in_rust() {
    a_disabled_package_is_never_scheduled_and_enabling_starts_it_again(&RUST);
}

#[test]
fn a_disabled_package_is_never_scheduled_and_enabling_starts_it_again_in_javascript() {
    a_disabled_package_is_never_scheduled_and_enabling_starts_it_again(&JAVASCRIPT);
}

#[test]
fn a_disabled_package_is_never_scheduled_and_enabling_starts_it_again_in_typescript() {
    a_disabled_package_is_never_scheduled_and_enabling_starts_it_again(&TYPESCRIPT);
}

#[test]
fn disabling_stops_a_pending_run_and_discards_its_answer_in_rust() {
    disabling_stops_a_pending_run_and_discards_its_answer(&RUST);
}

#[test]
fn disabling_stops_a_pending_run_and_discards_its_answer_in_javascript() {
    disabling_stops_a_pending_run_and_discards_its_answer(&JAVASCRIPT);
}

#[test]
fn disabling_stops_a_pending_run_and_discards_its_answer_in_typescript() {
    disabling_stops_a_pending_run_and_discards_its_answer(&TYPESCRIPT);
}

#[test]
fn ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many_in_rust() {
    ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many(&RUST);
}

#[test]
fn ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many_in_javascript() {
    ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many(&JAVASCRIPT);
}

#[test]
fn ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many_in_typescript() {
    ticks_that_fall_due_while_a_run_is_pending_start_one_run_not_many(&TYPESCRIPT);
}

#[test]
fn a_scheduled_run_does_not_block_the_launcher_in_rust() {
    a_scheduled_run_does_not_block_the_launcher(&RUST);
}

#[test]
fn a_scheduled_run_does_not_block_the_launcher_in_javascript() {
    a_scheduled_run_does_not_block_the_launcher(&JAVASCRIPT);
}

#[test]
fn a_scheduled_run_does_not_block_the_launcher_in_typescript() {
    a_scheduled_run_does_not_block_the_launcher(&TYPESCRIPT);
}

#[test]
fn a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped_in_rust() {
    a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped(&RUST);
}

#[test]
fn a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped_in_javascript()
{
    a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped(&JAVASCRIPT);
}

#[test]
fn a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped_in_typescript()
{
    a_restart_reschedules_without_replaying_work_that_fell_due_while_pane_was_stopped(&TYPESCRIPT);
}

#[test]
fn a_package_disabled_before_a_restart_is_not_scheduled_after_it_in_rust() {
    a_package_disabled_before_a_restart_is_not_scheduled_after_it(&RUST);
}

#[test]
fn a_package_disabled_before_a_restart_is_not_scheduled_after_it_in_javascript() {
    a_package_disabled_before_a_restart_is_not_scheduled_after_it(&JAVASCRIPT);
}

#[test]
fn a_package_disabled_before_a_restart_is_not_scheduled_after_it_in_typescript() {
    a_package_disabled_before_a_restart_is_not_scheduled_after_it(&TYPESCRIPT);
}

#[test]
fn reloading_stops_a_pending_run_and_the_replacement_schedules_again_in_rust() {
    reloading_stops_a_pending_run_and_the_replacement_schedules_again(&RUST);
}

#[test]
fn reloading_stops_a_pending_run_and_the_replacement_schedules_again_in_javascript() {
    reloading_stops_a_pending_run_and_the_replacement_schedules_again(&JAVASCRIPT);
}

#[test]
fn reloading_stops_a_pending_run_and_the_replacement_schedules_again_in_typescript() {
    reloading_stops_a_pending_run_and_the_replacement_schedules_again(&TYPESCRIPT);
}

#[test]
fn uninstalling_stops_the_schedule_in_rust() {
    uninstalling_stops_the_schedule(&RUST);
}

#[test]
fn uninstalling_stops_the_schedule_in_javascript() {
    uninstalling_stops_the_schedule(&JAVASCRIPT);
}

#[test]
fn uninstalling_stops_the_schedule_in_typescript() {
    uninstalling_stops_the_schedule(&TYPESCRIPT);
}

#[test]
fn three_scheduled_crashes_pause_the_package_and_stop_the_schedule_in_rust() {
    three_scheduled_crashes_pause_the_package_and_stop_the_schedule(&RUST);
}

#[test]
fn three_scheduled_crashes_pause_the_package_and_stop_the_schedule_in_javascript() {
    three_scheduled_crashes_pause_the_package_and_stop_the_schedule(&JAVASCRIPT);
}

#[test]
fn three_scheduled_crashes_pause_the_package_and_stop_the_schedule_in_typescript() {
    three_scheduled_crashes_pause_the_package_and_stop_the_schedule(&TYPESCRIPT);
}

#[test]
fn a_scheduled_run_that_stops_responding_is_counted_as_a_crash_in_rust() {
    a_scheduled_run_that_stops_responding_is_counted_as_a_crash(&RUST);
}

#[test]
fn a_scheduled_run_that_stops_responding_is_counted_as_a_crash_in_javascript() {
    a_scheduled_run_that_stops_responding_is_counted_as_a_crash(&JAVASCRIPT);
}

#[test]
fn a_scheduled_run_that_stops_responding_is_counted_as_a_crash_in_typescript() {
    a_scheduled_run_that_stops_responding_is_counted_as_a_crash(&TYPESCRIPT);
}

#[test]
fn an_impossible_schedule_is_refused_before_anything_is_installed_in_rust() {
    an_impossible_schedule_is_refused_before_anything_is_installed(&RUST);
}

#[test]
fn an_impossible_schedule_is_refused_before_anything_is_installed_in_javascript() {
    an_impossible_schedule_is_refused_before_anything_is_installed(&JAVASCRIPT);
}

#[test]
fn an_impossible_schedule_is_refused_before_anything_is_installed_in_typescript() {
    an_impossible_schedule_is_refused_before_anything_is_installed(&TYPESCRIPT);
}
