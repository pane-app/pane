//! What Windows' Run dialog (Win+R) runs (`pane:extension/run`, ADR
//! 0040): a command line naming a program by its name or path with
//! arguments, a Control Panel applet, a management console, a document, a
//! folder, a network path, a `shell:` or `ms-settings:` address or another
//! registered scheme, with environment variables expanded — run for the
//! command, through Windows' own elevation prompt (ADR 0033) when asked
//! — together with the Run dialog's own history, which this command and
//! Windows' share in both directions, and the completions for the text
//! typed in Run's field: history entries, programs, applets, consoles,
//! registered schemes and environment variables, each matching the typed
//! text. Windows only: elsewhere every function answers
//! [`RunError::NotAvailable`], which is not a failure.
//!
//! ```ignore
//! use pane_extension::run::{self, RunError};
//!
//! run::run("notepad.exe C:\\Notes\\todo.txt", false)?;
//! run::run("regedit", true)?; // Windows asks first
//! let history = run::history()?;
//! run::delete_from_history("notepad.exe C:\\Notes\\todo.txt")?;
//! let completions = run::completions("note")?; // what the field offers
//! let terminal = run::terminal()?; // Windows Terminal, when installed
//! ```
//!
//! A run that answered is recorded in the history, the line as the user
//! typed it; a failed run, or one the user declined at Windows' elevation
//! prompt, records nothing.

wit_bindgen::generate!({
    path: "wit",
    world: "run-user",
    default_bindings_module: "pane_extension::run",
});

pub use pane::extension::run::{
    Completion, CompletionSource, RunError, completions, delete_from_history, history, run,
    terminal,
};

impl RunError {
    /// What it says, for the user: why running what the Run dialog runs
    /// is not available here, why it failed, or what was declined.
    pub fn message(&self) -> &str {
        match self {
            RunError::NotAvailable(why) | RunError::Failed(why) | RunError::Declined(why) => why,
        }
    }
}
