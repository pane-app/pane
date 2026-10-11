//! The state handoff through the launcher's public interface (#159,
//! ADR 0041): a package whose component opts in by exporting the lifecycle
//! interface keeps what it had in memory when its code is replaced — on
//! Reload, an Update the user chose and a development-mode reload, only
//! once the replacement passed its checks, never after a crash, a pause, a
//! failure to start, Retry, a disable followed by an enable, or a restart
//! of Pane — and the command screen that was on display opens again on the
//! new code with its original launch record, for every package whether or
//! not it opted in: only the command's root view, not the views or forms
//! it had pushed.
//!
//! The handoff sample in Rust, JavaScript and TypeScript (real guests
//! `cargo xtask guests` assembles) keeps a counter and a draft in memory;
//! the handoff fixture (also Rust) drives the edges: an instance busy with
//! a call, a snapshot that misses the 1-second deadline or exceeds the
//! 1-megabyte limit, and a restore that errs or traps. Development mode's
//! diagnostics report each; elsewhere a dropped snapshot is silent.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, to_root};

/// One language's handoff sample.
struct Sample {
    /// Its assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title, as the install and reload report it.
    title: &'static str,
    /// Its command's title in root search.
    command: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-handoff",
        title: "Rust handoff sample",
        command: "Handoff",
    },
    Sample {
        package: "sample-handoff-js",
        title: "JavaScript handoff sample",
        command: "Handoff (JavaScript)",
    },
    Sample {
        package: "sample-handoff-ts",
        title: "TypeScript handoff sample",
        command: "Handoff (TypeScript)",
    },
];

/// How long a guest call, a build and a wait for a thread may take:
/// compiling the guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(20);

/// One test's Pane: its data folder (outliving restarts), its source
/// folders and its runtime cache.
struct Pane {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
}

