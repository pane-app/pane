//! Development mode through the launcher's public interface: a local
//! package whose source folder is watched is built after each save and, when
//! the build succeeds, reloaded, while Pane and other packages keep
//! running. A build that fails keeps the working code and shows its
//! diagnostics; a save during a build makes that build obsolete; ending
//! development stops the watcher and the build.
//!
//! The file watcher is the system's; the build is a stand-in that copies the
//! real guest named in the folder's `source.txt` (from `cargo xtask guests`)
//! to the package's component, so these tests need no compiler. The real
//! Rust and JavaScript builds are exercised in `develop_builds.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{Development, Launcher, PackageIdentity, Runtime, SavedData, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage extensions…";

/// The stand-in build's shared state: which packages it built, how often
/// it was stopped, and a gate builds wait at.
#[derive(Default)]
struct Probe {
    /// The folder names of the builds begun, in order.
    runs: Mutex<Vec<String>>,
    stopped: AtomicUsize,
    /// How many more builds may pass the gate; `None` while it is open.
    permits: Mutex<Option<usize>>,
    opened: Condvar,
    /// Whether builds ignore being stopped, as a tool might.
    stubborn: AtomicBool,
}

impl Probe {
    /// Makes builds wait until they are let through.
    fn close(&self) {
        *self.permits.lock().unwrap() = Some(0);
    }

    /// Lets the next waiting build through.
    fn let_one_through(&self) {
        if let Some(permits) = self.permits.lock().unwrap().as_mut() {
            *permits += 1;
        }
        self.opened.notify_all();
    }

    fn open(&self) {
        *self.permits.lock().unwrap() = None;
        self.opened.notify_all();
    }

    fn runs(&self) -> usize {
        self.runs.lock().unwrap().len()
    }

    fn runs_of(&self, title: &str) -> usize {
        self.runs
            .lock()
            .unwrap()
            .iter()
            .filter(|run| *run == title)
            .count()
    }

    fn stopped(&self) -> usize {
        self.stopped.load(Ordering::SeqCst)
    }
}

/// Builds a folder holding `source.txt` by staging the guest it names as
/// `command.wasm`, or fails with its text when that starts with "error".
struct FakeBuilder(Arc<Probe>);

struct FakeBuild {
    folder: PathBuf,
    probe: Arc<Probe>,
}

impl Builder for FakeBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        if !folder.join("source.txt").exists() {
            return Err("it has no source.txt".into());
        }
        Ok(Arc::new(FakeBuild {
            folder: folder.to_path_buf(),
            probe: self.0.clone(),
        }))
    }
}

impl Build for FakeBuild {
    fn command(&self) -> String {
        "fake build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == Path::new("command.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        // What was saved when the build started.
        let source = fs::read_to_string(self.folder.join("source.txt")).unwrap();
        let name = self
            .folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        self.probe.runs.lock().unwrap().push(name);
        let mut permits = self.probe.permits.lock().unwrap();
        while *permits == Some(0) {
            if job.is_stopped() && !self.probe.stubborn.load(Ordering::SeqCst) {
                self.probe.stopped.fetch_add(1, Ordering::SeqCst);
                return BuildOutcome::Stopped;
            }
            permits = self
                .probe
                .opened
                .wait_timeout(permits, Duration::from_millis(20))
                .unwrap()
                .0;
        }
        if let Some(permits) = permits.as_mut() {
            *permits -= 1;
        }
        drop(permits);
        let source = source.trim();
        if source.starts_with("error") {
            job.line("   Compiling dev");
            job.line(source);
            return BuildOutcome::Failed("fake build failed".into());
        }
        fs::copy(guest(source), job.staging().join("command.wasm")).unwrap();
        BuildOutcome::Built
    }
}

/// Writes a package folder titled `title` whose source is the guest
/// `source`, built.
fn package(folder: &Path, title: &str, source: &str) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        format!(
            r#"{{
  "manifestVersion": 1,
  "title": "{title}",
  "apiVersion": "0.1",
  "commands": [{{ "id": "open", "title": "Open {title}", "component": "command.wasm" }}]
}}"#
        ),
    )
    .unwrap();
    fs::write(folder.join("source.txt"), source).unwrap();
    fs::copy(guest(source), folder.join("command.wasm")).unwrap();
    folder.to_path_buf()
}

/// Saves `source` in the package's source, as an author's editor would.
fn save(folder: &Path, source: &str) {
    fs::write(folder.join("source.txt"), source).unwrap();
}

