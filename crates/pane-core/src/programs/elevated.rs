//! Running a program elevated: on Windows through the system's own
//! elevation prompt (`ShellExecuteExW` with the `runas` verb), which the
//! user may decline. Windows gives the program a console and streams of its
//! own that Pane cannot reach, so such a run answers only how the program
//! exited, and takes no input or environment changes. Pane ends it as it
//! ends any program it runs, as far as Windows lets a program that is not
//! elevated end one that is: its Job Object, if Windows let Pane make one,
//! and the program itself. On macOS and Linux an elevated run answers that
//! it is not available yet (the specification's proposed default).

use super::runner::{ErrorKind, Output, Owner, ProgramError, Request, Watching};

/// "Running a program elevated is not available on Linux yet".
#[cfg_attr(windows, allow(dead_code))]
fn unavailable_here() -> ProgramError {
    let here = match crate::platform::Platform::current() {
        Some(platform) => platform.to_string(),
        None => format!("this system ({})", std::env::consts::OS),
    };
    ProgramError::new(
        ErrorKind::Unavailable,
        format!("running a program elevated is not available on {here} yet"),
    )
}

/// Runs `request`'s program elevated for `owner`, on the calling thread,
/// its supervisor.
#[cfg(not(windows))]
pub(super) fn run(
    request: &Request,
    owner: &Owner,
    watching: Watching<'_>,
) -> Result<Output, ProgramError> {
    let _ = (request, owner, watching);
    Err(unavailable_here())
}

/// Runs `request`'s program elevated for `owner`, on the calling thread,
/// its supervisor: Windows shows its elevation prompt, and the run waits
/// until the program exits or must end.
#[cfg(windows)]
pub(super) fn run(
    request: &Request,
    owner: &Owner,
    watching: Watching<'_>,
) -> Result<Output, ProgramError> {
    use std::time::Instant;

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessId, TerminateProcess, WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
    use windows::core::PCWSTR;

    use super::runner::{self, stopped_code};
    use crate::process_tree::windows_job::Job;

    /// How long each wait for the program lasts before the run checks
    /// whether it must end, in milliseconds.
    const TICK_MS: u32 = 10;

    /// The process handle Windows gave Pane, closed when dropped.
    struct Process(HANDLE);

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: closes the handle this owns once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Ends the program and what it started, as far as Windows lets Pane.
    fn end(job: Option<&Job>, process: &Process) {
        if let Some(job) = job {
            job.terminate();
        }
        // SAFETY: the handle is open while `process` lives.
        unsafe {
            let _ = TerminateProcess(process.0, 1);
        }
    }

    if let Some(end) = owner.stopped() {
        return Err(stopped_code(end));
    }
    if owner.helpers.quitting() {
        return Err(runner::quitting(request));
    }
    let launch = runner::launch(request, owner)?;
    let wide = |text: &std::ffi::OsStr| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        text.encode_wide().chain([0]).collect()
    };
    let file = wide(launch.path.as_os_str());
    let parameters = wide(std::ffi::OsStr::new(&command_line(&request.args)));
    let folder = launch
        .folder
        .as_ref()
        .map(|folder| wide(folder.as_os_str()));
    let _com = Com::new();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // Pane waits for the program, and explains a failure itself.
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: windows::core::w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: folder
            .as_ref()
            .map_or(PCWSTR::null(), |folder| PCWSTR(folder.as_ptr())),
        nShow: if request.show_window {
            SW_SHOWNORMAL.0
        } else {
            SW_HIDE.0
        },
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size, and its strings are
    // NUL-terminated and outlive the call. It blocks while the elevation
    // prompt shows, on this thread of the program's own.
    let started = unsafe { ShellExecuteExW(&mut info) };
    if let Err(error) = started {
        if error.code() == ERROR_CANCELLED.to_hresult() {
            return Err(ProgramError::new(
                ErrorKind::Declined,
                format!(
                    "the user declined to run {} as an administrator",
                    request.shown()
                ),
            ));
        }
        return Err(ProgramError::new(
            ErrorKind::Unavailable,
            format!(
                "Windows did not start {} elevated: {}",
                request.shown(),
                error.message()
            ),
        ));
    }
    if info.hProcess.0.is_null() {
        return Err(ProgramError::new(
            ErrorKind::Failed,
            format!(
                "Windows started {} elevated but gave Pane no way to wait for it",
                request.shown()
            ),
        ));
    }
    let process = Process(info.hProcess);
    // Windows may refuse a program that is not elevated a job for one that
    // is: the program itself is then all Pane can end.
    let job = Job::contain(process.0);
    // SAFETY: the handle is open while `process` lives.
    let pid = unsafe { GetProcessId(process.0) };
    let (run, _registered) = match runner::register(
        request,
        owner,
        pid,
        launch.path.clone(),
        format!("{} (elevated)", launch.path.display()),
    ) {
        Ok(registered) => registered,
        Err(error) => {
            end(job.as_ref(), &process);
            return Err(error);
        }
    };
    watching.registered(run.clone());
    let deadline = request.timeout.map(|timeout| Instant::now() + timeout);
    loop {
        if let Some(error) = watching.must_end(owner, &run, deadline, request) {
            end(job.as_ref(), &process);
            return Err(error);
        }
        // SAFETY: the handle is open while `process` lives.
        let waited = unsafe { WaitForSingleObject(process.0, TICK_MS) };
        if waited == WAIT_OBJECT_0 {
            break;
        }
        if waited != WAIT_TIMEOUT {
            end(job.as_ref(), &process);
            return Err(ProgramError::new(
                ErrorKind::Failed,
                format!("Pane lost track of {}", request.shown()),
            ));
        }
    }
    let mut code = 0u32;
    // SAFETY: the handle is open while `process` lives; `code` is written.
    let read = unsafe { GetExitCodeProcess(process.0, &mut code) };
    // What it left running ends with it.
    if let Some(job) = &job {
        job.terminate();
    }
    read.map_err(|error| {
        ProgramError::new(
            ErrorKind::Failed,
            format!(
                "Pane could not read how {} exited: {}",
                request.shown(),
                error.message()
            ),
        )
    })?;
    Ok(Output {
        exit_code: Some(code as i32),
        stdout: Vec::new(),
        stderr: Vec::new(),
    })
}

