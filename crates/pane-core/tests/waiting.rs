//! Commands that wait while a required dependency cannot serve them (#152,
//! the first slice of #151's waiting commands): while a required
//! dependency of an enabled, unpaused package is missing, disabled, paused
//! or waiting itself, the package's commands stay listed saying what they
//! need, nothing of them runs — not their views, their scheduled work,
//! their services or their results — their package's operations answer
//! `unavailable` with the reason, and every way in (a quick slot, an
//! alias, a fallback) says the same and runs nothing. They come back by
//! themselves once what they need returns: the schedule from a full
//! interval, the service at once, root search on the next query. Waiting
//! ends no generation and stops no instance, and never counts towards
//! pausing. A chain names what is actually missing, a cycle of healthy
//! packages runs, a cycle with one member missing waits as a whole, and
//! an optional dependency never makes a command wait.
//!
//! Through the launcher's public interface, with the operations, schedule,
//! service and Rust samples from `cargo xtask guests`, the operations
//! fixture and the manual clock.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{
    Launcher, PackageIdentity, ResultAction, Row, Runtime, SavedData, Screen, SlotChange, Status,
    Unavailable,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest_file as guest;
use rows::{manage, select_title, titles, to_root};

/// How long the scheduler, the services thread and a call may take:
/// compiling a guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(8);

/// The Rust operations sample, which greets and is greeted: the greeter
/// the others require, and (as the caller) a command that calls by
/// dependency id.
const GREETER: &str = "sample-operations";