struct Dev {
    _sources: TempDir,
    _data: TempDir,
    sources: PathBuf,
    launcher: Launcher,
    probe: Arc<Probe>,
}

impl Dev {
    fn new() -> Dev {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let probe = Arc::new(Probe::default());
        let (changes, _) = pane_core::changes::channel();
        let launcher = Launcher::with_packages(
            Ok(Runtime::start().unwrap()),
            vec![],
            data.path().join("extensions"),
        )
        .with_development(Arc::new(FakeBuilder(probe.clone())), changes);
        Dev {
            sources: sources.path().to_path_buf(),
            _sources: sources,
            _data: data,
            launcher,
            probe,
        }
    }

    /// Installs the package `title` built from the guest `source`.
    fn install(&self, title: &str, source: &str) -> (PathBuf, PackageIdentity) {
        let folder = package(&self.sources.join(title), title, source);
        block_on(self.launcher.install_package(&folder));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
        (folder.clone(), PackageIdentity::local(&folder).unwrap())
    }

    /// Installs `title` and develops it.
    fn developing(&self, title: &str, source: &str) -> (PathBuf, PackageIdentity) {
        let (folder, identity) = self.install(title, source);
        block_on(self.launcher.start_developing(&identity));
        assert!(
            self.launcher.development(&identity).is_some(),
            "{:?}",
            self.launcher.view().status
        );
        (folder, identity)
    }

    /// Waits until `finished` of the package's saves have been acted on.
    fn finished(&self, identity: &PackageIdentity, finished: u64) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let development = self.launcher.development(identity);
            if development.as_ref().is_some_and(|d| d.finished >= finished) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {finished} saves acted on: {development:?}, builds {:?}",
                self.probe.runs.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits until the package's development has no build running or
    /// waiting, and no save not yet built.
    fn idle(&self, identity: &PackageIdentity) {
        self.until(identity, "no build", |d| {
            !d.building && !d.pending && !d.waiting
        });
    }

    /// Waits until the package's development says `what`.
    fn until(&self, identity: &PackageIdentity, what: &str, done: impl Fn(&Development) -> bool) {
        wait_until(what, || {
            self.launcher
                .development(identity)
                .is_some_and(|d| done(&d))
        });
    }

    /// Develops a second package, "Clock", whose builds mark time: once a
    /// save of it has been built, the watchers have told of earlier saves.
    fn clock(&self) -> (PathBuf, PackageIdentity) {
        self.developing("Clock", "sample_rust")
    }

    /// Saves the clock and waits until its build was acted on.
    fn tick(&self, clock: &(PathBuf, PackageIdentity), source: &str) {
        let finished = self.launcher.development(&clock.1).unwrap().finished;
        save(&clock.0, source);
        self.finished(&clock.1, finished + 1);
    }
}

/// How long a wait may take: long, as the tests may share a slow machine.
const DEADLINE: Duration = Duration::from_secs(120);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn to_root(launcher: &Launcher) {
    for _ in 0..3 {
        launcher.back();
    }
}

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    to_root(launcher);
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

fn manage(launcher: &Launcher) {
    to_root(launcher);
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// Activates the extension manager's row titled `row`, returning the
/// outcome.
fn press(launcher: &Launcher, row: &str) -> Status {
    manage(launcher);
    select_title(launcher, row);
    block_on(launcher.activate_selected());
    launcher.view().status
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

const RUST: &str = "Hello from the Rust guest";
const JAVASCRIPT: &str = "Hello from the JavaScript guest";
const TYPESCRIPT: &str = "Hello from the TypeScript guest";

#[test]
fn saving_builds_and_reloads_only_that_package() {
    let dev = Dev::new();
    dev.install("Other", "sample_js");
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );

    save(&folder, "sample_ts");
    dev.finished(&identity, 1);
    assert_eq!(
        dev.launcher.view().status,
        Status::Result("Reloaded Dev".into())
    );
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(TYPESCRIPT.into())
    );
    assert_eq!(
        run(&dev.launcher, "Open Other", "Say hello"),
        Status::Result(JAVASCRIPT.into())
    );

    // Each save builds again.
    save(&folder, "sample_js");
    dev.finished(&identity, 2);
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(JAVASCRIPT.into())
    );
    assert_eq!(dev.probe.runs(), 2);
}

