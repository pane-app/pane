//! Continuing services through the launcher's public interface, with the
//! Service sample in Rust, JavaScript and TypeScript, real guests `cargo
//! xtask guests` assembles, and a clock the tests set: a command whose
//! `pane.json` entry declares a service runs its cycles while its package
//! is enabled and not paused — at no interval the manifest declares, since
//! each cycle answers the status to show and when to run the next — so
//! installing the package starts its service and no other package's code;
//! the status of a cycle shows on the command's screen, and an error the
//! extension answers with shows as one while the service carries on;
//! disabling stops a cycle still pending, without its late answer being
//! shown, and enabling starts the service again with a fresh task; a
//! restart starts the service again where the package is enabled; and a
//! cycle that traps, or stops responding, is a crash like any call, so
//! three within five minutes pause the package and stop the service. A
//! manifest that declares a service its component does not export is
//! refused before anything is installed.

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

/// One language's Service sample package.
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
    package: "sample-service",
    component: "sample_service.wasm",
    title: "Service sample",
    command: "Watching",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-service-js",
    component: "sample_service_js.wasm",
    title: "JavaScript service sample",
    command: "Watching (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-service-ts",
    component: "sample_service_ts.wasm",
    title: "TypeScript service sample",
    command: "Watching (TypeScript)",
};

/// How long the services thread and a cycle may take: compiling the guest
/// once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(8);

/// How long a waiting cycle waits after it began: within this, a second
/// cycle cannot have begun while the first still runs (the cycle count,
/// which every cycle raises when it begins, proves it).
const SLOW_CYCLES: Duration = Duration::from_secs(10);

const SECOND: Duration = Duration::from_secs(1);

/// One test's Pane: its data folder (outliving restarts), its source
/// folders and the clock its services follow.
struct Pane {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
    /// Pane's clock for scheduled work, continuing services and clipboard
    /// expiry, which moves only when a test advances it. It starts a year
    /// ahead of the system's, which the launcher uses before it is given
    /// this one.
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

