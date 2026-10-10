//! What Windows' Run dialog (Win+R) runs, and the history it keeps
//! (`wit/run.wit`, ADR 0040, #254): a command line naming a program by its
//! name or path with arguments, a Control Panel applet, a management
//! console, a document, a folder, a network path, a `shell:` or
//! `ms-settings:` address or another registered scheme, with environment
//! variables expanded. Pane runs it for a command (elevated, through
//! Windows' own prompt, when asked, ADR 0033) and shares the Run dialog's
//! own history — Windows keeps it in the registry as Explorer's RunMRU
//! — in both directions: what ran in either appears in both. The
//! completions for the text typed in Run's field (#264) come from the
//! same places: the history first, then programs from App Paths and the
//! registry search path, Control Panel applets, management consoles,
//! registered schemes and environment variables, each matching the typed
//! text ([`complete`], pure like the rest of the decision logic).
//!
//! The decision logic is plain text work, compiled and tested on every
//! system ([`parse`], [`normalize`], [`classify`], [`mru`], [`complete`]):
//! how a command line splits into what runs and what it is given, how a
//! rooted path is spelled as the system spells it, what kind of thing the
//! head names, how the history's format reads and writes, and which
//! offered lines complete the typed text. Only the acting half is
//! Windows' ([`windows`]); elsewhere [`native`] answers that running what
//! the Run dialog runs is not available yet, which is not a failure.
//!
//! The launcher reaches it through one trait, [`Run`], given to it as the
//! system is ([`crate::Launcher::with_run`]): the app passes [`native`],
//! tests a recording fake, a launcher given none answers with [`none`]'s
//! refusal. Each call runs off the runtime's thread, so it may block for
//! as long as the system takes (a handler starting, the elevation prompt
//! waiting for the user, the registry held open by another program).

use std::sync::Arc;

mod classify;
mod complete;
mod mru;
mod normalize;
mod parse;

#[cfg(target_os = "windows")]
mod windows;

pub use classify::{Target, classify};
pub use complete::{Candidates, Completion, Source, complete};
pub use mru::{decode, encode, record, remove};
pub use normalize::normalize;
pub use parse::{Sources, Split, split};
#[cfg(target_os = "windows")]
pub use windows::WindowsRun;

/// The HRESULT `ShellExecuteExW` answers when the user declines
/// Windows' elevation prompt: `HRESULT_FROM_WIN32(ERROR_CANCELLED)`.
const DECLINED: i32 = 0x8007_04CB_u32 as i32;

/// Why a run or a history call did not answer (`run-error` in the WIT).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    /// Pane cannot run what the Run dialog runs here (Windows only, or a
    /// Pane that reaches no system): nothing went wrong, and the command
    /// can do something else. Says what is not available, for the user.
    NotAvailable(String),
    /// It went wrong: why, for the user. A run that ran but whose history
    /// could not be written says that, saying it ran.
    Failed(String),
    /// The user declined Windows' elevation prompt: nothing ran, and
    /// nothing was recorded. Says what was declined, for the user.
    Declined(String),
}

impl RunError {
    /// What it says, for the user: why this is not available, why it
    /// failed, or what was declined.
    pub fn message(&self) -> &str {
        match self {
            RunError::NotAvailable(why) | RunError::Failed(why) | RunError::Declined(why) => why,
        }
    }
}

/// The Run dialog's work, as a command reaches it through
/// `pane:extension/run`. Each function is called off the runtime's thread
/// and may block; an error explains to the user why nothing ran.
pub trait Run: Send + Sync + 'static {
    /// Runs the command line `line` as the Run dialog reads it (see
    /// [`parse`] for how it splits and [`classify`] for what it names),
    /// through Windows' own elevation prompt when `elevated`, and records
    /// the trimmed line in the Run dialog's history once it ran; a failed
    /// run, or a declined elevation, records nothing.
    fn run(&self, line: &str, elevated: bool) -> Result<(), RunError>;

    /// The Run dialog's history, newest first, as the user typed it.
    fn history(&self) -> Result<Vec<String>, RunError>;

    /// Removes `line` from the Run dialog's history, matched ignoring
    /// case, rewriting it without the entry.
    fn delete_from_history(&self, line: &str) -> Result<(), RunError>;

    /// The completions for `text`, the text typed so far in Run's field
    /// (see [`complete`]): the sources read as they are at the time of
    /// the call, so a tool installed after Pane started is completed; a
    /// source that cannot be read contributes nothing.
    fn completions(&self, text: &str) -> Result<Vec<Completion>, RunError>;

    /// Windows Terminal's `wt.exe`, as an absolute path, when it is
    /// installed — as App Paths registered it and then the search path
    /// spell it — so a command can run a command line in a new Windows
    /// Terminal tab through ADR 0033's run-program host function;
    /// `Ok(None)` when it is not installed, and the caller runs the
    /// command line another way.
    fn terminal(&self) -> Result<Option<String>, RunError>;
}