#[test]
fn a_build_that_fails_keeps_the_working_code_and_shows_its_diagnostics() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");

    save(&folder, "error[E0308]: mismatched types");
    dev.finished(&identity, 1);
    let message = error(dev.launcher.view().status);
    assert_eq!(
        message,
        "Dev did not build: error[E0308]: mismatched types. It keeps running its installed \
         code; the diagnostics are under \"Why Dev did not build\" in Manage extensions."
    );
    let failure = dev
        .launcher
        .development(&identity)
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.summary, "error[E0308]: mismatched types");
    assert_eq!(
        failure.output,
        [
            "   Compiling dev",
            "error[E0308]: mismatched types",
            "fake build failed"
        ]
    );
    // The whole output is in a log file under Pane's data folder.
    let log = failure.log.clone().unwrap();
    let data = fs::canonicalize(dev._data.path()).unwrap();
    assert!(
        fs::canonicalize(&log).unwrap().starts_with(&data),
        "{}",
        log.display()
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "   Compiling dev\nerror[E0308]: mismatched types\nfake build failed\n"
    );
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );

    // The diagnostics, and a row that builds again.
    press(&dev.launcher, "Why Dev did not build");
    let view = dev.launcher.view();
    assert!(matches!(view.screen, Screen::BuildDetails { .. }));
    assert_eq!(view.title, "Why Dev did not build");
    let details = view.details().to_vec();
    assert!(
        details.contains(&"Build command: fake build".to_string()),
        "{details:?}"
    );
    assert!(
        details.contains(&"error[E0308]: mismatched types".to_string()),
        "{details:?}"
    );
    assert!(
        details.contains(&format!("The whole output is in {}", log.display())),
        "{details:?}"
    );
    assert_eq!(titles(&dev.launcher), ["Build Dev again"]);
    block_on(dev.launcher.activate_selected());
    dev.finished(&identity, 2);
    assert_eq!(dev.probe.runs(), 2);

    // Fixing it reloads, and the failure is gone.
    save(&folder, "sample_ts");
    dev.finished(&identity, 3);
    assert_eq!(dev.launcher.development(&identity).unwrap().failure, None);
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(TYPESCRIPT.into())
    );
    manage(&dev.launcher);
    assert!(!titles(&dev.launcher).contains(&"Why Dev did not build".to_string()));
}

#[test]
fn a_build_that_fails_to_start_is_paused_with_retry_and_not_rolled_back() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");

    save(&folder, "failing_start");
    dev.finished(&identity, 1);
    let message = error(dev.launcher.view().status);
    assert!(
        message
            .starts_with("Reloaded Dev, but it failed to start; its earlier code is not restored."),
        "{message}"
    );
    manage(&dev.launcher);
    assert!(titles(&dev.launcher).contains(&"Retry starting Dev".to_string()));

    // A fixed save reloads it, which ends the pause.
    save(&folder, "sample_rust");
    dev.finished(&identity, 2);
    assert_eq!(
        dev.launcher.view().status,
        Status::Result("Reloaded Dev".into())
    );
    assert!(!titles(&dev.launcher).iter().any(|t| t.starts_with("Retry")));
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );
}

#[test]
fn a_save_during_a_build_makes_that_build_obsolete() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.close();

    save(&folder, "sample_js");
    wait_until("the first build", || dev.probe.runs() == 1);
    assert!(dev.launcher.development(&identity).unwrap().building);
    save(&folder, "sample_ts");
    dev.until(&identity, "the save during the build", |d| d.pending);
    // Let the first build finish: it built the older save.
    dev.probe.open();

    dev.finished(&identity, 1);
    let development = dev.launcher.development(&identity).unwrap();
    assert_eq!((development.obsolete, development.finished), (1, 1));
    assert_eq!(dev.probe.runs(), 2);
    // Only the newer build was reloaded, and it is in the source folder for
    // a later Reload.
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(TYPESCRIPT.into())
    );
    assert_eq!(
        fs::read(folder.join("command.wasm")).unwrap(),
        fs::read(guest("sample_ts")).unwrap()
    );
}

#[test]
fn an_obsolete_build_leaves_the_installed_code_in_the_source_folder() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.close();
    save(&folder, "sample_js");
    wait_until("the first build", || dev.probe.runs() == 1);
    // What a build that writes into the folder (cargo's target) leaves.
    fs::copy(guest("sample_js"), folder.join("command.wasm")).unwrap();
    save(&folder, "error: not yet");
    dev.until(&identity, "the save during the build", |d| d.pending);
    dev.probe.open();
    dev.finished(&identity, 1);
    assert!(
        dev.launcher
            .development(&identity)
            .unwrap()
            .failure
            .is_some()
    );

    // A Reload reloads the installed code, not the obsolete build.
    assert_eq!(
        fs::read(folder.join("command.wasm")).unwrap(),
        fs::read(guest("sample_rust")).unwrap()
    );
    block_on(dev.launcher.reload(&identity));
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );
}

