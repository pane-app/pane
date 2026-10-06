//! Native helpers through the launcher's public interface, with the helper
//! samples in Rust, JavaScript and TypeScript (`guests/sample-helper`,
//! `guests/sample-helper-js`, `guests/sample-helper-ts`) and their real
//! helper program, `pane-echo`, built for this system by `cargo xtask
//! guests` and put in each package as this target's file. Pane runs that
//! file and compiles nothing; it ends the helper's process when the guest
//! cancels the run, when the package is disabled, reloaded, updated or
//! uninstalled while it runs, and when Pane quits, keeping the package's
//! saved data.
//!
//! A check that a helper ended does not trust a process id, which the
//! system may give to another process once the helper is reaped: Pane must
//! list no running helper, and the file `pane-echo --wait` beats in every
//! 20 ms while it runs must stop growing.
//!
//! While a command's call waits on its helper, Pane serves other calls
//! (#136): the calculator, another package, answers root search meanwhile,
//! and a helper may run for as long as its work takes. A test stands in for
//! the clock of a long wait by writing the helper's release file rather than
//! waiting it out.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, Limits, PackageIdentity, Runtime, SavedData, Screen, Status, Target};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

/// Under the ten seconds "Echo after waiting" has its helper wait: a run
/// that ends sooner was stopped.
const STOPPED_WITHIN: Duration = Duration::from_secs(8);

/// How long starting the command and its helper may take.
const PROMPTLY: Duration = Duration::from_secs(6);

/// Where `pane-echo --wait` beats while it runs, in its working folder.
const ALIVE: &str = "pane-echo.alive";

/// What ends `pane-echo --wait` early, in its working folder: the test's
/// stand-in for the clock.
const RELEASE: &str = "pane-echo.release";

/// A helper sample in one language.
struct Sample {
    /// Its assembled package, under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    /// Its package and command title; the copies these tests install are
    /// retitled "Helper sample", so that every language reads the same.
    title: &'static str,
    /// Its package's version.
    version: &'static str,
}

const RUST: Sample = Sample {
    package: "sample-helper",
    component: "sample_helper.wasm",
    title: "Helper sample",
    version: "0.3.0",
};

const JAVASCRIPT: Sample = Sample {
    package: "sample-helper-js",
    component: "sample_helper_js.wasm",
    title: "JavaScript helper sample",
    version: "0.1.0",
};

const TYPESCRIPT: Sample = Sample {
    package: "sample-helper-ts",
    component: "sample_helper_ts.wasm",
    title: "TypeScript helper sample",
    version: "0.1.0",
};

fn this() -> Target {
    Target::current().expect("Pane names this system's target")
}

/// "Linux x86-64", as the helper names the system it was built for.
fn this_system() -> String {
    this().to_string()
}

impl Sample {
    /// The assembled package.
    fn assembled(&self) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(self.package);
        assert!(
            path.exists(),
            "{} is missing; run `cargo xtask guests`",
            path.display()
        );
        let manifest = fs::read_to_string(path.join("pane.json")).unwrap();
        assert!(
            manifest.contains(&format!("\"{}\"", this().id())),
            "the helper sample ships no helper for {}: it is built for the contributor \
             baselines (linux-x86_64, macos-aarch64, windows-x86_64) only",
            this()
        );
        path
    }

    /// Copies the assembled sample into `folder`, retitled "Helper sample"
    /// and with its `pane.json` then changed by `manifest`, and returns
    /// `folder`.
    fn copy_to(&self, folder: &Path, manifest: impl FnOnce(String) -> String) -> PathBuf {
        let from = self.assembled();
        fs::create_dir_all(folder.join(Path::new(&helper_file()).parent().unwrap())).unwrap();
        let text = fs::read_to_string(from.join("pane.json"))
            .unwrap()
            .replace(self.title, "Helper sample");
        fs::write(folder.join("pane.json"), manifest(text)).unwrap();
        for file in [self.component.to_owned(), helper_file()] {
            fs::copy(from.join(&file), folder.join(&file))
                .unwrap_or_else(|error| panic!("{file}: {error}"));
        }
        folder.to_path_buf()
    }
}

/// This system's helper file in the package, as its `pane.json` names it.
fn helper_file() -> String {
    format!("helpers/{}/pane-echo{}", this().id(), this().exe_suffix())
}

