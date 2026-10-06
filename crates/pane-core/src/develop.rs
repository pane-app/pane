//! Development mode's builds: how a local package's source folder is built
//! on save, and the processes such a build runs (ADR 0004; #12, #13).
//!
//! The launcher (`launcher::developing`) watches the folder with the
//! system's file watcher, runs the package's [`Build`] after each save and
//! reloads the package from what the build staged when it succeeds. This
//! module decides what a build is: one adapter per language, each running
//! the build command the guest README documents, in the package folder:
//!
//! - **Rust**, a folder with `Cargo.toml`: `cargo build --release --target
//!   wasm32-wasip2`, with cargo's JSON messages, so that the component taken
//!   is the one cargo reports it built this time, wherever its target folder
//!   is (a workspace member, `CARGO_TARGET_DIR`, `build.target-dir`), never
//!   an older file left where the manifest points.
//! - **JavaScript or TypeScript**, a folder with `package.json`: Pane's JS
//!   build, `python3 tools/componentize-js/pane_js.py build <folder> <out>`,
//!   once for each component the manifest names, writing straight into the
//!   staging folder.
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

use crate::packages::{MANIFEST_FILE, Manifest};
use crate::process_tree::ProcessTree;

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

/// What a build printed: the last lines in memory, and every line in a log
/// file. Cloning shares it.
#[derive(Clone)]
pub(crate) struct BuildOutput(Arc<Mutex<Printed>>);

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
        BuildOutput(Arc::new(Mutex::new(Printed {
            tail: VecDeque::new(),
            bytes: 0,
            dropped: 0,
            log,
        })))
    }

    pub(crate) fn line(&self, line: &str) {
        let mut printed = self.0.lock().unwrap_or_else(|p| p.into_inner());
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
        let printed = self.0.lock().unwrap_or_else(|p| p.into_inner());
        (printed.tail.iter().cloned().collect(), printed.dropped)
    }
}

/// Pane's builds, with the tools they run.
#[derive(Clone, Debug)]
pub struct Toolchains {
    /// Cargo, which builds Rust packages; `None` if none was found.
    pub cargo: Option<PathBuf>,
    /// The Python interpreter that runs Pane's JavaScript build.
    pub python: Option<PathBuf>,
    /// Pane's JavaScript build, `tools/componentize-js/pane_js.py` of a Pane
    /// source checkout; `None` if this Pane does not know one.
    pub componentize_js: Option<PathBuf>,
}

/// Where [`Toolchains::from_env`] looks, for the explanation when it finds
/// nothing.
const CARGO_LOOKUP: &str = "cargo on PATH, then ~/.cargo/bin/cargo";
const PYTHON_LOOKUP: &str = if cfg!(windows) {
    "PANE_PYTHON, then python3 and python on PATH (not the Microsoft Store's WindowsApps stub)"
} else {
    "PANE_PYTHON, then python3 on PATH"
};

impl Toolchains {
    /// The tools as this computer names them: `cargo` on `PATH` (rustup's
    /// proxy, so a package's `rust-toolchain.toml` applies), else in
    /// `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows); Python as
    /// `PANE_PYTHON`, else `python3` on `PATH`, else on Windows `python`
    /// (not the Microsoft Store stub in `WindowsApps`); and Pane's JS build
    /// as `PANE_COMPONENTIZE_JS`, else `default_js` if that file exists.
    pub fn from_env(default_js: Option<PathBuf>) -> Toolchains {
        let home = env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        let cargo = find_on_path("cargo", |_| true).or_else(|| {
            let cargo = home?.join(".cargo/bin").join(executable("cargo"));
            cargo.is_file().then_some(cargo)
        });
        let python = env_path("PANE_PYTHON")
            .or_else(|| find_on_path("python3", not_store_stub))
            .or_else(|| {
                cfg!(windows)
                    .then(|| find_on_path("python", not_store_stub))
                    .flatten()
            });
        let componentize_js =
            env_path("PANE_COMPONENTIZE_JS").or_else(|| default_js.filter(|path| path.is_file()));
        Toolchains {
            cargo,
            python,
            componentize_js,
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
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
fn find_on_path(name: &str, accept: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(executable(name)))
        .find(|candidate| candidate.is_file() && accept(candidate))
}

/// Windows' `python.exe` in `WindowsApps` only offers the Microsoft Store.
fn not_store_stub(path: &Path) -> bool {
    !path
        .components()
        .any(|part| part.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}

impl Builder for Toolchains {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        let manifest = Manifest::read_parsed(folder)
            .map_err(|error| error.to_string())?
            .0;
        let components = components(&manifest);
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
            }));
        }
        if folder.join("package.json").is_file() {
            let Some(python) = self.python.clone() else {
                return Err(format!(
                    "Pane found no Python to run its JavaScript build (it looked for {PYTHON_LOOKUP})"
                ));
            };
            let Some(tool) = self.componentize_js.clone() else {
                return Err("Pane does not know where its JavaScript build is: set \
                            PANE_COMPONENTIZE_JS to tools/componentize-js/pane_js.py in a Pane \
                            source checkout"
                    .into());
            };
            return Ok(Arc::new(JsBuild {
                python,
                tool,
                folder: folder.to_path_buf(),
                components,
            }));
        }
        Err(format!(
            "{} has neither Cargo.toml (Rust) nor package.json (JavaScript or TypeScript), so \
             Pane does not know how to build it; build it yourself and reload it",
            folder.display()
        ))
    }
}