#[test]
fn sources_that_keep_changing_stop_the_builds_until_the_next_save() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.close();
    let sources = ["sample_js", "sample_ts", "sample_js", "sample_ts"];
    save(&folder, sources[0]);
    for (build, source) in sources[1..].iter().enumerate() {
        wait_until("the build", || dev.probe.runs() == build + 1);
        save(&folder, source);
        dev.until(&identity, "the save during the build", |d| d.pending);
        dev.probe.let_one_through();
    }
    dev.finished(&identity, 1);
    let development = dev.launcher.development(&identity).unwrap();
    assert_eq!((development.obsolete, dev.probe.runs()), (3, 3));
    assert_eq!(
        error(dev.launcher.view().status),
        "Dev was not reloaded: its sources kept changing during 3 builds in a row. Save again \
         to build it."
    );
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );

    // The next save builds it.
    dev.probe.open();
    save(&folder, "sample_ts");
    dev.finished(&identity, 2);
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(TYPESCRIPT.into())
    );
}

#[test]
fn a_build_that_ends_during_a_reload_waits_for_it() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.close();
    save(&folder, "sample_js");
    wait_until("the build", || dev.probe.runs() == 1);

    // A Reload of the source folder holds the package until awaited.
    let manual = dev.launcher.reload(&identity);
    dev.probe.open();
    dev.until(&identity, "the build to wait", |d| d.waiting);
    block_on(manual);
    dev.finished(&identity, 1);
    assert_eq!(
        dev.launcher.view().status,
        Status::Result("Reloaded Dev".into())
    );
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(JAVASCRIPT.into())
    );
}

#[test]
fn a_build_that_ends_after_development_stopped_is_dropped() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.stubborn.store(true, Ordering::SeqCst);
    dev.probe.close();
    save(&folder, "sample_js");
    wait_until("the build", || dev.probe.runs() == 1);
    let manual = dev.launcher.reload(&identity);
    dev.probe.open();
    dev.until(&identity, "the build to wait", |d| d.waiting);

    dev.launcher.stop_developing(&identity);
    block_on(manual);
    assert_eq!(
        dev.launcher.view().status,
        Status::Result("Reloaded Dev".into())
    );
    let clock = dev.clock();
    dev.tick(&clock, "sample_ts");
    // No word of the dropped build, and only the Reload, of the source
    // folder, happened.
    assert_eq!(
        dev.launcher.view().status,
        Status::Result("Reloaded Clock".into())
    );
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );
}

#[test]
fn stopping_development_stops_its_build_and_its_watcher() {
    let dev = Dev::new();
    let clock = dev.clock();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.close();
    save(&folder, "sample_js");
    wait_until("the build", || dev.probe.runs_of("Dev") == 1);

    assert_eq!(
        press(&dev.launcher, "Stop developing Dev"),
        Status::Result("Stopped developing Dev".into())
    );
    wait_until("the build to stop", || dev.probe.stopped() == 1);
    assert!(dev.launcher.development(&identity).is_none());
    assert!(titles(&dev.launcher).contains(&"Develop Dev".to_string()));

    // Nothing watches the folder any more.
    dev.probe.open();
    save(&folder, "sample_ts");
    dev.tick(&clock, "sample_js");
    assert_eq!(dev.probe.runs_of("Dev"), 1);
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );
}

#[test]
fn disabling_or_uninstalling_a_package_ends_its_development() {
    let dev = Dev::new();
    let clock = dev.clock();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    dev.probe.close();
    save(&folder, "sample_js");
    wait_until("the build", || dev.probe.runs_of("Dev") == 1);

    block_on(dev.launcher.set_enabled(&identity, false));
    wait_until("the build to stop", || dev.probe.stopped() == 1);
    assert!(dev.launcher.development(&identity).is_none());
    // Enabling it again does not develop it again.
    block_on(dev.launcher.set_enabled(&identity, true));
    assert!(dev.launcher.development(&identity).is_none());
    dev.probe.open();

    block_on(dev.launcher.start_developing(&identity));
    assert!(dev.launcher.development(&identity).is_some());
    block_on(dev.launcher.uninstall(&identity, SavedData::Keep));
    assert!(dev.launcher.development(&identity).is_none());
    save(&folder, "sample_ts");
    dev.tick(&clock, "sample_js");
    assert_eq!(dev.probe.runs_of("Dev"), 1);
}