/// Source folders, Pane's data folder and the runtime the launchers share.
struct Dirs {
    sources: TempDir,
    data: TempDir,
    runtime: Runtime,
    /// Pane's clock for scheduled work, services and clipboard expiry,
    /// which moves only when a test advances it. It starts a year ahead of
    /// the system's, which the launcher uses before it is given this one.
    clock: Arc<ManualClock>,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
            clock: ManualClock::at(SystemClock.now() + 365 * 86_400_000),
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.sources.path().join(name)
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
    }

    /// The source other packages call the package in folder `name` by: its
    /// identity, as Pane shows it.
    fn source(&self, name: &str) -> String {
        self.identity(name).key()
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.extensions())
            .with_clock(self.clock.clone())
    }

    /// Copies the assembled sample package `package` into source folder
    /// `package`.
    fn sample(&self, package: &str) -> PathBuf {
        let assembled = guest("packages").join(package);
        let folder = self.folder(package);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Copies the assembled sample package `package` into source folder
    /// `package`, declaring `dependencies` (JSON array contents): the
    /// schedule, service and Rust samples whose manifests these tests
    /// change.
    fn requiring(&self, package: &str, dependencies: &str) -> PathBuf {
        let folder = self.sample(package);
        let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        manifest["dependencies"] = serde_json::from_str(dependencies).unwrap();
        fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
        folder
    }

    /// Writes package "Caller" in source folder `caller`: the JavaScript
    /// operations sample's command and component, declaring
    /// `dependencies` (JSON array contents).
    fn caller(&self, dependencies: &str) -> PathBuf {
        let folder = self.folder("caller");
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("sample_operations_js.wasm"),
            folder.join("caller.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Caller",
                "apiVersion": "0.1",
                "commands": [
                    {{ "id": "call", "title": "Call from JavaScript",
                       "component": "caller.wasm", "takesQuery": true }}
                ],
                "operations": [{{ "id": "greet", "version": 1, "component": "caller.wasm" }}],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Writes a fixture package in source folder `name`, titled
    /// "Package <name>", whose command and `operations` (JSON array
    /// contents) the operations fixture serves, declaring `dependencies`
    /// (JSON array contents).
    fn fixture(&self, name: &str, dependencies: &str, operations: &str) -> PathBuf {
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("operations_fixture.wasm"),
            folder.join("fixture.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Package {name}",
                "apiVersion": "0.1",
                "commands": [
                    {{ "id": "fixture", "title": "Fixture {name}", "component": "fixture.wasm" }}
                ],
                "operations": [{operations}],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// The fixture in folder `name` publishing `echo` 1, declaring
    /// `dependencies` (JSON array contents).
    fn echo_fixture(&self, name: &str, dependencies: &str) -> PathBuf {
        self.fixture(
            name,
            dependencies,
            r#"{ "id": "echo", "version": 1, "component": "fixture.wasm" }"#,
        )
    }

    /// Installs `folder`'s package, asserting it worked.
    fn install(&self, launcher: &Launcher, folder: &Path) {
        block_on(launcher.install_package(folder));
        let status = launcher.view().status;
        assert!(
            matches!(&status, Status::Result(text) if text.starts_with("Installed")),
            "installing {}: {status:?}",
            folder.display()
        );
    }

    /// Saves, as the fixture `a`'s settings, the sources it calls by name:
    /// those of `names`. Before any launcher opens the data folder.
    fn save_sources(&self, names: &[&str]) {
        let mut sources = serde_json::Map::new();
        for name in names {
            sources.insert((*name).into(), self.source(name).into());
        }
        let settings = serde_json::json!({
            "version": 1,
            "packages": {
                self.source("a"): { "sources": serde_json::Value::Object(sources).to_string() }
            }
        });
        fs::create_dir_all(self.extensions()).unwrap();
        fs::write(
            self.extensions().join("settings.json"),
            settings.to_string(),
        )
        .unwrap();
    }

    /// Whether an instance of the package in folder `name` is running.
    fn runs(&self, launcher: &Launcher, name: &str) -> bool {
        block_on(self.runtime.running())
            .iter()
            .any(|path| path.starts_with(self.folder(name)))
    }

    /// How many runs the Schedule sample has counted, kept in its content,
    /// or `None` before the first.
    fn runs_counted(&self, folder: &Path) -> Option<u64> {
        counted(self.extensions().join("content.json"), folder, "count")
    }

    /// How many cycles the Service sample has run, kept in its content, or
    /// `None` before the first.
    fn cycles(&self, folder: &Path) -> Option<u64> {
        counted(self.extensions().join("content.json"), folder, "cycles")
    }

    /// Advances the clock by `seconds`.
    fn advance(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}

/// A number the package of `folder` saved under `key` in its content, or
/// `None` before it saved one.
fn counted(content: PathBuf, folder: &Path, key: &str) -> Option<u64> {
    let text = fs::read_to_string(content).ok()?;
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let package = PackageIdentity::local(folder).unwrap().key();
    file["packages"][&package][key]
        .as_str()
        .and_then(|count| count.parse().ok())
}

/// A required dependency on the package in sibling folder `folder`,
/// calling `echo` at version 1.
fn needs(folder: &str) -> String {
    format!(
        r#"{{ "id": "{folder}", "source": "local:../{folder}",
             "operations": [{{ "id": "echo", "version": 1 }}] }}"#
    )
}

/// An optional dependency on the package in sibling folder `folder`.
fn uses(folder: &str) -> String {
    format!(
        r#"{{ "id": "{folder}", "source": "local:../{folder}", "optional": true,
             "operations": [{{ "id": "echo", "version": 1 }}] }}"#
    )
}

/// The root search row titled `title`.
fn row(launcher: &Launcher, title: &str) -> Row {
    let view = launcher.view();
    view.rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
}

/// Why the command titled `title` cannot run, as a waiting reason.
fn waiting_reason(launcher: &Launcher, title: &str) -> String {
    let reason = row(launcher, title)
        .unavailable
        .unwrap_or_else(|| panic!("{title:?} is available"));
    let Unavailable::Waiting(reason) = reason else {
        panic!("expected {title:?} to wait, got {reason:?}");
    };
    reason
}

/// From root search, activates the row titled `title`: opening a command,
/// or, for a waiting one, showing its reason with the row that fixes it.
fn open(launcher: &Launcher, title: &str) {
    to_root(launcher);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
}

/// Selects the row titled `title` on the screen shown and activates it.
fn activate(launcher: &Launcher, title: &str) -> Status {
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// The error the status line shows.
fn error(status: &Status) -> String {
    match status {
        Status::Error(message) => message.clone(),
        other => panic!("expected an error, got {other:?}"),
    }
}

/// Waits until `read` returns `expected`, saying `what` it was.
fn until<T: PartialEq + std::fmt::Debug>(what: &str, expected: T, mut read: impl FnMut() -> T) {
    let began = Instant::now();
    loop {
        let found = read();
        if found == expected {
            return;
        }
        assert!(
            began.elapsed() < PROMPTLY,
            "waiting for {what}: found {found:?}, expected {expected:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Gives the command titled `title` the alias `alias` in Manage extensions.
fn set_alias(launcher: &Launcher, title: &str, alias: &str) {
    manage(launcher);
    select_title(launcher, &format!("Alias for {title}"));
    block_on(launcher.activate_selected());
    let form = launcher.view().form().cloned().expect("the alias form");
    launcher.set_field_value(&form.fields[0].id, alias);
    block_on(launcher.submit_form());
}

/// Makes the command titled `title` a fallback in Manage extensions.
fn set_fallback(launcher: &Launcher, title: &str) {
    manage(launcher);
    select_title(launcher, &format!("Fallback: {title}"));
    block_on(launcher.activate_selected());
}

/// Installs the greeter (the Rust operations sample) and a caller that
/// requires it, leaving root search shown.
fn greeter_and_caller(dirs: &Dirs) -> Launcher {
    dirs.sample(GREETER);
    let caller = dirs.caller(&needs(GREETER));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);
    launcher.show_root_search();
    launcher
}

#[test]
fn a_dependent_waits_while_its_dependency_is_disabled_and_comes_back_when_it_is_enabled() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    let greeter = dirs.identity(GREETER);

    block_on(launcher.set_enabled(&greeter, false));
    launcher.show_root_search();

    // Its command stays listed, saying what it needs, and nothing of it
    // runs: opening it is refused with the reason, on a screen of Pane's
    // own.
    assert_eq!(
        waiting_reason(&launcher, "Call from JavaScript"),
        "Needs Rust operations sample, which is disabled"
    );
    open(&launcher, "Call from JavaScript");
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::WaitingDetails { .. }),
        "{view:?}"
    );
    assert_eq!(view.title, "Why Call from JavaScript cannot run");
    assert_eq!(
        view.details().first().map(String::as_str),
        Some("Needs Rust operations sample, which is disabled.")
    );
    assert_eq!(titles(&launcher), ["Enable Rust operations sample"]);
    assert!(!dirs.runs(&launcher, "caller"));

    // The fix row enables it; the dependent comes back by itself.
    assert_eq!(
        activate(&launcher, "Enable Rust operations sample"),
        Status::Result("Enabled Rust operations sample".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
    open(&launcher, "Call from JavaScript");
    assert_eq!(launcher.view().screen, Screen::Command);
    assert!(dirs.runs(&launcher, "caller"));
}

#[test]
fn a_dependent_waits_while_its_dependency_is_paused_and_comes_back_when_it_is_retried() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    drop(launcher);
    // Pane paused the greeter after it crashed, as recorded before a
    // restart.
    let registry = dirs.extensions().join("installed.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
    let dir = record["packages"][0]["dir"].clone();
    record["packages"][0]["paused"] = serde_json::json!({
        "after": "crashes", "why": "it crashed", "version": "0.1.0", "code": dir
    });
    fs::write(&registry, record.to_string()).unwrap();
    let launcher = dirs.launcher();

    // Its command waits for the paused dependency; Enter offers Retry.
    assert_eq!(
        waiting_reason(&launcher, "Call from JavaScript"),
        "Needs Rust operations sample, which is paused"
    );
    open(&launcher, "Call from JavaScript");
    assert_eq!(titles(&launcher), ["Retry Rust operations sample"]);
    // The fix row retries it; the dependent comes back by itself.
    assert_eq!(
        activate(&launcher, "Retry Rust operations sample"),
        Status::Result("Started Rust operations sample".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
    open(&launcher, "Call from JavaScript");
    assert_eq!(launcher.view().screen, Screen::Command);
}

#[test]
fn a_dependent_waits_while_its_dependency_is_uninstalled_and_comes_back_when_it_is_installed() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    let greeter = dirs.identity(GREETER);

    block_on(launcher.uninstall(&greeter, SavedData::Keep));
    launcher.show_root_search();

    // The uninstalled dependency is named by the title its retained data
    // was kept under; Enter offers Manage extensions.
    assert_eq!(
        waiting_reason(&launcher, "Call from JavaScript"),
        "Needs Rust operations sample, which is not installed"
    );
    open(&launcher, "Call from JavaScript");
    assert_eq!(titles(&launcher), ["Open Manage extensions"]);
    assert!(!dirs.runs(&launcher, "caller"));

    // Installing it again brings the dependent back, by itself.
    block_on(launcher.install_package(&dirs.folder(GREETER)));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
    open(&launcher, "Call from JavaScript");
    assert_eq!(launcher.view().screen, Screen::Command);
}

#[test]
fn a_waiting_package_answers_its_operations_unavailable_with_the_reason() {
    let dirs = Dirs::new();
    dirs.sample(GREETER);
    let caller = dirs.caller(&needs(GREETER));
    // The TypeScript sample calls the caller's operation by identity; it
    // depends on nothing, so it does not wait with it.
    let typescript = dirs.sample("sample-operations-ts");
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);
    dirs.install(&launcher, &typescript);
    block_on(launcher.set_enabled(&dirs.identity(GREETER), false));

    open(&launcher, "Call from TypeScript");
    assert_eq!(launcher.view().screen, Screen::Command);
    select_title(&launcher, "Greet through another extension");
    block_on(launcher.activate_selected());
    launcher.set_field_value("source", &dirs.source("caller"));
    launcher.set_field_value("name", "Ada");
    launcher.set_field_value("times", "once");
    block_on(launcher.submit_form());
    assert_eq!(
        error(&launcher.view().status),
        "unavailable: Caller is waiting for Rust operations sample, which is disabled"
    );
}

#[test]
fn waits_three_deep_name_what_is_actually_missing() {
    let dirs = Dirs::new();
    dirs.echo_fixture("c", "");
    dirs.echo_fixture("b", &needs("c"));
    let a = dirs.echo_fixture("a", &needs("b"));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &a);
    block_on(launcher.set_enabled(&dirs.identity("c"), false));
    launcher.show_root_search();

    // The chain names what is actually missing, at every depth.
    assert_eq!(
        waiting_reason(&launcher, "Fixture b"),
        "Needs Package c, which is disabled"
    );
    assert_eq!(
        waiting_reason(&launcher, "Fixture a"),
        "Needs Package b, which waits for Package c: Package c is disabled"
    );
    // The fix is for the root cause, not the direct dependency.
    open(&launcher, "Fixture a");
    assert_eq!(titles(&launcher), ["Enable Package c"]);
    assert_eq!(
        activate(&launcher, "Enable Package c"),
        Status::Result("Enabled Package c".into())
    );
    // Both dependents came back.
    assert_eq!(row(&launcher, "Fixture a").unavailable, None);
    assert_eq!(row(&launcher, "Fixture b").unavailable, None);
}

#[test]
fn a_cycle_of_healthy_packages_runs_and_one_with_a_member_missing_waits_as_a_whole() {
    let dirs = Dirs::new();
    // x and y require each other: installed together, they run.
    dirs.echo_fixture("x", &needs("y"));
    let y = dirs.echo_fixture("y", &needs("x"));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &y);
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Fixture x").unavailable, None);
    assert_eq!(row(&launcher, "Fixture y").unavailable, None);
    open(&launcher, "Fixture x");
    assert_eq!(launcher.view().screen, Screen::Command);

    // A cycle with one member missing waits as a whole.
    dirs.echo_fixture("p", &needs("q"));
    let q = dirs.echo_fixture("q", &needs("p"));
    dirs.install(&launcher, &q);
    block_on(launcher.uninstall(&dirs.identity("p"), SavedData::Keep));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Fixture q"),
        "Needs Package p, which is not installed"
    );
    open(&launcher, "Fixture q");
    assert!(matches!(
        launcher.view().screen,
        Screen::WaitingDetails { .. }
    ));
    assert!(!dirs.runs(&launcher, "q"));
}