/// What an elevated `ShellExecuteExW` answered, as the run answers: the
/// user declining Windows' prompt (`HRESULT_FROM_WIN32(ERROR_CANCELLED)`)
/// is [`RunError::Declined`], not a failure, so nothing ran and nothing
/// is recorded; any other code is a failure saying Windows did not start
/// it. A pure mapping of the code and message the call gives, tested on
/// every system; the call itself is Windows'.
pub fn elevated_answer(code: i32, why: &str, shown: &str) -> RunError {
    if code == DECLINED {
        return RunError::Declined(format!(
            "the user declined to run {shown} as an administrator"
        ));
    }
    RunError::Failed(format!(
        "Windows did not start {shown} as an administrator: {why}"
    ))
}

/// This system's adapter: Windows', or one that explains that running
/// what the Run dialog runs is not available here.
pub fn native() -> Arc<dyn Run> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsRun::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let here = crate::platform::Platform::current().map_or_else(
            || format!("this system ({})", std::env::consts::OS),
            |platform| platform.to_string(),
        );
        Arc::new(Unavailable(RunError::NotAvailable(format!(
            "Running what Windows' Run dialog runs is not available on {here} yet"
        ))))
    }
}

/// The Run dialog's work of a launcher given none: every function says
/// that this Pane runs nothing and keeps no Run history.
pub fn none() -> Arc<dyn Run> {
    Arc::new(Unavailable(RunError::NotAvailable(
        "Not available: this Pane runs nothing and keeps no Run history".to_string(),
    )))
}

/// Runs nothing, saying why.
struct Unavailable(RunError);

impl Run for Unavailable {
    fn run(&self, _line: &str, _elevated: bool) -> Result<(), RunError> {
        Err(self.0.clone())
    }

    fn history(&self) -> Result<Vec<String>, RunError> {
        Err(self.0.clone())
    }

    fn delete_from_history(&self, _line: &str) -> Result<(), RunError> {
        Err(self.0.clone())
    }

    fn completions(&self, _text: &str) -> Result<Vec<Completion>, RunError> {
        Err(self.0.clone())
    }

    fn terminal(&self) -> Result<Option<String>, RunError> {
        Err(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declined_elevation_is_answered_not_failed() {
        let declined = elevated_answer(DECLINED, "the prompt was refused", "notepad");
        assert_eq!(
            declined,
            RunError::Declined("the user declined to run notepad as an administrator".into())
        );
        // Any other code is a failure.
        let failed = elevated_answer(5, "access is denied", "notepad");
        assert_eq!(
            failed,
            RunError::Failed(
                "Windows did not start notepad as an administrator: access is denied".into()
            )
        );
    }

    #[test]
    fn a_run_of_a_pane_given_no_adapter_is_refused() {
        let run = none();
        let why = run.run("notepad", false).unwrap_err();
        assert!(matches!(why, RunError::NotAvailable(_)), "{why:?}");
        assert!(
            why.message().starts_with("Not available"),
            "{}",
            why.message()
        );
        assert_eq!(run.history().unwrap_err(), why);
        assert_eq!(run.delete_from_history("notepad").unwrap_err(), why);
        assert_eq!(run.completions("note").unwrap_err(), why);
        assert_eq!(run.terminal().unwrap_err(), why);
    }

    #[test]
    fn this_systems_adapter_says_so_where_it_cannot_run() {
        // On Windows it is the real adapter; elsewhere it says running
        // what the Run dialog runs is not available yet, which is not a
        // failure.
        if cfg!(windows) {
            return;
        }
        match native().history() {
            Err(RunError::NotAvailable(why)) => {
                assert!(why.contains("is not available on"), "{why}");
                assert!(why.ends_with(" yet"), "{why}");
            }
            other => panic!("expected not available, got {other:?}"),
        }
        // The completions and the terminal answer the same, as every
        // function of the capability does.
        assert!(
            matches!(native().completions("note"), Err(RunError::NotAvailable(_))),
            "the completions say why they are not available"
        );
        assert!(
            matches!(native().terminal(), Err(RunError::NotAvailable(_))),
            "the terminal says why it is not available"
        );
    }
}