#[test]
fn a_build_status_does_not_replace_what_another_screen_says() {
    let dev = Dev::new();
    dev.install("Other", "sample_js");
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    let answer = run(&dev.launcher, "Open Other", "Say hello");
    assert_eq!(answer, Status::Result(JAVASCRIPT.into()));

    save(&folder, "sample_ts");
    dev.finished(&identity, 1);
    assert!(matches!(dev.launcher.view().screen, Screen::Command));
    assert_eq!(shown(&dev.launcher), answer);
    // Back at root search, it is shown.
    to_root(&dev.launcher);
    assert_eq!(
        dev.launcher.view().status,
        Status::Result("Reloaded Dev".into())
    );
}

#[test]
fn a_folder_moved_into_the_source_folder_is_watched() {
    let dev = Dev::new();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    let outside = dev.sources.join("lib");
    fs::create_dir(&outside).unwrap();
    fs::rename(&outside, folder.join("lib")).unwrap();
    dev.finished(&identity, 1);
    dev.idle(&identity);

    // A save in it builds. How many events a move and a write are differs
    // by system, so what is counted is builds since.
    let finished = dev.launcher.development(&identity).unwrap().finished;
    let runs = dev.probe.runs_of("Dev");
    fs::write(folder.join("lib/util.txt"), "saved").unwrap();
    dev.finished(&identity, finished + 1);
    assert!(dev.probe.runs_of("Dev") > runs);
    dev.idle(&identity);

    // Editors' temporary files are not saves, although the folder they are
    // in changes (Windows reports it as modified).
    let clock = dev.clock();
    let runs = dev.probe.runs_of("Dev");
    fs::write(folder.join("lib/4913"), "").unwrap();
    fs::write(folder.join("lib/util.txt~"), "").unwrap();
    fs::write(folder.join("lib/.util.txt.swp"), "").unwrap();
    dev.tick(&clock, "sample_js");
    assert_eq!(dev.probe.runs_of("Dev"), runs);
}

#[test]
fn reading_the_sources_or_changing_only_their_metadata_is_not_a_save() {
    let dev = Dev::new();
    let clock = dev.clock();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    fs::create_dir(folder.join("src")).unwrap();
    fs::write(folder.join("src/lib.txt"), "saved").unwrap();
    dev.finished(&identity, 1);
    dev.idle(&identity);
    let runs = dev.probe.runs_of("Dev");

    // What a build, a copy, a backup or a reload does, and what FSEvents and
    // ReadDirectoryChangesW report as changes: a file read, its permissions
    // changed and back, a folder's.
    fs::read(folder.join("source.txt")).unwrap();
    for path in [folder.join("source.txt"), folder.join("src")] {
        let permissions = fs::metadata(&path).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        fs::set_permissions(&path, permissions).unwrap();
    }
    fs::copy(folder.join("pane.json"), dev.sources.join("pane.json")).unwrap();
    dev.tick(&clock, "sample_js");
    assert_eq!(dev.probe.runs_of("Dev"), runs);

    // A save still builds.
    save(&folder, "sample_ts");
    dev.finished(&identity, 2);
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(TYPESCRIPT.into())
    );
}

#[test]
fn dropping_the_launcher_stops_development() {
    let dev = Dev::new();
    let (folder, _) = dev.developing("Dev", "sample_rust");
    dev.probe.close();
    save(&folder, "sample_js");
    wait_until("the build", || dev.probe.runs() == 1);

    let Dev {
        launcher,
        probe,
        _sources,
        _data,
        ..
    } = dev;
    drop(launcher);
    wait_until("the build to stop", || probe.stopped() == 1);
}