    /// Starts Pane with `fixture`'s sample installed, its service running.
    fn installed(&self, fixture: &Fixture) -> (Started, PackageIdentity, PathBuf) {
        let started = self.start();
        let folder = self.package(fixture);
        block_on(started.launcher.install_package(&folder));
        assert_eq!(
            started.launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        (started, identity, folder)
    }

    /// The assembled sample of `fixture` copied into a source folder of
    /// this test.
    fn package(&self, fixture: &Fixture) -> PathBuf {
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
        for file in ["pane.json", fixture.component] {
            fs::copy(assembled.join(file), folder.join(file)).unwrap();
        }
        folder
    }

    /// The settings sample, installed to stand for an unrelated package
    /// that declares no continuing service.
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

    /// How many cycles the Service sample has run, kept in its content, or
    /// `None` before the first.
    fn cycles(&self, folder: &Path) -> Option<u64> {
        let text = fs::read_to_string(self.content_file()).ok()?;
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let key = PackageIdentity::local(folder).unwrap().key();
        file["packages"][&key]["cycles"]
            .as_str()
            .and_then(|count| count.parse().ok())
    }

    /// How many events the Service sample is watching, kept in its
    /// content, or `None` before any.
    fn events(&self, folder: &Path) -> Option<u64> {
        let text = fs::read_to_string(self.content_file()).ok()?;
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let key = PackageIdentity::local(folder).unwrap().key();
        file["packages"][&key]["events"]
            .as_str()
            .and_then(|events| events.parse().ok())
    }

    /// What a waiting cycle saved last: "started", "finished" or nothing.
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

    /// Waits until the services thread has looked at every change of the
    /// clock and of the packages so far, and every cycle it started has
    /// reported.
    fn settled(&self, started: &Started) {
        assert!(
            started.launcher.wait_for_services(PROMPTLY),
            "the services did not settle"
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
                began.elapsed() < SLOW_CYCLES + PROMPTLY,
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

/// Runs the command's item titled `title`, returning its answer.
fn run_item(launcher: &Launcher, command: &str, title: &str) {
    open(launcher, command);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
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

fn installing_the_service_starts_it_and_activates_nothing_else(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    // An unrelated package that declares no continuing service.
    let settings = pane.settings_package();
    block_on(started.launcher.install_package(&settings));
    let settings_identity = PackageIdentity::local(&settings).unwrap();

    // The service begins at once: the package is installed enabled, so its
    // code may run and its declared work runs. Nothing else of it, and
    // nothing of any other package, is activated.
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(1));
    assert!(started.is_running(&identity));
    assert!(!started.is_running(&settings_identity));

    // The service keeps its own cadence, one it asks Pane for, not one the
    // manifest declares: the next cycle runs when it said, neither sooner
    // (half the cadence passes without one) nor as a burst for the time
    // that passed (five cadences passing at once run one cycle).
    pane.clock.advance(Duration::from_millis(500));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(1));
    pane.clock.advance(5 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    assert!(!started.is_running(&settings_identity));
}

fn the_status_of_a_cycle_shows_on_the_command_screen(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture);
    pane.settled(&started);

    // A cycle's status shows in the status line of the command's screen
    // while it is open, without the window waiting for the cycle.
    open(&started.launcher, fixture.command);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        started.launcher.view().status,
        Status::Result("Watching: 0 events (cycle 2, 2 this run)".into())
    );

    // The service's work is visible in what it watches: an item adds an
    // event, and the next cycle reports it.
    run_item(&started.launcher, fixture.command, "Add an event");
    assert_eq!(
        shown(&started.launcher),
        Status::Result("Added event 1; the next cycle reports it".into())
    );
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        started.launcher.view().status,
        Status::Result("Watching: 1 events (cycle 3, 3 this run)".into())
    );
    assert_eq!(pane.events(&folder), Some(1));

    // The command's view, asked for again, shows what the service did.
    open(&started.launcher, fixture.command);
    assert_eq!(
        started.launcher.view().title,
        "Watching: 1 events (3 cycles)"
    );
}

fn an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture);
    pane.settled(&started);

    // An error the service answers with is an expected error: it shows as
    // one, however often, and never pauses the extension.
    open(&started.launcher, fixture.command);
    run_item(&started.launcher, fixture.command, "Fail the next cycle");
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        error(started.launcher.view().status.clone()),
        "The extension reported an error: The service sample refuses, to show how an error looks"
    );
    assert!(!is_paused(&started.launcher, fixture.command));

    // The service carries on: the next cycle runs again on its cadence,
    // and its status replaces the error.
    open(&started.launcher, fixture.command);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        started.launcher.view().status,
        Status::Result("Watching: 0 events (cycle 3, 3 this run)".into())
    );
    assert_eq!(pane.cycles(&folder), Some(3));
}

fn disabling_stops_a_pending_cycle_and_discards_its_answer(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);

    // A cycle that waits inside the guest, on its own clock, is a task the
    // service runs: it notes that it started, then waits ten seconds.
    run_item(&started.launcher, fixture.command, "Wait on the next cycle");
    pane.clock.advance(SECOND);
    pane.until("the cycle to begin", Some(2), || pane.cycles(&folder));
    pane.until(
        "the cycle to save “started”",
        Some("started".into()),
        || pane.slow_save(),
    );

    // Disabling the package ends its generation, which stops the cycle: it
    // never saves "finished".
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
    assert_eq!(pane.cycles(&folder), Some(2));
    // The disable's own outcome stays: the stopped cycle's late answer is
    // discarded, not shown over it.
    assert_eq!(
        started.launcher.view().status,
        Status::Result(format!("Disabled {}", fixture.title))
    );

    // The cadence passes while the package is disabled: no cycle runs, and
    // nothing of the package is left running.
    pane.clock.advance(10 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    assert!(!started.is_running(&identity));
}

fn enabling_starts_the_service_again_with_a_fresh_task(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);

    // Enabling the package starts the service again, at once: the cycle
    // count, kept in its content, carries on, while the task's own state
    // ("this run") begins again, since the old instance went with the old
    // generation.
    block_on(started.launcher.set_enabled(&identity, true));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    open(&started.launcher, fixture.command);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        started.launcher.view().status,
        Status::Result("Watching: 0 events (cycle 3, 2 this run)".into())
    );
}

