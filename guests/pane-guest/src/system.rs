//! The system Pane runs on (`pane:extension/system`): the clipboard,
//! opening anything, showing a path in the file manager and moving paths
//! to the Recycle Bin.
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
//! the user anything. The standard actions of [`crate::actions`] compose
//! them with [`crate::window::close`] and a HUD or a toast, as Raycast's
//! built-in actions do. Paths are absolute; a relative one is refused.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub use crate::pane::extension::system::{
    Clip, HostSystem, NotTrashed, copy, open, read_clipboard, reveal, running_on, trash,
};

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
