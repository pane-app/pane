//! System programs (#147, ADR 0033) through the launcher's public
//! interface, with the programs samples in Rust, JavaScript and TypeScript
//! (`guests/sample-programs`, `guests/sample-programs-js`,
//! `guests/sample-programs-ts`), which answer the same, and a real small
//! program, `pane-echo` (guests/helpers/echo), built for this system by
//! `cargo xtask guests`. Each test puts `pane-echo` in a folder of its own
//! and gives the runtime a search path with that folder (and the test's own
//! `PATH`), asked at each call, so that the samples find it by its bare
//! name, as they would a program the user installed.
//!
//! A program belongs to the call that started it: it ends, with every
//! process it started in turn, when the call returns or is dropped, when
//! the package is disabled, reloaded, updated or uninstalled, and when Pane
//! quits. What a program leaves running when it exits runs on until then,
//! not just until the program's exit. A check that a program ended does not trust a process id: Pane
//! must list no running program, and the files `pane-echo --hold` beats in
//! (beside the program) every 20 ms, one for the program and one for the
//! descendant it starts, must stop growing.
//!
//! The elevation prompt itself needs a person: on Windows it is a manual
//! smoke; elsewhere an elevated run answers that it is not available yet.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status, Target};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles};

/// How long starting the command and its program may take (a JavaScript
/// command's first call included).
const PROMPTLY: Duration = Duration::from_secs(20);

/// Well under the 30 or 60 seconds the samples' programs hold for: a call
/// that ends sooner was stopped.
const STOPPED_WITHIN: Duration = Duration::from_secs(20);

/// A programs sample in one language.
struct Sample {
    /// Its assembled package, under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    /// Its package and command title; the copies these tests install are
    /// retitled "Programs sample", so that every language reads the same.
    title: &'static str,
}

const RUST: Sample = Sample {
    package: "sample-programs",
    component: "sample_programs.wasm",
    title: "Programs sample",
};

const JAVASCRIPT: Sample = Sample {
    package: "sample-programs-js",
    component: "sample_programs_js.wasm",
    title: "JavaScript programs sample",
};

const TYPESCRIPT: Sample = Sample {
    package: "sample-programs-ts",
    component: "sample_programs_ts.wasm",
    title: "TypeScript programs sample",
};

fn this() -> Target {
    Target::current().expect("Pane names this system's target")
}

/// `name` as this system's programs are named.
fn exe(name: &str) -> String {
    format!("{name}{}", this().exe_suffix())
}

/// The packages `cargo xtask guests` assembled.
fn packages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages")
}

