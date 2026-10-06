//! Running a helper's process: which file may run (a regular file inside
//! the package, named and built for this target, read from its header), and
//! a supervising thread per run that feeds its input, collects its output
//! and ends and reaps the process when asked, when its generation ends or
//! when it writes too much. Nothing here depends on the extension runtime,
//! so it is checked for every system on its own.
//!
//! Pane sets no time limit on a run (#136, replacing #18's provisional 30
//! seconds): the runtime serves other calls while one waits for its helper,
//! so a helper runs for as long as its work takes. The command's own timeout
//! (dropping the run, which ends the process) and the ownership rules bound
//! it: the run ends with the call that started it, its instance, its
//! generation (each run is on the generation's undo list) and Pane.
//!
//! The process's standard input, output and error are unnamed scratch
//! files, not pipes: a process the helper starts that keeps them open
//! blocks nothing, and no thread waits on them. The supervising thread is
//! the only thread of a run, and it ends once it has reaped the process.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use pane_target::{Platform, Target};
use tokio::sync::oneshot;

use crate::generation::{End, Fence, Generation, Registration};
use crate::platform;

/// The largest input a helper run takes, in bytes.
pub const MAX_HELPER_INPUT: usize = 1 << 20;

/// The largest output (standard output) a helper run passes back, in
/// bytes; a helper writing more is stopped.
pub const MAX_HELPER_OUTPUT: usize = 1 << 20;

/// The most arguments a helper run takes.
const MAX_HELPER_ARGS: usize = 64;

/// The most bytes of all arguments together.
const MAX_HELPER_ARGS_BYTES: usize = 64 << 10;

/// The longest helper id.
pub(crate) const MAX_HELPER_ID: usize = 64;

/// How much of the end of a failed helper's standard error its error shows.
const STDERR_KEPT: u64 = 2048;

/// How often a supervising thread checks its process when nothing wakes it.
const TICK: Duration = Duration::from_millis(10);

/// How long stopping helpers waits for their supervising threads to reap
/// them, all together.
const REAP_WAIT: Duration = Duration::from_secs(5);

/// Why `id` cannot name a helper, if it cannot: ids are lowercase letters,
/// digits and dashes, starting with a letter or digit, at most
/// [`MAX_HELPER_ID`] long.
pub(crate) fn id_problem(id: &str) -> Option<String> {
    let starts_well = id
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let rest_well = id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !starts_well || !rest_well || id.len() > MAX_HELPER_ID {
        Some(format!(
            "helper id `{}` must be lowercase letters, digits and dashes, starting with a \
             letter or digit, at most {MAX_HELPER_ID} long",
            id.escape_debug()
        ))
    } else {
        None
    }
}

/// Why `file` cannot be the file of a helper built for `target`, from its
/// name alone: a Windows helper must be a `.exe` program (not a `.cmd` or
/// `.bat` script, which Windows would run through its command interpreter).
pub(crate) fn name_problem(file: &Path, target: Target) -> Option<String> {
    let suffix = target.exe_suffix();
    if suffix.is_empty() {
        return None;
    }
    let named = file
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(&suffix[1..]));
    (!named).then(|| {
        format!(
            "its file {} for {target} must be a program ending in {suffix}",
            file.display()
        )
    })
}

/// The absolute path that runs `file` of the package in `folder` as the
/// helper for `target`, with its extension, so that the system runs exactly
/// this file and never searches for or completes another one.
pub(crate) fn program_path(folder: &Path, file: &Path, target: Target) -> Result<PathBuf, String> {
    if let Some(problem) = name_problem(file, target) {
        return Err(problem);
    }
    std::path::absolute(folder.join(file))
        .map_err(|error| format!("its file {} has no usable path: {error}", file.display()))
}

/// Checks the helper `file` (relative, from `pane.json`) of the package in
/// `folder` for `target`, and returns the path to run it by: it must be a
/// regular file, not a symbolic link, inside the package folder once every
/// link on the way is resolved, and a program for `target`.
pub(crate) fn check_file(folder: &Path, file: &Path, target: Target) -> Result<PathBuf, String> {
    let program = program_path(folder, file, target)?;
    let shown = file.display();
    match std::fs::symlink_metadata(&program) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!("its file {shown} is missing"));
        }
        Err(error) => return Err(format!("its file {shown} cannot be read: {error}")),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "its file {shown} is a symbolic link; a helper must be a regular file in \
                 the package"
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!("its file {shown} is not a regular file"));
        }
        Ok(_) => {}
    }
    let inside = match (
        std::fs::canonicalize(&program),
        std::fs::canonicalize(folder),
    ) {
        (Ok(resolved), Ok(package)) => resolved.starts_with(package),
        _ => false,
    };
    if !inside {
        return Err(format!(
            "its file {shown} is outside the package folder once links are resolved"
        ));
    }
    match unfit(&program, file, target) {
        Some(problem) => Err(problem),
        None => Ok(program),
    }
}

/// Distinguishes [`copy_executable`]'s temporary names within one process.
static NEXT_COPY: AtomicU64 = AtomicU64::new(0);

/// Copies the executable file at `source` to `target`: to a fresh temporary
/// file beside `target` first, then renamed into place, so `target`'s name
/// never exists half-written.
///
/// This closes the classic Linux `ETXTBSY` race: the kernel refuses to
/// `exec` a file that any process, anywhere, still has open for writing, so
/// writing straight onto the path a helper is about to run under (a fresh
/// install run soon after, or a reinstall rewriting a helper another thread
/// is starting) can make that `exec` fail while the copy is still in
/// flight. `fs::copy` already opens its files the way Rust's `File` always
/// does, with `O_CLOEXEC`, so an unrelated fork mid-copy never hands a
/// child that descriptor across its own `exec`; what remains is `target`'s
/// own name being exec'd while its bytes are still arriving, which the
/// rename removes by only ever publishing a whole, already-closed file.
/// [`Helpers::start`]'s retry is the remaining backstop, for a race this
/// does not cover.
pub(crate) fn copy_executable(source: &Path, target: &Path) -> io::Result<()> {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::other(format!("{} names no file", target.display())))?;
    let temporary = dir.join(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        NEXT_COPY.fetch_add(1, Ordering::Relaxed)
    ));
    let copied = fs::copy(source, &temporary).and_then(|_| fs::rename(&temporary, target));
    if copied.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    copied.map(|_| ())
}