#[test]
fn an_optional_dependency_never_makes_a_command_wait() {
    let dirs = Dirs::new();
    let caller = dirs.caller(&uses(GREETER));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);
    launcher.show_root_search();

    // Not installed: the caller's command is available.
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
    open(&launcher, "Call from JavaScript");
    assert_eq!(launcher.view().screen, Screen::Command);
    launcher.back();

    // Installed, then disabled: still available. An optional dependency
    // never gates.
    dirs.install(&launcher, &dirs.sample(GREETER));
    block_on(launcher.set_enabled(&dirs.identity(GREETER), false));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
    open(&launcher, "Call from JavaScript");
    assert_eq!(launcher.view().screen, Screen::Command);
}

#[test]
fn waiting_skips_a_schedule_s_ticks_and_comes_back_from_a_full_interval() {
    let dirs = Dirs::new();
    dirs.sample(GREETER);
    let scheduled = set_every(&dirs.requiring("sample-schedule", &needs(GREETER)), 1);
    let launcher = dirs.launcher();
    dirs.install(&launcher, &scheduled);
    assert!(launcher.wait_for_schedules(PROMPTLY));

    // It runs every second while it may run.
    dirs.advance(1);
    until("the first scheduled run", Some(1), || {
        dirs.runs_counted(&scheduled)
    });
    assert!(launcher.wait_for_schedules(PROMPTLY));

    // Its dependency disabled: the command waits, and the ticks are
    // skipped, not replayed.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), false));
    dirs.advance(5);
    assert!(launcher.wait_for_schedules(PROMPTLY));
    assert_eq!(dirs.runs_counted(&scheduled), Some(1));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Counting"),
        "Needs Rust operations sample, which is disabled"
    );

    // Enabled again: the schedule starts from a full interval — no tick
    // that fell due while it waited is replayed — and then runs.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), true));
    dirs.advance(1);
    until("the schedule came back", Some(2), || {
        dirs.runs_counted(&scheduled)
    });
    assert_eq!(dirs.runs_counted(&scheduled), Some(2));
}

