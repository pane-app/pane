//! A package's builds: how a local package's source folder is built, and the
//! processes such a build runs (ADR 0004; #12, #13).
//!
//! A development session (`crate::session`) watches the folder with the
//! system's file watcher, runs the package's [`Build`] after each save and
//! hands what the build staged to its host when it succeeds;
//! [`build_package`] builds it once. This module decides what a build is:
//! one adapter per language, each running the build command the guest
//! README documents, in the package folder:
//!
//! - **Rust**, a folder with `Cargo.toml`: `cargo build --release --target
//!   wasm32-wasip2`, with cargo's JSON messages, so that the component taken
//!   is the one cargo reports it built this time, wherever its target folder
//!   is (a workspace member, `CARGO_TARGET_DIR`, `build.target-dir`), never
//!   an older file left where the manifest points.
//! - **JavaScript or TypeScript**, a folder with `package.json`: Pane's own
//!   JS build (`crate::js`: `npm ci` of the locked dependencies into a
//!   staging copy, the package's `tsc`, esbuild around Pane's adapter, and
//!   the componentizer), once for each component the manifest names,
//!   writing straight into the staging folder. It needs Node.js and npm
//!   alone; the componentizer is Pane's own, linked in (`pane-ext`, and
//!   Pane's tests) or the binary the package's `@pane-app/cli` platform
//!   package installed.
//!
//! It is not a general build system: there is no build command of the
//! package's own, and a folder with neither file cannot be developed.
//!
//! A build runs with Pane's environment, less what `cargo run` injected
//! for Pane itself (see [`without_pane_build_environment`]), so the author's
//! cargo configuration, registry credentials and proxies apply.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::process_tree::ProcessTree;
use crate::session::{BuildFailure, failure};
use crate::{MANIFEST_FILE, ManifestFiles};

/// Decides how a package is built on save. [`Toolchains`] is Pane's; tests
/// supply their own.
pub trait Builder: Send + Sync + 'static {
    /// The build Pane runs after each save in the package's source folder
    /// `folder`, or why Pane cannot build it there.
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String>;
}

/// One package's build.
pub trait Build: Send + Sync + 'static {
    /// The build command, as the author would type it in the package
    /// folder.
    fn command(&self) -> String;

    /// Whether a change at `path`, relative to the package folder, belongs
    /// to the build (its output, its lock file) rather than being a save.
    /// `target`, `node_modules` and `dist` folders, hidden files and editor
    /// temporary files are never saves either (see [`is_save`]).
    fn ignores(&self, path: &Path) -> bool;

    /// Builds the package, putting each component named by the manifest in
    /// [`BuildJob::staging`] (which already holds the `pane.json` saved
    /// when the build began) at the same relative path, and recording what
    /// it prints with [`BuildJob::line`]. Once the job is stopped, it stops
    /// what it started and returns [`BuildOutcome::Stopped`].
    fn run(&self, job: &BuildJob) -> BuildOutcome;
}

/// How a build ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildOutcome {
    /// The components are staged.
    Built,
    /// It failed, for this reason (the diagnostics are its output).
    Failed(String),
    /// It was stopped before it ended; its outcome is not used.
    Stopped,
}

/// One run of a build: where it stages the package, what it printed, and
/// whether it should stop.
pub struct BuildJob {
    stop: BuildStop,
    staging: PathBuf,
    output: BuildOutput,
}

impl BuildJob {
    /// A run that stages into `staging`, which holds the package's
    /// `pane.json`, and keeps what the build prints in memory; for building a
    /// package outside development mode, such as in a test.
    pub fn new(staging: PathBuf) -> BuildJob {
        BuildJob::with(BuildStop::default(), staging, BuildOutput::new(None))
    }

    /// A run like [`BuildJob::new`] whose lines are also shown as they are
    /// printed (`cargo xtask js-guests` builds in a terminal).
    pub fn echoing(staging: PathBuf, echo: Echo) -> BuildJob {
        let output = BuildOutput::new(None).echoing(Some(echo));
        BuildJob::with(BuildStop::default(), staging, output)
    }

    pub(crate) fn with(stop: BuildStop, staging: PathBuf, output: BuildOutput) -> BuildJob {
        BuildJob {
            stop,
            staging,
            output,
        }
    }

    /// The last lines the build printed.
    pub fn output(&self) -> Vec<String> {
        self.output.tail().0
    }

    /// The folder the build puts the package's components in.
    pub fn staging(&self) -> &Path {
        &self.staging
    }

    /// Records one line the build printed.
    pub fn line(&self, line: &str) {
        self.output.line(line);
    }

    /// Whether the build should stop.
    pub fn is_stopped(&self) -> bool {
        self.stop.is_stopped()
    }

    /// Runs `command` (shown as `shown`) to its end, recording what it
    /// prints; its standard output goes to `stdout` instead when given. When
    /// the job is stopped, the command and every process it started are
    /// killed at once.
    pub(crate) fn run_command(
        &self,
        command: Command,
        shown: &str,
        stdout: Option<&mut dyn FnMut(&str)>,
    ) -> BuildOutcome {
        run_command(command, shown, self, stdout)
    }
}

