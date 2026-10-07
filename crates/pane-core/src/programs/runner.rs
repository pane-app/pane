//! Running a system program for a guest: finding its file, starting it in a
//! process tree of its own ([`crate::process_tree`]), feeding its input,
//! reading its output and ending it with everything it started. Nothing
//! here depends on the extension runtime.
//!
//! Each program has one thread of its own that starts it (so that on
//! Linux the program's parent-death signal is tied to a thread that lives
//! as long as it does) and supervises it until it is reaped, and threads
//! that move its streams. The runtime thread only awaits.
//!
//! **Ownership** (ADR 0033). A program belongs to the call that started it,
//! as a helper does, and is registered with the runtime's helpers
//! ([`Helpers`]), so it ends the same ways: when the call returns or is
//! dropped, when its instance goes, when its package's generation ends
//! (each program is on the generation's undo list while it runs), when Pane
//! quits and when its runtime thread is given up on. Ending a program ends
//! every process it started (its tree), and so does its own exit: what it
//! leaves running ends with it, so that its output ends too. A command
//! that wants a program to outlive it opens it with the system instead.
//!
//! **Limits.** No fixed time limit: the command's own timeout and the
//! ownership rules bound a run. A `run` keeps at most
//! [`MAX_PROGRAM_OUTPUT`] bytes of each stream; a program writing more is
//! ended and the run fails, saying so. A `spawn`'s streams are read as the
//! command reads them, and a program that writes faster waits.

use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, watch};

use super::{elevated, search};
use crate::generation::{End, Fence, Generation};
use crate::helpers::runner::{Helpers, Kind, Refusal, Run};
use crate::process_tree::{self, ProcessTree};

/// The most bytes of each stream, standard output and standard error, a
/// program run with `run` may write: one writing more is ended and the run
/// fails (proposed by the specification; provisional).
pub const MAX_PROGRAM_OUTPUT: usize = 16 << 20;

/// The largest input a program is given in one piece, in bytes.
pub(crate) const MAX_PROGRAM_INPUT: usize = 16 << 20;

/// The most arguments a program takes.
const MAX_ARGS: usize = 4096;

/// The most bytes of all arguments together.
const MAX_ARGS_BYTES: usize = 1 << 20;

/// The most environment changes a program takes.
const MAX_ENVIRONMENT: usize = 1024;

/// How often a supervising thread checks its program when nothing wakes it.
const TICK: Duration = Duration::from_millis(10);

/// How much of a stream a `spawn` passes on at a time.
const CHUNK: usize = 64 << 10;

/// How many chunks of a `spawn`'s stream wait to be read before the
/// program waits too.
const BUFFERED_CHUNKS: usize = 16;

/// How long a run waits, once its program ended, for the rest of its output
/// (a process that left its tree may still hold its streams).
const DRAIN: Duration = Duration::from_secs(2);

/// Where programs named by a bare name are found: the search path, as the
/// system's `PATH` variable spells it, asked for at each call. Tests give
/// the runtime one of their own ([`crate::Runtime::set_program_search_path`]).
pub type SearchPath = Arc<dyn Fn() -> OsString + Send + Sync>;

/// Why a program did not answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgramError {
    pub kind: ErrorKind,
    pub message: String,
}

/// The WIT `program-error-kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    NotFound,
    Unavailable,
    /// The user declined the elevation prompt, which only Windows shows.
    #[cfg_attr(not(windows), allow(dead_code))]
    Declined,
    TimedOut,
    TooMuchOutput,
    Refused,
    Failed,
}