#[test]
fn waiting_stops_a_service_s_cycles_and_the_first_runs_at_once_when_it_comes_back() {
    let dirs = Dirs::new();
    dirs.sample(GREETER);
    let serving = dirs.requiring("sample-service", &needs(GREETER));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &serving);
    assert!(launcher.wait_for_services(PROMPTLY));

    // Its first cycle runs at once while it may run.
    until("the first cycle", Some(1), || dirs.cycles(&serving));

    // Its dependency disabled: the command waits, and the service does not
    // cycle.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), false));
    dirs.advance(30);
    assert!(launcher.wait_for_services(PROMPTLY));
    assert_eq!(dirs.cycles(&serving), Some(1));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Watching"),
        "Needs Rust operations sample, which is disabled"
    );

    // Enabled again: the first cycle runs at once, in the instance the
    // service still has.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), true));
    until("the service came back", Some(2), || dirs.cycles(&serving));
    assert_eq!(dirs.cycles(&serving), Some(2));
}

#[test]
fn root_results_are_not_asked_while_waiting_and_asked_again_once_it_is_back() {
    let dirs = Dirs::new();
    dirs.sample(GREETER);
    let sample = dirs.requiring("sample-rust", &needs(GREETER));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &sample);
    launcher.back();

    // A query asks the sample for its root results.
    block_on(launcher.set_query("reverse Pané"));
    assert_eq!(launcher.view().rows[0].title, "énaP");
    launcher.back();

    // Its dependency disabled: the command waits, its root results are not
    // asked for and none are listed.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), false));
    block_on(launcher.set_query("reverse another"));
    assert!(
        launcher
            .view()
            .rows
            .iter()
            .all(|row| row.title != "rehtona"),
        "{:?}",
        titles(&launcher)
    );
    assert!(!dirs.runs(&launcher, "sample-rust"));

    // Enabled again: the next query asks for its results.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), true));
    block_on(launcher.set_query("reverse back"));
    assert_eq!(launcher.view().rows[0].title, "kcab");
}

