//! System programs (ADR 0033, #147): programs installed on the system, such
//! as PowerShell, winget, git or any executable, which Pane runs for a
//! command through `pane:extension/programs` (wit/programs.wit), since a
//! WASI 0.3 guest cannot start a process. Native helpers ([`crate::helpers`])
//! stay for programs a package ships.
//!
//! A command names a program by an absolute path, run as given, or by a
//! bare name found on the user's search path at the time of the call
//! ([`search`]); its arguments reach the program as a list, which no shell
//! reads. `run` waits for the program and answers its exit code and what it
//! wrote; `spawn` answers a process id the command writes to, reads from
//! as the program writes (to report progress in a toast), waits for and
//! kills. Elevated runs go through Windows' own elevation prompt
//! ([`elevated`]).
//!
//! Each program belongs to the call that started it ([`runner`]): Pane ends
//! it and every process it started when the call returns or is dropped,
//! when the package's generation ends and when Pane quits. Use is visible,
//! not gated: a component importing the interface is noted at install,
//! update and reload, the extension list says "Runs system programs", and a
//! row lists the programs the package ran this session.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Accessor, HasData};

use crate::runtime::{GuestState, bindings};
use runner::{ErrorKind, MAX_PROGRAM_INPUT, Owner, ProgramError, Request, Spawned, Stream};

use bindings::pane::extension::programs as wit;

mod elevated;
pub(crate) mod runner;
mod search;

/// The interface a component imports to run system programs: a package
/// whose component imports it runs system programs, as the extension list
/// says.
pub(crate) const PROGRAMS_INTERFACE: &str = "pane:extension/programs@";

/// The host side of `pane:extension/programs`: runs programs for the
/// calling guest, each awaited off the runtime thread.
pub(crate) struct Calls;

impl HasData for Calls {
    type Data<'a> = &'a mut GuestState;
}

impl wit::Host for GuestState {}

/// The programs a guest instance's calls spawned, by id. Each call's are
/// ended, and forgotten, when the call ends; ids are never reused.
#[derive(Default)]
pub(crate) struct Processes {
    spawned: HashMap<u64, Arc<Spawned>>,
    last: u64,
}

impl Processes {
    fn insert(&mut self, spawned: Spawned) -> u64 {
        self.last += 1;
        self.spawned.insert(self.last, Arc::new(spawned));
        self.last
    }

    fn get(&self, id: u64) -> Result<Arc<Spawned>, ProgramError> {
        self.spawned.get(&id).cloned().ok_or_else(|| {
            ProgramError::new(
                ErrorKind::Refused,
                format!(
                    "process {id} is not one this call started: a spawned program ends with \
                     the call that started it"
                ),
            )
        })
    }

    /// Forgets the programs spawned so far, as their call ended (Pane ended
    /// them already); dropping them ends any that still run.
    pub(crate) fn clear(&mut self) {
        self.spawned.clear();
    }
}

/// `program`, `args` and `options` as Pane runs them.
fn request(program: String, args: Vec<String>, options: wit::Options) -> Request {
    Request {
        program,
        args,
        folder: options.folder,
        environment: options
            .environment
            .into_iter()
            .map(|variable| (variable.name, variable.value))
            .collect(),
        timeout: options
            .timeout_ms
            .map(|milliseconds| Duration::from_millis(milliseconds.into())),
        show_window: options.show_window,
        elevated: options.elevated,
    }
}

/// Whose a program `state`'s guest asks for is, once `request` and `input`
/// pass Pane's checks and the guest's code may still run.
fn owner_for(state: &GuestState, request: &Request, input: &[u8]) -> Result<Owner, ProgramError> {
    // Stopped code starts no more work.
    if let Some(end) = state.stopped() {
        return Err(runner::stopped_code(end));
    }
    request.check(input)?;
    Ok(state.program_owner())
}

impl<T> wit::HostWithStore<T> for Calls {
    async fn run(
        accessor: &Accessor<T, Self>,
        program: String,
        args: Vec<String>,
        input: Vec<u8>,
        options: wit::Options,
    ) -> Result<wit::Output, wit::ProgramError> {
        let request = request(program, args, options);
        let (owner, watch) = accessor.with(|mut view| {
            let state = view.get();
            (owner_for(state, &request, &input), state.watch())
        });
        let owner = owner?;
        // Waiting for the program is not the guest's computing, nor a hang.
        // Dropped here if the guest cancels the call: the program ends.
        crate::runtime::deadlines::hosted(watch, runner::run(request, input, owner))
            .await
            .map(wit::Output::from)
            .map_err(wit::ProgramError::from)
    }