impl Pane {
    fn new() -> Pane {
        Pane {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    /// Starts Pane on this data folder, as after a restart.
    fn start(&self) -> Launcher {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
    }

    /// Starts Pane with `sample`'s package installed.
    fn installed(&self, sample: &Sample) -> (Launcher, PackageIdentity, PathBuf) {
        let started = self.start();
        let folder = self.package(sample.package);
        block_on(started.install_package(&folder));
        assert_eq!(
            started.view().status,
            Status::Result(format!("Installed {}", sample.title))
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        (started, identity, folder)
    }

    /// The assembled package `package`, copied into a source folder of this
    /// test.
    fn package(&self, package: &str) -> PathBuf {
        let folder = self.sources.path().join(package);
        let assembled = assembled(package);
        fs::create_dir_all(&folder).unwrap();
        for file in ["pane.json", component_of(package)] {
            fs::copy(assembled.join(file), folder.join(file)).unwrap();
        }
        folder
    }

    /// The handoff fixture as a package of this test, with the settings
    /// `settings` saved for it: the key `snapshot` says what the old code's
    /// `snapshot` answers (`wait` or `oversized`; nothing by default), and
    /// `restore` what the new code's `restore` does with the state it is
    /// given (`reject` or `trap`; restoring it by default).
    fn fixture(&self, name: &str, settings: &[(&str, &str)]) -> PathBuf {
        let folder = self.sources.path().join(name);
        let fixture = guest("handoff_fixture");
        fs::create_dir_all(&folder).unwrap();
        fs::copy(&fixture, folder.join("fixture.wasm")).unwrap();
        fs::write(
            folder.join("pane.json"),
            r#"{ "manifestVersion": 1, "title": "Handoff fixture", "version": "0.1.0",
                "apiVersion": "0.1",
                "commands": [{ "id": "handoff", "title": "Handoff fixture",
                                "component": "fixture.wasm" }] }"#,
        )
        .unwrap();
        for (key, value) in settings {
            let file = self.data.path().join("extensions/settings.json");
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            let mut saved: serde_json::Value = match fs::read_to_string(&file) {
                Ok(text) => serde_json::from_str(&text).unwrap(),
                Err(_) => serde_json::json!({ "version": 1, "packages": {} }),
            };
            let identity = PackageIdentity::local(&folder).unwrap();
            let entry = saved["packages"]
                .as_object_mut()
                .unwrap()
                .entry(identity.key())
                .or_insert_with(|| serde_json::json!({}));
            entry[key] = serde_json::Value::String((*value).to_owned());
            fs::write(&file, serde_json::to_string(&saved).unwrap()).unwrap();
        }
        folder
    }

    /// What the fixture of `folder` kept in its content under `key`, or
    /// `None` before the first write.
    fn kept(&self, folder: &Path, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.data.path().join("extensions/content.json")).ok()?;
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        file["packages"][&PackageIdentity::local(folder).unwrap().key()][key]
            .as_str()
            .map(str::to_owned)
    }

    /// The lines of the extension log of the package with `identity`, as
    /// development mode's diagnostics read them.
    fn reported(&self, launcher: &Launcher, identity: &PackageIdentity) -> Vec<String> {
        launcher
            .extension_log(identity)
            .into_iter()
            .map(|line| line.text)
            .collect()
    }
}

/// The assembled package `name` under `target/guests/packages`.
fn assembled(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        folder.exists(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    folder
}

/// The component file `package`'s manifest names.
fn component_of(package: &str) -> &'static str {
    match package {
        "sample-handoff" => "sample_handoff.wasm",
        "sample-handoff-js" => "sample_handoff_js.wasm",
        "sample-handoff-ts" => "sample_handoff_ts.wasm",
        "sample-rust" => "sample_rust.wasm",
        other => panic!("unknown package {other}"),
    }
}

/// The guest component `name` (`<name>.wasm`) in `target/guests`; panics
/// if it was not built.
fn guest(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm"));
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// Opens the command titled `command` from root search, returning the
/// title of the view it shows.
fn open(launcher: &Launcher, command: &str) -> String {
    to_root(launcher);
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command, "{command}: {:?}", view.status);
    view.title.clone()
}

/// Opens the command titled `command` and runs its item titled `item`,
/// returning what it showed.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    open(launcher, command);
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// Activates the extension manager's row titled `row`, returning the
/// outcome.
fn press(launcher: &Launcher, row: &str) -> Status {
    manage(launcher);
    select_title(launcher, row);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// The view's title, for asserting what the command's screen says.
fn title(launcher: &Launcher) -> String {
    launcher.view().title.clone()
}

/// The titles of the rows of the extension list.
fn listed(launcher: &Launcher) -> Vec<String> {
    manage(launcher);
    launcher
        .view()
        .rows
        .iter()
        .map(|row| row.title.clone())
        .collect()
}

/// Selects the first row whose title starts with `prefix`; panics, listing
/// the rows, if there is none.
fn select_starting(launcher: &Launcher, prefix: &str) {
    let titles: Vec<String> = launcher
        .view()
        .rows
        .iter()
        .map(|row| row.title.clone())
        .collect();
    let index = titles
        .iter()
        .position(|row| row.starts_with(prefix))
        .unwrap_or_else(|| panic!("no row starting {prefix:?} in {titles:?}"));
    launcher.select(index);
}

/// Saves the draft `text` through `sample`'s form.
fn save_draft(launcher: &Launcher, sample: &Sample, text: &str) {
    open(launcher, sample.command);
    select_starting(launcher, "Edit the draft");
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Form(_)));
    launcher.set_field_value("draft", text);
    block_on(launcher.submit_form());
}

