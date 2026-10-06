//! Calls still pending when their package's code stops, through the
//! launcher's public interface: disabling, reloading or updating a package
//! stops its pending calls at once, and their late results never reach the
//! screen or the package's data. Every check runs against the settings
//! sample's "Save after waiting" in Rust, JavaScript and TypeScript, real
//! guests from `cargo xtask guests`: it saves "started", waits ten seconds,
//! then saves "finished".

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::select_title;

/// Under the ten seconds "Save after waiting" waits after saving "started":
/// a call that is not stopped cannot end sooner after it was asked, so one
/// that ends within this was stopped. The margin is for stopping it: a
/// reload first checks the new code, which a slow, busy machine takes
/// seconds to do even with the compiled code cached.
const STOPPED_WITHIN: Duration = Duration::from_secs(8);

/// How long a call may take to begin waiting, or the new code to open.
const PROMPTLY: Duration = Duration::from_secs(6);

/// A settings sample package: the same command in each language.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-settings",
    component: "sample_settings.wasm",
    title: "Settings sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-settings-js",
    component: "sample_settings_js.wasm",
    title: "JavaScript settings sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-settings-ts",
    component: "sample_settings_ts.wasm",
    title: "TypeScript settings sample",
};

/// Copies the assembled settings sample package of `fixture` into `folder`,
/// titled "Settings sample" whatever its language.
fn settings_package(fixture: &Fixture, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(fixture.package);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    let manifest = fs::read_to_string(assembled.join("pane.json")).unwrap();
    let original = format!("\"{}\"", fixture.title);
    assert!(manifest.contains(&original), "{manifest}");
    let manifest = manifest.replace(&original, "\"Settings sample\"");
    fs::write(folder.join("pane.json"), manifest).unwrap();
    fs::copy(
        assembled.join(fixture.component),
        folder.join(fixture.component),
    )
    .unwrap();
    folder.to_path_buf()
}

/// A launcher with the settings sample of one language installed. Its
/// runtime keeps compiled code in a cache, as Pane's does, so the code a
/// reload checks and starts is not compiled again (a JavaScript component
/// takes seconds to compile in a debug build on a slow machine).
struct Installed {
    _sources: TempDir,
    _cache: TempDir,
    data: TempDir,
    runtime: Runtime,
    launcher: Launcher,
    folder: PathBuf,
    identity: PackageIdentity,
}

impl Installed {
    fn new(fixture: &Fixture) -> Installed {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
        let folder = settings_package(fixture, &sources.path().join("settings"));
        block_on(launcher.install_package(&folder));
        let identity = PackageIdentity::local(&folder).unwrap();
        Installed {
            _sources: sources,
            _cache: cache,
            data,
            runtime,
            launcher,
            folder,
            identity,
        }
    }

    /// What "Save after waiting" has saved: "started", "finished" or
    /// nothing.
    fn slow_save(&self) -> Option<String> {
        let path = self.data.path().join("extensions/settings.json");
        let text = fs::read_to_string(path).unwrap_or_default();
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"slow-save\": \"{progress}\"")))
            .map(str::to_owned)
    }

    /// Opens the Greeting command and runs "Save after waiting" on another
    /// thread, returning once the guest has saved "started": its call is
    /// then waiting inside the guest.
    fn start_slow_save(&self) -> Pending {
        open_greeting_at(&self.launcher, "Save after waiting");
        let saving = self.launcher.activate_selected();
        let started = Instant::now();
        let thread = thread::spawn(move || {
            block_on(saving);
            Instant::now()
        });
        while self.slow_save().as_deref() != Some("started") {
            assert!(
                started.elapsed() < PROMPTLY,
                "the call did not start: {:?}",
                self.launcher.view().status
            );
            thread::sleep(Duration::from_millis(10));
        }
        Pending { thread, started }
    }
}

/// A call waiting inside the guest.
struct Pending {
    /// Answers when the call ended.
    thread: thread::JoinHandle<Instant>,
    /// When the call was asked, before the guest saved "started".
    started: Instant,
}

impl Pending {
    /// Waits for the call to end, asserting it was stopped rather than
    /// finishing its wait. Only the call is timed, not what the stopping
    /// operation does after it, such as a reload starting the new code.
    fn assert_stopped(self) {
        let ended = self.thread.join().unwrap();
        let took = ended - self.started;
        assert!(took < STOPPED_WITHIN, "the call ran for {took:?}");
    }
}

/// Opens the installed Greeting command from root search and selects its
/// item titled `item`.
fn open_greeting_at(launcher: &Launcher, item: &str) {
    launcher.back();
    launcher.back();
    select_title(launcher, "Greeting");
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);
    select_title(launcher, item);
}