impl ProgramError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> ProgramError {
        ProgramError {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// What to run, as the command asked.
#[derive(Clone, Debug, Default)]
pub(crate) struct Request {
    /// The program as the command named it: an absolute path, or a bare
    /// name found on the search path.
    pub program: String,
    pub args: Vec<String>,
    /// The folder it runs in; the user's home folder when `None`.
    pub folder: Option<String>,
    /// Variables to set (`Some`) or remove (`None`), after Pane's own.
    pub environment: Vec<(String, Option<String>)>,
    /// When to end it, counted from its start.
    pub timeout: Option<Duration>,
    /// Whether it gets a console window of its own on Windows.
    pub show_window: bool,
    /// Whether it runs elevated (Windows' elevation prompt).
    pub elevated: bool,
}

impl Request {
    /// Refuses what is over Pane's limits or cannot be passed to a
    /// program: a NUL character, a malformed variable name.
    pub(crate) fn check(&self, input: &[u8]) -> Result<(), ProgramError> {
        let refused = |message: String| ProgramError::new(ErrorKind::Refused, message);
        if input.len() > MAX_PROGRAM_INPUT {
            return Err(refused(format!(
                "the input is {} bytes; at most {MAX_PROGRAM_INPUT} are passed to a program",
                input.len()
            )));
        }
        if self.args.len() > MAX_ARGS {
            return Err(refused(format!(
                "{} arguments were given; a program takes at most {MAX_ARGS}",
                self.args.len()
            )));
        }
        let bytes: usize = self.args.iter().map(String::len).sum();
        if bytes > MAX_ARGS_BYTES {
            return Err(refused(format!(
                "the arguments are {bytes} bytes; at most {MAX_ARGS_BYTES} are passed"
            )));
        }
        if self.environment.len() > MAX_ENVIRONMENT {
            return Err(refused(format!(
                "{} environment changes were given; at most {MAX_ENVIRONMENT} are made",
                self.environment.len()
            )));
        }
        let mut texts = std::iter::once(&self.program)
            .chain(&self.args)
            .chain(&self.folder)
            .chain(self.environment.iter().map(|(name, _)| name))
            .chain(
                self.environment
                    .iter()
                    .filter_map(|(_, value)| value.as_ref()),
            );
        if texts.any(|text| text.contains('\0')) {
            return Err(refused(
                "the program, an argument, the folder or a variable contains a NUL character"
                    .into(),
            ));
        }
        if let Some((name, _)) = self
            .environment
            .iter()
            .find(|(name, _)| name.is_empty() || name.contains('='))
        {
            return Err(refused(format!(
                "`{}` cannot name an environment variable",
                name.escape_debug()
            )));
        }
        if self.elevated && !self.environment.is_empty() {
            return Err(refused(
                "an elevated program runs with the user's own environment; it takes no \
                 environment changes"
                    .into(),
            ));
        }
        if self.elevated && !input.is_empty() {
            return Err(refused(
                "an elevated program's input cannot be written; it takes none".into(),
            ));
        }
        Ok(())
    }

    /// "program `git`", for explanations.
    pub(crate) fn shown(&self) -> String {
        format!("program `{}`", self.program.escape_debug())
    }
}

/// Whose a program is.
#[derive(Clone)]
pub(crate) struct Owner {
    /// The runtime's processes, where the program is registered.
    pub helpers: Helpers,
    /// The guest instance that started it.
    pub owner: u64,
    /// The generation of the code running it: when it ends, so does the
    /// program.
    pub generation: Option<Generation>,
    /// The fence of the runtime thread running that code: once Pane gives
    /// up on the thread, the program ends too.
    pub fence: Option<Fence>,
    /// The identity key of the code's installed package, under which Pane
    /// notes the programs it ran; `None` for a command built into Pane.
    pub package: Option<String>,
}

impl Owner {
    /// Why the code is stopped, if it is.
    pub(crate) fn stopped(&self) -> Option<End> {
        self.generation
            .as_ref()
            .and_then(Generation::ended)
            .or_else(|| {
                self.fence
                    .as_ref()
                    .is_some_and(Fence::closed)
                    .then_some(End::Abandoned)
            })
    }

    /// Notes that the package ran `program` this session.
    fn note(&self, program: String) {
        if let Some(package) = &self.package {
            self.helpers.note_program(package, program);
        }
    }
}

/// Why stopped code may not run a program.
pub(crate) fn stopped_code(end: End) -> ProgramError {
    let why = match end {
        End::Disabled => "the extension is disabled",
        End::Replaced => "this code of the extension was replaced by a reload or an update",
        End::Uninstalled => "the extension was uninstalled",
        End::Paused => "the extension is paused after an error",
        End::Abandoned => {
            "Pane's extension runtime stopped responding and was replaced while this code ran"
        }
    };
    ProgramError::new(
        ErrorKind::Refused,
        format!("{why}; its programs were ended"),
    )
}

pub(crate) fn quitting(request: &Request) -> ProgramError {
    ProgramError::new(
        ErrorKind::Refused,
        format!("Pane is quitting; {} does not run", request.shown()),
    )
}

fn refusal(request: &Request, refusal: Refusal) -> ProgramError {
    match refusal {
        Refusal::Quitting => quitting(request),
        Refusal::Stopped(end) => stopped_code(end),
    }
}

/// What a finished `run` answers: how the program exited and what it wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Output {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// The exit code of `status`, or `None` when the system ended the process
/// (a signal on Unix).
fn exit_code(status: ExitStatus) -> Option<i32> {
    status.code()
}

/// What the caller of a program and its supervising thread share.
#[derive(Default)]
pub(crate) struct Control {
    /// The run, once registered.
    run: OnceLock<Arc<Run>>,
    /// The supervising thread.
    supervisor: OnceLock<Thread>,
    /// Set once the caller no longer wants the program: its future, or the
    /// spawned process's handle, was dropped.
    abandoned: AtomicBool,
    /// Set once the command killed the program (`kill`).
    killed: AtomicBool,
}

impl Control {
    fn wake(&self) {
        if let Some(supervisor) = self.supervisor.get() {
            supervisor.unpark();
        }
    }

    /// The caller is gone: the program ends.
    fn abandon(&self) {
        self.abandoned.store(true, Ordering::SeqCst);
        match self.run.get() {
            Some(run) => run.request_stop(),
            None => self.wake(),
        }
    }

    /// The command kills the program; it answers how it exited.
    fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        self.wake();
    }

    fn abandoned(&self) -> bool {
        self.abandoned.load(Ordering::SeqCst)
    }
}

/// Ends its program when dropped: the caller gave up on it.
struct Abandon(Option<Arc<Control>>);

impl Abandon {
    /// No longer ends the program: something else holds it now.
    fn disarm(mut self) -> Arc<Control> {
        self.0.take().expect("disarmed once")
    }
}

impl Drop for Abandon {
    fn drop(&mut self) {
        if let Some(control) = self.0.take() {
            control.abandon();
        }
    }
}

fn unavailable_thread(error: io::Error) -> ProgramError {
    ProgramError::new(
        ErrorKind::Unavailable,
        format!("Pane could not start a thread for the program: {error}"),
    )
}

/// Runs the program `request` names for `owner` with `input` on its
/// standard input, and answers how it exited and what it wrote, once it
/// exited. Dropping the future ends the program.
pub(crate) fn run(
    request: Request,
    input: Vec<u8>,
    owner: Owner,
) -> impl Future<Output = Result<Output, ProgramError>> + Send + 'static {
    let control = Arc::new(Control::default());
    let (reply, answer) = oneshot::channel();
    let started = {
        let control = control.clone();
        thread::Builder::new()
            .name("pane-program".into())
            .spawn(move || {
                let _ = control.supervisor.set(thread::current());
                let outcome = if request.elevated {
                    elevated::run(&request, &owner, Watching::from(&*control))
                } else {
                    run_here(&request, input, &owner, &control)
                };
                let _ = reply.send(outcome);
            })
    };
    let abandon = Abandon(Some(control));
    async move {
        let _abandon = abandon;
        started.map_err(unavailable_thread)?;
        answer.await.unwrap_or_else(|_| {
            Err(ProgramError::new(
                ErrorKind::Failed,
                "Pane's thread running the program stopped",
            ))
        })
    }
}