fn a_cycle_in_flight_prevents_a_second_one(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);

    // A cycle that waits ten seconds holds the service: the cadence passes
    // meanwhile, and no second cycle of the service is asked for (the
    // count, which every cycle raises when it begins, proves it).
    run_item(&started.launcher, fixture.command, "Wait on the next cycle");
    pane.clock.advance(SECOND);
    pane.until("the cycle to begin", Some(2), || pane.cycles(&folder));
    pane.clock.advance(5 * SECOND);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(pane.cycles(&folder), Some(2));

    // Ending the package's generation (disabling it) stops the pending
    // cycle, which saves nothing more, and no further cycle begins.
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    pane.clock.advance(10 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
}

fn a_service_cycle_does_not_block_the_launcher(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);

    // While the cycle waits inside the guest, on its own thread, the
    // launcher answers the user at once.
    run_item(&started.launcher, fixture.command, "Wait on the next cycle");
    pane.clock.advance(SECOND);
    pane.until("the cycle to begin", Some(2), || pane.cycles(&folder));
    pane.until(
        "the cycle to save “started”",
        Some("started".into()),
        || pane.slow_save(),
    );

    let began = Instant::now();
    block_on(started.launcher.set_query("anything"));
    started.launcher.back();
    assert!(began.elapsed() < SECOND, "took {:?}", began.elapsed());
    assert!(matches!(
        started.launcher.view().screen,
        Screen::Root { .. }
    ));
    // Ending the package's generation (disabling it) stops the pending
    // cycle, which saves nothing more.
    block_on(started.launcher.set_enabled(&identity, false));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
}

fn a_restart_starts_the_service_again_where_the_package_is_enabled(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture);
    pane.settled(&started);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    drop(started);

    // While Pane is stopped no cycle runs (the cadence passes with no
    // effect), and a restart starts the service again where the package is
    // enabled: its first cycle runs at once, the count carrying on and the
    // task's state beginning again.
    pane.clock.advance(10 * SECOND);
    let restarted = pane.start();
    pane.settled(&restarted);
    assert_eq!(pane.cycles(&folder), Some(3));
    pane.clock.advance(SECOND);
    pane.settled(&restarted);
    assert_eq!(pane.cycles(&folder), Some(4));

    // A package disabled before the restart is not served after it.
    let pane = Pane::new();
    let (restarted, identity, folder) = pane.installed(fixture);
    pane.settled(&restarted);
    assert_eq!(pane.cycles(&folder), Some(1));
    block_on(restarted.launcher.set_enabled(&identity, false));
    pane.settled(&restarted);
    drop(restarted);
    pane.clock.advance(10 * SECOND);
    let restarted = pane.start();
    pane.settled(&restarted);
    assert_eq!(pane.cycles(&folder), Some(1));
    // Enabling it on the restarted Pane starts the service.
    block_on(restarted.launcher.set_enabled(&identity, true));
    pane.settled(&restarted);
    assert_eq!(pane.cycles(&folder), Some(2));
}

fn reloading_stops_a_pending_cycle_and_the_replacement_serves_again(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture);
    pane.settled(&started);

    // Reloading the package replaces its code, which ends the old code's
    // generation: the pending cycle stops and never saves "finished".
    run_item(&started.launcher, fixture.command, "Wait on the next cycle");
    pane.clock.advance(SECOND);
    pane.until("the cycle to begin", Some(2), || pane.cycles(&folder));
    pane.until(
        "the cycle to save “started”",
        Some("started".into()),
        || pane.slow_save(),
    );
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(started.launcher.reload(&identity));
    pane.settled(&started);
    assert_eq!(pane.slow_save().as_deref(), Some("started"));
    // The reload's own outcome stays: the stopped cycle's late answer is
    // discarded, not shown over it.
    assert_eq!(
        started.launcher.view().status,
        Status::Result(format!("Reloaded {}", fixture.title))
    );

    // The replacement's service runs again, at once, in a fresh task.
    pane.until("the replacement's cycle to begin", Some(3), || {
        pane.cycles(&folder)
    });
    open(&started.launcher, fixture.command);
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(
        started.launcher.view().status,
        Status::Result("Watching: 0 events (cycle 4, 2 this run)".into())
    );
}

fn uninstalling_stops_the_service(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(1));

    // Uninstalling ends the package's generation, with its service.
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
    assert_eq!(pane.cycles(&folder), Some(1));
    let location = started
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity)
        .map(|package| package.location);
    assert!(location.is_none());
    assert!(block_on(started.runtime.running()).is_empty());
}

