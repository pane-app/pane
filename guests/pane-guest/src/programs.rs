//! System programs (`pane:extension/programs`, ADR 0033): programs
//! installed on the system, such as PowerShell, winget, git or any
//! executable, which Pane runs for the command.
//!
//! [`run`] waits for a program and answers its exit code and what it wrote
//! ([`Output`], bytes, with text helpers); [`spawn`] answers a [`Process`]
//! the command writes to, reads from as it writes (to report progress in a
//! toast, say), waits for and kills. A program is named by an absolute
//! path, or by a bare name Pane finds on the user's search path at the time
//! of the call; each argument reaches it as it is, since no shell reads
//! them. [`powershell`], [`cmd`] and [`sh`] run a script with the system's
//! shells.
//!
//! ```ignore
//! use pane_guest::programs::{Options, run};
//!
//! let output = run("git", &["status", "--short"], b"", Options::default()).await?;
//! let changes = output.stdout_text();
//! ```
//!
//! A program belongs to the call that started it: Pane ends it, and every
//! process it started, when the call returns, when the extension is
//! disabled, reloaded, updated or uninstalled, and when Pane quits.
//! Dropping the future of a run (when a timer wins a race with it) ends it
//! too. A command whose component imports this module is listed as one
//! that runs system programs.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

wit_bindgen::generate!({
    path: "../../wit",
    world: "programs-user",
    default_bindings_module: "pane_guest::programs",
});

use pane::extension::programs as wit;
pub use wit::{EnvVar, Options, Output, ProcessId, ProgramError, ProgramErrorKind};

impl ProgramErrorKind {
    /// The kind's WIT name, such as `not-found`, as JavaScript sees it too.
    pub fn name(&self) -> &'static str {
        match self {
            ProgramErrorKind::NotFound => "not-found",
            ProgramErrorKind::Unavailable => "unavailable",
            ProgramErrorKind::Declined => "declined",
            ProgramErrorKind::TimedOut => "timed-out",
            ProgramErrorKind::TooMuchOutput => "too-much-output",
            ProgramErrorKind::Refused => "refused",
            ProgramErrorKind::Failed => "failed",
        }
    }
}

impl ProgramError {
    /// `<kind>: <message>`, such as "not-found: there is no program `git`
    /// on the search path", to show people.
    pub fn explain(&self) -> String {
        format!("{}: {}", self.kind.name(), self.message)
    }
}

impl Default for Options {
    /// In the user's home folder, with Pane's environment, no timeout, no
    /// window and not elevated.
    fn default() -> Options {
        Options {
            folder: None,
            environment: Vec::new(),
            timeout_ms: None,
            show_window: false,
            elevated: false,
        }
    }
}

impl Options {
    /// Runs the program in `folder`, an absolute path.
    pub fn in_folder(mut self, folder: impl Into<String>) -> Options {
        self.folder = Some(folder.into());
        self
    }

    /// Sets the environment variable `name` to `value` for the program.
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Options {
        self.environment.push(EnvVar {
            name: name.into(),
            value: Some(value.into()),
        });
        self
    }

    /// Removes the environment variable `name` for the program.
    pub fn without_env(mut self, name: impl Into<String>) -> Options {
        self.environment.push(EnvVar {
            name: name.into(),
            value: None,
        });
        self
    }

    /// Ends the program after `milliseconds`; the call then answers
    /// [`ProgramErrorKind::TimedOut`].
    pub fn timeout(mut self, milliseconds: u32) -> Options {
        self.timeout_ms = Some(milliseconds);
        self
    }

    /// Gives the program a console window of its own on Windows.
    pub fn with_window(mut self) -> Options {
        self.show_window = true;
        self
    }

    /// Runs the program as an administrator, through Windows' elevation
    /// prompt: its run answers only its exit code. Elsewhere it is
    /// unavailable for now.
    pub fn as_administrator(mut self) -> Options {
        self.elevated = true;
        self
    }
}

/// `bytes` as text, with any invalid UTF-8 replaced.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Output {
    /// Whether the program exited with code 0.
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// What it wrote to its standard output, as text.
    pub fn stdout_text(&self) -> String {
        text(&self.stdout)
    }

    /// What it wrote to its standard error, as text.
    pub fn stderr_text(&self) -> String {
        text(&self.stderr)
    }
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