/// `pane-echo`, as `cargo xtask guests` built it for this system.
fn built_echo() -> PathBuf {
    let path = packages()
        .join("sample-helper/helpers")
        .join(this().id())
        .join(exe("pane-echo"));
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests` (the helper sample ships pane-echo for \
         linux-x86_64, macos-aarch64 and windows-x86_64 only)",
        path.display()
    );
    path
}

impl Sample {
    fn assembled(&self) -> PathBuf {
        let path = packages().join(self.package);
        assert!(
            path.exists(),
            "{} is missing; run `cargo xtask guests`",
            path.display()
        );
        path
    }

    /// Copies the assembled sample into `folder`, retitled "Programs
    /// sample", with `component` as its component's file, and returns
    /// `folder`.
    fn copy_with(&self, folder: &Path, component: &Path) -> PathBuf {
        fs::create_dir_all(folder).unwrap();
        let text = fs::read_to_string(self.assembled().join("pane.json"))
            .unwrap()
            .replace(self.title, "Programs sample");
        fs::write(folder.join("pane.json"), text).unwrap();
        fs::copy(component, folder.join(self.component)).unwrap();
        folder.to_path_buf()
    }

    fn component_file(&self) -> PathBuf {
        self.assembled().join(self.component)
    }
}

/// The search path a test gives the runtime: the folder holding
/// `pane-echo` when `on_path`, then the test's own `PATH` (for the
/// system's shells).
fn search_path(bin: &Path, on_path: bool) -> OsString {
    let mut folders: Vec<PathBuf> = Vec::new();
    if on_path {
        folders.push(bin.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        folders.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(folders).unwrap()
}

/// The length of the heartbeat file at `path`, if it exists.
fn beats(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

/// "The extension reported an error: `message`", as the launcher shows an
/// error a command answered with.
fn error(message: &str) -> Status {
    Status::Error(format!("The extension reported an error: {message}"))
}

fn result(text: &str) -> Status {
    Status::Result(text.into())
}

struct Installed {
    _sources: TempDir,
    _cache: TempDir,
    data: TempDir,
    /// Where `pane-echo` is, and where it beats.
    bin: TempDir,
    /// Whether `bin` is on the search path the runtime asks for.
    on_path: Arc<Mutex<bool>>,
    runtime: Runtime,
    launcher: Launcher,
    folder: PathBuf,
    identity: PackageIdentity,
}

/// However a test ends, its programs end with it, as they do when Pane
/// quits.
impl Drop for Installed {
    fn drop(&mut self) {
        self.runtime.stop_helpers();
    }
}

impl Installed {
    fn new(sample: &Sample) -> Installed {
        Installed::with(sample, true, &sample.component_file())
    }

    /// The sample installed with `component` as its component, `pane-echo`
    /// on the search path if `on_path`.
    fn with(sample: &Sample, on_path: bool, component: &Path) -> Installed {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        fs::copy(built_echo(), bin.path().join(exe("pane-echo"))).unwrap();
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        let on_path = Arc::new(Mutex::new(on_path));
        {
            let (on_path, folder) = (on_path.clone(), bin.path().to_path_buf());
            runtime.set_program_search_path(Arc::new(move || {
                search_path(&folder, *on_path.lock().unwrap())
            }));
        }
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
        let folder = sample.copy_with(&sources.path().join("programs"), component);
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result("Installed Programs sample".into())
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        Installed {
            _sources: sources,
            _cache: cache,
            data,
            bin,
            on_path,
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

    /// Where `pane-echo --hold <seconds> <name>` beats.
    fn alive(&self, name: &str) -> PathBuf {
        self.bin.path().join(format!("pane-echo.{name}.alive"))
    }

    /// What "Run until stopped" has noted: "started", "finished" or
    /// nothing.
    fn noted(&self) -> Option<String> {
        let path = self.data.path().join("extensions/settings.json");
        let text = fs::read_to_string(path).unwrap_or_default();
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"programs-long\": \"{progress}\"")))
            .map(str::to_owned)
    }

    /// Runs `item` on another thread and returns once its program and the
    /// descendant it started run: Pane lists the program, and both beat.
    fn start(&self, item: &str) -> Pending {
        let (parent, descendant) = (self.alive("parent"), self.alive("descendant"));
        let before = (beats(&parent), beats(&descendant));
        open_sample_at(&self.launcher, item);
        let running = self.launcher.activate_selected();
        let started = Instant::now();
        let thread = thread::spawn(move || {
            block_on(running);
            Instant::now()
        });
        while self.runtime.program_processes().len() != 1
            || beats(&parent) <= before.0
            || beats(&descendant) <= before.1
        {
            assert!(
                started.elapsed() < PROMPTLY,
                "the program did not start: {:?}",
                shown(&self.launcher)
            );
            thread::sleep(Duration::from_millis(5));
        }
        Pending {
            thread,
            started,
            alive: vec![parent, descendant],
        }
    }

    /// Whether "Leave a descendant" noted that its run answered.
    fn left(&self) -> bool {
        let path = self.data.path().join("extensions/settings.json");
        fs::read_to_string(path)
            .unwrap_or_default()
            .contains("\"programs-left\": \"ran\"")
    }

    /// Checks that Pane runs no program, now that the call that ran one
    /// ended: what a program left running is ended with its call, by the
    /// thread holding it, at once.
    fn assert_none_runs(&self, since: Instant) {
        while !self.runtime.program_processes().is_empty() {
            assert!(
                since.elapsed() < STOPPED_WITHIN,
                "Pane still runs {:?}",
                self.runtime.program_processes()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Checks that Pane runs no program, and that neither the program nor
    /// its descendant beats any more.
    fn assert_ended(&self, alive: &[PathBuf], since: Instant) {
        self.assert_none_runs(since);
        let last: Vec<Option<u64>> = alive.iter().map(|path| beats(path)).collect();
        thread::sleep(Duration::from_millis(300));
        let now: Vec<Option<u64>> = alive.iter().map(|path| beats(path)).collect();
        assert_eq!(now, last, "a process still beats in {alive:?}");
    }

    /// Ends the holds of the programs running now, as their clock would.
    fn release(&self) {
        fs::write(self.bin.path().join("pane-echo.release"), "").unwrap();
    }
}

/// A command whose program is running.
struct Pending {
    thread: thread::JoinHandle<Instant>,
    started: Instant,
    /// Where the program and its descendant beat.
    alive: Vec<PathBuf>,
}

impl Pending {
    /// Waits for the call to end, and checks that it ended well before the
    /// program would have, and that the program and its descendant are
    /// gone.
    fn assert_stopped(self, installed: &Installed) {
        let ended = self.thread.join().unwrap();
        let took = ended - self.started;
        assert!(took < STOPPED_WITHIN, "the call ran for {took:?}");
        installed.assert_ended(&self.alive, self.started);
    }
}

/// Opens the installed programs sample from root search and selects its
/// item titled `item`.
fn open_sample_at(launcher: &Launcher, item: &str) {
    launcher.back();
    launcher.back();
    block_on(launcher.set_query(""));
    select_title(launcher, "Programs sample");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view()
    );
    select_title(launcher, item);
}

// The contract every programs sample meets, whatever its language.

fn a_program_answers_its_exit_code_output_and_errors(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Run a program"),
        result("Exit code 3, output \"out: hello\", errors \"err: hello\"")
    );
    installed.assert_none_runs(Instant::now());
}

fn a_bare_name_is_found_on_the_search_path_at_the_time_of_the_call(sample: &Sample) {
    let installed = Installed::with(sample, false, &sample.component_file());

    assert_eq!(
        installed.run("Run a program"),
        error("not-found: there is no program `pane-echo` on the search path")
    );

    // Installed since: the next call finds it, with no restart.
    *installed.on_path.lock().unwrap() = true;
    assert_eq!(
        installed.run("Run a program"),
        result("Exit code 3, output \"out: hello\", errors \"err: hello\"")
    );
}

fn an_absolute_path_runs_as_given_and_its_arguments_arrive_unparsed(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Run by its path"),
        result(
            "By its path, the arguments arrived as [two words] [\"quoted\"] [$HOME] [a\\b] [*] []"
        )
    );
}

fn a_spawned_program_streams_its_output_and_is_waited_for(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Stream progress"),
        result("Streamed progress 1/3, progress 2/3, progress 3/3; exit code 0")
    );
    installed.assert_none_runs(Instant::now());
}

fn a_spawned_program_takes_input(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Talk to a program"),
        result("Answered got one, got two; exit code 0")
    );
}

fn a_spawned_program_is_killed(sample: &Sample) {
    let installed = Installed::new(sample);
    let since = Instant::now();

    assert_eq!(
        installed.run("Kill a program"),
        result("Killed the program")
    );
    installed.assert_ended(&[installed.alive("killed")], since);
}

fn the_command_s_timeout_ends_its_program_with_an_error_it_handles(sample: &Sample) {
    let installed = Installed::new(sample);
    let since = Instant::now();

    assert_eq!(
        installed.run("Run with a timeout"),
        result(
            "The timeout ended it: program `pane-echo` ran longer than its timeout of 0.5 \
             seconds; Pane ended it and every process it started"
        )
    );
    assert!(since.elapsed() < STOPPED_WITHIN);
    installed.assert_ended(&[installed.alive("timeout")], since);
}

fn output_over_the_bound_ends_the_program_and_fails_the_run(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Write too much"),
        error(
            "too-much-output: program `pane-echo` wrote more than 16 MiB to its standard \
             output; Pane ended it and every process it started"
        )
    );
    installed.assert_none_runs(Instant::now());
}

fn a_missing_program_is_explained(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Run a missing program"),
        error("not-found: there is no program `pane-no-such-program` on the search path")
    );
}

fn a_script_runs_with_the_system_s_shell(sample: &Sample) {
    let installed = Installed::new(sample);

    assert_eq!(
        installed.run("Run a shell script"),
        result("The script said script ran")
    );
}

fn the_folder_and_environment_reach_the_program(sample: &Sample) {
    let installed = Installed::new(sample);
    let folder = installed
        .bin
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    assert_eq!(
        installed.run("Run in a folder with a variable"),
        result(&format!("in {folder}; PANE_SAMPLE_VALUE=set by the sample"))
    );
}

/// The command returns while its spawned program and the program that one
/// started run: both end with the call.
fn descendants_end_when_the_call_returns(sample: &Sample) {
    let installed = Installed::new(sample);
    let since = Instant::now();
    let descendant = installed.alive("descendant");

    assert_eq!(
        installed.run("Start a descendant"),
        result("It said \"started a descendant\"; returned without waiting")
    );

    assert!(beats(&descendant).is_some(), "the descendant never ran");
    installed.assert_ended(&[installed.alive("parent"), descendant], since);
}

/// The program exits at once, leaving a descendant that holds its output
/// open: the run answers all the same, and the descendant runs on, held by
/// Pane, until the call that started the program ends, not just until the
/// program's exit; then it ends.
fn what_a_program_leaves_runs_until_its_call_ends(sample: &Sample) {
    let installed = Installed::new(sample);
    let descendant = installed.alive("descendant");
    open_sample_at(&installed.launcher, "Leave a descendant");
    let running = installed.launcher.activate_selected();
    let started = Instant::now();
    let call = thread::spawn(move || block_on(running));

    // The run answered: its program exited.
    while !installed.left() {
        assert!(
            started.elapsed() < PROMPTLY,
            "the run never answered: {:?}",
            shown(&installed.launcher)
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!call.is_finished(), "the call goes on after the run");
    // What it left runs on, held as the call's.
    let before = beats(&descendant);
    thread::sleep(Duration::from_millis(300));
    assert!(
        beats(&descendant) > before,
        "the descendant ended with the program, before its call"
    );
    assert_eq!(installed.runtime.program_processes().len(), 1);

    call.join().unwrap();
    assert_eq!(
        shown(&installed.launcher),
        result("It said \"left a descendant\"; the call went on")
    );
    installed.assert_ended(&[descendant], started);
}

/// The run is dropped when a timer wins the race with it (in JavaScript the
/// call returns, leaving the run behind): the program and its descendant
/// end.
fn descendants_end_when_the_call_is_dropped(sample: &Sample) {
    let installed = Installed::new(sample);
    let since = Instant::now();
    let descendant = installed.alive("descendant");

    assert_eq!(
        installed.run("Give up after a second"),
        result("Gave up after a second")
    );

    assert!(beats(&descendant).is_some(), "the descendant never ran");
    installed.assert_ended(&[installed.alive("parent"), descendant], since);
}

fn disabling_while_a_program_runs_ends_it_and_its_descendant(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Run until stopped");
    assert_eq!(installed.noted().as_deref(), Some("started"));

    block_on(installed.launcher.set_enabled(&installed.identity, false));
    pending.assert_stopped(&installed);

    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Disabled Programs sample".into())
    );
    assert_eq!(installed.noted().as_deref(), Some("started"));
    block_on(installed.launcher.set_enabled(&installed.identity, true));
    assert_eq!(
        installed.run("Stream progress"),
        result("Streamed progress 1/3, progress 2/3, progress 3/3; exit code 0")
    );
}

fn reloading_while_a_program_runs_ends_it_and_its_descendant(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Run until stopped");

    block_on(installed.launcher.reload(&installed.identity));
    pending.assert_stopped(&installed);

    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Reloaded Programs sample".into())
    );
    assert_eq!(installed.noted().as_deref(), Some("started"));
}

fn updating_while_a_program_runs_ends_it_and_its_descendant(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Run until stopped");

    block_on(installed.launcher.preview_package(&installed.folder));
    select_title(&installed.launcher, "Update");
    block_on(installed.launcher.activate_selected());
    pending.assert_stopped(&installed);

    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Updated Programs sample to 0.1.0".into())
    );
    assert_eq!(installed.noted().as_deref(), Some("started"));
}

fn uninstalling_while_a_program_runs_ends_it_and_its_descendant(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Run until stopped");

    block_on(
        installed
            .launcher
            .uninstall(&installed.identity, SavedData::Keep),
    );
    pending.assert_stopped(&installed);

    assert_eq!(installed.noted().as_deref(), Some("started"));
}

/// Quitting Pane (`main.rs` quits the runtime when the app quits) ends a
/// program still running, and what it started.
fn quitting_while_a_program_runs_ends_it_and_its_descendant(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Run until stopped");

    installed.runtime.quit();
    pending.assert_stopped(&installed);

    assert_eq!(installed.noted().as_deref(), Some("started"));
}

/// A program runs for as long as its work takes: released, the program
/// and its descendant finish, and the command notes that it did.
fn a_long_run_finishes_when_its_program_does(sample: &Sample) {
    let installed = Installed::new(sample);
    let pending = installed.start("Run until stopped");

    installed.release();

    pending.thread.join().unwrap();
    assert_eq!(installed.noted().as_deref(), Some("finished"));
    assert_eq!(
        shown(&installed.launcher),
        result("Ran to the end, with exit code 0")
    );
    installed.assert_none_runs(Instant::now());
}

/// A no-view command keeps its call, and so its program, for as long as the
/// work takes, reporting its progress in a toast.
fn a_no_view_command_reports_a_program_s_progress(sample: &Sample) {
    let installed = Installed::new(sample);
    rows::to_root(&installed.launcher);
    block_on(installed.launcher.set_query(""));
    select_title(&installed.launcher, "Report progress");

    block_on(installed.launcher.activate_selected());

    assert_eq!(
        shown(&installed.launcher),
        result("Streamed progress 1/3, progress 2/3, progress 3/3; exit code 0")
    );
}

/// The elevation prompt is Windows' own, and needs a person there (the
/// manual smoke); elsewhere an elevated run answers that it is not
/// available yet, and runs nothing.
#[cfg(not(windows))]
fn an_elevated_run_is_not_available_here_yet(sample: &Sample) {
    let installed = Installed::new(sample);
    let here = pane_core::Platform::current().unwrap();

    assert_eq!(
        installed.run("Run elevated"),
        error(&format!(
            "unavailable: running a program elevated is not available on {here} yet"
        ))
    );
    installed.assert_none_runs(Instant::now());
}

#[cfg(not(windows))]
mod elevated {
    #[test]
    fn rust() {
        super::an_elevated_run_is_not_available_here_yet(&super::RUST);
    }

    #[test]
    fn javascript() {
        super::an_elevated_run_is_not_available_here_yet(&super::JAVASCRIPT);
    }

    #[test]
    fn typescript() {
        super::an_elevated_run_is_not_available_here_yet(&super::TYPESCRIPT);
    }
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
    a_program_answers_its_exit_code_output_and_errors,
    a_bare_name_is_found_on_the_search_path_at_the_time_of_the_call,
    an_absolute_path_runs_as_given_and_its_arguments_arrive_unparsed,
    a_spawned_program_streams_its_output_and_is_waited_for,
    a_spawned_program_takes_input,
    a_spawned_program_is_killed,
    the_command_s_timeout_ends_its_program_with_an_error_it_handles,
    output_over_the_bound_ends_the_program_and_fails_the_run,
    a_missing_program_is_explained,
    a_script_runs_with_the_system_s_shell,
    the_folder_and_environment_reach_the_program,
    descendants_end_when_the_call_returns,
    descendants_end_when_the_call_is_dropped,
    what_a_program_leaves_runs_until_its_call_ends,
    disabling_while_a_program_runs_ends_it_and_its_descendant,
    reloading_while_a_program_runs_ends_it_and_its_descendant,
    updating_while_a_program_runs_ends_it_and_its_descendant,
    uninstalling_while_a_program_runs_ends_it_and_its_descendant,
    quitting_while_a_program_runs_ends_it_and_its_descendant,
    a_long_run_finishes_when_its_program_does,
    a_no_view_command_reports_a_program_s_progress,
);

/// While a command waits on its program, the calculator, another package,
/// answers root search: waiting holds no other extension's calls (#136).
#[test]
fn other_extensions_answer_while_a_program_runs() {
    let installed = Installed::new(&RUST);
    let calculator = packages().join("calculator");
    block_on(installed.launcher.install_package(&calculator));
    let pending = installed.start("Run until stopped");

    rows::to_root(&installed.launcher);
    block_on(installed.launcher.set_query("1 + 1"));
    assert_eq!(
        titles(&installed.launcher).first().map(String::as_str),
        Some("2"),
        "{:?}",
        installed.launcher.view()
    );
    assert!(!pending.thread.is_finished(), "the call ended first");

    block_on(installed.launcher.set_enabled(&installed.identity, false));
    pending.assert_stopped(&installed);
}

/// A program is on its package generation's undo list while it runs, and
/// the generation's end runs the list: the program ends with its
/// descendant, and the list is empty.
#[cfg(debug_assertions)]
#[test]
fn a_running_program_is_on_its_generation_s_undo_list() {
    let installed = Installed::new(&RUST);
    let pending = installed.start("Run until stopped");
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        ["extension instance", "system program"]
    );

    block_on(installed.launcher.set_enabled(&installed.identity, false));

    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        Vec::<&str>::new()
    );
    pending.assert_stopped(&installed);
}

/// The row of the extension list naming `title`, as it reads.
fn row_subtitle(launcher: &Launcher, title: &str) -> String {
    launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == title)
        .and_then(|row| row.subtitle)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
}

/// What the preview of a package whose code runs system programs says.
const PREVIEW_NOTE: &str = "Runs system programs: its code imports pane:extension/programs, so \
                            it can run any program on this computer";

/// Installing notes a component that imports the programs interface: the
/// preview says so, the extension list says "Runs system programs", and a
/// row of it lists the programs the package ran this session. A package
/// whose code imports none says neither.
#[test]
fn installing_notes_the_program_import_and_the_list_shows_the_programs_run() {
    let installed = Installed::new(&RUST);
    let calculator = packages().join("calculator");
    block_on(installed.launcher.install_package(&calculator));

    block_on(installed.launcher.preview_package(&installed.folder));
    assert!(
        installed
            .launcher
            .view()
            .details()
            .iter()
            .any(|line| line == PREVIEW_NOTE),
        "{:?}",
        installed.launcher.view().details()
    );

    manage(&installed.launcher);
    assert!(
        row_subtitle(&installed.launcher, "Programs sample").contains(" · Runs system programs"),
        "{}",
        row_subtitle(&installed.launcher, "Programs sample")
    );
    assert!(!row_subtitle(&installed.launcher, "Calculator").contains("Runs system programs"));
    assert!(
        !titles(&installed.launcher).contains(&"Programs run by Calculator".to_owned()),
        "{:?}",
        titles(&installed.launcher)
    );
    select_title(&installed.launcher, "Programs run by Programs sample");
    block_on(installed.launcher.activate_selected());
    let view = installed.launcher.view();
    assert!(
        matches!(view.screen, Screen::ProgramDetails { .. }),
        "{view:?}"
    );
    assert!(
        view.details()
            .contains(&"It has run no program this session.".to_owned()),
        "{:?}",
        view.details()
    );

    assert_eq!(
        installed.run("Run a program"),
        result("Exit code 3, output \"out: hello\", errors \"err: hello\"")
    );

    manage(&installed.launcher);
    select_title(&installed.launcher, "Programs run by Programs sample");
    block_on(installed.launcher.activate_selected());
    let ran = installed
        .bin
        .path()
        .join(exe("pane-echo"))
        .display()
        .to_string();
    let details = installed.launcher.view().details().to_vec();
    assert!(details.contains(&ran), "{details:?}");
    assert!(
        details.contains(&"Programs it ran this session:".to_owned()),
        "{details:?}"
    );
}

/// Reloading and updating note the import again: a package first installed
/// with code that runs no program says nothing, and says so once its
/// reloaded code imports the interface, and still after an update.
#[test]
fn reloading_and_updating_note_the_program_import() {
    let quiet = packages().join("sample-rust/sample_rust.wasm");
    let installed = Installed::with(&RUST, true, &quiet);
    manage(&installed.launcher);
    assert!(!row_subtitle(&installed.launcher, "Programs sample").contains("Runs system programs"));

    fs::copy(RUST.component_file(), installed.folder.join(RUST.component)).unwrap();
    block_on(installed.launcher.reload(&installed.identity));
    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Reloaded Programs sample".into())
    );

    manage(&installed.launcher);
    assert!(
        row_subtitle(&installed.launcher, "Programs sample").contains(" · Runs system programs")
    );
    assert!(titles(&installed.launcher).contains(&"Programs run by Programs sample".to_owned()));

    block_on(installed.launcher.preview_package(&installed.folder));
    assert!(
        installed
            .launcher
            .view()
            .details()
            .iter()
            .any(|line| line == PREVIEW_NOTE)
    );
    select_title(&installed.launcher, "Update");
    block_on(installed.launcher.activate_selected());
    manage(&installed.launcher);
    assert!(
        row_subtitle(&installed.launcher, "Programs sample").contains(" · Runs system programs")
    );
}