/// The distinct components `manifest` names, relative to its folder.
pub(crate) fn components(manifest: &Manifest) -> Vec<PathBuf> {
    let mut components: Vec<PathBuf> = Vec::new();
    for (_, component) in manifest.components() {
        if !components.iter().any(|known| known == component) {
            components.push(component.to_path_buf());
        }
    }
    components
}

/// The components named by the `pane.json` staged for `job`.
fn staged_components(job: &BuildJob) -> Result<Vec<PathBuf>, String> {
    Manifest::read_parsed(job.staging())
        .map(|(manifest, _)| components(&manifest))
        .map_err(|error| error.to_string())
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
        let components = match staged_components(job) {
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

/// A JavaScript or TypeScript package: Pane's JS build, once per component.
struct JsBuild {
    python: PathBuf,
    tool: PathBuf,
    folder: PathBuf,
    components: Vec<PathBuf>,
}

impl JsBuild {
    fn command_for(&self, out: &Path) -> String {
        format!(
            "{} {} build {} {}",
            quoted_path(&self.python),
            quoted_path(&self.tool),
            quoted_path(&self.folder),
            quoted_path(out)
        )
    }
}

impl Build for JsBuild {
    fn command(&self) -> String {
        let commands: Vec<String> = self
            .components
            .iter()
            .map(|component| self.command_for(&self.folder.join(component)))
            .collect();
        commands.join(" && ")
    }

    fn ignores(&self, path: &Path) -> bool {
        is_output(path, &self.components)
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let components = match staged_components(job) {
            Ok(components) => components,
            Err(error) => return BuildOutcome::Failed(error),
        };
        for component in components {
            let out = job.staging().join(&component);
            let mut command = Command::new(&self.python);
            command
                .current_dir(&self.folder)
                .arg(&self.tool)
                .arg("build")
                .arg(&self.folder)
                .arg(&out);
            match job.run_command(command, &self.command_for(&out), None) {
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
pub(crate) fn is_save(path: &Path, build: &dyn Build) -> bool {
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
pub(crate) fn stage_package(folder: &Path, staging: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(staging)?;
    std::fs::copy(folder.join(MANIFEST_FILE), staging.join(MANIFEST_FILE))?;
    let Ok((manifest, _)) = Manifest::read_parsed(staging) else {
        // The install checks explain it.
        return Ok(());
    };
    for file in manifest.helpers.iter().filter_map(|h| h.for_this_system()) {
        let from = folder.join(file);
        if std::fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_file()) {
            stage(&from, &staging.join(file))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(folder: &Path, component: &str) {
        std::fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Hello", "apiVersion": "0.1",
                     "commands": [{{ "id": "hello", "title": "Hello", "component": "{component}" }}] }}"#
            ),
        )
        .unwrap();
    }

    fn toolchains() -> Toolchains {
        Toolchains {
            cargo: Some("cargo".into()),
            python: Some("python3".into()),
            componentize_js: Some(PathBuf::from("/pane/tools/componentize-js/pane_js.py")),
        }
    }

    #[test]
    fn a_folder_with_cargo_toml_builds_with_cargo_and_ignores_its_output() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "target/wasm32-wasip2/release/hello.wasm");
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        assert_eq!(
            build.command(),
            "cargo build --release --target wasm32-wasip2 --message-format=json-render-diagnostics"
        );
        for output in [
            "target",
            "target/wasm32-wasip2/release/hello.wasm",
            "Cargo.lock",
            "crates/inner/target/debug/x",
            "web/node_modules/zod/index.js",
            "web/dist/app.js",
        ] {
            assert!(!is_save(Path::new(output), &*build), "{output}");
        }
        for source in ["src/lib.rs", "Cargo.toml", "pane.json", "wit/world.wit"] {
            assert!(is_save(Path::new(source), &*build), "{source}");
        }
    }

    #[test]
    fn a_folder_with_package_json_builds_each_component_with_pane_js() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "dist/hello.wasm");
        std::fs::write(folder.path().join("package.json"), "{}").unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        let dir = folder.path().display();
        let out = folder.path().join("dist/hello.wasm");
        assert_eq!(
            build.command(),
            format!(
                "python3 /pane/tools/componentize-js/pane_js.py build {dir} {}",
                out.display()
            )
        );
        for output in ["dist", "dist/hello.wasm", "node_modules/zod/index.js"] {
            assert!(!is_save(Path::new(output), &*build), "{output}");
        }
        for source in ["src/index.ts", "package.json", "tsconfig.json", "pane.json"] {
            assert!(is_save(Path::new(source), &*build), "{source}");
        }
    }

    #[test]
    fn commands_quote_paths_with_spaces() {
        let folder = tempfile::tempdir().unwrap();
        let spaced = folder.path().join("my package");
        std::fs::create_dir(&spaced).unwrap();
        manifest(&spaced, "dist/hello.wasm");
        std::fs::write(spaced.join("package.json"), "{}").unwrap();
        let tools = Toolchains {
            python: Some("/opt/my python/bin/python3".into()),
            ..toolchains()
        };
        let command = tools.build_for(&spaced).unwrap().command();
        assert!(
            command.starts_with("\"/opt/my python/bin/python3\" "),
            "{command}"
        );
        assert!(
            command.contains(&format!("\"{}\"", spaced.display())),
            "{command}"
        );
    }

    #[test]
    fn hidden_files_and_editor_temporaries_are_not_saves() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "hello.wasm");
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        for path in [
            ".git/index",
            "src/.lib.rs.swp",
            "src/lib.rs.swp",
            "src/4913",
            "src/lib.rs~",
            "src/lib.rs___jb_tmp___",
            "src/.#lib.rs",
            "src/#lib.rs#",
            "hello.wasm",
            "",
        ] {
            assert!(!is_save(Path::new(path), &*build), "{path}");
        }
    }

    #[test]
    fn a_folder_without_a_known_build_or_tool_is_explained() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "hello.wasm");
        let error = toolchains().build_for(folder.path()).err().unwrap();
        assert!(error.contains("neither Cargo.toml"), "{error}");
        std::fs::write(folder.path().join("package.json"), "{}").unwrap();
        let no_js = Toolchains {
            componentize_js: None,
            ..toolchains()
        };
        let error = no_js.build_for(folder.path()).err().unwrap();
        assert!(error.contains("PANE_COMPONENTIZE_JS"), "{error}");
        let no_python = Toolchains {
            python: None,
            ..toolchains()
        };
        let error = no_python.build_for(folder.path()).err().unwrap();
        assert!(error.contains("PANE_PYTHON, then python3"), "{error}");
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        let no_cargo = Toolchains {
            cargo: None,
            ..toolchains()
        };
        let error = no_cargo.build_for(folder.path()).err().unwrap();
        assert!(
            error.contains("cargo on PATH, then ~/.cargo/bin/cargo"),
            "{error}"
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
    fn staging_takes_pane_json_and_this_systems_helper_files() {
        let target = pane_target::Target::current().expect("Pane names this system's target");
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("source");
        let file = format!("helpers/tool{}", target.exe_suffix());
        let other = if target.id() == "linux-x86_64" {
            "linux-aarch64"
        } else {
            "linux-x86_64"
        };
        std::fs::create_dir_all(folder.join("helpers")).unwrap();
        std::fs::write(folder.join(&file), b"a program").unwrap();
        std::fs::write(folder.join("helpers/other"), b"another system's").unwrap();
        std::fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Tool", "apiVersion": "0.1",
                     "commands": [{{ "id": "c", "title": "C", "component": "command.wasm" }}],
                     "helpers": [{{ "id": "tool", "targets": {{ "{}": "{file}", "{other}": "helpers/other" }} }}] }}"#,
                target.id()
            ),
        )
        .unwrap();
        let staging = dir.path().join("staging");
        stage_package(&folder, &staging).unwrap();
        assert!(staging.join("pane.json").is_file());
        assert_eq!(std::fs::read(staging.join(&file)).unwrap(), b"a program");
        assert!(!staging.join("helpers/other").exists());
        // The build adds the components.
        assert!(!staging.join("command.wasm").exists());
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
