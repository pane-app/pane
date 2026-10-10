//! The taskbar, shown while the launcher is open (#268, ADR 0039): for a
//! user whose taskbar hides itself, the "Show the taskbar when Pane
//! opens" General-page choice keeps it on screen while the launcher
//! window is shown — so the Start button stays one click away once the
//! Windows key opens Pane — and puts it back exactly as the user had it
//! when the launcher hides.
//!
//! One small seam, [`Taskbar`], that the launcher window calls as it
//! shows and hides (and as the choice changes while it is shown). Windows
//! is the only adapter ([`native`] answers `None` elsewhere, so the
//! General page explains the choice there instead of offering it); the
//! tests replace it through [`crate::settings::attach_taskbar`] with a
//! fake that records what it was asked, so the choice is exercised at the
//! boundary a user sees it — the launcher's opening and closing.

use std::sync::Arc;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::WindowsTaskbar;

/// Shows the taskbar while the launcher is open, and puts it back as the
/// user had it when the launcher hides (#268). The adapter is kept for
/// the whole process, so the state it remembers between a show and its
/// restore survives the launcher's every opening.
pub trait Taskbar: Send + Sync + 'static {
    /// Shows the taskbar now, where it hides itself: the adapter remembers
    /// the state the user's own taskbar setting had, so
    /// [`Taskbar::restore`] puts it back. A taskbar that does not hide
    /// itself is already on screen, and nothing is taken or put back.
    /// Repeating a show that is already in effect changes nothing.
    fn show_while_open(&self);

    /// Puts the taskbar back as the user had it, where a show changed it:
    /// an auto-hiding taskbar hides itself again. Nothing to restore — no
    /// show happened, or the taskbar does not hide itself — is nothing
    /// done, so a launcher that hides without having shown one disturbs
    /// nothing.
    fn restore(&self);
}

/// This system's taskbar adapter: Windows' on Windows; `None` elsewhere —
/// macOS and Linux have no taskbar of the kind the choice is about, so
/// the General page explains the choice there rather than offering a
/// switch that would pretend.
pub fn native() -> Option<Arc<dyn Taskbar>> {
    #[cfg(target_os = "windows")]
    return Some(Arc::new(WindowsTaskbar::new()));
    #[cfg(not(target_os = "windows"))]
    None
}