/// What an elevated run sees of its caller and its supervisor: whether it
/// was given up on, and where its run is registered.
pub(crate) struct Watching<'a> {
    control: &'a Control,
}

impl<'a> From<&'a Control> for Watching<'a> {
    fn from(control: &'a Control) -> Watching<'a> {
        Watching { control }
    }
}

// Only Windows runs a program elevated.
#[cfg_attr(not(windows), allow(dead_code))]
impl Watching<'_> {
    /// Notes `run` as the program's registration.
    pub(crate) fn registered(&self, run: Arc<Run>) {
        let _ = self.control.run.set(run);
    }

    /// Why the program must end now, if it must.
    pub(crate) fn must_end(
        &self,
        owner: &Owner,
        run: &Run,
        deadline: Option<Instant>,
        request: &Request,
    ) -> Option<ProgramError> {
        must_end(owner, run, self.control, deadline, request, || None).map(|ended| {
            ended
                .into_error(request)
                .unwrap_or_else(|| stopped_before(request))
        })
    }
}

fn stopped_before(request: &Request) -> ProgramError {
    ProgramError::new(
        ErrorKind::Refused,
        format!(
            "{} was ended before it finished: the call that started it ended",
            request.shown()
        ),
    )
}

/// Registers the program `pid` running `path` for `owner` on the calling
/// thread, puts it on its generation's undo list and notes it, unless Pane
/// is quitting or the code stopped.
pub(crate) fn register(
    request: &Request,
    owner: &Owner,
    pid: u32,
    path: PathBuf,
    noted: String,
) -> Result<(Arc<Run>, Registered), ProgramError> {
    let run = owner
        .helpers
        .register(Kind::Program, owner.owner, pid, path, || owner.stopped())
        .map_err(|why| refusal(request, why))?;
    let undo = owner.generation.as_ref().map(|generation| {
        let run = run.clone();
        generation.on_end("system program", move || {
            run.request_stop();
            Ok(())
        })
    });
    owner.note(noted);
    Ok((
        run.clone(),
        Registered {
            helpers: owner.helpers.clone(),
            run,
            _undo: undo,
        },
    ))
}