/// Waits until `what` holds, asserting it did within [`PROMPTLY`].
fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(
            started.elapsed() < PROMPTLY,
            "{what} did not happen: {:?}",
            started.elapsed()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_reload_hands_the_counter_and_draft_over_and_reopens_the_screen() {
    for sample in &SAMPLES {
        let pane = Pane::new();
        let (launcher, identity, folder) = pane.installed(sample);

        // The state the sample keeps in memory: two counts and a draft.
        open(&launcher, sample.command);
        assert_eq!(title(&launcher), "Handoff: 0 counted, draft nothing");
        assert_eq!(
            run(&launcher, sample.command, "Add one (0 so far)"),
            Status::Result("Counted one more".into())
        );
        run(&launcher, sample.command, "Add one (1 so far)");
        save_draft(&launcher, sample, "what a replacement must keep");
        assert_eq!(
            open(&launcher, sample.command),
            "Handoff: 2 counted, draft “what a replacement must keep”"
        );

        // Its screen on display, the package is reloaded: the old code's
        // instance is asked for its state within the deadline, the new
        // code's first start restores it before anything else runs, and
        // the command's screen opens again with its launch record.
        block_on(launcher.reload(&identity));
        assert_eq!(
            launcher.view().screen,
            Screen::Command,
            "the screen that was open reopens: {:?}",
            launcher.view().status
        );
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Reloaded {}", sample.title))
        );
        assert_eq!(
            title(&launcher),
            "Handoff: 2 counted, draft “what a replacement must keep”"
        );

        // The state is the new code's own now: it carries on from it.
        assert_eq!(
            run(&launcher, sample.command, "Add one (2 so far)"),
            Status::Result("Counted one more".into())
        );
        assert_eq!(
            open(&launcher, sample.command),
            "Handoff: 3 counted, draft “what a replacement must keep”"
        );
        let _ = folder;
    }
}

#[test]
fn an_update_the_user_chose_hands_the_state_over_and_reopens_the_screen() {
    for sample in &SAMPLES {
        let pane = Pane::new();
        let (launcher, _identity, folder) = pane.installed(sample);
        run(&launcher, sample.command, "Add one (0 so far)");
        run(&launcher, sample.command, "Add one (1 so far)");
        save_draft(&launcher, sample, "kept by an update");

        // The preview's Update row, chosen with the command's screen
        // still open — as a user in Settings updates a package whose
        // command they have open in the launcher: the replacement hands
        // the state over and reopens the screen.
        block_on(launcher.preview_package(&folder));
        select_title(&launcher, "Update");
        let updating = launcher.activate_selected();
        open(&launcher, sample.command);
        block_on(updating);
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Updated {} to 0.1.0", sample.title))
        );
        assert_eq!(
            launcher.view().screen,
            Screen::Command,
            "{:?}",
            launcher.view().status
        );
        assert_eq!(
            title(&launcher),
            "Handoff: 2 counted, draft “kept by an update”"
        );
    }
}

#[test]
fn nothing_is_handed_over_after_a_disable_and_enable_or_a_restart() {
    let pane = Pane::new();
    let (launcher, identity, _folder) = pane.installed(&SAMPLES[0]);
    run(&launcher, SAMPLES[0].command, "Add one (0 so far)");

    // Disabled and enabled again: a new generation, and nothing is handed
    // over to it.
    block_on(launcher.set_enabled(&identity, false));
    block_on(launcher.set_enabled(&identity, true));
    assert_eq!(
        open(&launcher, SAMPLES[0].command),
        "Handoff: 0 counted, draft nothing"
    );

    run(&launcher, SAMPLES[0].command, "Add one (0 so far)");
    // A restart of Pane: snapshots are kept in memory only, so the new
    // code starts fresh.
    let restarted = pane.start();
    assert_eq!(
        open(&restarted, SAMPLES[0].command),
        "Handoff: 0 counted, draft nothing"
    );
}

#[test]
fn a_crashed_instance_hands_nothing_over_when_it_is_replaced() {
    let pane = Pane::new();
    let folder = pane.fixture("crashed", &[]);
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    run(&launcher, "Handoff fixture", "Add one (0 so far)");

    // The command's crash drops its instance: a replacement of the code
    // has no instance left to ask, and the new code starts fresh.
    assert_eq!(
        run(&launcher, "Handoff fixture", "Add one (1 so far)"),
        Status::Result("Counted one more".into())
    );
    let crashed = run(&launcher, "Handoff fixture", "Crash the command");
    assert!(
        matches!(crashed, Status::Error(ref message) if message.contains("crashed")),
        "{crashed:?}"
    );
    block_on(launcher.reload(&identity));
    assert_eq!(
        open(&launcher, "Handoff fixture"),
        "Handoff fixture: 0 counted"
    );
}