/// The length of the heartbeat file at `path`, if it exists.
fn beats(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

struct Installed {
    _sources: TempDir,
    _cache: TempDir,
    data: TempDir,
    runtime: Runtime,
    launcher: Launcher,
    folder: PathBuf,
    identity: PackageIdentity,
}

/// However a test ends, its helpers end with it, as they do when Pane
/// quits.
impl Drop for Installed {
    fn drop(&mut self) {
        self.runtime.stop_helpers();
    }
}

impl Installed {
    fn new(sample: &Sample) -> Installed {
        Installed::with_manifest(sample, |text| text)
    }

    fn with_manifest(sample: &Sample, manifest: impl FnOnce(String) -> String) -> Installed {
        Installed::with_files(sample, manifest, &[])
    }

    /// Like [`Installed::with_manifest`], with `files` copied into the
    /// package too.
    fn with_files(
        sample: &Sample,
        manifest: impl FnOnce(String) -> String,
        files: &[PathBuf],
    ) -> Installed {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
        let folder = sample.copy_to(&sources.path().join("helper"), manifest);
        for file in files {
            fs::copy(file, folder.join(file.file_name().unwrap())).unwrap();
        }
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result("Installed Helper sample".into())
        );
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

    /// Opens the command and runs its item `item`, returning what it
    /// showed: its toast, or the status line.
    fn run(&self, item: &str) -> Status {
        open_sample_at(&self.launcher, item);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    /// What "Echo after waiting" has noted: "started", "finished" or
    /// nothing.
    fn waiting(&self) -> Option<String> {
        self.noted("helper-wait")
    }

    /// What the sample noted under `key`: "started", "finished" or nothing.
    fn noted(&self, key: &str) -> Option<String> {
        let path = self.data.path().join("extensions/settings.json");
        let text = fs::read_to_string(path).unwrap_or_default();
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"{key}\": \"{progress}\"")))
            .map(str::to_owned)
    }

    /// The installed helper's working folder, where it beats and is
    /// released.
    fn helper_folder(&self) -> PathBuf {
        self.launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == self.identity)
            .expect("the helper sample is installed")
            .location
            .join(Path::new(&helper_file()).parent().unwrap())
    }

    /// Ends the waits of the helpers running now, as their clock would.
    fn release(&self) {
        fs::write(self.helper_folder().join(RELEASE), "").unwrap();
    }

    /// Installs the calculator, another package, which answers root search
    /// with its guest.
    fn install_calculator(&self) {
        let calculator = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/calculator");
        block_on(self.launcher.install_package(&calculator));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
    }

    /// Asks root search for `expression`, which the calculator answers,
    /// and checks that it did, with `answer`, while the helper of `pending`
    /// still runs: the call waiting on it held up no other package's.
    fn calculator_answers_meanwhile(&self, pending: &Pending, expression: &str, answer: &str) {
        rows::to_root(&self.launcher);
        block_on(self.launcher.set_query(expression));
        assert_eq!(
            titles(&self.launcher).first().map(String::as_str),
            Some(answer),
            "{:?}",
            self.launcher.view()
        );
        assert!(
            !pending.thread.is_finished(),
            "the call waiting on its helper ended first"
        );
        assert_eq!(self.runtime.helper_processes().len(), 1);
        block_on(self.launcher.set_query(""));
    }

    /// Runs `item` on another thread and returns once its helper runs:
    /// Pane lists it, and it beats.
    fn start(&self, item: &str) -> Pending {
        let alive = self.helper_folder().join(ALIVE);
        let before = beats(&alive);
        open_sample_at(&self.launcher, item);
        let running = self.launcher.activate_selected();
        let started = Instant::now();
        let thread = thread::spawn(move || {
            block_on(running);
            Instant::now()
        });
        while self.runtime.helper_processes().len() != 1 || beats(&alive) <= before {
            assert!(
                started.elapsed() < PROMPTLY,
                "the helper did not start: {:?}",
                self.launcher.view().status
            );
            thread::sleep(Duration::from_millis(5));
        }
        Pending {
            thread,
            started,
            alive,
        }
    }
}

/// A command whose helper is running.
struct Pending {
    thread: thread::JoinHandle<Instant>,
    started: Instant,
    /// The helper's heartbeat file.
    alive: PathBuf,
}