/// COM on the calling thread for the shell's elevation, while this lives.
#[cfg(windows)]
struct Com {
    initialized: bool,
}

#[cfg(windows)]
impl Com {
    fn new() -> Com {
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
        };
        // SAFETY: no reserved pointer; paired with CoUninitialize in `drop`.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Com {
            initialized: result.is_ok(),
        }
    }
}

#[cfg(windows)]
impl Drop for Com {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: pairs the successful CoInitializeEx in `new`.
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

/// `args` as one Windows command line, quoted so that the program's
/// standard parsing (`CommandLineToArgvW`, the C runtime's) gives them back
/// exactly: the elevation prompt takes the arguments as one string.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `arg` quoted for a Windows command line, where needed.
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return arg.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in arg.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                // Each backslash before a quote is doubled, and the quote
                // escaped.
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }
    // Backslashes before the closing quote are doubled.
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(args: &[&str]) -> String {
        command_line(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn arguments_are_quoted_so_that_windows_gives_them_back_as_they_were() {
        assert_eq!(line(&["/quiet", "--id", "Git.Git"]), "/quiet --id Git.Git");
        assert_eq!(line(&["two words"]), "\"two words\"");
        assert_eq!(line(&[""]), "\"\"");
        assert_eq!(line(&["say \"hi\""]), "\"say \\\"hi\\\"\"");
        assert_eq!(line(&[r"C:\Program Files\"]), "\"C:\\Program Files\\\\\"");
        assert_eq!(line(&[r"a\b"]), r"a\b");
        assert_eq!(line(&[r#"a\"b"#]), "\"a\\\\\\\"b\"");
    }

    #[cfg(not(windows))]
    #[test]
    fn an_elevated_run_is_unavailable_here() {
        let error = unavailable_here();
        assert_eq!(error.kind, ErrorKind::Unavailable);
        assert!(error.message.ends_with("yet"), "{}", error.message);
    }
}