#[test]
fn a_published_copy_keeps_its_own_identity_and_code() {
    let dev = Dev::new();
    // The published copy: built components only, as a package is shipped.
    let published = dev.sources.join("published");
    package(&published, "Dev", "sample_rust");
    fs::remove_file(published.join("source.txt")).unwrap();
    block_on(dev.launcher.install_package(&published));
    let published = PackageIdentity::local(&published).unwrap();
    let (folder, identity) = dev.developing("Dev", "sample_rust");
    assert_ne!(published, identity);

    save(&folder, "sample_js");
    dev.finished(&identity, 1);
    // Both are titled Dev: the first command is the published copy's.
    to_root(&dev.launcher);
    let answers: Vec<Status> = (0..2)
        .map(|index| {
            to_root(&dev.launcher);
            dev.launcher.select(index);
            block_on(dev.launcher.activate_selected());
            select_title(&dev.launcher, "Say hello");
            block_on(dev.launcher.activate_selected());
            shown(&dev.launcher)
        })
        .collect();
    assert_eq!(
        answers,
        [
            Status::Result(RUST.into()),
            Status::Result(JAVASCRIPT.into())
        ]
    );
    assert!(dev.launcher.development(&published).is_none());

    // The published copy cannot be developed: it has no source to build.
    block_on(dev.launcher.start_developing(&published));
    assert_eq!(
        dev.launcher.view().status,
        Status::Error("Cannot develop Dev: it has no source.txt".into())
    );
    assert!(dev.launcher.development(&published).is_none());
}

#[test]
fn development_is_started_and_stopped_in_manage_extensions() {
    let dev = Dev::new();
    let (folder, identity) = dev.install("Dev", "sample_rust");
    // The folder as Pane resolves it (macOS: /private/var for /var).
    let resolved = identity.local_folder().unwrap().to_path_buf();
    let status = press(&dev.launcher, "Develop Dev");
    assert_eq!(
        status,
        Status::Result(format!(
            "Developing Dev: each save in {} runs `fake build`, then reloads it",
            resolved.display()
        ))
    );
    let view = dev.launcher.view();
    assert!(
        view.rows[0]
            .subtitle
            .as_deref()
            .is_some_and(|s| s.starts_with("Enabled · Developing · ")),
        "{:?}",
        view.rows[0]
    );
    // The row that started it stops it, and stays selected.
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Stop developing Dev")
    );
    let development = dev.launcher.development(&identity).unwrap();
    assert_eq!(development.folder, resolved);
    assert_eq!(development.command, "fake build");

    // Developing it again does nothing more.
    block_on(dev.launcher.start_developing(&identity));
    save(&folder, "sample_js");
    dev.finished(&identity, 1);
    assert_eq!(dev.probe.runs(), 1);

    press(&dev.launcher, "Stop developing Dev");
    assert!(titles(&dev.launcher).contains(&"Develop Dev".to_string()));
}

#[test]
fn a_launcher_that_develops_still_pauses_a_package_that_keeps_crashing() {
    let dev = Dev::new();
    let (_, identity) = dev.install("Dev", "sample_settings");
    for _ in 0..3 {
        error(run(&dev.launcher, "Open Dev", "Crash"));
    }
    manage(&dev.launcher);
    assert!(titles(&dev.launcher).contains(&"Retry Dev".to_string()));
    block_on(dev.launcher.records_written());
    let installed = fs::read_to_string(dev._data.path().join("extensions/installed.json")).unwrap();
    assert!(installed.contains("\"paused\""), "{installed}");
    // Developing it and saving a fix recovers it.
    block_on(dev.launcher.start_developing(&identity));
    save(&dev.sources.join("Dev"), "sample_rust");
    dev.finished(&identity, 1);
    assert_eq!(
        run(&dev.launcher, "Open Dev", "Say hello"),
        Status::Result(RUST.into())
    );
}

#[test]
fn the_window_is_told_of_each_change() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let probe = Arc::new(Probe::default());
    let (sender, mut changes) = pane_core::changes::channel();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
    .with_development(Arc::new(FakeBuilder(probe)), sender);
    let folder = package(&sources.path().join("Dev"), "Dev", "sample_rust");
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.start_developing(&identity));

    save(&folder, "sample_js");
    // Building, then reloaded.
    block_on(changes.next()).unwrap();
    wait_until("the reload", || {
        launcher.view().status == Status::Result("Reloaded Dev".into())
    });
    block_on(changes.next()).unwrap();
}

#[test]
fn a_package_being_updated_or_installed_is_not_developed_meanwhile() {
    let dev = Dev::new();
    let (folder, identity) = dev.install("Dev", "sample_rust");
    block_on(dev.launcher.preview_package(&folder));
    select_title(&dev.launcher, "Update");
    let update = dev.launcher.activate_selected();

    block_on(dev.launcher.start_developing(&identity));
    assert_eq!(
        dev.launcher.view().status,
        Status::Error("Dev is updating".into())
    );
    assert!(dev.launcher.development(&identity).is_none());
    block_on(update);

    // Once updated, it is developed as ever.
    block_on(dev.launcher.start_developing(&identity));
    assert!(dev.launcher.development(&identity).is_some());
}