fn three_crashing_cycles_pause_the_package_and_stop_the_service(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);

    // A cycle that traps is a crash of the package, like a call the user
    // asked for: the first two are reported; the third pauses it.
    for expected in [2, 3] {
        run_item(&started.launcher, fixture.command, "Crash the next cycle");
        pane.clock.advance(SECOND);
        pane.settled(&started);
        assert_eq!(pane.cycles(&folder), Some(expected));
        assert!(!is_paused(&started.launcher, fixture.command));
    }
    run_item(&started.launcher, fixture.command, "Crash the next cycle");
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(4));
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

    // The pause ends the generation, with the service: the cadence passes
    // and no cycle runs, and nothing of the package is left running.
    pane.clock.advance(5 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(4));
    assert!(!started.is_running(&identity));

    // Retrying it starts the code again in a new generation, with its
    // service and its crash count afresh.
    assert_eq!(
        press(&started.launcher, &format!("Retry {}", fixture.title)),
        Status::Result(format!("Started {}", fixture.title))
    );
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(5));
    assert!(!is_paused(&started.launcher, fixture.command));
}

fn a_cycle_that_stops_responding_is_counted_as_a_crash(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, identity, folder) = pane.installed(fixture);
    pane.settled(&started);
    // A guest call may compute for half a second before Pane stops it
    // where it yields, as #18's unresponsive tests shorten the limit.
    started.runtime.set_limits(Limits {
        compute: Duration::from_millis(500),
        warn: Duration::from_secs(10),
        unresponsive: Duration::from_secs(30),
    });

    // A cycle that computes without waiting is stopped by Pane and counted
    // as a crash of the package, as any call's is: the third within five
    // minutes pauses it, which ends the service.
    for expected in [2, 3] {
        run_item(
            &started.launcher,
            fixture.command,
            "Stop responding on the next cycle",
        );
        pane.clock.advance(SECOND);
        pane.settled(&started);
        assert_eq!(pane.cycles(&folder), Some(expected));
        assert!(!is_paused(&started.launcher, fixture.command));
    }
    run_item(
        &started.launcher,
        fixture.command,
        "Stop responding on the next cycle",
    );
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(4));
    assert!(is_paused(&started.launcher, fixture.command));
    let toast = error(started.launcher.view().status.clone());
    assert!(
        toast.starts_with(&format!(
            "{} stopped responding 3 times within 5 minutes and is paused",
            fixture.title
        )),
        "{toast}"
    );

    // The extension list says how it paused, and the cadence that passes
    // while it is paused runs nothing.
    manage(&started.launcher);
    let subtitle = row(&started.launcher, fixture.title).subtitle.unwrap();
    assert!(
        subtitle.starts_with("Enabled · Paused after not responding"),
        "{subtitle}"
    );
    pane.clock.advance(5 * SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(4));
    assert!(!started.is_running(&identity));
}

fn a_manifest_declaring_a_service_the_component_does_not_export_is_refused(_fixture: &Fixture) {
    let pane = Pane::new();
    // The settings sample's component exports no continuing service.
    let folder = pane.sources.path().join("refused");
    fs::create_dir_all(&folder).unwrap();
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-settings");
    let manifest = fs::read_to_string(assembled.join("pane.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    manifest["commands"][0]["service"] = serde_json::json!(true);
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    fs::copy(
        assembled.join("sample_settings.wasm"),
        folder.join("sample_settings.wasm"),
    )
    .unwrap();

    let started = pane.start();
    block_on(started.launcher.install_package(&folder));
    let refusal = error(started.launcher.view().status.clone());
    assert!(
        refusal.starts_with(
            "\"Greeting\": Incompatible extension: it does not implement Pane's extension \
             interface: its manifest says it runs a continuing service, but it does not export \
             pane:extension/service@0.1.0 with the functions Pane calls: "
        ),
        "{refusal}"
    );
    // Nothing was installed, and nothing runs.
    assert!(started.launcher.packages().is_empty());
    pane.clock.advance(60 * SECOND);
    pane.settled(&started);
    assert!(block_on(started.runtime.running()).is_empty());
}

#[test]
fn installing_the_service_starts_it_and_activates_nothing_else_in_rust() {
    installing_the_service_starts_it_and_activates_nothing_else(&RUST);
}

#[test]
fn the_status_of_a_cycle_shows_on_the_command_screen_in_rust() {
    the_status_of_a_cycle_shows_on_the_command_screen(&RUST);
}

#[test]
fn an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on_in_rust() {
    an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on(&RUST);
}

#[test]
fn disabling_stops_a_pending_cycle_and_discards_its_answer_in_rust() {
    disabling_stops_a_pending_cycle_and_discards_its_answer(&RUST);
}

#[test]
fn enabling_starts_the_service_again_with_a_fresh_task_in_rust() {
    enabling_starts_the_service_again_with_a_fresh_task(&RUST);
}

fn a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(1));

    // A cycle may answer 0 seconds — no wait at all — which Pane clamps
    // to the 1-second minimum a cadence may be: the next cycle runs a
    // second after the answer, not at once, so a service cannot
    // busy-loop itself.
    run_item(
        &started.launcher,
        fixture.command,
        "Ask for a 0-second cadence",
    );
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    pane.clock.advance(Duration::from_millis(500));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    pane.clock.advance(Duration::from_millis(500));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(3));
}

