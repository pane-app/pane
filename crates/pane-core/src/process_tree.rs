//! A process and every process it starts, ended together: a development
//! build ([`crate::develop`]) and a system program an extension runs
//! ([`crate::programs`]).
//!
//! - Unix: a process group of its own, killed with `SIGKILL`. On Linux the
//!   command also gets `SIGKILL` if the thread that started it ends (Pane's
//!   threads end with Pane: `PR_SET_PDEATHSIG`); macOS has no such signal,
//!   so there a tree outlives a Pane that dies without quitting. A process
//!   that leaves the group itself (`setsid`, `setpgid`) escapes it.
//! - Windows: a Job Object that kills its processes when it is closed
//!   (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), so they go with Pane however it
//!   ends; the command runs in a new process group. A development build is
//!   assigned to the job just after it starts, so what it starts in that
//!   moment escapes it; a system program starts suspended and runs only
//!   once it is in its job, so nothing it starts escapes.

use std::io;
use std::process::{Child, Command};

pub(crate) struct ProcessTree {
    #[cfg(unix)]
    group: i32,
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl ProcessTree {
    /// Prepares a development build's `command`: a process group of its
    /// own, and no console window on Windows.
    pub(crate) fn prepare(command: &mut Command) {
        prepare_with(command, false, false);
    }

    /// Prepares a system program's `command`: a process group of its own,
    /// started suspended on Windows (see [`ProcessTree::adopt_program`]),
    /// with a console window of its own there only if `console`.
    pub(crate) fn prepare_program(command: &mut Command, console: bool) {
        prepare_with(command, true, console);
    }

    /// The tree of `child`, started with [`ProcessTree::prepare`].
    pub(crate) fn adopt(child: &Child) -> ProcessTree {
        #[cfg(unix)]
        {
            // The group `prepare` made has the child's id.
            ProcessTree {
                group: i32::try_from(child.id()).unwrap_or(0),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            ProcessTree {
                job: windows_job::Job::contain(windows::Win32::Foundation::HANDLE(
                    child.as_raw_handle(),
                )),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            ProcessTree {}
        }
    }

    /// The tree of `child`, started with [`ProcessTree::prepare_program`]:
    /// on Windows it is put in its Job Object, then let run. An error when
    /// it cannot be let run; the caller ends it then.
    pub(crate) fn adopt_program(child: &Child) -> io::Result<ProcessTree> {
        let tree = ProcessTree::adopt(child);
        #[cfg(windows)]
        windows_job::resume(child.id())?;
        Ok(tree)
    }

    /// Ends every process of the tree that is still running.
    pub(crate) fn kill(&self) {
        #[cfg(unix)]
        if self.group > 0 {
            // SAFETY: kill(2) with a negative id signals that process group;
            // it has no memory effects.
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
    }

    /// Whether nothing of the tree runs any more, as far as the system
    /// tells: on Windows its Job Object holds no running process; elsewhere
    /// `false`, as a process group is not told apart from its exited,
    /// unreaped leader.
    pub(crate) fn none_running(&self) -> bool {
        #[cfg(windows)]
        {
            self.job
                .as_ref()
                .is_some_and(|job| job.active_processes() == Some(0))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Whether the tree's processes are in a Job Object (Windows), which a
    /// system may refuse; elsewhere, always (a process group).
    #[allow(dead_code)]
    pub(crate) fn contained(&self) -> bool {
        #[cfg(windows)]
        {
            self.job.is_some()
        }
        #[cfg(not(windows))]
        {
            true
        }
    }
}

fn prepare_with(command: &mut Command, suspended: bool, console: bool) {
    #[cfg(unix)]
    {
        let _ = (suspended, console);
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        #[cfg(target_os = "linux")]
        // SAFETY: prctl is async-signal-safe and touches only the child.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::{
            CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED,
        };
        let mut flags = CREATE_NEW_PROCESS_GROUP.0;
        flags |= if console {
            CREATE_NEW_CONSOLE.0
        } else {
            CREATE_NO_WINDOW.0
        };
        if suspended {
            flags |= CREATE_SUSPENDED.0;
        }
        command.creation_flags(flags);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (command, suspended, console);
    }
}

/// How `child` exited, if it has: `Some` with its exit code, itself `None`
/// when the system ended it (a signal on Unix). It is not reaped on Unix:
/// its process group keeps its id until it is, so [`ProcessTree::kill`]
/// after this never reaches a group that took the id since, and what the
/// program left running in its group stays reachable until it is reaped.
pub(crate) fn exit_code(child: &mut Child) -> io::Result<Option<Option<i32>>> {
    #[cfg(unix)]
    {
        // SAFETY: a zeroed siginfo_t is a valid value for waitid to fill;
        // the call only writes to it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if waited != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: waitid filled the fields of a child's state change, or
        // left them zero when none happened.
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }
        // An exit's status is its code; a signal's is the signal.
        let code = (info.si_code == libc::CLD_EXITED).then(|| unsafe { info.si_status() });
        Ok(Some(code))
    }
    #[cfg(not(unix))]
    {
        // Windows: the child's handle keeps its status; nothing is reaped,
        // and the Job Object holds what it left running.
        child
            .try_wait()
            .map(|status| status.map(|status| status.code()))
    }
}

#[cfg(windows)]
pub(crate) mod windows_job {
    use std::io;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// A Job Object killing its processes when closed.
    pub struct Job(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// A new job holding the process `process` (a handle the caller
        /// keeps open meanwhile), or `None` if the system refuses one (an
        /// elevated process, for a Pane that is not).
        pub fn contain(process: HANDLE) -> Option<Job> {
            // SAFETY: plain Win32 calls on a handle this owns and the
            // process handle, which the caller keeps open.
            unsafe {
                let job = Job(CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?);
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const core::ffi::c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .ok()?;
                AssignProcessToJobObject(job.0, process).ok()?;
                Some(job)
            }
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is open until drop.
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
            }
        }

        /// How many processes of the job run now; `None` if the system
        /// does not say.
        pub fn active_processes(&self) -> Option<u32> {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: the handle is open until drop, and `info` is sized
            // as the call requires.
            unsafe {
                QueryInformationJobObject(
                    Some(self.0),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    None,
                )
                .ok()?;
            }
            Some(info.ActiveProcesses)
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closes the handle this owns once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Lets the process `pid`, started suspended, run: resumes each of its
    /// threads (a process just started has one).
    pub fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: plain Win32 calls; every handle opened here is closed
        // here, and `entry` is sized as the calls require.
        unsafe {
            let snapshot =
                CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(io::Error::from)?;
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut resumed = 0;
            let mut next = Thread32First(snapshot, &mut entry);
            while next.is_ok() {
                if entry.th32OwnerProcessID == pid
                    && let Ok(thread) = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                {
                    if ResumeThread(thread) != u32::MAX {
                        resumed += 1;
                    }
                    let _ = CloseHandle(thread);
                }
                next = Thread32Next(snapshot, &mut entry);
            }
            let _ = CloseHandle(snapshot);
            if resumed == 0 {
                return Err(io::Error::other(format!(
                    "no thread of process {pid} could be resumed"
                )));
            }
            Ok(())
        }
    }
}

/// Starting a process tree as a system program does, for the adapter test
/// (`tests/program_adapters.rs`), which checks the system's containment
/// itself. Not part of Pane's interface.
#[doc(hidden)]
pub mod testing {
    use std::io;
    use std::process::{Child, Command};

    use super::ProcessTree;

    /// A program started in a tree of its own.
    pub struct Contained {
        pub child: Child,
        tree: ProcessTree,
    }

    /// Starts `command` as a system program is started: in a tree of its
    /// own, with no console window.
    pub fn start(mut command: Command) -> io::Result<Contained> {
        ProcessTree::prepare_program(&mut command, false);
        let mut child = command.spawn()?;
        match ProcessTree::adopt_program(&child) {
            Ok(tree) => Ok(Contained { child, tree }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    impl Contained {
        /// Whether the system put the tree in a Job Object (Windows); always
        /// elsewhere.
        pub fn contained(&self) -> bool {
            self.tree.contained()
        }

        /// Ends every process of the tree, then reaps the program.
        pub fn end(mut self) -> io::Result<()> {
            self.tree.kill();
            let _ = self.child.kill();
            self.child.wait().map(|_| ())
        }
    }
}