/// Tells a running build to stop, killing the processes it started at
/// once. Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct BuildStop(Arc<StopState>);

#[derive(Default)]
struct StopState {
    stopped: AtomicBool,
    /// The processes of the command running, if one is.
    running: Mutex<Option<ProcessTree>>,
}

impl BuildStop {
    pub(crate) fn is_stopped(&self) -> bool {
        self.0.stopped.load(Ordering::SeqCst)
    }

    /// Stops the build: the processes of its running command are killed
    /// before this returns, and it starts no other. Never waits for them.
    pub(crate) fn stop(&self) {
        self.0.stopped.store(true, Ordering::SeqCst);
        if let Some(tree) = self.running().as_ref() {
            tree.kill();
        }
    }

    fn running(&self) -> std::sync::MutexGuard<'_, Option<ProcessTree>> {
        self.0.running.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Shows each line a build prints as it prints it, such as in the terminal
/// running `pane-ext dev`.
pub type Echo = Arc<dyn Fn(&str) + Send + Sync>;

/// What a build printed: the last lines in memory, and every line in a log
/// file, each also shown by its [`Echo`] if it has one. Cloning shares it.
#[derive(Clone)]
pub(crate) struct BuildOutput {
    printed: Arc<Mutex<Printed>>,
    echo: Option<Echo>,
}

struct Printed {
    tail: VecDeque<String>,
    bytes: usize,
    dropped: usize,
    log: Option<File>,
}

/// How much of a build's output is kept in memory; the log file has all.
const TAIL_LINES: usize = 2000;
const TAIL_BYTES: usize = 256 * 1024;

impl BuildOutput {
    /// Output also written, whole, to `log` if it can be created.
    pub(crate) fn new(log: Option<&Path>) -> BuildOutput {
        let log = log.and_then(|path| {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            File::create(path).ok()
        });
        BuildOutput {
            printed: Arc::new(Mutex::new(Printed {
                tail: VecDeque::new(),
                bytes: 0,
                dropped: 0,
                log,
            })),
            echo: None,
        }
    }

    /// This output, each line of which `echo` also shows.
    pub(crate) fn echoing(self, echo: Option<Echo>) -> BuildOutput {
        BuildOutput { echo, ..self }
    }

    pub(crate) fn line(&self, line: &str) {
        if let Some(echo) = &self.echo {
            echo(line);
        }
        let mut printed = self.printed.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(log) = &mut printed.log {
            let _ = writeln!(log, "{line}");
        }
        printed.bytes += line.len();
        printed.tail.push_back(line.to_owned());
        while printed.tail.len() > TAIL_LINES || printed.bytes > TAIL_BYTES {
            let Some(first) = printed.tail.pop_front() else {
                break;
            };
            printed.bytes -= first.len();
            printed.dropped += 1;
        }
    }

    /// The lines kept in memory, and how many earlier ones are only in the
    /// log.
    pub(crate) fn tail(&self) -> (Vec<String>, usize) {
        let printed = self.printed.lock().unwrap_or_else(|p| p.into_inner());
        (printed.tail.iter().cloned().collect(), printed.dropped)
    }
}

/// Pane's builds, with the tools they run, reading what a package's
/// manifest names with `M`.
#[derive(Clone, Debug)]
pub struct Toolchains<M> {
    /// Cargo, which builds Rust packages; `None` if none was found.
    pub cargo: Option<PathBuf>,
    /// How a JavaScript or TypeScript package is componentized (see
    /// [`Componentizer`]).
    pub componentizer: Componentizer,
    /// Reads the components a package's manifest names.
    pub manifests: M,
}

/// Where [`Toolchains::from_env`] looks, for the explanation when it finds
/// nothing.
const CARGO_LOOKUP: &str = "cargo on PATH, then ~/.cargo/bin/cargo";

/// How a JavaScript or TypeScript package's build componentizes it.
#[derive(Clone, Debug)]
pub enum Componentizer {
    /// A componentizer binary: the folder [`Toolchains::from_env`] resolved
    /// holds `componentize-qjs-p3`, `runtime.wasm` and `libc.so` — named by
    /// `PANE_COMPONENTIZER`, or the one a Pane source checkout builds
    /// (`target/guests/componentizer`, which `cargo xtask guests` makes) —
    /// else the package's own `@pane-app/cli` platform package
    /// (`node_modules/@pane-app/cli-<target>/`, which `npm install`
    /// provides), whose version the package pins.
    Binary(Option<PathBuf>),
    /// In this process: the componentizer pane-build links when built with
    /// its `componentizer` feature (`pane-ext` is, and Pane's own tests
    /// through its dev-dependencies), embedding the committed `runtime.wasm`
    /// and `libc.so` ([`crate::js_assets`]). [`Toolchains::from_env`] returns
    /// it whenever it is compiled in.
    #[cfg(feature = "componentizer")]
    Linked,
}

impl<M: ManifestFiles + Default> Toolchains<M> {
    /// The tools as this computer names them: `cargo` on `PATH` (rustup's
    /// proxy, so a package's `rust-toolchain.toml` applies), else in
    /// `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows); and how a
    /// JavaScript or TypeScript package is componentized: the folder
    /// `PANE_COMPONENTIZER` names, else this process's own componentizer
    /// where one is linked in, else the default folder when it holds one,
    /// else the binary the package itself installed.
    pub fn from_env(default_folder: Option<PathBuf>) -> Toolchains<M> {
        let home = env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        let cargo = find_on_path("cargo", |_| true).or_else(|| {
            let cargo = home?.join(".cargo/bin").join(executable("cargo"));
            cargo.is_file().then_some(cargo)
        });
        let componentizer = match env_path("PANE_COMPONENTIZER") {
            // Even a build with a componentizer of its own linked in obeys
            // it, so a specific binary is built with (one a workflow built,
            // a package's own): PANE_COMPONENTIZER names a folder holding
            // `componentize-qjs-p3`, `runtime.wasm` and `libc.so`.
            Some(folder) => Componentizer::Binary(Some(folder)),
            None => default_componentizer(default_folder),
        };
        Toolchains {
            cargo,
            componentizer,
            manifests: M::default(),
        }
    }
}

/// The componentizer a build uses when none is named: its own, linked in
/// (see [`Componentizer::Linked`]), else the default folder when it holds
/// one, else whatever the package itself installed.
fn default_componentizer(default: Option<PathBuf>) -> Componentizer {
    // With its `componentizer` feature pane-build componentizes in this
    // process; without it, a checkout's componentizer folder serves, and
    // without either the package's own does.
    #[cfg(feature = "componentizer")]
    {
        let _ = default;
        return Componentizer::Linked;
    }
    #[cfg(not(feature = "componentizer"))]
    {
        return match default.filter(|folder| crate::js::componentizer_parts(folder).is_ok()) {
            Some(folder) => Componentizer::Binary(Some(folder)),
            None => Componentizer::Binary(None),
        };
    }
}

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The first file `name` (with `.exe` on Windows) on `PATH` that `accept`
/// takes.
pub(crate) fn find_on_path(name: &str, accept: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(executable(name)))
        .find(|candidate| candidate.is_file() && accept(candidate))
}