impl Pending {
    /// Waits for the call to end, and checks it ended well before the
    /// helper would have finished, and that the helper is gone: Pane soon
    /// lists none, and it beats no more.
    fn assert_stopped(self, runtime: &Runtime) {
        let ended = self.thread.join().unwrap();
        let took = ended - self.started;
        assert!(took < STOPPED_WITHIN, "the call ran for {took:?}");
        // The call may answer before the helper's process is reaped.
        while !runtime.helper_processes().is_empty() {
            assert!(
                self.started.elapsed() < STOPPED_WITHIN,
                "Pane still runs {:?}",
                runtime.helper_processes()
            );
            thread::sleep(Duration::from_millis(5));
        }
        let last = beats(&self.alive);
        thread::sleep(Duration::from_millis(200));
        assert_eq!(beats(&self.alive), last, "the helper still beats");
    }
}

/// Opens the installed helper sample from root search and selects its item
/// titled `item`.
fn open_sample_at(launcher: &Launcher, item: &str) {
    launcher.back();
    launcher.back();
    block_on(launcher.set_query(""));
    select_title(launcher, "Helper sample");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view()
    );
    select_title(launcher, item);
}

fn error(message: &str) -> Status {
    Status::Error(format!("The extension reported an error: {message}"))
}

fn echoed() -> Status {
    Status::Result(format!("Echoed \"hello from Pane\" on {}", this_system()))
}

/// A launcher with no packages, for previews and refused installs.
fn bare_launcher(data: &TempDir) -> Launcher {
    Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
}

// The contract every helper sample meets, whatever its language.

fn the_helper_for_this_system_answers_and_leaves_no_process(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(installed.run("Echo through the helper"), echoed());
    assert_eq!(installed.runtime.helper_processes(), Vec::<u32>::new());
}

fn installing_copies_only_this_system_s_helper_file_ready_to_run(sample: &Sample) {
    let installed = Installed::new(sample);

    let location = installed.launcher.packages()[0].location.clone();
    let file = location.join(helper_file());
    assert!(file.is_file(), "{} was not copied", file.display());
    let helpers: Vec<PathBuf> = fs::read_dir(location.join("helpers"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into())
        .collect();
    assert_eq!(helpers, [PathBuf::from(this().id())]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o755, "{mode:o}");
    }
}

fn a_failing_helper_is_explained_with_its_exit_code_and_errors(sample: &Sample) {
    let installed = Installed::new(sample);

    let status = installed.run("Make the helper fail");

    assert_eq!(
        status,
        error("failed: helper `echo` failed (exit code 3): pane-echo was asked to fail")
    );
    assert_eq!(installed.runtime.helper_processes(), Vec::<u32>::new());
}

fn an_undeclared_helper_is_explained(sample: &Sample) {
    let installed = Installed::new(sample);

    let status = installed.run("Run an undeclared helper");

    assert_eq!(
        status,
        error(
            "not-found: Helper sample declares no helper `absent` in its pane.json; \
             it declares `echo`"
        )
    );
}

fn a_helper_not_built_for_this_system_is_explained_and_the_rest_works(sample: &Sample) {
    // The package ships echo only for a target other than this one.
    let other = if this().id() == "windows-aarch64" {
        "linux-aarch64"
    } else {
        "windows-aarch64"
    };
    let other = Target::parse(other).unwrap();
    let file = format!("helpers/{}/pane-echo{}", other.id(), other.exe_suffix());
    let installed = Installed::with_manifest(sample, |text| {
        let echo = text
            .find("\"targets\"")
            .expect("the sample declares targets");
        let end = echo + text[echo..].find('}').unwrap() + 1;
        format!(
            "{}\"targets\": {{ \"{}\": \"{file}\" }}{}",
            &text[..echo],
            other.id(),
            &text[end..]
        )
    });

    let status = installed.run("Echo through the helper");

    assert_eq!(
        status,
        error(&format!(
            "unavailable: Not available on {}: helper `echo` is built only for {other}",
            this_system()
        ))
    );
    // The command's other items still work.
    assert_eq!(
        installed.run("Run an undeclared helper"),
        error(
            "not-found: Helper sample declares no helper `absent` in its pane.json; \
             it declares `echo`"
        )
    );
}