    async fn spawn(
        accessor: &Accessor<T, Self>,
        program: String,
        args: Vec<String>,
        options: wit::Options,
    ) -> Result<u64, wit::ProgramError> {
        let request = request(program, args, options);
        if request.elevated {
            return Err(ProgramError::new(
                ErrorKind::Unavailable,
                format!(
                    "{} cannot be spawned elevated: an elevated program's streams cannot be \
                     reached; run it instead",
                    request.shown()
                ),
            )
            .into());
        }
        let (owner, watch) = accessor.with(|mut view| {
            let state = view.get();
            (owner_for(state, &request, &[]), state.watch())
        });
        let owner = owner?;
        let spawned =
            crate::runtime::deadlines::hosted(watch, runner::spawn(request, owner)).await?;
        Ok(accessor.with(|mut view| view.get().programs.insert(spawned)))
    }

    async fn write_input(
        accessor: &Accessor<T, Self>,
        process: u64,
        bytes: Vec<u8>,
    ) -> Result<(), wit::ProgramError> {
        if bytes.len() > MAX_PROGRAM_INPUT {
            return Err(ProgramError::new(
                ErrorKind::Refused,
                format!(
                    "{} bytes were given at once; at most {MAX_PROGRAM_INPUT} are written",
                    bytes.len()
                ),
            )
            .into());
        }
        let (writing, watch) = accessor.with(|mut view| {
            let state = view.get();
            let writing = state
                .programs
                .get(process)
                .map(|spawned| spawned.write(bytes));
            (writing, state.watch())
        });
        let writing = writing?;
        crate::runtime::deadlines::hosted(watch, writing)
            .await
            .map_err(wit::ProgramError::from)
    }

    async fn close_input(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<(), wit::ProgramError> {
        accessor.with(|mut view| {
            view.get()
                .programs
                .get(process)
                .map(|spawned| spawned.close_input())
                .map_err(wit::ProgramError::from)
        })
    }

    async fn read_output(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<Option<Vec<u8>>, wit::ProgramError> {
        read(accessor, process, Stream::Output).await
    }

    async fn read_error(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<Option<Vec<u8>>, wit::ProgramError> {
        read(accessor, process, Stream::Errors).await
    }

    async fn wait(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<Option<i32>, wit::ProgramError> {
        let (waiting, watch) = accessor.with(|mut view| {
            let state = view.get();
            (
                state.programs.get(process).map(|spawned| spawned.wait()),
                state.watch(),
            )
        });
        let waiting = waiting?;
        crate::runtime::deadlines::hosted(watch, waiting)
            .await
            .map_err(wit::ProgramError::from)
    }

    async fn kill(accessor: &Accessor<T, Self>, process: u64) -> Result<(), wit::ProgramError> {
        accessor.with(|mut view| {
            view.get()
                .programs
                .get(process)
                .map(|spawned| spawned.kill())
                .map_err(wit::ProgramError::from)
        })
    }
}

/// The next bytes the program `process` wrote to `stream`.
async fn read<T>(
    accessor: &Accessor<T, Calls>,
    process: u64,
    stream: Stream,
) -> Result<Option<Vec<u8>>, wit::ProgramError> {
    let (reading, watch) = accessor.with(|mut view| {
        let state = view.get();
        (
            state
                .programs
                .get(process)
                .map(|spawned| spawned.read(stream)),
            state.watch(),
        )
    });
    let reading = reading?;
    Ok(crate::runtime::deadlines::hosted(watch, reading).await)
}

/// The one mapping of Pane's error kinds to the WIT's.
impl From<ProgramError> for wit::ProgramError {
    fn from(error: ProgramError) -> wit::ProgramError {
        use wit::ProgramErrorKind as Wit;
        let kind = match error.kind {
            ErrorKind::NotFound => Wit::NotFound,
            ErrorKind::Unavailable => Wit::Unavailable,
            ErrorKind::Declined => Wit::Declined,
            ErrorKind::TimedOut => Wit::TimedOut,
            ErrorKind::TooMuchOutput => Wit::TooMuchOutput,
            ErrorKind::Refused => Wit::Refused,
            ErrorKind::Failed => Wit::Failed,
        };
        wit::ProgramError {
            kind,
            message: error.message,
        }
    }
}

impl From<runner::Output> for wit::Output {
    fn from(output: runner::Output) -> wit::Output {
        wit::Output {
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}