impl<M: ManifestFiles + Clone> Builder for Toolchains<M> {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        let components = self.manifests.components(folder)?;
        let manifests: Arc<dyn ManifestFiles> = Arc::new(self.manifests.clone());
        if folder.join("Cargo.toml").is_file() {
            let Some(cargo) = self.cargo.clone() else {
                return Err(format!(
                    "Pane found no cargo to build it (it looked for {CARGO_LOOKUP})"
                ));
            };
            return Ok(Arc::new(CargoBuild {
                cargo,
                folder: folder.to_path_buf(),
                components,
                manifests,
            }));
        }
        if folder.join("package.json").is_file() {
            let componentizer = match self.componentizer.clone() {
                #[cfg(feature = "componentizer")]
                Componentizer::Linked => Componentizer::Linked,
                Componentizer::Binary(None) => match crate::js::package_componentizer(folder) {
                    Ok(package) => Componentizer::Binary(Some(package)),
                    Err(reason) => return Err(reason),
                },
                Componentizer::Binary(Some(folder)) => {
                    match crate::js::componentizer_parts(&folder) {
                        Ok(_) => Componentizer::Binary(Some(folder)),
                        Err(reason) => {
                            return Err(format!(
                                "PANE_COMPONENTIZER ({}) holds no componentizer: {reason}",
                                folder.display()
                            ));
                        }
                    }
                }
            };
            return Ok(Arc::new(JsBuild {
                componentizer,
                folder: folder.to_path_buf(),
                components,
                manifests,
            }));
        }
        Err(format!(
            "{} has neither Cargo.toml (Rust) nor package.json (JavaScript or TypeScript), so \
             Pane does not know how to build it; build it yourself and reload it",
            folder.display()
        ))
    }
}

/// The components named by the `pane.json` staged for `job`.
fn staged_components(
    manifests: &dyn ManifestFiles,
    job: &BuildJob,
) -> Result<Vec<PathBuf>, String> {
    manifests.components(job.staging())
}

/// Whether `path`, relative to a package folder, is or is inside one of
/// `outputs` (components the build writes), or is inside the top-level
/// folder of one that is in a folder (`dist/a.wasm` makes `dist` output).
fn is_output(path: &Path, outputs: &[PathBuf]) -> bool {
    outputs.iter().any(|output| {
        let mut parts = output.components();
        let top = parts.next();
        let nested = parts.next().is_some();
        path.starts_with(output) || (nested && top.is_some_and(|top| path.starts_with(top)))
    })
}

/// `text` as a shell would need it: quoted if it has spaces or quotes.
fn quoted(text: &str) -> String {
    if text.is_empty() || text.contains([' ', '\t', '"', '\'']) {
        format!("\"{}\"", text.replace('"', "\\\""))
    } else {
        text.to_owned()
    }
}

fn quoted_path(path: &Path) -> String {
    quoted(&path.display().to_string())
}