fn a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum(fixture: &Fixture) {
    let pane = Pane::new();
    let (started, _, folder) = pane.installed(fixture);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(1));

    // A cycle may answer 31 days, beyond the 30-day maximum a cadence may
    // be, which Pane clamps to: the next cycle runs thirty days after the
    // answer, to the second.
    run_item(
        &started.launcher,
        fixture.command,
        "Ask for a 31-day cadence",
    );
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    pane.clock.advance(Duration::from_secs(30 * 86_400 - 1));
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(2));
    pane.clock.advance(SECOND);
    pane.settled(&started);
    assert_eq!(pane.cycles(&folder), Some(3));
}

#[test]
fn a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence_in_rust() {
    a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence(&RUST);
}

#[test]
fn a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum_in_rust() {
    a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum(&RUST);
}

#[test]
fn a_cycle_in_flight_prevents_a_second_one_in_rust() {
    a_cycle_in_flight_prevents_a_second_one(&RUST);
}

#[test]
fn a_service_cycle_does_not_block_the_launcher_in_rust() {
    a_service_cycle_does_not_block_the_launcher(&RUST);
}

#[test]
fn a_restart_starts_the_service_again_where_the_package_is_enabled_in_rust() {
    a_restart_starts_the_service_again_where_the_package_is_enabled(&RUST);
}

#[test]
fn reloading_stops_a_pending_cycle_and_the_replacement_serves_again_in_rust() {
    reloading_stops_a_pending_cycle_and_the_replacement_serves_again(&RUST);
}

#[test]
fn uninstalling_stops_the_service_in_rust() {
    uninstalling_stops_the_service(&RUST);
}

#[test]
fn three_crashing_cycles_pause_the_package_and_stop_the_service_in_rust() {
    three_crashing_cycles_pause_the_package_and_stop_the_service(&RUST);
}

#[test]
fn a_cycle_that_stops_responding_is_counted_as_a_crash_in_rust() {
    a_cycle_that_stops_responding_is_counted_as_a_crash(&RUST);
}

#[test]
fn a_manifest_declaring_a_service_the_component_does_not_export_is_refused_in_rust() {
    a_manifest_declaring_a_service_the_component_does_not_export_is_refused(&RUST);
}

#[test]
fn installing_the_service_starts_it_and_activates_nothing_else_in_javascript() {
    installing_the_service_starts_it_and_activates_nothing_else(&JAVASCRIPT);
}

#[test]
fn installing_the_service_starts_it_and_activates_nothing_else_in_typescript() {
    installing_the_service_starts_it_and_activates_nothing_else(&TYPESCRIPT);
}

#[test]
fn the_status_of_a_cycle_shows_on_the_command_screen_in_javascript() {
    the_status_of_a_cycle_shows_on_the_command_screen(&JAVASCRIPT);
}

#[test]
fn the_status_of_a_cycle_shows_on_the_command_screen_in_typescript() {
    the_status_of_a_cycle_shows_on_the_command_screen(&TYPESCRIPT);
}

#[test]
fn an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on_in_javascript() {
    an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on(&JAVASCRIPT);
}

#[test]
fn an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on_in_typescript() {
    an_error_a_cycle_answers_with_shows_as_one_and_the_service_carries_on(&TYPESCRIPT);
}

#[test]
fn disabling_stops_a_pending_cycle_and_discards_its_answer_in_javascript() {
    disabling_stops_a_pending_cycle_and_discards_its_answer(&JAVASCRIPT);
}

