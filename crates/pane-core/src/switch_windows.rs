//! The open windows, as a command reaches them through Pane
//! (`wit/windows.wit`, the Windows power features' Switch Windows,
//! #125, ADR 0040): the windows Windows' own Alt+Tab would show, and
//! bringing one of them to the front.
//!
//! Which windows count is a pure rule over the facts the adapter reads of
//! a window ([`predicate`]), compiled and tested on every system: a
//! visible, unowned top-level window that is not "no activate", not a
//! tool window unless the application forced it onto the taskbar, not
//! removed from the taskbar by its application, not a Store app's inner
//! core window (its frame is kept), not one of the shell's own surfaces,
//! and titled; a window the application cloaked is dropped, and one the
//! shell cloaked because it is on another virtual desktop is kept only
//! where the user's Alt+Tab shows all desktops, marked as [`Window`]'s
//! `elsewhere`.
//!
//! The application a window belongs to — its name and its icon — is
//! resolved best-effort the way the front application is (#253): the
//! window's own AppUserModelID first, then its program's path, then its
//! process's package identity, matched against the installed
//! applications ([`window`] here, [`crate::system::front`] there).
//! Bringing one to the front restores it first when it is minimized,
//! then follows the paste worker's path (the plain foreground call, then
//! attaching to the foreground thread's input and trying again). Only
//! the Windows half is Windows' ([`windows`]); elsewhere [`native`]
//! answers that the open windows are not available there, which is not a
//! failure.
//!
//! The launcher reaches the windows through one trait,
//! [`SwitchWindows`], given to it as the system is
//! ([`crate::Launcher::with_switch_windows`]): the app passes
//! [`native`], tests a recording fake, and a launcher given none answers
//! with [`none`]'s refusal. Each call runs off the runtime's thread, so
//! it may block for as long as the system takes (a window's process
//! asked for its elevation, the foreground coming back).

mod predicate;

#[cfg(target_os = "windows")]
mod windows;

pub use predicate::{Attributes, Scope, counts, shows_all_desktops};
#[cfg(target_os = "windows")]
pub use windows::WindowsSwitchWindows;

use std::sync::Arc;

use crate::applications::identity::Catalog;
use crate::system::FrontApplication;
use crate::system::front::{Target, front_application as resolve};

/// One open window, as the extension sees it (`window` in the WIT).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// Identifies the window for this session alone: opaque, and names
    /// it to [`SwitchWindows::activate`]. A window that closed is gone,
    /// and another may have taken its place.
    pub id: String,
    /// The window's title, as the system shows it ("notes.txt -
    /// Notepad").
    pub title: String,
    /// The name of the application the window belongs to, best-effort:
    /// the installed application it resolves to, or one named for the
    /// window itself where none matches.
    pub application: String,
    /// What the application's icon is the system's icon of: its
    /// program's path, or a Windows `shell:AppsFolder` name. `None` when
    /// nothing of the application is known.
    pub icon: Option<String>,
    /// Whether the window is minimized.
    pub minimized: bool,
    /// Whether the window is maximized.
    pub maximized: bool,
    /// Whether the window is on another virtual desktop than the one
    /// shown.
    pub elsewhere: bool,
    /// Whether the window's process is running as administrator, as far
    /// as Pane can tell.
    pub elevated: bool,
}

/// Why a call did not answer (`windows-error` in the WIT): the windows
/// are not available here, or it went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsError {
    /// Pane cannot list the open windows here (Windows only, or a Pane
    /// that reaches no system): nothing went wrong, and the command can
    /// do something else. Says what is not available, for the user.
    NotAvailable(String),
    /// It went wrong: why, for the user — a window gone, or one that did
    /// not come to the front.
    Failed(String),
}

impl WindowsError {
    /// What it says, for the user: why the windows are not available
    /// here, or why it went wrong.
    pub fn message(&self) -> &str {
        match self {
            WindowsError::NotAvailable(why) | WindowsError::Failed(why) => why,
        }
    }
}