/// A Rust package: `cargo build --release --target wasm32-wasip2`.
struct CargoBuild {
    cargo: PathBuf,
    folder: PathBuf,
    components: Vec<PathBuf>,
    manifests: Arc<dyn ManifestFiles>,
}

impl CargoBuild {
    /// The documented command, with cargo's messages as JSON on standard
    /// output (diagnostics still rendered on standard error) to learn where
    /// the component was built.
    const ARGS: [&'static str; 5] = [
        "build",
        "--release",
        "--target",
        "wasm32-wasip2",
        "--message-format=json-render-diagnostics",
    ];
}

impl Build for CargoBuild {
    fn command(&self) -> String {
        format!("cargo {}", CargoBuild::ARGS.join(" "))
    }

    fn ignores(&self, path: &Path) -> bool {
        path == Path::new("Cargo.lock") || is_output(path, &self.components)
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let mut command = Command::new(&self.cargo);
        command.current_dir(&self.folder).args(CargoBuild::ARGS);
        without_pane_build_environment(&mut command);
        let mut built: Vec<PathBuf> = Vec::new();
        let mut messages = |line: &str| built.extend(wasm_artifacts(line));
        let shown = format!("{} (in {})", self.command(), quoted_path(&self.folder));
        match job.run_command(command, &shown, Some(&mut messages)) {
            BuildOutcome::Built => {}
            other => return other,
        }
        let components = match staged_components(&*self.manifests, job) {
            Ok(components) => components,
            Err(error) => return BuildOutcome::Failed(error),
        };
        for component in components {
            let name = component.file_name().unwrap_or_default();
            // This build's artifact of that name, wherever cargo put it.
            let Some(artifact) = built
                .iter()
                .rev()
                .find(|path| path.file_name() == Some(name))
            else {
                let names: Vec<String> = built
                    .iter()
                    .filter_map(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .collect();
                return BuildOutcome::Failed(format!(
                    "cargo built no {} this time, which pane.json names as {}, so Pane does not \
                     reload an older file left there (it built: {})",
                    name.to_string_lossy(),
                    component.display(),
                    if names.is_empty() {
                        "no component".into()
                    } else {
                        names.join(", ")
                    }
                ));
            };
            if let Err(error) = stage(artifact, &job.staging().join(&component)) {
                return BuildOutcome::Failed(format!(
                    "Pane could not stage {}: {error}",
                    artifact.display()
                ));
            }
        }
        BuildOutcome::Built
    }
}

/// The `.wasm` files a line of cargo's JSON messages reports built (a
/// `compiler-artifact` message), fresh or not.
pub(crate) fn wasm_artifacts(line: &str) -> Vec<PathBuf> {
    let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    if message["reason"] != "compiler-artifact" {
        return Vec::new();
    }
    message["filenames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str())
        .filter(|name| name.ends_with(".wasm"))
        .map(PathBuf::from)
        .collect()
}

/// Copies `from` to `to`, creating its folder.
fn stage(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to).map(|_| ())
}

/// A JavaScript or TypeScript package: Pane's own JS build
/// (`crate::js`), once per component.
struct JsBuild {
    componentizer: Componentizer,
    folder: PathBuf,
    components: Vec<PathBuf>,
    manifests: Arc<dyn ManifestFiles>,
}

impl JsBuild {
    /// The build's steps, as the author would read them: the tools the
    /// build runs, in order, in the package folder. `tsc` runs only where
    /// the package has a `tsconfig.json`; `npm ci` only when the lockfile
    /// changed.
    fn steps(&self) -> String {
        if self.folder.join("tsconfig.json").is_file() {
            "npm ci --ignore-scripts && tsc -p tsconfig.json && esbuild --bundle && componentize"
        } else {
            "npm ci --ignore-scripts && esbuild --bundle && componentize"
        }
        .into()
    }
}

impl Build for JsBuild {
    fn command(&self) -> String {
        self.steps()
    }

    fn ignores(&self, path: &Path) -> bool {
        is_output(path, &self.components)
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let components = match staged_components(&*self.manifests, job) {
            Ok(components) => components,
            Err(error) => return BuildOutcome::Failed(error),
        };
        for component in components {
            let out = job.staging().join(&component);
            match crate::js::build_js_command(job, &self.folder, &out, &self.componentizer) {
                BuildOutcome::Built => {}
                other => return other,
            }
        }
        BuildOutcome::Built
    }
}

/// Removes from `command`'s environment what `cargo run` (or `cargo xtask`,
/// or a test harness) injected for Pane itself, so the package's own
/// toolchain file and target folder apply. The author's own settings, such
/// as `CARGO_HOME`, `CARGO_TARGET_DIR`, registry tokens, `CARGO_HTTP_*` or
/// `CARGO_NET_OFFLINE`, are kept. (`pane_js.py`'s `clean_env` strips every
/// `CARGO_*` but `CARGO_HOME` for the toolchain it builds itself; its builds
/// of a package run cargo only for that toolchain.)
pub(crate) fn without_pane_build_environment(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        if injected_for_pane(key) {
            command.env_remove(key);
        }
    }
    // The dynamic library paths `cargo run` extends with Pane's target and
    // toolchain folders: only those entries go.
    if std::env::var_os("CARGO_PKG_NAME").is_some() {
        for key in [
            "LD_LIBRARY_PATH",
            "DYLD_LIBRARY_PATH",
            "DYLD_FALLBACK_LIBRARY_PATH",
            "PATH",
        ] {
            let Some(value) = std::env::var_os(key) else {
                continue;
            };
            let kept: Vec<PathBuf> = std::env::split_paths(&value)
                .filter(|entry| key == "PATH" || !injected_library_folder(entry))
                .collect();
            if key != "PATH" {
                match std::env::join_paths(kept) {
                    Ok(joined) if !joined.is_empty() => command.env(key, joined),
                    _ => command.env_remove(key),
                };
            }
        }
    }
}