/// What a file is, as far as starting it goes, from its first bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Program {
    /// A native program for each of these targets (more than one for a
    /// macOS universal program).
    Native(Vec<Target>),
    /// A script starting with `#!`.
    Script,
    /// Not a program Pane recognizes.
    Unknown,
}

/// Reads what kind of program `path` is from its header.
fn program(path: &Path) -> io::Result<Program> {
    let mut header = Vec::new();
    File::open(path)?.take(4096).read_to_end(&mut header)?;
    Ok(program_of(&header))
}

fn program_of(header: &[u8]) -> Program {
    use pane_target::Arch;
    let u16_le = |at: usize| {
        header
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_le = |at: usize| {
        header
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let u32_be = |at: usize| {
        header
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let native = |os, archs: Vec<Option<Arch>>| {
        let targets: Vec<Target> = archs
            .into_iter()
            .flatten()
            .map(|arch| Target { os, arch })
            .collect();
        if targets.is_empty() {
            Program::Unknown
        } else {
            Program::Native(targets)
        }
    };
    let mach_arch = |cpu: u32| match cpu {
        0x0100_0007 => Some(Arch::X86_64),
        0x0100_000C => Some(Arch::Aarch64),
        _ => None,
    };
    if header.starts_with(b"#!") {
        return Program::Script;
    }
    if header.starts_with(b"\x7fELF") {
        // Both processors Pane names run 64-bit (class 2), little-endian
        // (data 1) programs; a 32-bit or big-endian one is another target.
        if header.get(4) != Some(&2) || header.get(5) != Some(&1) {
            return Program::Unknown;
        }
        let arch = match u16_le(18) {
            Some(0x3E) => Some(Arch::X86_64),
            Some(0xB7) => Some(Arch::Aarch64),
            _ => None,
        };
        return native(Platform::Linux, vec![arch]);
    }
    // A 64-bit little-endian Mach-O program.
    if header.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) {
        return native(Platform::Macos, vec![u32_le(4).and_then(mach_arch)]);
    }
    // A universal (fat) program shares its magic with Java class files,
    // whose next word is a version of at least 45; a fat header's is its
    // small number of programs.
    if u32_be(0) == Some(0xCAFE_BABE) {
        let count = u32_be(4).unwrap_or(0) as usize;
        if !(1..=20).contains(&count) {
            return Program::Unknown;
        }
        let archs = (0..count)
            .map(|i| u32_be(8 + i * 20).and_then(mach_arch))
            .collect();
        return native(Platform::Macos, archs);
    }
    if header.starts_with(b"MZ")
        && let Some(pe) = u32_le(0x3C).map(|at| at as usize)
        && header.get(pe..pe + 4) == Some(b"PE\0\0")
    {
        let arch = match u16_le(pe + 4) {
            Some(0x8664) => Some(Arch::X86_64),
            Some(0xAA64) => Some(Arch::Aarch64),
            _ => None,
        };
        return native(Platform::Windows, vec![arch]);
    }
    Program::Unknown
}

/// Why the file at `path` (named `shown` in explanations) cannot run as the
/// helper for `target`, if it cannot: it is missing, unreadable, a script,
/// or a program for another system.
fn unfit(path: &Path, shown: &Path, target: Target) -> Option<String> {
    let shown = shown.display();
    let kind = match program(path) {
        Ok(kind) => kind,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Some(format!("its file {shown} is missing"));
        }
        Err(error) => return Some(format!("its file {shown} cannot be read: {error}")),
    };
    match kind {
        Program::Native(targets) if targets.contains(&target) => None,
        Program::Native(targets) => {
            let names: Vec<String> = targets.iter().map(Target::to_string).collect();
            Some(format!(
                "its file {shown} is a program for {}, not {target}",
                platform::join(&names)
            ))
        }
        Program::Script => Some(format!(
            "its file {shown} is a script; a helper must be a native program built for \
             {target}"
        )),
        Program::Unknown => Some(format!(
            "its file {shown} is not a program Pane recognizes for {target}"
        )),
    }
}

/// Why a helper run did not produce the helper's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HelperError {
    pub kind: HelperErrorKind,
    pub message: String,
}

/// The WIT `helper-error-kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelperErrorKind {
    NotFound,
    Unavailable,
    Failed,
    Refused,
}