/// A registered program: taken off the processes running, and off its
/// generation's undo list, once dropped (after it was reaped).
pub(crate) struct Registered {
    helpers: Helpers,
    run: Arc<Run>,
    _undo: Option<crate::generation::Registration>,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.helpers.unregister(&self.run);
    }
}

/// Where and how a program starts, once its name was resolved.
pub(crate) struct Launch {
    /// The file it runs.
    pub path: PathBuf,
    /// The folder it runs in.
    pub folder: Option<PathBuf>,
    /// The search path its name was found on, which it gets as its `PATH`.
    pub search_path: OsString,
}

/// Resolves `request`'s program and folder, as the search path is now.
pub(crate) fn launch(request: &Request, owner: &Owner) -> Result<Launch, ProgramError> {
    let search_path = owner
        .helpers
        .search_path()
        .map(|search_path| search_path())
        .unwrap_or_else(search::system_search_path);
    let path = search::resolve(&request.program, &search_path)?;
    let folder = match &request.folder {
        None => search::home_folder(),
        Some(folder) => {
            let given = PathBuf::from(folder);
            if !given.is_absolute() {
                return Err(ProgramError::new(
                    ErrorKind::Refused,
                    format!(
                        "the folder `{}` is not an absolute path",
                        folder.escape_debug()
                    ),
                ));
            }
            if !given.is_dir() {
                return Err(ProgramError::new(
                    ErrorKind::NotFound,
                    format!("there is no folder at {folder}"),
                ));
            }
            Some(given)
        }
    };
    Ok(Launch {
        path,
        folder,
        search_path,
    })
}