fn a_helper_file_replaced_after_install_is_explained_when_run(sample: &Sample) {
    let installed = Installed::new(sample);
    let location = installed.launcher.packages()[0].location.clone();
    let shown = Path::new(&helper_file()).display().to_string();
    for (content, problem) in [
        (
            &b"not a program"[..],
            format!("is not a program Pane recognizes for {}", this_system()),
        ),
        (
            &b"#!/bin/sh\necho replaced\n"[..],
            format!(
                "is a script; a helper must be a native program built for {}",
                this_system()
            ),
        ),
    ] {
        fs::write(location.join(helper_file()), content).unwrap();

        let status = installed.run("Echo through the helper");

        assert_eq!(
            status,
            error(&format!(
                "unavailable: helper `echo` cannot run: its file {shown} {problem}"
            ))
        );
    }
}

fn cancelling_a_run_ends_the_helper_s_process(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Echo within a second");

    pending.assert_stopped(&installed.runtime);

    assert_eq!(
        shown(&installed.launcher),
        Status::Result("Stopped the helper after one second".into())
    );
    // The command runs its helper again at once.
    assert_eq!(installed.run("Echo through the helper"), echoed());
}

fn disabling_while_the_helper_runs_ends_its_process_and_keeps_saved_data(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Echo after waiting");
    assert_eq!(installed.waiting().as_deref(), Some("started"));

    block_on(installed.launcher.set_enabled(&installed.identity, false));
    pending.assert_stopped(&installed.runtime);

    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Disabled Helper sample".into())
    );
    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());

    block_on(installed.launcher.set_enabled(&installed.identity, true));
    assert_eq!(installed.run("Echo through the helper"), echoed());
    assert_eq!(installed.waiting().as_deref(), Some("started"));
}

fn reloading_while_the_helper_runs_ends_its_process_and_the_new_code_runs_it(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Echo after waiting");

    block_on(installed.launcher.reload(&installed.identity));
    pending.assert_stopped(&installed.runtime);

    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Reloaded Helper sample".into())
    );
    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(installed.run("Echo through the helper"), echoed());
}

fn updating_while_the_helper_runs_ends_its_process(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Echo after waiting");

    block_on(installed.launcher.preview_package(&installed.folder));
    select_title(&installed.launcher, "Update");
    block_on(installed.launcher.activate_selected());
    pending.assert_stopped(&installed.runtime);

    assert_eq!(
        installed.launcher.view().status,
        Status::Result(format!("Updated Helper sample to {}", sample.version))
    );
    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(installed.run("Echo through the helper"), echoed());
}