impl HelperError {
    pub(crate) fn new(kind: HelperErrorKind, message: impl Into<String>) -> HelperError {
        HelperError {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for HelperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Why stopped code may not start a helper.
pub(crate) fn stopped_code(end: End) -> HelperError {
    let why = match end {
        End::Disabled => "the extension is disabled",
        End::Replaced => "this code of the extension was replaced by a reload or an update",
        End::Uninstalled => "the extension was uninstalled",
        End::Paused => "the extension is paused after an error",
        End::Abandoned => {
            "Pane's extension runtime stopped responding and was replaced while this code ran"
        }
    };
    HelperError::new(
        HelperErrorKind::Refused,
        format!("{why}; its helpers do not run"),
    )
}

/// Why the code running `spec` is stopped, if it is: its generation ended,
/// or Pane gave up on its runtime thread.
fn stopped(spec: &Spec) -> Option<End> {
    spec.generation
        .as_ref()
        .and_then(Generation::ended)
        .or_else(|| {
            spec.fence
                .as_ref()
                .is_some_and(Fence::closed)
                .then_some(End::Abandoned)
        })
}

/// Refuses input or arguments over Pane's limits.
pub(crate) fn check_limits(args: &[String], input: &str) -> Result<(), HelperError> {
    let refused = |message: String| HelperError::new(HelperErrorKind::Refused, message);
    if input.len() > MAX_HELPER_INPUT {
        return Err(refused(format!(
            "the input is {} bytes; at most {MAX_HELPER_INPUT} are passed to a helper",
            input.len()
        )));
    }
    if args.len() > MAX_HELPER_ARGS {
        return Err(refused(format!(
            "{} arguments were given; a helper takes at most {MAX_HELPER_ARGS}",
            args.len()
        )));
    }
    let bytes: usize = args.iter().map(String::len).sum();
    if bytes > MAX_HELPER_ARGS_BYTES {
        return Err(refused(format!(
            "the arguments are {bytes} bytes; at most {MAX_HELPER_ARGS_BYTES} are passed"
        )));
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return Err(refused("an argument contains a NUL character".into()));
    }
    Ok(())
}

/// What to run: a helper's file, its arguments and input, and whose it is.
pub(crate) struct Spec {
    /// The helper's id, for explanations.
    pub name: String,
    /// The absolute path of the checked file ([`check_file`]).
    pub program: PathBuf,
    pub args: Vec<String>,
    pub input: String,
    /// The generation of the code running it: when it ends, so does the
    /// process.
    pub generation: Option<Generation>,
    /// The fence of the runtime thread running that code: once Pane gives
    /// up on the thread, the process ends too.
    pub fence: Option<Fence>,
    /// The guest instance that started it (see [`Helpers::stop_owned_by`]).
    pub owner: u64,
}

/// The helper processes of one runtime, for stopping and diagnostics.
/// Cloning shares them.
#[derive(Clone, Default)]
pub(crate) struct Helpers {
    state: Arc<Mutex<State>>,
    next_owner: Arc<AtomicU64>,
}

#[derive(Default)]
struct State {
    runs: Vec<Arc<Run>>,
    /// Set once Pane is quitting: no helper starts any more.
    quitting: bool,
}

/// One helper process and the thread supervising it.
struct Run {
    owner: u64,
    pid: u32,
    /// The file it runs.
    program: PathBuf,
    /// Set to end the process.
    stop: AtomicBool,
    /// The thread supervising the process, once it runs.
    supervisor: OnceLock<Thread>,
    /// Set once the process has been reaped.
    done: Mutex<bool>,
    reaped: Condvar,
}

impl Run {
    /// Asks the supervising thread to end and reap the process, without
    /// waiting for it.
    fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(supervisor) = self.supervisor.get() {
            supervisor.unpark();
        }
    }

    /// Waits until the process has been reaped, or `deadline` passes.
    fn wait_reaped(&self, deadline: Instant) {
        let mut done = self.done.lock().unwrap_or_else(|p| p.into_inner());
        while !*done {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return;
            };
            done = self
                .reaped
                .wait_timeout(done, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

/// How many times starting a helper retries after the system refuses to run
/// its file because another process still has it open for writing
/// (`ETXTBSY`), and the backoff before each retry, growing by `10ms` each time:
/// the standard mitigation for this Linux race, also used by cargo and
/// rustup. [`copy_executable`] closes the window for Pane's own writes, but
/// not one held open a moment longer by an antivirus scanner or another
/// writer entirely.
const SPAWN_BUSY_RETRIES: u32 = 5;
const SPAWN_BUSY_BACKOFF: Duration = Duration::from_millis(10);

/// Spawns `command`, retrying with [`SPAWN_BUSY_BACKOFF`] while the system
/// answers `ETXTBSY`.
fn spawn_retrying_busy(command: &mut Command) -> io::Result<Child> {
    let mut attempt = 0;
    loop {
        match command.spawn() {
            Err(error)
                if attempt < SPAWN_BUSY_RETRIES
                    && error.kind() == io::ErrorKind::ExecutableFileBusy =>
            {
                attempt += 1;
                thread::sleep(SPAWN_BUSY_BACKOFF * attempt);
            }
            result => return result,
        }
    }
}

/// Asks every run in `runs` to stop, then waits for all of them together.
fn stop_and_wait(runs: &[Arc<Run>]) {
    for run in runs {
        run.request_stop();
    }
    let deadline = Instant::now() + REAP_WAIT;
    for run in runs {
        run.wait_reaped(deadline);
    }
}

impl Helpers {
    /// A fresh owner id for a guest instance; never reused.
    pub fn new_owner(&self) -> u64 {
        self.next_owner.fetch_add(1, Ordering::Relaxed)
    }

    /// The process ids of the helpers running now, not yet reaped. A
    /// diagnostic for tests and logs.
    pub fn running(&self) -> Vec<u32> {
        self.lock().runs.iter().map(|run| run.pid).collect()
    }

    /// Asks the supervising threads to end every helper process `owner`
    /// started, without waiting: they reap them. Called on the runtime
    /// thread, which must not wait.
    pub fn stop_owned_by(&self, owner: u64) {
        for run in self.lock().runs.iter().filter(|run| run.owner == owner) {
            run.request_stop();
        }
    }

    /// Ends every helper process running a file inside `folder`, and waits
    /// (briefly) until each is reaped: before a package's managed copy is
    /// removed, so no running program holds it.
    pub fn stop_in(&self, folder: &Path) {
        let inside: Vec<Arc<Run>> = self
            .lock()
            .runs
            .iter()
            .filter(|run| run.program.starts_with(folder))
            .cloned()
            .collect();
        stop_and_wait(&inside);
    }

    /// Ends every helper process and waits (briefly) until each is reaped,
    /// for Pane quitting: no helper starts afterwards.
    pub fn stop_all(&self) {
        let all = {
            let mut state = self.lock();
            state.quitting = true;
            state.runs.clone()
        };
        stop_and_wait(&all);
    }

    /// Ends every helper process running now and waits (briefly) until each
    /// is reaped, without keeping later ones from starting: after the
    /// runtime thread crashed, when every run belonged to its guests.
    pub fn stop_running(&self) {
        let all = self.lock().runs.clone();
        stop_and_wait(&all);
    }

    /// Whether Pane is quitting ([`Helpers::stop_all`]).
    pub fn quitting(&self) -> bool {
        self.lock().quitting
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Starts the helper that `spec` finds, on a thread of its own: finding
    /// its file and starting a process may block, and the runtime thread
    /// only awaits. A run whose caller is gone by then ends at once.
    pub fn start_off_thread<S>(
        &self,
        spec: S,
    ) -> impl std::future::Future<Output = Result<Running, HelperError>> + Send + 'static + use<S>
    where
        S: FnOnce() -> Result<Spec, HelperError> + Send + 'static,
    {
        let helpers = self.clone();
        let (reply, started) = oneshot::channel();
        let spawned = thread::Builder::new()
            .name("pane-helper-start".into())
            .spawn(move || {
                // Dropped with the run, ending it, if nobody waits for it.
                let _ = reply.send(spec().and_then(|spec| helpers.start(spec)));
            });
        async move {
            spawned.map_err(|error| {
                HelperError::new(
                    HelperErrorKind::Unavailable,
                    format!("Pane could not start a helper: {error}"),
                )
            })?;
            started.await.unwrap_or_else(|_| {
                Err(HelperError::new(
                    HelperErrorKind::Unavailable,
                    "Pane's thread starting the helper stopped",
                ))
            })
        }
    }

    /// Starts the helper of `spec`. The returned run ends the process when
    /// it is dropped before it finishes. It may block (the system starts a
    /// process); the helpers' lock is not held meanwhile.
    pub fn start(&self, spec: Spec) -> Result<Running, HelperError> {
        let name = spec.name.clone();
        let unavailable = |what: &str, error: io::Error| {
            HelperError::new(
                HelperErrorKind::Unavailable,
                format!("{what} helper `{name}`: {error}"),
            )
        };
        let io = Streams::create(&spec.input)
            .map_err(|error| unavailable("Pane could not prepare the input of", error))?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::from(io.input))
            .stdout(Stdio::from(
                io.output
                    .try_clone()
                    .map_err(|error| unavailable("Pane could not start", error))?,
            ))
            .stderr(Stdio::from(
                io.errors
                    .try_clone()
                    .map_err(|error| unavailable("Pane could not start", error))?,
            ));
        if let Some(dir) = spec.program.parent() {
            command.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashes up for a helper of a GUI launcher.
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;
            command.creation_flags(CREATE_NO_WINDOW.0);
        }
        let quitting = || {
            HelperError::new(
                HelperErrorKind::Refused,
                format!("Pane is quitting; helper `{name}` does not start"),
            )
        };
        if self.quitting() {
            return Err(quitting());
        }
        // Code stopped since it asked (its generation ended, or Pane gave up
        // on its runtime thread) has no process started; one that stops
        // while the process starts is caught below, and the process ended.
        if let Some(end) = stopped(&spec) {
            return Err(stopped_code(end));
        }
        // Started without the lock held, which stopping helpers takes: a
        // start the system is slow with (or never finishes) holds up no one.
        let child = Owned(
            spawn_retrying_busy(&mut command)
                .map_err(|error| unavailable("the system did not start", error))?,
        );
        // Registered under the lock, so that quitting, or ending the helpers
        // of a runtime thread Pane gave up on, either sees this run or keeps
        // it from running on: the process is ended (by `Owned`) here then.
        let mut state = self.lock();
        if state.quitting {
            return Err(quitting());
        }
        if let Some(end) = stopped(&spec) {
            return Err(stopped_code(end));
        }
        let run = Arc::new(Run {
            owner: spec.owner,
            pid: child.0.id(),
            program: spec.program.clone(),
            stop: AtomicBool::new(false),
            supervisor: OnceLock::new(),
            done: Mutex::new(false),
            reaped: Condvar::new(),
        });
        state.runs.push(run.clone());
        drop(state);
        // On its generation's undo list while it runs: the generation's end
        // stops it at once. Finished or dropped, the run leaves the list.
        let undo = spec.generation.as_ref().map(|generation| {
            let run = run.clone();
            generation.on_end("native helper", move || {
                run.request_stop();
                Ok(())
            })
        });
        let (reply, result) = oneshot::channel();
        let supervised = {
            let (helpers, run) = (self.clone(), run.clone());
            let (output, errors) = (io.output, io.errors);
            move || {
                let outcome = supervise(child, &spec, &run, output, errors);
                helpers
                    .lock()
                    .runs
                    .retain(|other| !Arc::ptr_eq(other, &run));
                *run.done.lock().unwrap_or_else(|p| p.into_inner()) = true;
                run.reaped.notify_all();
                let _ = reply.send(outcome);
            }
        };
        // A fixed name: nothing of the package's reaches the thread's.
        match thread::Builder::new()
            .name("pane-helper".into())
            .spawn(supervised)
        {
            Ok(supervisor) => {
                let _ = run.supervisor.set(supervisor.thread().clone());
                // A stop asked for before the thread was known is seen by
                // its first check.
            }
            Err(error) => {
                // The process went with the closure, ended by `Owned`.
                self.lock().runs.retain(|other| !Arc::ptr_eq(other, &run));
                return Err(unavailable("Pane could not supervise", error));
            }
        }
        Ok(Running {
            run: Some(run),
            result,
            name,
            _undo: undo,
        })
    }
}

/// The helper's standard input, output and error: unnamed scratch files,
/// removed by the system once the last handle closes, readable only by the
/// user.
struct Streams {
    input: File,
    output: File,
    errors: File,
}

impl Streams {
    fn create(input: &str) -> io::Result<Streams> {
        let mut input_file = scratch_file()?;
        input_file.write_all(input.as_bytes())?;
        input_file.seek(SeekFrom::Start(0))?;
        Ok(Streams {
            input: input_file,
            output: scratch_file()?,
            errors: scratch_file()?,
        })
    }
}

/// A new file in the system's temporary folder that no other program can
/// find: removed at once on macOS and Linux (the open handles keep it),
/// deleted when its last handle closes on Windows.
fn scratch_file() -> io::Result<File> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir();
    loop {
        let path = dir.join(format!(
            "pane-helper-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const GENERIC_READ_WRITE_DELETE: u32 = 0x8000_0000 | 0x4000_0000 | 0x0001_0000;
            const SHARE_READ_WRITE_DELETE: u32 = 0x1 | 0x2 | 0x4;
            const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
            options
                .access_mode(GENERIC_READ_WRITE_DELETE)
                .share_mode(SHARE_READ_WRITE_DELETE)
                .custom_flags(FILE_FLAG_DELETE_ON_CLOSE);
        }
        match options.open(&path) {
            Ok(file) => {
                #[cfg(unix)]
                std::fs::remove_file(&path)?;
                return Ok(file);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// A child process that is ended and reaped when dropped, unless it was
/// reaped already.
struct Owned(Child);

impl Drop for Owned {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A helper run in progress. Dropping it before it finishes ends the
/// process: the guest cancelled the call, or its instance went.
pub(crate) struct Running {
    run: Option<Arc<Run>>,
    result: oneshot::Receiver<Result<String, HelperError>>,
    name: String,
    /// Its entry on its generation's undo list, taken off once it is done.
    _undo: Option<Registration>,
}

impl Running {
    /// The helper's output once it has exited, or why there is none.
    pub async fn finish(mut self) -> Result<String, HelperError> {
        let result = (&mut self.result).await;
        // Finished: nothing to end on drop.
        self.run = None;
        result.unwrap_or_else(|_| {
            Err(HelperError::new(
                HelperErrorKind::Failed,
                format!("helper `{}` ended without an answer", self.name),
            ))
        })
    }
}

impl Drop for Running {
    /// Asks for the process to end; its supervising thread reaps it, so
    /// the runtime thread dropping this never waits.
    fn drop(&mut self) {
        if let Some(run) = self.run.take() {
            run.request_stop();
        }
    }
}

/// Why a supervised process was ended before it exited by itself.
enum Ended {
    Generation(End),
    Stopped,
    TooMuchOutput,
}

/// Ends the process when asked to, when its generation ends or when it
/// writes too much, reaps it, then reads what it wrote.
fn supervise(
    mut child: Owned,
    spec: &Spec,
    run: &Run,
    mut output: File,
    mut errors: File,
) -> Result<String, HelperError> {
    let name = &spec.name;
    let child = &mut child.0;
    let written = |file: &File| file.metadata().map_or(0, |metadata| metadata.len());
    let status: Result<ExitStatus, Ended> = loop {
        // A generation's end comes first: its undo list also asks for the
        // stop, and the run says why it was stopped.
        let ended = if let Some(end) = stopped(spec) {
            Some(Ended::Generation(end))
        } else if run.stop.load(Ordering::SeqCst) {
            Some(Ended::Stopped)
        } else if written(&output) > MAX_HELPER_OUTPUT as u64 {
            Some(Ended::TooMuchOutput)
        } else {
            None
        };
        if let Some(ended) = ended {
            let _ = child.kill();
            let _ = child.wait();
            break Err(ended);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::park_timeout(TICK),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Ended::Stopped);
            }
        }
    };
    let too_much = || {
        HelperError::new(
            HelperErrorKind::Refused,
            format!(
                "helper `{name}` wrote more than {MAX_HELPER_OUTPUT} bytes of output; \
                 Pane stopped it"
            ),
        )
    };
    let status = match status {
        Ok(status) => status,
        Err(Ended::Generation(end)) => return Err(stopped_code(end)),
        Err(Ended::Stopped) => {
            return Err(HelperError::new(
                HelperErrorKind::Refused,
                format!("helper `{name}` was stopped before it finished"),
            ));
        }
        Err(Ended::TooMuchOutput) => return Err(too_much()),
    };
    // What it wrote by the time it exited; a process it started may still
    // write, and is not waited for.
    let mut answer = Vec::new();
    let read = output.seek(SeekFrom::Start(0)).and_then(|_| {
        (&mut output)
            .take(MAX_HELPER_OUTPUT as u64 + 1)
            .read_to_end(&mut answer)
    });
    if let Err(error) = read {
        return Err(HelperError::new(
            HelperErrorKind::Failed,
            format!("reading the output of helper `{name}` failed: {error}"),
        ));
    }
    if answer.len() > MAX_HELPER_OUTPUT {
        return Err(too_much());
    }
    if !status.success() {
        let mut tail = Vec::new();
        let from = written(&errors).saturating_sub(STDERR_KEPT);
        let _ = errors
            .seek(SeekFrom::Start(from))
            .and_then(|_| (&mut errors).take(STDERR_KEPT).read_to_end(&mut tail));
        let tail = String::from_utf8_lossy(&tail);
        let tail = tail.trim();
        let status = describe_status(status);
        return Err(HelperError::new(
            HelperErrorKind::Failed,
            if tail.is_empty() {
                format!("helper `{name}` failed ({status})")
            } else {
                format!("helper `{name}` failed ({status}): {tail}")
            },
        ));
    }
    String::from_utf8(answer).map_err(|_| {
        HelperError::new(
            HelperErrorKind::Failed,
            format!("helper `{name}` wrote output that is not UTF-8 text"),
        )
    })
}

/// "exit code 3", or how the system ended the process.
fn describe_status(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => status.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str) -> Target {
        Target::parse(id).unwrap()
    }

    fn elf(machine: u16) -> Vec<u8> {
        let mut header = b"\x7fELF\x02\x01\x01".to_vec();
        header.resize(18, 0);
        header.extend_from_slice(&machine.to_le_bytes());
        header.resize(64, 0);
        header
    }

    fn mach_o(cpu: u32) -> Vec<u8> {
        let mut header = vec![0xCF, 0xFA, 0xED, 0xFE];
        header.extend_from_slice(&cpu.to_le_bytes());
        header.resize(32, 0);
        header
    }

    fn universal(cpus: &[u32]) -> Vec<u8> {
        let mut header = 0xCAFE_BABEu32.to_be_bytes().to_vec();
        header.extend_from_slice(&(cpus.len() as u32).to_be_bytes());
        for cpu in cpus {
            let mut arch = cpu.to_be_bytes().to_vec();
            arch.resize(20, 0);
            header.extend_from_slice(&arch);
        }
        header
    }

    fn pe(machine: u16) -> Vec<u8> {
        let mut header = b"MZ".to_vec();
        header.resize(0x3C, 0);
        header.extend_from_slice(&0x80u32.to_le_bytes());
        header.resize(0x80, 0);
        header.extend_from_slice(b"PE\0\0");
        header.extend_from_slice(&machine.to_le_bytes());
        header
    }

    #[test]
    fn a_program_s_system_and_processor_are_read_from_its_header() {
        let native = |ids: &[&str]| Program::Native(ids.iter().map(|id| target(id)).collect());
        assert_eq!(program_of(&elf(0x3E)), native(&["linux-x86_64"]));
        assert_eq!(program_of(&elf(0xB7)), native(&["linux-aarch64"]));
        assert_eq!(program_of(&mach_o(0x0100_000C)), native(&["macos-aarch64"]));
        assert_eq!(
            program_of(&universal(&[0x0100_0007, 0x0100_000C])),
            native(&["macos-x86_64", "macos-aarch64"])
        );
        assert_eq!(program_of(&pe(0x8664)), native(&["windows-x86_64"]));
        assert_eq!(program_of(&pe(0xAA64)), native(&["windows-aarch64"]));
        assert_eq!(program_of(b"#!/bin/sh\necho hi\n"), Program::Script);
        assert_eq!(program_of(b"hello"), Program::Unknown);
        assert_eq!(program_of(&elf(0x28)), Program::Unknown);
        assert_eq!(program_of(b"MZ"), Program::Unknown);
    }

    #[test]
    fn a_32_bit_or_big_endian_elf_program_is_not_one_for_a_known_target() {
        let mut thirty_two = elf(0x3E);
        thirty_two[4] = 1;
        assert_eq!(program_of(&thirty_two), Program::Unknown);
        let mut big_endian = elf(0xB7);
        big_endian[5] = 2;
        assert_eq!(program_of(&big_endian), Program::Unknown);
    }

    #[test]
    fn a_java_class_file_is_not_a_universal_program() {
        // CAFEBABE, then minor version 0 and major version 65 (Java 21).
        let mut class = 0xCAFE_BABEu32.to_be_bytes().to_vec();
        class.extend_from_slice(&[0, 0, 0, 65]);
        class.resize(64, 0);
        assert_eq!(program_of(&class), Program::Unknown);
        assert_eq!(program_of(&universal(&[])), Program::Unknown);
    }

    #[test]
    fn a_file_for_another_target_or_a_script_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let shown = Path::new("helpers/linux-x86_64/echo");
        let linux = write("linux", &elf(0x3E));
        assert_eq!(unfit(&linux, shown, target("linux-x86_64")), None);
        assert_eq!(
            unfit(&linux, shown, target("linux-aarch64")).as_deref(),
            Some(
                "its file helpers/linux-x86_64/echo is a program for Linux x86-64, \
                 not Linux arm64"
            )
        );
        let windows = write("windows", &pe(0x8664));
        assert_eq!(
            unfit(&windows, shown, target("linux-x86_64")).as_deref(),
            Some(
                "its file helpers/linux-x86_64/echo is a program for Windows x86-64, \
                 not Linux x86-64"
            )
        );
        let fat = write("fat", &universal(&[0x0100_0007, 0x0100_000C]));
        assert_eq!(unfit(&fat, shown, target("macos-aarch64")), None);
        assert_eq!(unfit(&fat, shown, target("macos-x86_64")), None);
        let script = write("script", b"#!/bin/sh\n");
        for system in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
            let problem = unfit(&script, shown, target(system)).unwrap();
            assert!(problem.contains("is a script"), "{problem}");
        }
        let text = write("text", b"not a program");
        assert_eq!(
            unfit(&text, shown, target("linux-x86_64")).as_deref(),
            Some(
                "its file helpers/linux-x86_64/echo is not a program Pane recognizes \
                 for Linux x86-64"
            )
        );
        assert_eq!(
            unfit(&dir.path().join("absent"), shown, target("linux-x86_64")).as_deref(),
            Some("its file helpers/linux-x86_64/echo is missing")
        );
    }

    /// The path logic, the same on every system: a Windows helper must name
    /// a `.exe` file, and the path run is absolute with that extension, so
    /// Windows never searches for another program or appends `.exe`.
    #[test]
    fn the_path_run_is_the_absolute_file_with_its_extension() {
        let windows = target("windows-x86_64");
        for refused in [
            "helpers/echo",
            "helpers/echo.cmd",
            "helpers/echo.bat",
            "helpers/echo.ps1",
        ] {
            let problem = program_path(Path::new("package"), Path::new(refused), windows);
            assert_eq!(
                problem,
                Err(format!(
                    "its file {refused} for Windows x86-64 must be a program ending in .exe"
                )),
            );
        }
        for accepted in ["helpers/echo.exe", "helpers/ECHO.EXE"] {
            let path = program_path(Path::new("package"), Path::new(accepted), windows).unwrap();
            assert!(path.is_absolute(), "{}", path.display());
            assert!(path.ends_with(accepted), "{}", path.display());
        }
        let linux = program_path(
            Path::new("package"),
            Path::new("helpers/echo"),
            target("linux-x86_64"),
        )
        .unwrap();
        assert!(linux.is_absolute());
        assert!(linux.ends_with("package/helpers/echo"));
    }

    #[test]
    fn helper_ids_are_lowercase_letters_digits_and_dashes() {
        for good in [
            "echo",
            "a",
            "0",
            "file-convert-2",
            &"x".repeat(MAX_HELPER_ID),
        ] {
            assert_eq!(id_problem(good), None, "{good}");
        }
        for bad in [
            "",
            "-echo",
            "Echo",
            "echo_1",
            "echo.exe",
            "a\0b",
            "é",
            "a b",
            &"x".repeat(65),
        ] {
            assert!(id_problem(bad).is_some(), "{bad:?}");
        }
    }

    /// A package folder in a temporary folder holding `file` as this
    /// system's real helper.
    fn package_with_echo(file: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(echo(), &path).unwrap();
        dir
    }

    fn this() -> Target {
        Target::current().expect("Pane names this system's target")
    }

    fn exe(file: &str) -> String {
        format!("{file}{}", this().exe_suffix())
    }

    #[test]
    fn a_regular_file_for_this_system_inside_the_package_runs() {
        let file = exe("helpers/echo");
        let package = package_with_echo(&file);

        let program = check_file(package.path(), Path::new(&file), this()).unwrap();

        assert!(program.is_absolute());
        assert_eq!(
            std::fs::canonicalize(&program).unwrap(),
            std::fs::canonicalize(package.path().join(&file)).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_or_a_file_outside_the_package_is_refused() {
        use std::os::unix::fs::symlink;
        let outside = package_with_echo("real/pane-echo");
        let package = tempfile::tempdir().unwrap();
        let helpers = package.path().join("helpers");
        std::fs::create_dir_all(&helpers).unwrap();
        // The file itself a link, even to a real program.
        symlink(
            outside.path().join("real/pane-echo"),
            helpers.join("linked"),
        )
        .unwrap();
        assert_eq!(
            check_file(package.path(), Path::new("helpers/linked"), this()),
            Err(
                "its file helpers/linked is a symbolic link; a helper must be a regular \
                 file in the package"
                    .into()
            )
        );
        // A folder on the way a link to somewhere else.
        symlink(
            outside.path().join("real"),
            package.path().join("elsewhere"),
        )
        .unwrap();
        assert_eq!(
            check_file(package.path(), Path::new("elsewhere/pane-echo"), this()),
            Err(
                "its file elsewhere/pane-echo is outside the package folder once links \
                 are resolved"
                    .into()
            )
        );
        std::fs::create_dir(helpers.join("folder")).unwrap();
        assert_eq!(
            check_file(package.path(), Path::new("helpers/folder"), this()),
            Err("its file helpers/folder is not a regular file".into())
        );
    }

    /// The helper sample's `pane-echo`, built for this system by `cargo
    /// xtask guests`.
    fn echo() -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/sample-helper/helpers")
            .join(this().id())
            .join(exe("pane-echo"));
        assert!(
            path.exists(),
            "{} is missing; run `cargo xtask guests`",
            path.display()
        );
        path
    }

    /// How long a test's waiting helper waits: short, so that one left
    /// behind by a failing test ends soon by itself.
    const WAIT: &str = "5";

    /// The runs of one test, whose processes all end with the test, even
    /// when it fails.
    struct Runs(Helpers);

    impl Drop for Runs {
        fn drop(&mut self) {
            self.0.stop_all();
        }
    }

    impl std::ops::Deref for Runs {
        type Target = Helpers;
        fn deref(&self) -> &Helpers {
            &self.0
        }
    }

    fn runs() -> Runs {
        Runs(Helpers::default())
    }

    fn spec(args: &[&str], generation: Option<Generation>, owner: u64) -> Spec {
        Spec {
            fence: None,
            name: "echo".into(),
            program: echo(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            input: "hi".into(),
            generation,
            owner,
        }
    }

    /// Waits until `helpers` runs no process, or panics.
    fn until_none_run(helpers: &Helpers) {
        let started = Instant::now();
        while !helpers.running().is_empty() {
            assert!(started.elapsed() < Duration::from_secs(5), "still running");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_run_answers_with_the_helper_s_output() {
        let helpers = runs();

        let running = helpers.start(spec(&[], None, 0)).unwrap();

        let answer = futures::executor::block_on(running.finish()).unwrap();
        assert_eq!(answer, format!("Echoed \"hi\" on {}", this()));
        assert_eq!(helpers.running(), Vec::<u32>::new());
    }

    #[test]
    fn a_failing_helper_s_errors_are_its_explanation() {
        let helpers = runs();

        let running = helpers.start(spec(&["--fail"], None, 0)).unwrap();

        let error = futures::executor::block_on(running.finish()).unwrap_err();
        assert_eq!(error.kind, HelperErrorKind::Failed);
        assert_eq!(
            error.message,
            "helper `echo` failed (exit code 3): pane-echo was asked to fail"
        );
    }

    #[test]
    fn dropping_a_run_ends_its_process_without_waiting() {
        let helpers = runs();
        let running = helpers.start(spec(&["--wait", WAIT], None, 0)).unwrap();
        assert_eq!(helpers.running().len(), 1);

        let dropped = Instant::now();
        drop(running);

        assert!(dropped.elapsed() < Duration::from_millis(100));
        until_none_run(&helpers);
    }

    /// A run is on its generation's undo list while it runs, and leaves it
    /// once it finished: the list holds only what is still set up.
    #[test]
    fn a_run_is_on_its_generation_s_undo_list_until_it_finishes() {
        let helpers = runs();
        let generation = Generation::new();

        let running = helpers
            .start(spec(&[], Some(generation.clone()), 0))
            .unwrap();

        assert_eq!(generation.undo_list(), ["native helper"]);
        futures::executor::block_on(running.finish()).unwrap();
        assert_eq!(generation.undo_list(), Vec::<&str>::new());
    }

    /// No one polls the run: its supervising thread sees the generation end
    /// by itself, as it must while the runtime thread is busy in a guest.
    #[test]
    fn a_generation_ending_ends_its_helpers_without_waiting_for_the_runtime() {
        let helpers = runs();
        let generation = Generation::new();
        let running = helpers
            .start(spec(&["--wait", WAIT], Some(generation.clone()), 0))
            .unwrap();

        drop(generation.end(End::Disabled));
        until_none_run(&helpers);

        assert_eq!(
            futures::executor::block_on(running.finish()),
            Err(stopped_code(End::Disabled))
        );
    }

    /// Stopped code (its generation ended, or its runtime thread's fence
    /// closed) has no helper process started at all: the check comes before
    /// the system is asked to start one, here a file that does not exist,
    /// whose start the system would refuse with another error.
    #[test]
    fn stopped_code_has_no_helper_process_started() {
        let helpers = runs();
        let dir = tempfile::tempdir().unwrap();
        let generation = Generation::new();
        drop(generation.end(End::Disabled));
        let mut ended = spec(&[], Some(generation), 0);
        ended.program = dir.path().join("never-started");
        let fence = Fence::default();
        fence.close();
        let mut fenced = spec(&[], None, 0);
        fenced.program = dir.path().join("never-started");
        fenced.fence = Some(fence);

        assert_eq!(
            helpers.start(ended).err(),
            Some(stopped_code(End::Disabled))
        );
        assert_eq!(
            helpers.start(fenced).err(),
            Some(stopped_code(End::Abandoned))
        );
        assert_eq!(helpers.running(), Vec::<u32>::new());
    }

    #[test]
    fn stopping_an_owner_s_helpers_leaves_the_others_running() {
        let helpers = runs();
        let (first, second) = (helpers.new_owner(), helpers.new_owner());
        let _mine = helpers.start(spec(&["--wait", WAIT], None, first)).unwrap();
        let theirs = helpers
            .start(spec(&["--wait", WAIT], None, second))
            .unwrap();
        let theirs_pid = helpers.running()[1];

        helpers.stop_owned_by(first);
        until_one_runs(&helpers);

        assert_eq!(helpers.running(), [theirs_pid]);
        drop(theirs);
        until_none_run(&helpers);
    }

    fn until_one_runs(helpers: &Helpers) {
        let started = Instant::now();
        while helpers.running().len() > 1 {
            assert!(started.elapsed() < Duration::from_secs(5), "still running");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn stopping_all_ends_every_helper_at_once_and_starts_no_more() {
        let helpers = runs();
        let (first, second) = (helpers.new_owner(), helpers.new_owner());
        let mine = helpers.start(spec(&["--wait", WAIT], None, first)).unwrap();
        let _theirs = helpers
            .start(spec(&["--wait", WAIT], None, second))
            .unwrap();

        let stopping = Instant::now();
        helpers.stop_all();

        assert!(stopping.elapsed() < Duration::from_secs(2));
        assert_eq!(helpers.running(), Vec::<u32>::new());
        let stopped = futures::executor::block_on(mine.finish()).unwrap_err();
        assert_eq!(stopped.kind, HelperErrorKind::Refused);
        assert_eq!(
            stopped.message,
            "helper `echo` was stopped before it finished"
        );
        let later = helpers.start(spec(&[], None, first)).err().unwrap();
        assert_eq!(later.kind, HelperErrorKind::Refused);
        assert_eq!(
            later.message,
            "Pane is quitting; helper `echo` does not start"
        );
        assert_eq!(helpers.running(), Vec::<u32>::new());
    }

    #[test]
    fn stopping_the_helpers_in_a_folder_waits_for_them_and_leaves_the_others() {
        let helpers = runs();
        let other = package_with_echo(&exe("pane-echo"));
        let _inside = helpers.start(spec(&["--wait", WAIT], None, 0)).unwrap();
        let mut elsewhere = spec(&["--wait", WAIT], None, 0);
        elsewhere.program = other.path().join(exe("pane-echo"));
        let _outside = helpers.start(elsewhere).unwrap();

        helpers.stop_in(echo().parent().unwrap());

        assert_eq!(helpers.running().len(), 1);
    }

    #[test]
    fn a_helper_writing_too_much_is_stopped() {
        let helpers = runs();

        let running = helpers.start(spec(&["--flood"], None, 0)).unwrap();

        let error = futures::executor::block_on(running.finish()).unwrap_err();
        assert_eq!(error.kind, HelperErrorKind::Refused);
        assert!(error.message.contains("more than"), "{}", error.message);
    }

    #[test]
    fn a_program_that_cannot_start_is_unavailable() {
        let helpers = runs();
        let mut missing = spec(&[], None, 0);
        missing.program = std::path::absolute("nonexistent/pane-echo").unwrap();

        let error = helpers.start(missing).err().unwrap();

        assert_eq!(error.kind, HelperErrorKind::Unavailable);
        assert!(
            error
                .message
                .starts_with("the system did not start helper `echo`: "),
            "{}",
            error.message
        );
        assert_eq!(helpers.running(), Vec::<u32>::new());
    }

    /// The classic Linux `ETXTBSY` race: a helper file rewritten (a fresh
    /// install run soon after, or a reinstall) while it, or another
    /// generation of it, is being spawned elsewhere. Many threads copy and
    /// spawn the same few files at once; `copy_executable`'s rename and
    /// `spawn_retrying_busy`'s retry (see [`super::spawn_retrying_busy`])
    /// must mean none of it ever fails. Unix only: Windows refuses to
    /// replace a running program at all, which is why Pane stops a
    /// generation's helpers before replacing their files.
    #[cfg(unix)]
    #[test]
    fn many_threads_copying_and_spawning_the_same_helper_never_see_it_busy() {
        let helpers = runs();
        let file = exe("pane-echo");
        // A few shared paths, so copies and spawns collide on the same
        // file repeatedly rather than each getting its own.
        let dirs: Vec<tempfile::TempDir> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        let paths: Vec<PathBuf> = dirs
            .iter()
            .map(|dir| {
                let path = dir.path().join(&file);
                copy_executable(&echo(), &path).unwrap();
                path
            })
            .collect();

        thread::scope(|scope| {
            for path in &paths {
                scope.spawn(move || {
                    for _ in 0..30 {
                        copy_executable(&echo(), path).unwrap();
                    }
                });
            }
            for path in &paths {
                let helpers = &helpers;
                for _ in 0..4 {
                    scope.spawn(move || {
                        for _ in 0..10 {
                            let mut one = spec(&[], None, 0);
                            one.program = path.clone();
                            let running = helpers.start(one).unwrap();
                            let answer = futures::executor::block_on(running.finish()).unwrap();
                            assert!(answer.starts_with("Echoed"), "{answer}");
                        }
                    });
                }
            }
        });

        until_none_run(&helpers);
    }

    #[test]
    fn inputs_over_the_limits_are_refused() {
        assert!(check_limits(&[], "hello").is_ok());
        let big = "x".repeat(MAX_HELPER_INPUT + 1);
        assert_eq!(
            check_limits(&[], &big).unwrap_err().kind,
            HelperErrorKind::Refused
        );
        let many = vec![String::new(); MAX_HELPER_ARGS + 1];
        assert!(check_limits(&many, "").is_err());
        assert!(check_limits(&["a\0b".into()], "").is_err());
    }
}