/// Runs `program` with `args` and `input` on its standard input, and
/// answers how it exited and what it wrote, whatever its exit code.
pub async fn run(
    program: &str,
    args: &[&str],
    input: &[u8],
    options: Options,
) -> Result<Output, ProgramError> {
    wit::run(program.to_string(), owned(args), input.to_vec(), options).await
}

/// Starts `program` with `args`, answering the [`Process`] to write to,
/// read from, wait for and kill while it runs.
pub async fn spawn(
    program: &str,
    args: &[&str],
    options: Options,
) -> Result<Process, ProgramError> {
    let id = wit::spawn(program.to_string(), owned(args), options).await?;
    Ok(Process { id })
}

/// Runs `script` with Windows PowerShell (`powershell`), passed encoded so
/// that no quoting changes it.
pub async fn powershell(script: &str, options: Options) -> Result<Output, ProgramError> {
    let encoded = encoded_command(script);
    run(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ],
        b"",
        options,
    )
    .await
}

/// Runs `script` with Windows' command interpreter (`cmd /d /s /c`). It
/// reads the script itself: quotes in it may not survive Windows' quoting
/// of arguments, which [`powershell`] avoids.
pub async fn cmd(script: &str, options: Options) -> Result<Output, ProgramError> {
    run("cmd", &["/d", "/s", "/c", script], b"", options).await
}

/// Runs `script` with the system's shell (`sh -c`), on macOS and Linux.
pub async fn sh(script: &str, options: Options) -> Result<Output, ProgramError> {
    run("sh", &["-c", script], b"", options).await
}

/// `script` as PowerShell's `-EncodedCommand` takes it: its UTF-16LE bytes
/// in base64.
fn encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

/// `bytes` in standard base64, padded.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (triple >> (18 - 6 * index)) & 0x3f;
                encoded.push(char::from(ALPHABET[sextet as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// A program [`spawn`] started, which ends with the call that started it.
pub struct Process {
    id: ProcessId,
}

impl Process {
    /// Pane's id for it.
    pub fn id(&self) -> ProcessId {
        self.id
    }

    /// Writes `bytes` to its standard input.
    pub async fn write(&self, bytes: &[u8]) -> Result<(), ProgramError> {
        wit::write_input(self.id, bytes.to_vec()).await
    }

    /// Closes its standard input: it reads its end once it read what was
    /// written before.
    pub async fn close_input(&self) -> Result<(), ProgramError> {
        wit::close_input(self.id).await
    }

    /// The next bytes it wrote to its standard output; `None` once it
    /// ended.
    pub async fn read_output(&self) -> Result<Option<Vec<u8>>, ProgramError> {
        wit::read_output(self.id).await
    }

    /// The next bytes it wrote to its standard error; `None` once it ended.
    pub async fn read_error(&self) -> Result<Option<Vec<u8>>, ProgramError> {
        wit::read_error(self.id).await
    }

    /// Waits for it to exit: its exit code, `None` when the system ended
    /// it.
    pub async fn wait(&self) -> Result<Option<i32>, ProgramError> {
        wit::wait(self.id).await
    }

    /// Ends it and every process it started; [`Process::wait`] answers how
    /// it exited.
    pub async fn kill(&self) -> Result<(), ProgramError> {
        wit::kill(self.id).await
    }

    /// Its standard output, line by line, as it writes it.
    pub fn lines(&self) -> Lines<'_> {
        Lines {
            process: self,
            buffer: Vec::new(),
            ended: false,
        }
    }
}

/// A spawned program's standard output, line by line ([`Process::lines`]).
pub struct Lines<'a> {
    process: &'a Process,
    buffer: Vec<u8>,
    ended: bool,
}

impl Lines<'_> {
    /// The next line it wrote, without its line ending, once it wrote a
    /// whole one (or the last, unended, once its output ended); `None` once
    /// there are no more.
    pub async fn next(&mut self) -> Result<Option<String>, ProgramError> {
        loop {
            if let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                let line = text(&line);
                return Ok(Some(line.trim_end_matches(['\n', '\r']).to_string()));
            }
            if self.ended {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                let line = text(&core::mem::take(&mut self.buffer));
                return Ok(Some(line));
            }
            match self.process.read_output().await? {
                Some(bytes) => self.buffer.extend_from_slice(&bytes),
                None => self.ended = true,
            }
        }
    }
}