#[test]
fn a_failing_replacement_takes_no_snapshot_and_changes_nothing() {
    let pane = Pane::new();
    let (launcher, identity, folder) = pane.installed(&SAMPLES[0]);
    run(&launcher, SAMPLES[0].command, "Add one (0 so far)");

    // A replacement that fails its checks: nothing changes, the working
    // code keeps running with its state, and no snapshot is taken.
    fs::remove_file(folder.join("pane.json")).unwrap();
    let message = match press(&launcher, &format!("Reload {}", SAMPLES[0].title)) {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    };
    assert!(
        message.starts_with(&format!("{} was not reloaded: ", SAMPLES[0].title)),
        "{message}"
    );
    assert!(message.contains("no pane.json"), "{message}");
    // The state is the running code's own, never snapshot nor restored.
    assert_eq!(
        run(&launcher, SAMPLES[0].command, "Add one (1 so far)"),
        Status::Result("Counted one more".into())
    );
    let _ = identity;
}

#[test]
fn a_busy_instance_gives_no_snapshot_and_is_stopped_as_today() {
    let pane = Pane::new();
    let folder = pane.fixture("busy", &[]);
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    run(&launcher, "Handoff fixture", "Add one (0 so far)");

    // An item that holds for ten seconds, run on another thread: the
    // instance is busy with its call when the code is replaced.
    open(&launcher, "Handoff fixture");
    select_title(&launcher, "Hold for ten seconds");
    let holding = launcher.activate_selected();
    let held = thread::spawn(move || block_on(holding));
    until("the item to start holding", || {
        pane.kept(&folder, "held").as_deref() == Some("started")
    });

    block_on(launcher.reload(&identity));
    // The call was stopped by the replacement, as today: it never
    // finished.
    held.join().unwrap();
    assert_eq!(pane.kept(&folder, "held").as_deref(), Some("started"));

    // The busy instance gave no snapshot: the new code starts fresh, and
    // development mode's diagnostics report the dropped snapshot.
    assert_eq!(title(&launcher), "Handoff fixture: 0 counted");
    until("the diagnostic", || {
        pane.reported(&launcher, &identity).iter().any(|line| {
            line.contains("the state was not handed over to the new code: a call was still running")
        })
    });
}

#[test]
fn a_late_snapshot_is_dropped_and_the_reload_goes_ahead() {
    let pane = Pane::new();
    let folder = pane.fixture("late", &[("snapshot", "wait")]);
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    run(&launcher, "Handoff fixture", "Add one (0 so far)");

    // The old code waits ten seconds to answer: the reload waits only the
    // one second of the deadline.
    let started = Instant::now();
    assert_eq!(
        press(&launcher, "Reload Handoff fixture"),
        Status::Result("Reloaded Handoff fixture".into())
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "the deadline did not bound the reload: {:?}",
        started.elapsed()
    );

    // The late snapshot was dropped: the new code starts fresh, and
    // development mode's diagnostics report it.
    assert_eq!(
        open(&launcher, "Handoff fixture"),
        "Handoff fixture: 0 counted"
    );
    until("the diagnostic", || {
        pane.reported(&launcher, &identity).iter().any(|line| {
            line.contains(
                "the state was not handed over to the new code: the old code did not \
                 answer within 1 second",
            )
        })
    });
}