#[test]
fn waiting_leaves_an_open_screen_and_its_calls_alone_and_never_pauses_it() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    let greeter = dirs.identity(GREETER);

    // The caller's command is open when its dependency is disabled: the
    // screen stays, and its calls answer as calls do.
    open(&launcher, "Call from JavaScript");
    assert_eq!(launcher.view().screen, Screen::Command);
    block_on(launcher.set_enabled(&greeter, false));
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "the open screen stays"
    );
    assert!(dirs.runs(&launcher, "caller"), "its instance stays");
    select_title(&launcher, "Greet through another extension");
    block_on(launcher.activate_selected());
    launcher.set_field_value("source", "greeter");
    launcher.set_field_value("name", "Ada");
    launcher.set_field_value("times", "once");
    block_on(launcher.submit_form());
    assert_eq!(
        error(&launcher.view().status),
        "disabled: Rust operations sample is disabled; Pane does not enable it for a call, \
         enable it in Settings"
    );
    // Waiting never counts towards pausing: no Retry row for the caller.
    manage(&launcher);
    assert!(!titles(&launcher).contains(&"Retry Caller".to_owned()));
}

#[test]
fn a_call_of_a_waiting_package_already_running_finishes() {
    let dirs = Dirs::new();
    dirs.echo_fixture("c", "");
    let wait = r#"{ "id": "wait", "version": 1, "component": "fixture.wasm" },
        { "id": "echo", "version": 1, "component": "fixture.wasm" }"#;
    dirs.fixture("b", &needs("c"), wait);
    dirs.fixture("a", "", wait);
    dirs.save_sources(&["b"]);
    let launcher = dirs.launcher();
    dirs.install(&launcher, &dirs.folder("a"));
    dirs.install(&launcher, &dirs.folder("b"));
    dirs.install(&launcher, &dirs.folder("c"));

    // a calls b's `wait`, which takes ten seconds; while it runs, c is
    // disabled, so b waits. The call already running finishes: waiting
    // ends no generation and stops no instance.
    open(&launcher, "Fixture a");
    select_title(&launcher, "Call b's wait");
    let calling = launcher.activate_selected();
    let calling = thread::spawn(move || block_on(calling));
    until("b's wait began", "started".to_owned(), || {
        b_saved(&dirs, "waiting").unwrap_or_default()
    });

    block_on(launcher.set_enabled(&dirs.identity("c"), false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Fixture b"),
        "Needs Package c, which is disabled"
    );

    // The call finishes with its answer, not with an error.
    calling.join().unwrap();
    assert_eq!(
        shown(&launcher),
        Status::Result(r#"answered: {"waited":true}"#.into())
    );
    assert_eq!(b_saved(&dirs, "waiting").as_deref(), Some("finished"));
}

/// What the fixture `b` saved under `key` in its settings, or `None`.
fn b_saved(dirs: &Dirs, key: &str) -> Option<String> {
    let text = fs::read_to_string(dirs.extensions().join("settings.json")).ok()?;
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    file["packages"][&dirs.source("b")][key]
        .as_str()
        .map(str::to_owned)
}

#[test]
fn a_quick_slot_and_an_alias_and_a_fallback_of_a_waiting_command_say_why_and_run_nothing() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    let greeter = dirs.identity(GREETER);

    // The user pinned the command, gave it an alias and made it a
    // fallback.
    to_root(&launcher);
    let target = {
        let view = launcher.view();
        let index = view
            .rows
            .iter()
            .position(|row| row.title == "Call from JavaScript")
            .unwrap();
        launcher.select(index);
        view.rows[index].id.clone()
    };
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    block_on(recorded);
    assert_eq!(change, SlotChange::Changed(Some(0)));
    set_alias(&launcher, "Call from JavaScript", "cal");
    set_fallback(&launcher, "Call from JavaScript");

    block_on(launcher.set_enabled(&greeter, false));
    launcher.show_root_search();

    // The quick slot says why it cannot run and runs nothing.
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "Call from JavaScript");
    assert_eq!(
        slot.unavailable.as_deref(),
        Some("Needs Rust operations sample, which is disabled")
    );
    block_on(launcher.activate_quick_slot(0));
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(
        error(&launcher.view().status),
        "Needs Rust operations sample, which is disabled"
    );
    assert!(!dirs.runs(&launcher, "caller"));

    // The alias and the fallback say the same and run nothing.
    block_on(launcher.set_query("cal Ada"));
    assert_eq!(
        row(&launcher, "Call from JavaScript")
            .unavailable
            .as_ref()
            .map(|reason| reason.reason().to_owned()),
        Some("Needs Rust operations sample, which is disabled".to_owned())
    );
    select_title(&launcher, "Call from JavaScript");
    block_on(launcher.activate_selected());
    assert_eq!(
        error(&launcher.view().status),
        "Needs Rust operations sample, which is disabled"
    );
    assert!(!dirs.runs(&launcher, "caller"));
}