/// Whether `cargo run`, `cargo test` or rustup set the variable `key` for
/// the Pane process rather than the user.
pub(crate) fn injected_for_pane(key: &str) -> bool {
    const NAMED: [&str; 12] = [
        "CARGO",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_BIN_NAME",
        "CARGO_CRATE_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_TARGET_TMPDIR",
        "CARGO_RUSTC_CURRENT_DIR",
        "OUT_DIR",
        // rustup's proxy pins its toolchain for the processes it starts;
        // the package's rust-toolchain.toml decides instead.
        "RUSTUP_TOOLCHAIN",
        "RUSTC",
        "RUSTDOC",
    ];
    NAMED.contains(&key) || key.starts_with("CARGO_PKG_") || key.starts_with("CARGO_BIN_EXE_")
}

/// Whether a dynamic library folder is one `cargo run` added for Pane: its
/// build output or a rustup toolchain's libraries.
fn injected_library_folder(entry: &Path) -> bool {
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf));
    let in_target = target.is_some_and(|target| entry.starts_with(target));
    let toolchain = entry
        .components()
        .any(|part| part.as_os_str() == "toolchains");
    in_target || toolchain
}

/// How often a running command is checked for having finished.
const POLL: Duration = Duration::from_millis(50);

/// How long the pipes of a finished command are read after its processes
/// are gone: a process outside its group (a daemon it started) may hold
/// them open, and is not waited for.
const DRAIN: Duration = Duration::from_secs(2);

enum Piped {
    Stdout(String),
    Stderr(String),
    Closed,
}

fn run_command(
    mut command: Command,
    shown: &str,
    job: &BuildJob,
    mut stdout: Option<&mut dyn FnMut(&str)>,
) -> BuildOutcome {
    if job.is_stopped() {
        return BuildOutcome::Stopped;
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    ProcessTree::prepare(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return BuildOutcome::Failed(format!("Pane could not run `{shown}`: {error}"));
        }
    };
    let tree = ProcessTree::adopt(&child);
    {
        let mut running = job.stop.running();
        if job.is_stopped() {
            tree.kill();
        }
        *running = Some(tree);
    }
    let (lines, piped) = mpsc::channel();
    read_lines(child.stdout.take(), lines.clone(), Piped::Stdout);
    read_lines(child.stderr.take(), lines, Piped::Stderr);
    let mut open = 2;
    let mut handle = |piped: Piped, open: &mut usize| match piped {
        Piped::Stdout(line) => match &mut stdout {
            Some(stdout) => stdout(&line),
            None => job.line(&line),
        },
        Piped::Stderr(line) => job.line(&line),
        Piped::Closed => *open -= 1,
    };
    let status: Option<ExitStatus> = loop {
        match piped.recv_timeout(POLL) {
            Ok(piped) => handle(piped, &mut open),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if job.is_stopped() {
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
    };
    // The command has ended or is stopped: what it started goes too, so no
    // process of the build outlives it.
    if let Some(tree) = job.stop.running().take() {
        tree.kill();
    }
    let _ = child.kill();
    let _ = child.wait();
    let deadline = Instant::now() + DRAIN;
    while open > 0 {
        let left = deadline.saturating_duration_since(Instant::now());
        match piped.recv_timeout(left) {
            Ok(piped) => handle(piped, &mut open),
            Err(_) => break,
        }
    }
    match status {
        _ if job.is_stopped() => BuildOutcome::Stopped,
        Some(status) if status.success() => BuildOutcome::Built,
        Some(status) => BuildOutcome::Failed(format!("`{shown}` failed ({})", describe(status))),
        None => BuildOutcome::Failed(format!("Pane lost track of `{shown}`")),
    }
}

fn describe(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => status.to_string(),
    }
}

/// Sends each line `pipe` carries, decoded on its own, then `Closed`.
fn read_lines(
    pipe: Option<impl Read + Send + 'static>,
    lines: mpsc::Sender<Piped>,
    wrap: fn(String) -> Piped,
) {
    let Some(pipe) = pipe else {
        let _ = lines.send(Piped::Closed);
        return;
    };
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end_matches(['\n', '\r']).to_owned();
                    if lines.send(wrap(text)).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = lines.send(Piped::Closed);
    });
}

