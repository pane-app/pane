//! The system Pane runs on (`pane:extension/system`): the clipboard,
//! opening anything, showing a path in the file manager and moving paths
//! to the Recycle Bin; pasting into the application that was in front
//! before Pane, that application, and the text selected in it.
//!
//! ```ignore
//! use pane_guest::system::{self, Clip};
//!
//! system::copy(&Clip::Text("hunter2".into()), true)?; // concealed
//! system::open("mailto:someone@example.com", None)?;
//! system::open(r"C:\Notes\todo.txt", Some(r"C:\Windows\System32\notepad.exe"))?;
//! ```
//!
//! Each function does only what it names: none closes the window or tells
//! the user anything but [`paste`], which closes it by nature. The
//! standard actions of [`crate::actions`] compose them with
//! [`crate::window::close`] and a HUD or a toast, as Raycast's built-in
//! actions do. Paths are absolute; a relative one is refused.
//!
//! [`paste`], [`front_application`] and [`selected_text`] answer
//! [`SystemError::NotAvailable`] where Pane cannot do them yet (on macOS
//! and Linux, and on Windows until Pane's Windows power features land),
//! which is not a failure:
//!
//! ```ignore
//! use pane_guest::system::{self, SystemError};
//!
//! let title = match system::front_application() {
//!     Ok(Some(front)) => format!("Paste to {}", front.name),
//!     _ => "Paste".into(),
//! };
//! match system::selected_text() {
//!     Ok(Some(text)) => { /* search for it */ }
//!     Ok(None) => { /* nothing is selected */ }
//!     Err(SystemError::NotAvailable(why)) => { /* do something else */ }
//!     Err(SystemError::Failed(why)) => return Err(why),
//! }
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub use crate::pane::extension::system::{
    Clip, FrontApp, HostSystem, NotTrashed, SystemError, copy, front_application, open, paste,
    read_clipboard, reveal, running_on, selected_text, trash,
};

impl SystemError {
    /// What it says, for the user: why the function is not available, or
    /// why it failed.
    pub fn message(&self) -> &str {
        match self {
            SystemError::NotAvailable(why) | SystemError::Failed(why) => why,
        }
    }
}

/// What the system's file manager is called: "Explorer" on Windows,
/// "Finder" on macOS, "File Manager" elsewhere ("Show in Explorer").
pub fn file_manager_name() -> &'static str {
    match running_on() {
        HostSystem::Windows => "Explorer",
        HostSystem::Macos => "Finder",
        HostSystem::Linux | HostSystem::Other => "File Manager",
    }
}

/// What the system's trash is called: "Recycle Bin" on Windows, "Trash"
/// elsewhere ("Move to Recycle Bin").
pub fn trash_name() -> &'static str {
    match running_on() {
        HostSystem::Windows => "Recycle Bin",
        HostSystem::Macos | HostSystem::Linux | HostSystem::Other => "Trash",
    }
}

/// What [`trash`] could not move, in one sentence for the user: "Could not
/// move 1 of 2 items to the Recycle Bin: C:\a.txt: It does not exist".
pub fn describe_not_trashed(not_trashed: &[NotTrashed], of: usize) -> String {
    let items = if of == 1 { "item" } else { "items" };
    let reasons: Vec<String> = not_trashed
        .iter()
        .map(|not| format!("{}: {}", not.path, not.reason))
        .collect();
    format!(
        "Could not move {} of {of} {items} to the {}: {}",
        not_trashed.len(),
        trash_name(),
        reasons.join("; ")
    )
}