#[test]
fn an_oversized_snapshot_is_dropped_and_the_reload_goes_ahead() {
    let pane = Pane::new();
    let folder = pane.fixture("oversized", &[("snapshot", "oversized")]);
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    run(&launcher, "Handoff fixture", "Add one (0 so far)");

    assert_eq!(
        press(&launcher, "Reload Handoff fixture"),
        Status::Result("Reloaded Handoff fixture".into())
    );
    assert_eq!(
        open(&launcher, "Handoff fixture"),
        "Handoff fixture: 0 counted"
    );
    until("the diagnostic", || {
        pane.reported(&launcher, &identity)
            .iter()
            .any(|line| line.contains("the old code answered 1048577 bytes"))
    });
}

#[test]
fn a_restore_that_errs_starts_fresh_and_is_not_a_failure() {
    let pane = Pane::new();
    let folder = pane.fixture("rejecting", &[("restore", "reject")]);
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    run(&launcher, "Handoff fixture", "Add one (0 so far)");
    run(&launcher, "Handoff fixture", "Add one (1 so far)");

    // The new code answers an error from `restore`: the state is discarded
    // and the extension starts fresh, which is not a failure — the reload
    // succeeds and nothing is paused. Development mode's diagnostics
    // report the rejected state.
    assert_eq!(
        press(&launcher, "Reload Handoff fixture"),
        Status::Result("Reloaded Handoff fixture".into())
    );
    assert_eq!(
        open(&launcher, "Handoff fixture"),
        "Handoff fixture: 0 counted"
    );
    let rows = listed(&launcher);
    assert!(
        !rows.iter().any(|row| row.starts_with("Retry")),
        "nothing is paused: {rows:?}"
    );
    until("the diagnostic", || {
        pane.reported(&launcher, &identity)
            .iter()
            .any(|line| line.contains("the new code rejected it"))
    });
}

#[test]
fn a_restore_that_traps_during_a_reloads_start_pauses_the_package() {
    let pane = Pane::new();
    let folder = pane.fixture("trapping", &[("restore", "trap")]);
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    run(&launcher, "Handoff fixture", "Add one (0 so far)");

    // A trap in `restore` during the reload's start is a startup failure:
    // the package is paused, with Retry, and the state is lost.
    let message = match press(&launcher, "Reload Handoff fixture") {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    };
    assert!(
        message.starts_with("Reloaded Handoff fixture, but it failed to start"),
        "{message}"
    );
    assert!(
        message.contains("\"Why Handoff fixture is paused\""),
        "{message}"
    );
    assert!(
        listed(&launcher).contains(&"Retry starting Handoff fixture".to_string()),
        "no Retry row"
    );

    // Retry starts the same code again: nothing was handed over to it, so
    // it starts fresh.
    assert_eq!(
        press(&launcher, "Retry starting Handoff fixture"),
        Status::Result("Started Handoff fixture".into())
    );
    assert_eq!(
        open(&launcher, "Handoff fixture"),
        "Handoff fixture: 0 counted"
    );
    let _ = identity;
}

#[test]
fn the_open_screen_of_a_package_that_does_not_opt_in_reopens() {
    let pane = Pane::new();
    let folder = pane.package("sample-rust");
    let launcher = pane.start();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();

    // The sample's screen, with a view it pushed: the command's root view
    // is reopened, not the pushed one. The row the user had selected is
    // selected again where the new list has it: the host-owned state kept
    // by key.
    open(&launcher, "Rust sample");
    launcher.select(1);
    select_title(&launcher, "Choose a color");
    // The root list's selection as the view is pushed: the row the user
    // chose, which the reopened screen keeps by key.
    let view = launcher.view();
    let selected = view
        .selected
        .and_then(|at| view.rows.get(at))
        .map(|row| row.id.clone())
        .unwrap_or_default();
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::CustomView(_)));

    block_on(launcher.reload(&identity));
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "the command's root view reopens: {:?}",
        launcher.view().status
    );
    assert_eq!(
        launcher.view().status,
        Status::Result("Reloaded Rust sample".into())
    );
    assert_eq!(title(&launcher), "Rust sample");
    let view = launcher.view();
    let at = view.selected.unwrap_or_default();
    assert_eq!(view.rows[at].id, selected, "the selection is kept by key");
}