#[test]
fn disabling_stops_a_pending_cycle_and_discards_its_answer_in_typescript() {
    disabling_stops_a_pending_cycle_and_discards_its_answer(&TYPESCRIPT);
}

#[test]
fn enabling_starts_the_service_again_with_a_fresh_task_in_javascript() {
    enabling_starts_the_service_again_with_a_fresh_task(&JAVASCRIPT);
}

#[test]
fn enabling_starts_the_service_again_with_a_fresh_task_in_typescript() {
    enabling_starts_the_service_again_with_a_fresh_task(&TYPESCRIPT);
}

#[test]
fn a_cycle_in_flight_prevents_a_second_one_in_javascript() {
    a_cycle_in_flight_prevents_a_second_one(&JAVASCRIPT);
}

#[test]
fn a_cycle_in_flight_prevents_a_second_one_in_typescript() {
    a_cycle_in_flight_prevents_a_second_one(&TYPESCRIPT);
}

#[test]
fn a_service_cycle_does_not_block_the_launcher_in_javascript() {
    a_service_cycle_does_not_block_the_launcher(&JAVASCRIPT);
}

#[test]
fn a_service_cycle_does_not_block_the_launcher_in_typescript() {
    a_service_cycle_does_not_block_the_launcher(&TYPESCRIPT);
}

#[test]
fn a_restart_starts_the_service_again_where_the_package_is_enabled_in_javascript() {
    a_restart_starts_the_service_again_where_the_package_is_enabled(&JAVASCRIPT);
}

#[test]
fn a_restart_starts_the_service_again_where_the_package_is_enabled_in_typescript() {
    a_restart_starts_the_service_again_where_the_package_is_enabled(&TYPESCRIPT);
}

#[test]
fn reloading_stops_a_pending_cycle_and_the_replacement_serves_again_in_javascript() {
    reloading_stops_a_pending_cycle_and_the_replacement_serves_again(&JAVASCRIPT);
}

#[test]
fn reloading_stops_a_pending_cycle_and_the_replacement_serves_again_in_typescript() {
    reloading_stops_a_pending_cycle_and_the_replacement_serves_again(&TYPESCRIPT);
}

#[test]
fn uninstalling_stops_the_service_in_javascript() {
    uninstalling_stops_the_service(&JAVASCRIPT);
}

#[test]
fn uninstalling_stops_the_service_in_typescript() {
    uninstalling_stops_the_service(&TYPESCRIPT);
}

#[test]
fn three_crashing_cycles_pause_the_package_and_stop_the_service_in_javascript() {
    three_crashing_cycles_pause_the_package_and_stop_the_service(&JAVASCRIPT);
}

#[test]
fn three_crashing_cycles_pause_the_package_and_stop_the_service_in_typescript() {
    three_crashing_cycles_pause_the_package_and_stop_the_service(&TYPESCRIPT);
}

#[test]
fn a_cycle_that_stops_responding_is_counted_as_a_crash_in_javascript() {
    a_cycle_that_stops_responding_is_counted_as_a_crash(&JAVASCRIPT);
}

#[test]
fn a_cycle_that_stops_responding_is_counted_as_a_crash_in_typescript() {
    a_cycle_that_stops_responding_is_counted_as_a_crash(&TYPESCRIPT);
}

#[test]
fn a_manifest_declaring_a_service_the_component_does_not_export_is_refused_in_javascript() {
    a_manifest_declaring_a_service_the_component_does_not_export_is_refused(&JAVASCRIPT);
}

#[test]
fn a_manifest_declaring_a_service_the_component_does_not_export_is_refused_in_typescript() {
    a_manifest_declaring_a_service_the_component_does_not_export_is_refused(&TYPESCRIPT);
}

#[test]
fn a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence_in_javascript() {
    a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence(&JAVASCRIPT);
}

#[test]
fn a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence_in_typescript() {
    a_cycle_asking_for_no_wait_runs_again_at_the_minimum_cadence(&TYPESCRIPT);
}

#[test]
fn a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum_in_javascript() {
    a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum(&JAVASCRIPT);
}

#[test]
fn a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum_in_typescript() {
    a_cycle_asking_for_more_than_30_days_runs_again_at_the_maximum(&TYPESCRIPT);
}