/// The line of a build's output to show first: the first reporting an
/// error as rustc or cargo (`error:`, `error[E0308]:`), TypeScript
/// (`error TS2322`) or Pane's JS build (`pane-js: error:`) do, if one does.
pub(crate) fn first_error<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    lines
        .into_iter()
        .map(str::trim)
        .find(|line| reports_error(line))
}

fn reports_error(line: &str) -> bool {
    let rust = line.strip_prefix("error").is_some_and(|rest| {
        rest.starts_with(':')
            || rest.strip_prefix("[E").is_some_and(|code| {
                let digits = code.bytes().take_while(u8::is_ascii_digit).count();
                digits > 0 && code[digits..].starts_with("]:")
            })
    });
    let typescript = line.match_indices("error TS").any(|(at, found)| {
        line[at + found.len()..]
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_digit())
    });
    rust || typescript || line.starts_with("pane-js: error:")
}

/// Folders that belong to builds wherever they are.
const BUILD_FOLDERS: [&str; 3] = ["target", "node_modules", "dist"];

/// Whether `name` is a file editors write while saving: vim's `4913` probe
/// and swap files, backups (`name~`), JetBrains' `___jb_tmp___` and
/// `___jb_old___`, and Emacs's `.#name` locks and `#name#` autosaves.
fn is_editor_temporary(name: &str) -> bool {
    name == "4913"
        || name.ends_with('~')
        || [".swp", ".swo", ".swx"]
            .iter()
            .any(|end| name.ends_with(end))
        || name.ends_with("___jb_tmp___")
        || name.ends_with("___jb_old___")
        || name.starts_with(".#")
        || (name.starts_with('#') && name.ends_with('#'))
}

/// Whether a change at `path`, relative to a package folder, is a save for
/// `build`: not inside a `target`, `node_modules` or `dist` folder at any
/// depth, a hidden file or folder, an editor's temporary file, or the
/// build's own output.
pub fn is_save(path: &Path, build: &dyn Build) -> bool {
    let parts: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let excluded = parts
        .iter()
        .any(|part| part.starts_with('.') || BUILD_FOLDERS.contains(&part.as_str()));
    let temporary = parts.last().is_some_and(|name| is_editor_temporary(name));
    !parts.is_empty() && !excluded && !temporary && !build.ignores(path)
}

/// Stages the package in `folder` into `staging` for a build to add its
/// components to: its `pane.json`, and the files of the helpers it ships
/// for this system, which no build makes. A helper file that is not a
/// regular file (a link) is not copied, so the install checks refuse it as
/// they would in the source folder.
pub fn stage_package(
    manifests: &dyn ManifestFiles,
    folder: &Path,
    staging: &Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(staging)?;
    std::fs::copy(folder.join(MANIFEST_FILE), staging.join(MANIFEST_FILE))?;
    let Ok(files) = manifests.helper_files(staging) else {
        // The install checks explain it.
        return Ok(());
    };
    for file in files {
        let from = folder.join(&file);
        if std::fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_file()) {
            stage(&from, &staging.join(&file))?;
        }
    }
    Ok(())
}

