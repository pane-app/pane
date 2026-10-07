//! A crash of Pane's extension runtime thread itself (#17), through the
//! launcher's public interface, with real guests from `cargo xtask guests`:
//! the settings sample and the helper sample, whose helper program runs
//! while the runtime crashes. The crash is injected (`Runtime::inject`): no
//! guest can crash the runtime thread, only a fault in Pane or Wasmtime.
//!
//! Pane then keeps its window's navigation and Manage extensions usable,
//! explains the failure without naming an extension (it cannot tell which,
//! if any, caused it), pauses nothing, ends the helpers the runtime ran,
//! keeps saved data, restarts the runtime once by itself but not again
//! after a second crash soon after, and never runs a call again by itself:
//! an action whose answer was lost after it saved stays done once.
//!
//! Faults are injected in debug builds only.
#![cfg(debug_assertions)]

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{
    CallError, Fault, Launcher, PackageIdentity, Runtime, RuntimeStatus, Screen, Status, Target,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles, to_root};

const WHY_ROW: &str = "Why the extension runtime stopped";
const RESTART_ROW: &str = "Restart the extension runtime";

/// How long the runtime may take to crash, clean up and report.
const PROMPTLY: Duration = Duration::from_secs(8);

fn assembled(package: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(package);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// Copies the files `files` of the assembled package `package` into
/// `folder`.
fn copy_package(package: &str, files: &[String], folder: &Path) -> PathBuf {
    let from = assembled(package);
    for file in files.iter().map(String::as_str).chain(["pane.json"]) {
        let to = folder.join(file);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(from.join(file), &to).unwrap_or_else(|error| panic!("{file}: {error}"));
    }
    folder.to_path_buf()
}

/// Where `pane-echo --wait` beats every 20 ms while it runs, in its
/// working folder.
const ALIVE: &str = "pane-echo.alive";

/// The length of the heartbeat file at `path`, if it exists.
fn beats(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn this() -> Target {
    Target::current().expect("Pane names this system's target")
}

/// This system's helper file in the helper sample.
fn helper_file() -> String {
    format!("helpers/{}/pane-echo{}", this().id(), this().exe_suffix())
}

struct Pane {
    _sources: TempDir,
    data: TempDir,
    runtime: Runtime,
    launcher: Launcher,
    settings: PackageIdentity,
    helper: PackageIdentity,
}

/// However a test ends, its helpers end with it, as they do when Pane
/// quits.
impl Drop for Pane {
    fn drop(&mut self) {
        self.runtime.stop_helpers();
    }
}

impl Pane {
    /// A Pane with the settings sample and the helper sample installed.
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
        let settings = copy_package(
            "sample-settings",
            &["sample_settings.wasm".into()],
            &sources.path().join("settings"),
        );
        let helper = copy_package(
            "sample-helper",
            &["sample_helper.wasm".into(), helper_file()],
            &sources.path().join("helper"),
        );
        for (folder, title) in [(&settings, "Settings sample"), (&helper, "Helper sample")] {
            block_on(launcher.install_package(folder));
            assert_eq!(
                launcher.view().status,
                Status::Result(format!("Installed {title}"))
            );
        }
        Pane {
            settings: PackageIdentity::local(&settings).unwrap(),
            helper: PackageIdentity::local(&helper).unwrap(),
            _sources: sources,
            data,
            runtime,
            launcher,
        }
    }

    /// The saved value `key` of `kind` ("settings", "content") of the
    /// package `identity`.
    fn saved(&self, kind: &str, identity: &PackageIdentity, key: &str) -> Option<String> {
        let path = self.data.path().join(format!("extensions/{kind}.json"));
        let text = fs::read_to_string(path).ok()?;
        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        saved["packages"][identity.key()][key]
            .as_str()
            .map(str::to_owned)
    }

    /// Waits until the runtime reports a crash, and returns what it does.
    fn crashed(&self) -> RuntimeStatus {
        let started = Instant::now();
        loop {
            let status = self.runtime.status();
            if status != RuntimeStatus::Running {
                return status;
            }
            assert!(started.elapsed() < PROMPTLY, "no crash was reported");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Crashes the runtime while nothing runs, and waits for the report.
    fn crash(&self) -> RuntimeStatus {
        let before = self.runtime.status();
        self.runtime.inject(Fault::Crash);
        let started = Instant::now();
        loop {
            let status = self.runtime.status();
            if status != before && status != RuntimeStatus::Running {
                return status;
            }
            assert!(started.elapsed() < PROMPTLY, "no crash was reported");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits until the launcher's status line shows the crash, as the
    /// crash report or the lost call's answer puts it, and returns it.
    fn toast(&self, wanted: &str) -> String {
        let started = Instant::now();
        loop {
            if let Status::Error(text) = self.launcher.view().status
                && text.contains(wanted)
            {
                return text;
            }
            assert!(
                started.elapsed() < PROMPTLY,
                "no {wanted:?} in {:?}",
                self.launcher.view().status
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Whether Pane paused any package.
    fn any_paused(&self) -> bool {
        manage(&self.launcher);
        self.launcher
            .view()
            .rows
            .iter()
            .any(|row| row.subtitle.as_deref().unwrap_or("").contains("Paused"))
    }
}

/// From root search, opens `command` and selects its item `item`.
fn open_at(launcher: &Launcher, command: &str, item: &str) {
    to_root(launcher);
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
    select_title(launcher, item);
}

/// From root search, opens `command` and runs its item `item`, returning
/// the outcome: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    open_at(launcher, command, item);
    block_on(launcher.activate_selected());
    shown(launcher)
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

/// Neither package's title appears in `text`: the crash is not blamed on
/// the last extension that ran.
fn names_no_extension(text: &str) {
    for title in ["Settings sample", "Helper sample", "Greeting"] {
        assert!(!text.contains(title), "{title} is named in {text:?}");
    }
}

#[test]
fn a_crash_while_two_extensions_run_keeps_pane_usable_and_ends_the_helper() {
    let pane = Pane::new();
    // The settings sample runs, and so does the helper sample's helper.
    assert_eq!(
        run(&pane.launcher, "Greeting", "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );
    let helper = pane
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == pane.helper)
        .unwrap();
    let alive = helper
        .location
        .join(Path::new(&helper_file()).parent().unwrap())
        .join(ALIVE);
    open_at(&pane.launcher, "Helper sample", "Echo after waiting");
    let waiting = pane.launcher.activate_selected();
    let started = Instant::now();
    let call = thread::spawn(move || block_on(waiting));
    while pane.runtime.helper_processes().is_empty() || beats(&alive).is_none() {
        assert!(started.elapsed() < PROMPTLY, "the helper did not start");
        thread::sleep(Duration::from_millis(5));
    }

    pane.runtime.inject(Fault::Crash);

    // The waiting call ends at once, and so does the helper.
    call.join().unwrap();
    assert!(
        started.elapsed() < PROMPTLY,
        "the call waited for its helper"
    );
    let why = match pane.crashed() {
        RuntimeStatus::Restarted { why, .. } => why,
        other => panic!("expected a restart, got {other:?}"),
    };
    assert!(why.contains("fault injected"), "{why}");
    assert_eq!(pane.runtime.helper_processes(), Vec::<u32>::new());
    // A process id is not trusted (the system may reuse it): the helper
    // beats no more.
    let last = beats(&alive);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(beats(&alive), last, "the helper still beats");
    // The status line explains it, naming no extension.
    let toast = pane.toast("xtension runtime");
    names_no_extension(&toast);
    // Navigation and Manage extensions work; the details name no extension
    // either, and nothing is paused.
    assert!(!pane.any_paused());
    press(&pane.launcher, WHY_ROW);
    let view = pane.launcher.view();
    assert!(
        matches!(view.screen, Screen::RuntimeDetails { .. }),
        "{:?}",
        view.screen
    );
    let details = view.details().join("\n");
    names_no_extension(&details);
    assert!(details.contains("fault injected"), "{details}");
    assert!(details.contains("started it again"), "{details}");
    // Saved data is kept: the style, and the helper sample's note that it
    // started waiting, which did not finish.
    assert_eq!(
        pane.saved("settings", &pane.settings, "greeting-style"),
        Some("formal".into())
    );
    assert_eq!(
        pane.saved("settings", &pane.helper, "helper-wait"),
        Some("started".into())
    );
    // The restarted runtime runs both extensions again, when asked.
    assert_eq!(
        run(&pane.launcher, "Greeting", "Greet me"),
        Status::Result("Good day to you".into())
    );
    assert_eq!(
        run(&pane.launcher, "Helper sample", "Echo through the helper"),
        Status::Result(format!("Echoed \"hello from Pane\" on {}", this()))
    );
    // Disable and uninstall work.
    assert_eq!(
        press(&pane.launcher, "Helper sample"),
        Status::Result("Disabled Helper sample".into())
    );
    press(&pane.launcher, "Uninstall Settings sample");
    assert!(matches!(
        pane.launcher.view().screen,
        Screen::Confirm { .. }
    ));
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Uninstalled Settings sample; its settings and content are kept".into())
    );
}

#[test]
fn a_second_crash_soon_after_stops_the_runtime_until_the_user_restarts_it() {
    let pane = Pane::new();
    assert!(matches!(pane.crash(), RuntimeStatus::Restarted { .. }));

    let status = pane.crash();

    let RuntimeStatus::Stopped { not_restarted, .. } = &status else {
        panic!("expected it stopped, got {status:?}");
    };
    assert!(
        not_restarted.contains("twice within 5 minutes"),
        "{not_restarted}"
    );
    let toast = pane.toast("not restarted");
    names_no_extension(&toast);
    // Nothing runs: a command explains why and where to restart it.
    to_root(&pane.launcher);
    select_title(&pane.launcher, "Greeting");
    block_on(pane.launcher.activate_selected());
    let refused = error(pane.launcher.view().status);
    assert!(
        refused.contains("restart it in Manage extensions"),
        "{refused}"
    );
    // The extension list works, and pauses nothing; a disable is recorded.
    assert!(!pane.any_paused());
    assert_eq!(
        press(&pane.launcher, "Helper sample"),
        Status::Result("Disabled Helper sample".into())
    );
    press(&pane.launcher, WHY_ROW);
    let details = pane.launcher.view().details().join("\n");
    assert!(details.contains("did not start it again"), "{details}");
    assert_eq!(titles(&pane.launcher), [RESTART_ROW]);

    // Restarting it runs extensions again.
    block_on(pane.launcher.activate_selected());

    assert_eq!(pane.runtime.status(), RuntimeStatus::Running);
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Restarted the extension runtime".into())
    );
    manage(&pane.launcher);
    assert!(
        !titles(&pane.launcher)
            .iter()
            .any(|title| title == RESTART_ROW)
    );
    assert!(!titles(&pane.launcher).iter().any(|title| title == WHY_ROW));
    assert_eq!(
        run(&pane.launcher, "Greeting", "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
    // A crash after the user's restart is a first one again.
    assert!(matches!(pane.crash(), RuntimeStatus::Restarted { .. }));
}

#[test]
fn an_action_whose_answer_was_lost_is_not_run_again() {
    let pane = Pane::new();
    assert_eq!(
        run(&pane.launcher, "Greeting", "Count"),
        Status::Result("Counted 1".into())
    );

    open_at(&pane.launcher, "Greeting", "Count");
    pane.runtime.inject(Fault::CrashBeforeAnswer {
        item: "count".into(),
    });
    block_on(pane.launcher.activate_selected());

    // The action ran and saved; its answer was lost.
    assert!(matches!(pane.crashed(), RuntimeStatus::Restarted { .. }));
    assert_eq!(
        pane.saved("content", &pane.settings, "count"),
        Some("2".into())
    );
    assert!(!pane.any_paused());
    // Nothing runs it again by itself, restarted or not.
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        pane.saved("content", &pane.settings, "count"),
        Some("2".into())
    );
    // Running it again is the user's choice.
    assert_eq!(
        run(&pane.launcher, "Greeting", "Count"),
        Status::Result("Counted 3".into())
    );
}

#[test]
fn a_call_whose_answer_was_lost_says_it_was_not_run_again() {
    let pane = Pane::new();
    pane.runtime.inject(Fault::CrashBeforeAnswer {
        item: "count".into(),
    });
    let path = pane.launcher.packages()[0]
        .location
        .join("sample_settings.wasm");
    // Other calls answer as usual (here, that a command built into Pane
    // keeps no settings): the fault is for Count only.
    let view = block_on(pane.runtime.render(&path));
    assert!(matches!(view, Err(CallError::Guest(_))), "{view:?}");

    let answer = block_on(pane.runtime.run_item(&path, "count"));

    let Err(CallError::RuntimeUnavailable(reason)) = answer else {
        panic!("expected the runtime unavailable, got {answer:?}");
    };
    assert!(reason.contains("does not run this again"), "{reason}");
}