/// The open windows, as a command reaches them through
/// `pane:extension/windows`. Each function is called off the runtime's
/// thread and may block; an error explains to the user why nothing
/// happened.
pub trait SwitchWindows: Send + Sync + 'static {
    /// The windows Alt+Tab would show, in z-order with the front
    /// application's window first.
    fn list(&self) -> Result<Vec<Window>, WindowsError>;

    /// Brings the window `id` names to the front, restoring it first if
    /// it is minimized. An id no window of this session names, or one
    /// whose window closed since it was listed, is an error.
    fn activate(&self, id: &str) -> Result<(), WindowsError>;
}

/// A window the adapter read, before its application is resolved: the
/// same facts the front application is read of (#253), with what the
/// record says of the window itself.
pub struct Facts {
    /// The window, as an address: a handle is not `Send`. Its opaque id
    /// is this, spelled as a number — the adapter's own, so it can find
    /// the window again when it is activated.
    pub window: usize,
    /// The window's title, as the system shows it.
    pub title: String,
    /// The window's own AppUserModelID, if it has one: what the taskbar
    /// groups and launches it by, which a packaged application or a web
    /// app has and a plain desktop program usually does not.
    pub aumid: Option<String>,
    /// The full path of the program of the window's process, if it could
    /// be read.
    pub program: Option<String>,
    /// The package family name of the window's process, if it has one.
    pub family: Option<String>,
    /// Whether the window is minimized.
    pub minimized: bool,
    /// Whether the window is maximized.
    pub maximized: bool,
    /// Whether the window is on another virtual desktop than the one
    /// shown.
    pub elsewhere: bool,
    /// Whether the window's process is running as administrator, as far
    /// as Pane can tell.
    pub elevated: bool,
}

/// `facts` as the record the extension sees, its application resolved
/// against the `installed` applications in the order the specification
/// names — the window's own AppUserModelID first, then its program's
/// path, then its process's package identity, the front application's
/// chain (#253). A pure mapping, tested on every system; reading the
/// facts is the adapter's.
pub fn window(facts: &Facts, installed: &Catalog) -> Window {
    let target = Target {
        title: facts.title.clone(),
        aumid: facts.aumid.clone(),
        program: facts.program.clone(),
        family: facts.family.clone(),
    };
    let FrontApplication { name, icon } = resolve(&target, installed);
    Window {
        id: facts.window.to_string(),
        title: facts.title.clone(),
        application: name,
        icon,
        minimized: facts.minimized,
        maximized: facts.maximized,
        elsewhere: facts.elsewhere,
        elevated: facts.elevated,
    }
}

/// This system's adapter: Windows', or one that explains that the open
/// windows are not available here.
pub fn native() -> Arc<dyn SwitchWindows> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsSwitchWindows::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let here = crate::platform::Platform::current().map_or_else(
            || format!("this system ({})", std::env::consts::OS),
            |platform| platform.to_string(),
        );
        Arc::new(Unavailable(WindowsError::NotAvailable(format!(
            "The open windows are not available on {here} yet"
        ))))
    }
}

/// The open windows of a launcher given none: every call says that this
/// Pane lists no open windows.
pub fn none() -> Arc<dyn SwitchWindows> {
    Arc::new(Unavailable(WindowsError::NotAvailable(
        "Not available: this Pane lists no open windows".into(),
    )))
}

/// Lists no windows, saying why.
struct Unavailable(WindowsError);

impl SwitchWindows for Unavailable {
    fn list(&self) -> Result<Vec<Window>, WindowsError> {
        Err(self.0.clone())
    }

    fn activate(&self, _id: &str) -> Result<(), WindowsError> {
        Err(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_of_a_pane_given_no_adapter_is_refused() {
        let windows = none();
        let why = windows.list().unwrap_err();
        assert!(matches!(why, WindowsError::NotAvailable(_)), "{why:?}");
        assert!(
            why.message().starts_with("Not available"),
            "{}",
            why.message()
        );
        assert_eq!(windows.activate("1").unwrap_err(), why);
    }

    #[test]
    fn this_systems_adapter_says_so_where_it_cannot_list() {
        // On Windows it is the real adapter; elsewhere it says the open
        // windows are not available yet, which is not a failure.
        if cfg!(windows) {
            return;
        }
        match native().list() {
            Err(WindowsError::NotAvailable(why)) => {
                assert!(why.contains("not available on"), "{why}");
                assert!(why.ends_with(" yet"), "{why}");
            }
            other => panic!("expected not available, got {other:?}"),
        }
    }
}