/// Builds the package in `folder` once with `builder`, staging it into
/// `staging` (its `pane.json`, its helpers' files and the components its
/// build makes) and keeping the build's whole output in `log` when given.
/// Returns the staged components, relative to `staging`, or why it did not
/// build.
pub fn build_package(
    builder: &dyn Builder,
    manifests: &dyn ManifestFiles,
    folder: &Path,
    staging: &Path,
    log: Option<&Path>,
) -> Result<Vec<PathBuf>, BuildFailure> {
    let output = BuildOutput::new(log);
    let failed = |reason: String, output: BuildOutput| {
        failure(reason, output, || {
            log.filter(|log| log.is_file()).map(Path::to_path_buf)
        })
    };
    let build = match builder.build_for(folder) {
        Ok(build) => build,
        Err(reason) => return Err(failed(reason, output)),
    };
    if let Err(error) = stage_package(manifests, folder, staging) {
        let reason = format!(
            "Pane could not stage the package in {}: {error}",
            staging.display()
        );
        return Err(failed(reason, output));
    }
    let job = BuildJob::with(BuildStop::default(), staging.to_path_buf(), output.clone());
    match build.run(&job) {
        BuildOutcome::Built => manifests
            .components(staging)
            .map_err(|reason| failed(reason, output)),
        BuildOutcome::Failed(reason) => Err(failed(reason, output)),
        BuildOutcome::Stopped => Err(failed("The build was stopped".into(), output)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest that names `hello.wasm` and no helpers.
    struct Hello;

    impl ManifestFiles for Hello {
        fn components(&self, folder: &Path) -> Result<Vec<PathBuf>, String> {
            std::fs::read(folder.join(MANIFEST_FILE)).map_err(|error| error.to_string())?;
            Ok(vec![PathBuf::from("hello.wasm")])
        }

        fn helper_files(&self, _folder: &Path) -> Result<Vec<PathBuf>, String> {
            Ok(Vec::new())
        }
    }

    /// A build that prints, then writes `hello.wasm` or fails.
    struct Printing {
        fails: bool,
    }

    impl Builder for Printing {
        fn build_for(&self, _folder: &Path) -> Result<Arc<dyn Build>, String> {
            Ok(Arc::new(Printing { fails: self.fails }))
        }
    }

    impl Build for Printing {
        fn command(&self) -> String {
            "print".into()
        }

        fn ignores(&self, _path: &Path) -> bool {
            false
        }

        fn run(&self, job: &BuildJob) -> BuildOutcome {
            job.line("   Compiling hello");
            if self.fails {
                job.line("error[E0308]: mismatched types");
                return BuildOutcome::Failed("`print` failed (exit code 101)".into());
            }
            std::fs::write(job.staging().join("hello.wasm"), b"\0asm").unwrap();
            BuildOutcome::Built
        }
    }

    #[test]
    fn a_package_built_once_returns_its_staged_components_or_why_it_failed() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("source");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(MANIFEST_FILE), "{}").unwrap();
        let staging = dir.path().join("staging");
        let built = build_package(&Printing { fails: false }, &Hello, &folder, &staging, None);
        assert_eq!(built, Ok(vec![PathBuf::from("hello.wasm")]));
        assert!(staging.join(MANIFEST_FILE).is_file());
        assert!(staging.join("hello.wasm").is_file());

        let log = dir.path().join("logs/build.log");
        let failed = build_package(
            &Printing { fails: true },
            &Hello,
            &folder,
            &dir.path().join("again"),
            Some(&log),
        )
        .unwrap_err();
        assert_eq!(failed.summary, "error[E0308]: mismatched types");
        assert_eq!(
            failed.output.last().map(String::as_str),
            Some("`print` failed (exit code 101)")
        );
        assert_eq!(failed.log.as_deref(), Some(&*log));
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("   Compiling hello")
        );
    }

    #[test]
    fn the_first_error_is_a_real_diagnostic() {
        let cargo = "   Compiling thiserror v1.0.69\n   Compiling hello v0.1.0\n\
                     warning: unused variable `errors`\nerror[E0308]: mismatched types\n \
                     --> src/lib.rs:3:5\nerror: could not compile `hello`";
        assert_eq!(
            first_error(cargo.lines()),
            Some("error[E0308]: mismatched types")
        );
        assert_eq!(
            first_error("   Compiling thiserror v1.0\nerror: linking failed".lines()),
            Some("error: linking failed")
        );
        let tsc = "pane-js: type-checking hello-ts\n\
                   src/index.ts(12,7): error TS2322: Type 'number' is not assignable\n\
                   pane-js: error: `node tsc` failed with exit code 2";
        assert_eq!(
            first_error(tsc.lines()),
            Some("src/index.ts(12,7): error TS2322: Type 'number' is not assignable")
        );
        assert_eq!(
            first_error("pane-js: building\npane-js: error: `node` was not found".lines()),
            Some("pane-js: error: `node` was not found")
        );
        // Words that only contain "error" are not errors.
        let words =
            "   Compiling thiserror v1.0\nerrors: none\nerror[E]: x\nerror TSX\nno error: here";
        assert_eq!(first_error(words.lines()), None);
    }

    #[test]
    fn cargo_reports_where_it_built_the_wasm() {
        let artifact = r#"{"reason":"compiler-artifact","package_id":"x","target":{"kind":["cdylib"]},"filenames":["/elsewhere/wasm32-wasip2/release/hello.wasm","/elsewhere/wasm32-wasip2/release/deps/hello.d"],"fresh":true}"#;
        assert_eq!(
            wasm_artifacts(artifact),
            [PathBuf::from("/elsewhere/wasm32-wasip2/release/hello.wasm")]
        );
        assert!(wasm_artifacts(r#"{"reason":"build-finished","success":true}"#).is_empty());
        assert!(wasm_artifacts("   Compiling hello").is_empty());
    }

    #[test]
    fn only_what_cargo_run_injected_is_removed() {
        for injected in [
            "CARGO_MANIFEST_DIR",
            "CARGO_PKG_NAME",
            "CARGO_BIN_NAME",
            "CARGO_CRATE_NAME",
            "CARGO_PRIMARY_PACKAGE",
            "OUT_DIR",
            "CARGO",
            "RUSTUP_TOOLCHAIN",
        ] {
            assert!(injected_for_pane(injected), "{injected}");
        }
        for users in [
            "CARGO_HOME",
            "CARGO_TARGET_DIR",
            "CARGO_REGISTRIES_MINE_TOKEN",
            "CARGO_HTTP_PROXY",
            "CARGO_NET_OFFLINE",
            "RUSTUP_HOME",
            "PATH",
        ] {
            assert!(!injected_for_pane(users), "{users}");
        }
    }

    #[test]
    fn output_keeps_the_last_lines_and_logs_them_all() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs/build.log");
        let output = BuildOutput::new(Some(&log));
        for index in 0..TAIL_LINES + 5 {
            output.line(&format!("line {index}"));
        }
        let (tail, dropped) = output.tail();
        assert_eq!((tail.len(), dropped), (TAIL_LINES, 5));
        assert_eq!(tail[0], "line 5");
        let logged = std::fs::read_to_string(&log).unwrap();
        assert_eq!(logged.lines().count(), TAIL_LINES + 5);
    }

    #[test]
    fn an_echoed_output_shows_each_line_as_it_is_printed() {
        let shown = Arc::new(Mutex::new(Vec::new()));
        let echo: Echo = {
            let shown = shown.clone();
            Arc::new(move |line: &str| shown.lock().unwrap().push(line.to_owned()))
        };
        let output = BuildOutput::new(None).echoing(Some(echo));
        output.line("Compiling hello");
        output.clone().line("error: expected `;`");
        assert_eq!(
            *shown.lock().unwrap(),
            ["Compiling hello", "error: expected `;`"]
        );
        assert_eq!(output.tail().0.len(), 2);
    }

    fn job(dir: &Path) -> (BuildJob, BuildOutput) {
        let output = BuildOutput::new(None);
        let job = BuildJob::with(BuildStop::default(), dir.to_path_buf(), output.clone());
        (job, output)
    }

    /// A command that prints, then fails or succeeds.
    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", script]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        }
    }

    #[test]
    fn a_command_reports_its_lines_whole() {
        let dir = tempfile::tempdir().unwrap();
        let (job, output) = job(dir.path());
        let outcome = job.run_command(shell("echo built && echo warned 1>&2"), "echo", None);
        assert_eq!(outcome, BuildOutcome::Built);
        let (lines, _) = output.tail();
        // cmd keeps the space before `&&` and `1>&2` in what it echoes.
        let mut lines: Vec<&str> = lines.iter().map(|line| line.trim_end()).collect();
        lines.sort();
        assert_eq!(lines, ["built", "warned"]);
        let failed = job.run_command(shell("echo broken && exit 3"), "fail", None);
        assert_eq!(
            failed,
            BuildOutcome::Failed("`fail` failed (exit code 3)".into())
        );
        let missing = job.run_command(Command::new("pane-no-such-program"), "nothing", None);
        assert!(
            matches!(&missing, BuildOutcome::Failed(why) if why.starts_with("Pane could not run `nothing`")),
            "{missing:?}"
        );
    }

    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        // SAFETY: kill(2) with signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    fn wait_until_gone(pid: i32) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while process_exists(pid) {
            assert!(Instant::now() < deadline, "{pid} still runs");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    fn read_pid(file: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(pid) = std::fs::read_to_string(file)
                && let Ok(pid) = pid.trim().parse()
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "no pid in {}", file.display());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn stopping_a_command_kills_what_it_started_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 30 & echo $! > {}; wait", pid_file.display());
        let (job, _) = job(dir.path());
        let stop = job.stop.clone();
        let running = std::thread::spawn(move || job.run_command(shell(&script), "sleep", None));
        let grandchild = read_pid(&pid_file);
        let started = Instant::now();
        // Stopping kills before it returns, without waiting for the build.
        stop.stop();
        assert!(started.elapsed() < Duration::from_secs(1));
        wait_until_gone(grandchild);
        assert_eq!(running.join().unwrap(), BuildOutcome::Stopped);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_command_that_leaves_a_process_holding_its_pipes_still_returns() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // The shell exits at once; its child keeps stdout open.
        let script = format!("sleep 30 & echo $! > {}; echo done", pid_file.display());
        let (job, output) = job(dir.path());
        let started = Instant::now();
        let outcome = job.run_command(shell(&script), "leave", None);
        assert_eq!(outcome, BuildOutcome::Built);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(output.tail().0.contains(&"done".to_string()));
        // It was in the build's group, so it is gone too.
        wait_until_gone(read_pid(&pid_file));
    }

    #[cfg(unix)]
    #[test]
    fn a_process_outside_the_group_holding_the_pipes_is_not_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // setsid moves the sleeper to a session of its own, as a daemon
        // does: killing the group misses it, and it keeps the pipes open.
        // Perl's, since macOS has no setsid command. The shell waits for the
        // daemon's pid file instead of a fixed time: the group is killed the
        // moment the shell exits, and a slow start (a loaded runner) would
        // otherwise leave the daemon still inside the group when that
        // happens, so its pid never appears and the test times out (run
        // 36791672656's Linux leg). Waiting keeps the daemon's escape before
        // the exit, bounded so a daemon that never starts still fails fast.
        let script = format!(
            "perl -MPOSIX -e 'POSIX::setsid(); open(my $f, \">\", $ARGV[0]) or die; \
             print $f $$; close $f; exec(\"sleep\", \"30\")' '{}' & \
             i=0; while [ ! -s '{}' ] && [ \"$i\" -lt 200 ]; do sleep 0.01; i=$((i+1)); done; \
             echo done",
            pid_file.display(),
            pid_file.display()
        );
        let (job, _) = job(dir.path());
        let started = Instant::now();
        let outcome = job.run_command(shell(&script), "daemon", None);
        assert_eq!(outcome, BuildOutcome::Built);
        assert!(started.elapsed() < DRAIN + Duration::from_secs(5));
        let daemon = read_pid(&pid_file);
        // SAFETY: kill(2) on the test's own daemon.
        unsafe {
            libc::kill(daemon, libc::SIGKILL);
        }
    }
}