fn disabling_stops_a_pending_call_and_discards_its_result(fixture: &Fixture) {
    let installed = Installed::new(fixture);
    let pending = installed.start_slow_save();

    block_on(installed.launcher.set_enabled(&installed.identity, false));
    pending.assert_stopped();

    let view = installed.launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Disabled Settings sample".into())
    );
    assert_eq!(installed.slow_save().as_deref(), Some("started"));
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

fn reloading_stops_a_pending_call_and_the_new_code_runs(fixture: &Fixture) {
    let installed = Installed::new(fixture);
    let pending = installed.start_slow_save();

    block_on(installed.launcher.reload(&installed.identity));
    pending.assert_stopped();

    // The command of the old code closed; its answer never shows, not even
    // over the new code's screens.
    let view = installed.launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Reloaded Settings sample".into())
    );
    assert_eq!(installed.slow_save().as_deref(), Some("started"));
    open_greeting_at(&installed.launcher, "Use a casual greeting");
    block_on(installed.launcher.activate_selected());
    assert_eq!(
        shown(&installed.launcher),
        Status::Result("Saved the casual greeting".into())
    );
    assert_eq!(installed.slow_save().as_deref(), Some("started"));
}

fn updating_stops_a_pending_call(fixture: &Fixture) {
    let installed = Installed::new(fixture);
    let pending = installed.start_slow_save();

    block_on(installed.launcher.preview_package(&installed.folder));
    select_title(&installed.launcher, "Update");
    block_on(installed.launcher.activate_selected());
    pending.assert_stopped();

    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Updated Settings sample to 0.1.0".into())
    );
    assert_eq!(installed.slow_save().as_deref(), Some("started"));
}

fn uninstalling_stops_a_pending_call_and_it_saves_nothing_more(fixture: &Fixture) {
    let installed = Installed::new(fixture);
    let pending = installed.start_slow_save();

    block_on(
        installed
            .launcher
            .uninstall(&installed.identity, SavedData::Keep),
    );
    pending.assert_stopped();

    let view = installed.launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Uninstalled Settings sample; its settings and content are kept".into())
    );
    assert_eq!(installed.slow_save().as_deref(), Some("started"));
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

fn repeated_disables_and_reloads_leave_nothing_running(fixture: &Fixture) {
    let installed = Installed::new(fixture);
    for _ in 0..3 {
        let pending = installed.start_slow_save();
        block_on(installed.launcher.set_enabled(&installed.identity, false));
        pending.assert_stopped();
        assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
        assert_eq!(block_on(installed.runtime.view_count()), 0);
        block_on(installed.launcher.set_enabled(&installed.identity, true));

        let pending = installed.start_slow_save();
        block_on(installed.launcher.reload(&installed.identity));
        pending.assert_stopped();
        // Only the new code, started by the reload, is running.
        let running = block_on(installed.runtime.running());
        assert_eq!(running.len(), 1, "{running:?}");
        let current = installed.launcher.packages()[0].location.clone();
        assert!(running[0].starts_with(&current), "{running:?}");
    }
    assert_eq!(installed.slow_save().as_deref(), Some("started"));
}

fn a_call_waiting_behind_a_stopped_one_is_served_at_once(fixture: &Fixture) {
    let installed = Installed::new(fixture);
    let pending = installed.start_slow_save();

    // Opening the command again waits for the runtime, busy with the
    // pending call, and is asked while its package is still enabled.
    installed.launcher.back();
    select_title(&installed.launcher, "Greeting");
    let opening = installed.launcher.activate_selected();
    let opened = thread::spawn(move || block_on(opening));
    thread::sleep(Duration::from_millis(100));
    block_on(installed.launcher.reload(&installed.identity));
    pending.assert_stopped();
    opened.join().unwrap();

    // The opening belonged to the replaced code, so it is not shown; the
    // new code opens at once.
    let view = installed.launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    let started = Instant::now();
    open_greeting_at(&installed.launcher, "Save after waiting");
    assert!(started.elapsed() < PROMPTLY);
}

/// Declares one test per check for each language's settings sample.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    disabling_stops_a_pending_call_and_discards_its_result,
    reloading_stops_a_pending_call_and_the_new_code_runs,
    updating_stops_a_pending_call,
    uninstalling_stops_a_pending_call_and_it_saves_nothing_more,
    repeated_disables_and_reloads_leave_nothing_running,
    a_call_waiting_behind_a_stopped_one_is_served_at_once,
);