#[test]
fn a_global_hotkey_of_a_waiting_command_says_why_and_runs_nothing() {
    let dirs = Dirs::new();
    dirs.sample(GREETER);
    let caller = dirs.caller(&needs(GREETER));
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.extensions())
        .with_clock(dirs.clock.clone())
        .with_hotkeys(Arc::new(AnyHotkeys::default()));
    dirs.install(&launcher, &caller);

    // The user gave the command a global hotkey; pressing it from any
    // application shows the reason and runs nothing.
    set_hotkey(&launcher, "Call from JavaScript", "ctrl+alt+c");
    block_on(launcher.set_enabled(&dirs.identity(GREETER), false));
    launcher.show_root_search();

    let shortcut = Shortcut::parse("ctrl+alt+c").unwrap();
    let opening = launcher
        .press_hotkey(&shortcut)
        .expect("the hotkey still opens its command");
    block_on(opening);
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(
        error(&launcher.view().status),
        "Needs Rust operations sample, which is disabled"
    );
    assert!(!dirs.runs(&launcher, "caller"));

    // Once what it needs is back, the hotkey opens it again.
    block_on(launcher.set_enabled(&dirs.identity(GREETER), true));
    let opening = launcher
        .press_hotkey(&shortcut)
        .expect("the hotkey still opens its command");
    block_on(opening);
    assert_eq!(launcher.view().screen, Screen::Command);
}