fn uninstalling_while_the_helper_runs_ends_its_process_and_keeps_saved_data(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Echo after waiting");

    block_on(
        installed
            .launcher
            .uninstall(&installed.identity, SavedData::Keep),
    );
    pending.assert_stopped(&installed.runtime);

    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

/// Quitting Pane (`main.rs` stops the runtime's helpers when the app quits)
/// ends a helper that is still running, and no helper starts afterwards.
fn quitting_while_the_helper_runs_ends_its_process(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Echo after waiting");

    installed.runtime.stop_helpers();
    pending.assert_stopped(&installed.runtime);

    assert_eq!(
        shown(&installed.launcher),
        error("refused: helper `echo` was stopped before it finished")
    );
    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(
        installed.run("Echo through the helper"),
        error("refused: Pane is quitting; helper `echo` does not start")
    );
}

fn repeated_stops_leave_no_helper_running(sample: &Sample) {
    let installed = Installed::new(sample);
    for _ in 0..3 {
        let pending = installed.start("Echo after waiting");
        block_on(installed.launcher.set_enabled(&installed.identity, false));
        pending.assert_stopped(&installed.runtime);
        block_on(installed.launcher.set_enabled(&installed.identity, true));

        let pending = installed.start("Echo after waiting");
        block_on(installed.launcher.reload(&installed.identity));
        pending.assert_stopped(&installed.runtime);

        let pending = installed.start("Echo within a second");
        pending.assert_stopped(&installed.runtime);
    }
    assert_eq!(installed.waiting().as_deref(), Some("started"));
}

/// While the command waits on its helper, the calculator, another package,
/// answers root search: waiting holds no other extension's calls.
fn other_extensions_answer_while_the_helper_runs(sample: &Sample) {
    let installed = Installed::new(sample);
    installed.install_calculator();
    let pending = installed.start("Echo after waiting");

    installed.calculator_answers_meanwhile(&pending, "1 + 1", "2");

    block_on(installed.launcher.set_enabled(&installed.identity, false));
    pending.assert_stopped(&installed.runtime);
    assert_eq!(installed.waiting().as_deref(), Some("started"));
}

/// A helper runs for as long as its work takes: longer than every limit
/// Pane applies to the runtime (here shortened, as the old 30 seconds of a
/// helper would have been), since waiting is never the guest's computing
/// and the thread waiting for work is not stuck. The call then answers, and
/// nothing was stopped, blamed or given up on.
fn a_helper_outlasting_every_runtime_limit_completes(sample: &Sample) {
    let installed = Installed::new(sample);
    installed.install_calculator();
    // Both instances run before the limits shrink: starting one is not what
    // is checked.
    assert_eq!(installed.run("Echo through the helper"), echoed());
    rows::to_root(&installed.launcher);
    block_on(installed.launcher.set_query("1 + 1"));
    assert_eq!(
        titles(&installed.launcher).first().map(String::as_str),
        Some("2")
    );
    let limits = Limits {
        compute: Duration::from_millis(500),
        warn: Duration::from_millis(300),
        unresponsive: Duration::from_millis(600),
    };
    installed.runtime.set_limits(limits);
    let pending = installed.start("Echo after a long wait");

    // Past every limit, the call still waits on its helper.
    thread::sleep(Duration::from_secs(1));
    installed.calculator_answers_meanwhile(&pending, "2 * 3", "6");
    assert_eq!(installed.noted("helper-long-wait"), None);
    installed.release();

    pending.thread.join().unwrap();
    assert_eq!(
        installed.noted("helper-long-wait").as_deref(),
        Some("finished")
    );
    assert_eq!(
        installed.runtime.status(),
        pane_core::RuntimeStatus::Running
    );
    assert_eq!(installed.runtime.abandoned_threads(), 0);
    let package = installed
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == installed.identity)
        .unwrap();
    assert!(package.enabled);
    // A helper that finished by itself removes its heartbeat file.
    assert!(!installed.helper_folder().join(ALIVE).exists());
    assert_eq!(installed.runtime.helper_processes(), Vec::<u32>::new());
}

/// Runs each check of the contract above for each sample, as a test in the
/// module of its language.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::RUST);
                }
            )*
        }
        mod javascript {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::JAVASCRIPT);
                }
            )*
        }
        mod typescript {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::TYPESCRIPT);
                }
            )*
        }
    };
}

contract!(
    the_helper_for_this_system_answers_and_leaves_no_process,
    installing_copies_only_this_system_s_helper_file_ready_to_run,
    a_failing_helper_is_explained_with_its_exit_code_and_errors,
    an_undeclared_helper_is_explained,
    a_helper_not_built_for_this_system_is_explained_and_the_rest_works,
    a_helper_file_replaced_after_install_is_explained_when_run,
    cancelling_a_run_ends_the_helper_s_process,
    disabling_while_the_helper_runs_ends_its_process_and_keeps_saved_data,
    reloading_while_the_helper_runs_ends_its_process_and_the_new_code_runs_it,
    updating_while_the_helper_runs_ends_its_process,
    uninstalling_while_the_helper_runs_ends_its_process_and_keeps_saved_data,
    quitting_while_the_helper_runs_ends_its_process,
    repeated_stops_leave_no_helper_running,
    other_extensions_answer_while_the_helper_runs,
    a_helper_outlasting_every_runtime_limit_completes,
);

/// Two calls into one instance never interleave: the command's next call
/// waits for the one waiting on its helper, and starts (its own helper
/// runs) only once that one returned, while another package's calls are
/// served meanwhile.
#[test]
fn two_calls_into_one_instance_run_one_after_another() {
    let installed = Installed::new(&RUST);
    installed.install_calculator();
    let first = installed.start("Echo after a long wait");
    select_title(&installed.launcher, "Echo through the helper");
    let second = {
        let running = installed.launcher.activate_selected();
        thread::spawn(move || block_on(running))
    };

    installed.calculator_answers_meanwhile(&first, "1 + 1", "2");
    thread::sleep(Duration::from_millis(300));
    assert!(
        !second.is_finished(),
        "the second call ran beside the first"
    );
    assert_eq!(installed.runtime.helper_processes().len(), 1);
    installed.release();

    first.thread.join().unwrap();
    second.join().unwrap();
    assert_eq!(
        installed.noted("helper-long-wait").as_deref(),
        Some("finished")
    );
    assert_eq!(installed.runtime.helper_processes(), Vec::<u32>::new());
}