impl Launch {
    /// The command that starts the program, its streams piped.
    fn command(&self, request: &Request) -> Command {
        let mut command = Command::new(&self.path);
        command
            .args(&request.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(folder) = &self.folder {
            command.current_dir(folder);
        }
        // The search path its name was found on, so that what it runs in
        // turn is found as it was; then the command's own changes.
        command.env("PATH", &self.search_path);
        for (name, value) in &request.environment {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        ProcessTree::prepare_program(&mut command, request.show_window);
        command
    }
}

/// A started program and its tree: dropping it ends both and reaps it.
struct Process {
    child: Child,
    tree: ProcessTree,
    reaped: Option<ExitStatus>,
}

impl Process {
    /// Starts the program `launch` resolved; an explanation if the system
    /// would not.
    fn start(launch: &Launch, request: &Request) -> Result<Process, ProgramError> {
        let mut command = launch.command(request);
        let mut child = command.spawn().map_err(|error| {
            let kind = match error.kind() {
                io::ErrorKind::NotFound => ErrorKind::NotFound,
                _ => ErrorKind::Unavailable,
            };
            ProgramError::new(
                kind,
                format!(
                    "the system did not start {} ({}): {error}",
                    request.shown(),
                    launch.path.display()
                ),
            )
        })?;
        match ProcessTree::adopt_program(&child) {
            Ok(tree) => Ok(Process {
                child,
                tree,
                reaped: None,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(ProgramError::new(
                    ErrorKind::Unavailable,
                    format!("the system did not let {} run: {error}", request.shown()),
                ))
            }
        }
    }

    /// Ends the program and everything it started, without reaping it.
    fn end(&mut self) {
        self.tree.kill();
        let _ = self.child.kill();
    }

    /// Reaps the program, once it exited or was ended.
    fn reap(&mut self) -> Option<ExitStatus> {
        if self.reaped.is_none() {
            self.reaped = self.child.wait().ok();
        }
        self.reaped
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.reaped.is_none() {
            self.end();
            self.reap();
        }
    }
}

/// A started program and its registration, dropped in that order: the
/// program is ended and reaped before it leaves the processes running.
struct Supervised {
    process: Process,
    _registered: Registered,
}

/// Registers `process`, which runs `launch`'s file for `owner`; ends it if
/// it may not run on.
fn registered(
    request: &Request,
    owner: &Owner,
    process: Process,
    launch: &Launch,
) -> Result<(Arc<Run>, Supervised), ProgramError> {
    let (run, registered) = register(
        request,
        owner,
        process.child.id(),
        launch.path.clone(),
        launch.path.display().to_string(),
    )?;
    Ok((
        run,
        Supervised {
            process,
            _registered: registered,
        },
    ))
}

/// Why a supervised program stopped.
enum Ended {
    /// It exited by itself, or after the command killed it.
    Exited,
    /// Its code's generation ended.
    Generation(End),
    /// Pane ended it: its call ended or was dropped, its instance went, or
    /// Pane is quitting.
    Stopped,
    /// The command's timeout passed.
    TimedOut(Duration),
    /// It wrote more than a run keeps to this stream.
    TooMuch(&'static str),
}

impl Ended {
    /// The error this ending answers; `None` for an exit.
    fn into_error(self, request: &Request) -> Option<ProgramError> {
        match self {
            Ended::Exited => None,
            Ended::Generation(end) => Some(stopped_code(end)),
            Ended::Stopped => Some(stopped_before(request)),
            Ended::TimedOut(timeout) => Some(ProgramError::new(
                ErrorKind::TimedOut,
                format!(
                    "{} ran longer than its timeout of {}; Pane ended it and every process \
                     it started",
                    request.shown(),
                    seconds(timeout)
                ),
            )),
            Ended::TooMuch(stream) => Some(ProgramError::new(
                ErrorKind::TooMuchOutput,
                format!(
                    "{} wrote more than {} MiB to its {stream}; Pane ended it and every \
                     process it started",
                    request.shown(),
                    MAX_PROGRAM_OUTPUT >> 20
                ),
            )),
        }
    }
}

/// "0.5 seconds", "1 second", "90 seconds".
fn seconds(duration: Duration) -> String {
    if duration == Duration::from_secs(1) {
        "1 second".into()
    } else {
        format!("{} seconds", duration.as_millis() as f64 / 1000.0)
    }
}

/// Why the program must end now, if it must: its code stopped, Pane ended
/// it, its caller gave up on it, it wrote too much or its time is up. A
/// kill by the command is not an end here: the program then exits.
fn must_end(
    owner: &Owner,
    run: &Run,
    control: &Control,
    deadline: Option<Instant>,
    request: &Request,
    too_much: impl Fn() -> Option<&'static str>,
) -> Option<Ended> {
    // A generation's end comes first: its undo list also asks for the stop,
    // and the error says why.
    if let Some(end) = owner.stopped() {
        return Some(Ended::Generation(end));
    }
    if run.stop_requested() || control.abandoned() {
        return Some(Ended::Stopped);
    }
    if let Some(stream) = too_much() {
        return Some(Ended::TooMuch(stream));
    }
    if let (Some(deadline), Some(timeout)) = (deadline, request.timeout)
        && Instant::now() >= deadline
    {
        return Some(Ended::TimedOut(timeout));
    }
    None
}

/// Supervises `process` until it exits or must end, ending its tree either
/// way, then reaps it.
fn supervise(
    process: &mut Process,
    owner: &Owner,
    run: &Run,
    control: &Control,
    request: &Request,
    too_much: impl Fn() -> Option<&'static str>,
) -> (Ended, Option<ExitStatus>) {
    let deadline = request.timeout.map(|timeout| Instant::now() + timeout);
    let ended = loop {
        if let Some(ended) = must_end(owner, run, control, deadline, request, &too_much) {
            process.end();
            break ended;
        }
        if control.killed.load(Ordering::SeqCst) {
            // The command's kill: the program exits, and its exit is the
            // answer.
            process.end();
        }
        match process_tree::has_exited(&mut process.child) {
            // What it left running ends with it, so its streams end too.
            Ok(true) => {
                process.tree.kill();
                break Ended::Exited;
            }
            Ok(false) => thread::park_timeout(TICK),
            Err(_) => {
                process.end();
                break Ended::Stopped;
            }
        }
    };
    (ended, process.reap())
}

/// A stream a `run` keeps, read by a thread of its own.
struct Kept {
    bytes: Arc<Mutex<Vec<u8>>>,
    over: Arc<AtomicBool>,
    done: std::sync::mpsc::Receiver<()>,
}

impl Kept {
    /// Reads `pipe` into memory on a thread of its own, up to
    /// [`MAX_PROGRAM_OUTPUT`] bytes; past that it stops and wakes
    /// `supervisor`, which ends the program.
    fn start(mut pipe: impl Read + Send + 'static, supervisor: Thread) -> io::Result<Kept> {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let over = Arc::new(AtomicBool::new(false));
        let (finished, done) = std::sync::mpsc::channel();
        {
            let (bytes, over) = (bytes.clone(), over.clone());
            thread::Builder::new()
                .name("pane-program-output".into())
                .spawn(move || {
                    let mut chunk = vec![0; CHUNK];
                    loop {
                        match pipe.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(read) => {
                                let mut kept = lock(&bytes);
                                if kept.len() + read > MAX_PROGRAM_OUTPUT {
                                    over.store(true, Ordering::SeqCst);
                                    supervisor.unpark();
                                    break;
                                }
                                kept.extend_from_slice(&chunk[..read]);
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                    let _ = finished.send(());
                })?;
        }
        Ok(Kept { bytes, over, done })
    }

    fn over(&self) -> bool {
        self.over.load(Ordering::SeqCst)
    }

    /// What was read, once the stream ended, or once `DRAIN` passed, and
    /// whether the program wrote more than a run keeps.
    fn finish(self) -> (Vec<u8>, bool) {
        let _ = self.done.recv_timeout(DRAIN);
        (std::mem::take(&mut *lock(&self.bytes)), self.over())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// Writes `input` to `stdin` on a thread of its own, then closes it.
fn feed(mut stdin: impl Write + Send + 'static, input: Vec<u8>) -> io::Result<()> {
    thread::Builder::new()
        .name("pane-program-input".into())
        .spawn(move || {
            // A program that exits without reading all of it is its own
            // business: the error is not the run's.
            let _ = stdin.write_all(&input);
        })
        .map(|_| ())
}

/// Starts and supervises a `run` on the calling thread, its supervisor.
fn run_here(
    request: &Request,
    input: Vec<u8>,
    owner: &Owner,
    control: &Control,
) -> Result<Output, ProgramError> {
    let early = |control: &Control| {
        if let Some(end) = owner.stopped() {
            return Err(stopped_code(end));
        }
        if owner.helpers.quitting() {
            return Err(quitting(request));
        }
        if control.abandoned() {
            return Err(stopped_before(request));
        }
        Ok(())
    };
    early(control)?;
    let launch = launch(request, owner)?;
    // Stopped since it asked: nothing starts.
    early(control)?;
    let process = Process::start(&launch, request)?;
    let (run, mut supervised) = registered(request, owner, process, &launch)?;
    let _ = control.run.set(run.clone());
    let process = &mut supervised.process;
    let supervisor = thread::current();
    let streams = (
        process.child.stdin.take(),
        process.child.stdout.take(),
        process.child.stderr.take(),
    );
    let (Some(stdin), Some(stdout), Some(stderr)) = streams else {
        return Err(ProgramError::new(
            ErrorKind::Failed,
            format!("Pane could not reach the streams of {}", request.shown()),
        ));
    };
    feed(stdin, input).map_err(unavailable_thread)?;
    let out = Kept::start(stdout, supervisor.clone()).map_err(unavailable_thread)?;
    let err = Kept::start(stderr, supervisor).map_err(unavailable_thread)?;
    let too_much = || {
        if out.over() {
            Some("standard output")
        } else if err.over() {
            Some("standard error")
        } else {
            None
        }
    };
    let (ended, status) = supervise(process, owner, &run, control, request, too_much);
    if let Some(error) = ended.into_error(request) {
        return Err(error);
    }
    let Some(status) = status else {
        return Err(ProgramError::new(
            ErrorKind::Failed,
            format!("Pane lost track of {}", request.shown()),
        ));
    };
    // A program may exit before its reader saw it write too much.
    let (stdout, out_over) = out.finish();
    let (stderr, err_over) = err.finish();
    let over = match (out_over, err_over) {
        (true, _) => Some("standard output"),
        (false, true) => Some("standard error"),
        (false, false) => None,
    };
    if let Some(error) = over.and_then(|stream| Ended::TooMuch(stream).into_error(request)) {
        return Err(error);
    }
    Ok(Output {
        exit_code: exit_code(status),
        stdout,
        stderr,
    })
}

/// A command's answer of how its spawned program ended, once it did.
type Status = Option<Result<Option<i32>, ProgramError>>;

/// A program a command spawned, which it reads and writes while it runs.
/// Dropping it ends the program.
pub(crate) struct Spawned {
    request: Request,
    /// Where its input goes, until the command closes it.
    input: Mutex<Option<std::sync::mpsc::Sender<Writing>>>,
    output: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    errors: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    status: watch::Receiver<Status>,
    control: Arc<Control>,
}

/// Which of a spawned program's streams to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stream {
    Output,
    Errors,
}

/// Bytes for a spawned program's input, and where to say they were
/// written.
struct Writing {
    bytes: Vec<u8>,
    done: oneshot::Sender<io::Result<()>>,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        self.control.abandon();
    }
}

/// The ends a spawned program's supervisor hands its caller.
struct Ends {
    input: std::sync::mpsc::Sender<Writing>,
    output: mpsc::Receiver<Vec<u8>>,
    errors: mpsc::Receiver<Vec<u8>>,
    status: watch::Receiver<Status>,
}

/// Starts the program `request` names for `owner`, answering once it runs
/// with the handle the command reads, writes, waits for and kills it
/// through. The program ends when the handle is dropped, and when its
/// call ends (Pane ends every program the call started).
pub(crate) fn spawn(
    request: Request,
    owner: Owner,
) -> impl Future<Output = Result<Spawned, ProgramError>> + Send + 'static {
    let control = Arc::new(Control::default());
    let (reply, started) = oneshot::channel::<Result<Ends, ProgramError>>();
    let thread = {
        let (control, request) = (control.clone(), request.clone());
        thread::Builder::new()
            .name("pane-program".into())
            .spawn(move || {
                let _ = control.supervisor.set(thread::current());
                spawn_here(&request, &owner, &control, reply);
            })
    };
    let abandon = Abandon(Some(control));
    async move {
        thread.map_err(unavailable_thread)?;
        let ends = started.await.unwrap_or_else(|_| {
            Err(ProgramError::new(
                ErrorKind::Failed,
                "Pane's thread running the program stopped",
            ))
        })?;
        // From here the handle ends the program when dropped.
        let control = abandon.disarm();
        Ok(Spawned {
            request,
            input: Mutex::new(Some(ends.input)),
            output: Arc::new(tokio::sync::Mutex::new(ends.output)),
            errors: Arc::new(tokio::sync::Mutex::new(ends.errors)),
            status: ends.status,
            control,
        })
    }
}

/// Moves what `pipe` gives to `chunks`, as the command reads them.
fn pass_on(mut pipe: impl Read + Send + 'static, chunks: mpsc::Sender<Vec<u8>>) -> io::Result<()> {
    thread::Builder::new()
        .name("pane-program-stream".into())
        .spawn(move || {
            let mut chunk = vec![0; CHUNK];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        // The command dropped its handle: nobody reads.
                        if chunks.blocking_send(chunk[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        })
        .map(|_| ())
}

/// Writes what the command sends to `stdin`, until it closes its input.
fn take_input(
    mut stdin: impl Write + Send + 'static,
    writes: std::sync::mpsc::Receiver<Writing>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("pane-program-input".into())
        .spawn(move || {
            for writing in writes {
                let written = stdin.write_all(&writing.bytes).and_then(|()| stdin.flush());
                let _ = writing.done.send(written);
            }
            // The command closed its input (or dropped the handle): the
            // program reads its end.
        })
        .map(|_| ())
}

/// Starts and supervises a `spawn` on the calling thread, its supervisor,
/// answering `reply` once it runs.
fn spawn_here(
    request: &Request,
    owner: &Owner,
    control: &Control,
    reply: oneshot::Sender<Result<Ends, ProgramError>>,
) {
    let started = (|| {
        if let Some(end) = owner.stopped() {
            return Err(stopped_code(end));
        }
        if owner.helpers.quitting() {
            return Err(quitting(request));
        }
        let launch = launch(request, owner)?;
        let process = Process::start(&launch, request)?;
        Ok((launch, process))
    })();
    let (run, mut supervised) =
        match started.and_then(|(launch, process)| registered(request, owner, process, &launch)) {
            Ok(registered) => registered,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
    let _ = control.run.set(run.clone());
    let process = &mut supervised.process;
    let ends = (|| {
        let streams = (
            process.child.stdin.take(),
            process.child.stdout.take(),
            process.child.stderr.take(),
        );
        let (Some(stdin), Some(stdout), Some(stderr)) = streams else {
            return Err(ProgramError::new(
                ErrorKind::Failed,
                format!("Pane could not reach the streams of {}", request.shown()),
            ));
        };
        let (input, writes) = std::sync::mpsc::channel();
        let (output_chunks, output) = mpsc::channel(BUFFERED_CHUNKS);
        let (error_chunks, errors) = mpsc::channel(BUFFERED_CHUNKS);
        take_input(stdin, writes).map_err(unavailable_thread)?;
        pass_on(stdout, output_chunks).map_err(unavailable_thread)?;
        pass_on(stderr, error_chunks).map_err(unavailable_thread)?;
        Ok((input, output, errors))
    })();
    let (input, output, errors) = match ends {
        Ok(ends) => ends,
        Err(error) => {
            let _ = reply.send(Err(error));
            return;
        }
    };
    let (status_sender, status) = watch::channel::<Status>(None);
    // A caller gone by now is seen by the supervision: it ends the program.
    let _ = reply.send(Ok(Ends {
        input,
        output,
        errors,
        status,
    }));
    let (ended, exit) = supervise(process, owner, &run, control, request, || None);
    let answer = match ended.into_error(request) {
        Some(error) => Err(error),
        None => match exit {
            Some(status) => Ok(exit_code(status)),
            None => Err(ProgramError::new(
                ErrorKind::Failed,
                format!("Pane lost track of {}", request.shown()),
            )),
        },
    };
    let _ = status_sender.send_replace(Some(answer));
}

impl Spawned {
    /// Writes `bytes` to the program's standard input, answering once they
    /// were written (the program may have to read some first).
    pub(crate) fn write(
        &self,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<(), ProgramError>> + Send + 'static + use<> {
        let input = lock(&self.input).clone();
        let shown = self.request.shown();
        async move {
            let input = input.ok_or_else(|| {
                ProgramError::new(
                    ErrorKind::Refused,
                    format!("the input of {shown} is closed"),
                )
            })?;
            let ended = || {
                ProgramError::new(
                    ErrorKind::Failed,
                    format!("{shown} ended before it read its input"),
                )
            };
            let (done, written) = oneshot::channel();
            input.send(Writing { bytes, done }).map_err(|_| ended())?;
            match written.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) | Err(_) => Err(ended()),
            }
        }
    }

    /// Closes the program's standard input: it reads its end once what was
    /// written before is.
    pub(crate) fn close_input(&self) {
        lock(&self.input).take();
    }

    /// The next bytes the program wrote to `stream`, once it wrote some;
    /// `None` once the stream ended.
    pub(crate) fn read(
        &self,
        stream: Stream,
    ) -> impl Future<Output = Option<Vec<u8>>> + Send + 'static + use<> {
        let chunks = match stream {
            Stream::Output => self.output.clone(),
            Stream::Errors => self.errors.clone(),
        };
        async move { chunks.lock().await.recv().await }
    }

    /// How the program ended, once it did: its exit code (`None` when the
    /// system ended it), or why Pane ended it.
    pub(crate) fn wait(
        &self,
    ) -> impl Future<Output = Result<Option<i32>, ProgramError>> + Send + 'static + use<> {
        let mut status = self.status.clone();
        let shown = self.request.shown();
        async move {
            let answer = match status.wait_for(Option::is_some).await {
                Ok(seen) => seen.clone(),
                Err(_) => None,
            };
            answer.unwrap_or_else(|| {
                Err(ProgramError::new(
                    ErrorKind::Failed,
                    format!("Pane lost track of {shown}"),
                ))
            })
        }
    }

    /// Ends the program and every process it started; [`Spawned::wait`]
    /// answers how it exited.
    pub(crate) fn kill(&self) {
        self.control.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(program: &str) -> Request {
        Request {
            program: program.into(),
            ..Request::default()
        }
    }

    #[test]
    fn requests_pane_cannot_pass_on_are_refused() {
        assert_eq!(request("git").check(b"input"), Ok(()));
        let nul = Request {
            args: vec!["a\0b".into()],
            ..request("git")
        };
        assert_eq!(nul.check(b"").unwrap_err().kind, ErrorKind::Refused);
        let named = Request {
            environment: vec![("A=B".into(), Some("c".into()))],
            ..request("git")
        };
        assert_eq!(named.check(b"").unwrap_err().kind, ErrorKind::Refused);
        let big = vec![0; MAX_PROGRAM_INPUT + 1];
        assert_eq!(
            request("git").check(&big).unwrap_err().kind,
            ErrorKind::Refused
        );
        let many = Request {
            args: vec![String::new(); MAX_ARGS + 1],
            ..request("git")
        };
        assert_eq!(many.check(b"").unwrap_err().kind, ErrorKind::Refused);
    }

    #[test]
    fn an_elevated_run_takes_no_input_or_environment() {
        let elevated = Request {
            elevated: true,
            ..request("setup")
        };
        assert_eq!(elevated.check(b""), Ok(()));
        assert_eq!(elevated.check(b"yes").unwrap_err().kind, ErrorKind::Refused);
        let with_environment = Request {
            environment: vec![("A".into(), None)],
            ..elevated
        };
        assert_eq!(
            with_environment.check(b"").unwrap_err().kind,
            ErrorKind::Refused
        );
    }

    #[test]
    fn a_timeout_is_said_in_seconds() {
        assert_eq!(seconds(Duration::from_millis(500)), "0.5 seconds");
        assert_eq!(seconds(Duration::from_secs(1)), "1 second");
        assert_eq!(seconds(Duration::from_secs(90)), "90 seconds");
    }
}