#[test]
fn a_reload_of_the_dependency_that_fails_to_start_leaves_its_dependents_waiting() {
    let dirs = Dirs::new();
    let greeter = dirs.sample(GREETER);
    let caller = dirs.caller(&needs(GREETER));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);

    // The greeter's source is replaced by code that fails to start:
    // reloading it fails, and it is paused for that. The dependent waits.
    fs::copy(guest("failing_start.wasm"), greeter.join("sample_operations.wasm")).unwrap();
    manage(&launcher);
    select_title(&launcher, "Reload Rust operations sample");
    block_on(launcher.activate_selected());
    let status = launcher.view().status.clone();
    assert!(
        matches!(&status, Status::Error(text)
            if text.starts_with("Reloaded Rust operations sample, but it failed to start")),
        "{status:?}"
    );
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Call from JavaScript"),
        "Needs Rust operations sample, which is paused"
    );

    // A reload that starts brings them back.
    fs::copy(guest("sample_operations.wasm"), greeter.join("sample_operations.wasm")).unwrap();
    manage(&launcher);
    select_title(&launcher, "Reload Rust operations sample");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Reloaded Rust operations sample".into())
    );
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
}

/// A hotkey system that registers everything, so a test can assign a
/// shortcut and press it.
#[derive(Default)]
struct AnyHotkeys(Mutex<Vec<Shortcut>>);

impl Hotkeys for AnyHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        self.0.lock().unwrap().push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.0.lock().unwrap().retain(|kept| kept != shortcut);
    }
}

/// Gives the command titled `title` the global hotkey `shortcut`, as the
/// user does in Manage extensions.
fn set_hotkey(launcher: &Launcher, title: &str, shortcut: &str) {
    manage(launcher);
    activate(
        launcher,
        &format!("Hotkey for {title}"),
    );
    assert!(
        matches!(launcher.view().screen, Screen::Hotkey { .. }),
        "{:?}",
        launcher.view()
    );
    block_on(launcher.record_hotkey(Shortcut::parse(shortcut).unwrap()));
}

/// Sets the Schedule sample's command to run its item every `every`
/// seconds, and returns its folder.
fn set_every(folder: &Path, every: u64) -> PathBuf {
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    manifest["commands"][0]["schedule"] =
        serde_json::json!({ "everySeconds": every, "item": "count" });
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    folder.to_path_buf()
}