/// What a package's generation sets up is on its undo list (the instance,
/// and the helper while it runs), and its end runs the list: the helper's
/// process ends, the instance goes, and the list is empty.
#[cfg(debug_assertions)]
#[test]
fn a_generation_s_undo_list_holds_what_it_set_up_and_is_empty_after_its_end() {
    let installed = Installed::new(&RUST);
    assert_eq!(installed.run("Echo through the helper"), echoed());
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        ["extension instance"]
    );
    let pending = installed.start("Echo after waiting");
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        ["extension instance", "native helper"]
    );

    block_on(installed.launcher.set_enabled(&installed.identity, false));

    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        Vec::<&str>::new()
    );
    pending.assert_stopped(&installed.runtime);
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

/// Pausing the package while a call waits on its helper ends the wait and
/// the helper's process. The pause comes from another command of the same
/// package, the settings sample's Greeting, crashing three times: it runs
/// while the first command waits, since only an instance's own calls wait
/// for each other.
#[test]
fn pausing_while_the_helper_runs_ends_its_process() {
    let greeting = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-settings/sample_settings.wasm");
    let installed = Installed::with_files(
        &RUST,
        |text| {
            text.replacen(
                "\"commands\": [",
                "\"commands\": [{ \"id\": \"greeting\", \"title\": \"Greeting\", \
                 \"component\": \"sample_settings.wasm\" },",
                1,
            )
        },
        &[greeting],
    );
    let pending = installed.start("Echo after waiting");

    for crash in 1..=3 {
        rows::to_root(&installed.launcher);
        block_on(installed.launcher.set_query(""));
        select_title(&installed.launcher, "Greeting");
        block_on(installed.launcher.activate_selected());
        select_title(&installed.launcher, "Crash");
        block_on(installed.launcher.activate_selected());
        if crash < 3 {
            assert!(!pending.thread.is_finished(), "crash {crash}");
        }
    }

    pending.assert_stopped(&installed.runtime);
    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

/// Pane quitting while a call waits on its helper ends the wait at once:
/// the helper's process ends, and the call answers.
#[test]
fn quitting_while_a_call_waits_on_its_helper_ends_the_wait() {
    let installed = Installed::new(&RUST);
    let pending = installed.start("Echo after waiting");

    installed.runtime.quit();

    pending.assert_stopped(&installed.runtime);
    assert_eq!(installed.waiting().as_deref(), Some("started"));
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

// What Pane checks of a package's helpers, whatever its code's language.

#[test]
fn the_package_preview_lists_its_helpers_and_this_system() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let launcher = bare_launcher(&data);
    let targets = ["linux-x86_64", "macos-aarch64", "windows-x86_64"];
    let folder = RUST.copy_to(&sources.path().join("helper"), |text| text);

    block_on(launcher.preview_package(&folder));

    let listed: Vec<String> = targets
        .iter()
        .map(|id| {
            let target = Target::parse(id).unwrap();
            if target == this() {
                format!("{target} (this system)")
            } else {
                target.to_string()
            }
        })
        .collect();
    let expected = format!(
        "Helpers: echo for {}, {} and {}{}",
        listed[0],
        listed[1],
        listed[2],
        if targets.contains(&this().id().as_str()) {
            ""
        } else {
            " (none for this system)"
        }
    );
    let view = launcher.view();
    assert!(view.details().contains(&expected), "{:?}", view.details());
}

/// The first bytes of a program for another system than this one.
fn program_for_another_system() -> (Vec<u8>, &'static str) {
    if cfg!(windows) {
        let mut elf = b"\x7fELF\x02\x01\x01".to_vec();
        elf.resize(18, 0);
        elf.extend_from_slice(&0x3Eu16.to_le_bytes());
        elf.resize(64, 0);
        (elf, "Linux x86-64")
    } else {
        let mut pe = b"MZ".to_vec();
        pe.resize(0x3C, 0);
        pe.extend_from_slice(&0x80u32.to_le_bytes());
        pe.resize(0x80, 0);
        pe.extend_from_slice(b"PE\0\0");
        pe.extend_from_slice(&0x8664u16.to_le_bytes());
        (pe, "Windows x86-64")
    }
}

#[test]
fn a_helper_file_for_another_system_a_script_or_missing_is_refused_at_install() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let launcher = bare_launcher(&data);
    let folder = RUST.copy_to(&sources.path().join("helper"), |text| text);
    let shown = Path::new(&helper_file()).display().to_string();
    let (program, found) = program_for_another_system();
    for (content, problem) in [
        (
            program,
            format!("is a program for {found}, not {}", this_system()),
        ),
        (
            b"#!/bin/sh\necho hello\n".to_vec(),
            format!(
                "is a script; a helper must be a native program built for {}",
                this_system()
            ),
        ),
    ] {
        fs::write(folder.join(helper_file()), content).unwrap();

        block_on(launcher.install_package(&folder));

        assert_eq!(
            launcher.view().status,
            Status::Error(format!(
                "Not ready to run: the package ships helper `echo` for {}, but its file \
                 {shown} {problem}",
                this_system()
            ))
        );
        assert!(launcher.packages().is_empty());
    }

    fs::remove_file(folder.join(helper_file())).unwrap();
    block_on(launcher.install_package(&folder));

    assert_eq!(
        launcher.view().status,
        Status::Error(format!(
            "Not ready to run: the package ships helper `echo` for {}, but its file {shown} \
             is missing",
            this_system()
        ))
    );
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_helper_declaration_pane_cannot_use_is_an_invalid_manifest() {
    let declared = |helpers: &'static str| {
        move |text: String| {
            let at = text
                .find("\"helpers\"")
                .expect("the sample declares helpers");
            let end = text.rfind(']').unwrap() + 1;
            format!("{}\"helpers\": {helpers}{}", &text[..at], &text[end..])
        }
    };
    for (helpers, explanation) in [
        (
            r#"[{ "id": "echo", "targets": { "linux-arm64": "echo" } }]"#,
            "unknown target `linux-arm64` of helper `echo`; use windows, macos or linux, \
             a dash, and x86_64 or aarch64, such as \"linux-x86_64\"",
        ),
        (
            r#"[{ "id": "echo", "targets": { "linux-x86_64": "../echo" } }]"#,
            "helper file `../echo` must be a relative path inside the package folder",
        ),
        (
            r#"[{ "id": "echo", "targets": { "windows-x86_64": "helpers/echo.cmd" } }]"#,
            "helper `echo`: its file helpers/echo.cmd for Windows x86-64 must be a program \
             ending in .exe",
        ),
        (
            r#"[{ "id": "echo", "targets": { "windows-x86_64": "helpers/echo" } }]"#,
            "helper `echo`: its file helpers/echo for Windows x86-64 must be a program \
             ending in .exe",
        ),
        (
            r#"[{ "id": "Echo", "targets": { "linux-x86_64": "echo" } }]"#,
            "helper id `Echo` must be lowercase letters, digits and dashes, starting with a \
             letter or digit, at most 64 long",
        ),
        (
            r#"[{ "id": "echo\u0000", "targets": { "linux-x86_64": "echo" } }]"#,
            "helper id `echo\\0` must be lowercase letters",
        ),
        (
            r#"[{ "id": "echo", "targets": {} }]"#,
            "helper `echo` has no `targets`",
        ),
        (
            r#"[{ "id": "echo", "targets": { "linux-x86_64": "a" } },
                { "id": "echo", "targets": { "linux-x86_64": "b" } }]"#,
            "helper id `echo` is repeated",
        ),
        (
            r#"[{ "id": "echo", "file": "echo" }]"#,
            "unknown field `file`",
        ),
    ] {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let launcher = bare_launcher(&data);
        let folder = RUST.copy_to(&sources.path().join("helper"), declared(helpers));

        block_on(launcher.preview_package(&folder));

        let Status::Error(message) = launcher.view().status else {
            panic!("{helpers}: {:?}", launcher.view().status);
        };
        assert!(message.starts_with("Invalid pane.json: "), "{message}");
        assert!(message.contains(explanation), "{helpers}: {message}");
    }
}

/// Development builds that stage the component already in the folder, and
/// count how many ran.
struct StagingBuilder(&'static str, Arc<AtomicUsize>);

struct StageAsIs {
    folder: PathBuf,
    component: &'static str,
    builds: Arc<AtomicUsize>,
}

impl pane_core::develop::Builder for StagingBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn pane_core::develop::Build>, String> {
        Ok(Arc::new(StageAsIs {
            folder: folder.to_path_buf(),
            component: self.0,
            builds: self.1.clone(),
        }))
    }
}

impl pane_core::develop::Build for StageAsIs {
    fn command(&self) -> String {
        "stage as is".into()
    }

    // Not even the component, which Pane copies into the folder after a
    // reload: that copy is not a save.
    fn ignores(&self, _: &Path) -> bool {
        false
    }

    fn run(&self, job: &pane_core::develop::BuildJob) -> pane_core::develop::BuildOutcome {
        self.builds.fetch_add(1, Ordering::SeqCst);
        fs::copy(
            self.folder.join(self.component),
            job.staging().join(self.component),
        )
        .unwrap();
        pane_core::develop::BuildOutcome::Built
    }
}

#[test]
fn a_development_build_reloaded_while_the_helper_runs_ends_its_process() {
    let mut installed = Installed::new(&RUST);
    let (changes, _) = pane_core::changes::channel();
    let builds = Arc::new(AtomicUsize::new(0));
    installed.launcher = installed.launcher.clone().with_development(
        Arc::new(StagingBuilder(RUST.component, builds.clone())),
        changes,
    );
    block_on(installed.launcher.start_developing(&installed.identity));
    let pending = installed.start("Echo after waiting");

    // A save: the build is staged with this system's helper file, and
    // reloading it stops the running helper before its copy is replaced.
    fs::write(installed.folder.join("notes.txt"), "saved").unwrap();
    let started = Instant::now();
    while installed
        .launcher
        .development(&installed.identity)
        .is_none_or(|development| development.finished == 0)
    {
        assert!(started.elapsed() < Duration::from_secs(120), "not reloaded");
        thread::sleep(Duration::from_millis(5));
    }
    // Once reloaded, as after a Reload: until the copy is replaced, a
    // stopped helper's heartbeat file is still there.
    pending.assert_stopped(&installed.runtime);
    // Pane's copy of the component into the folder did not start another
    // build, which would have been under way by now.
    thread::sleep(Duration::from_millis(500));
    let development = installed.launcher.development(&installed.identity).unwrap();
    assert_eq!(
        (
            development.finished,
            development.building,
            development.pending
        ),
        (1, false, false)
    );
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Reloaded Helper sample".into())
    );
    assert_eq!(installed.run("Echo through the helper"), echoed());
}

/// A helper runs no longer than the runtime thread its guest ran on: when
/// Pane gives up on a stuck thread (a runtime hang, a fault injected in
/// debug builds), the helpers it started end, the call answers that the
/// runtime stopped responding, and the command runs its helper again on
/// the fresh thread.
#[cfg(debug_assertions)]
#[test]
fn a_helper_is_ended_when_its_runtime_thread_is_given_up_on() {
    use pane_core::{Fault, Limits, RuntimeFailure};

    let long = Duration::from_secs(120);
    let installed = Installed::new(&RUST);
    installed.runtime.set_limits(Limits {
        warn: Duration::from_secs(1),
        unresponsive: Duration::from_secs(2),
        ..Limits::default()
    });
    let pending = installed.start("Echo after waiting");

    installed.runtime.inject(Fault::Hang);

    pending.thread.join().unwrap();
    let started = Instant::now();
    while !installed.runtime.helper_processes().is_empty() {
        assert!(started.elapsed() < long, "the helper still runs");
        thread::sleep(Duration::from_millis(5));
    }
    let last = beats(&pending.alive);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(beats(&pending.alive), last, "the helper still beats");
    assert_eq!(
        installed.runtime.status().failure(),
        Some(RuntimeFailure::Unresponsive)
    );
    let Status::Error(said) = installed.launcher.view().status else {
        panic!(
            "expected an error, got {:?}",
            installed.launcher.view().status
        );
    };
    assert!(said.contains("stopped responding"), "{said}");
    assert_eq!(installed.waiting().as_deref(), Some("started"));

    installed.runtime.inject(Fault::Release);
    while installed.runtime.abandoned_threads() > 0 {
        assert!(started.elapsed() < long, "the stuck thread did not end");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(installed.run("Echo through the helper"), echoed());
    assert_eq!(installed.waiting().as_deref(), Some("started"));
}
