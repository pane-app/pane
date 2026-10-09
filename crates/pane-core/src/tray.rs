//! The tray or menu-bar entry: Pane's item in the system's tray (Windows)
//! or the menu bar (macOS), whose menu offers Open Pane, Settings and
//! Quit — the entry the General page's visibility preference turns on and
//! off, and the one way to reach Pane from any application while the
//! launcher is hidden besides the global hotkey.
//!
//! The launcher decides nothing here but the wiring: the window layer
//! ([`crate`]'s consumer, in Pane's window) takes the selections an
//! adapter reports and summons the launcher, opens Settings or quits. The
//! system is reached through one small trait, [`Tray`], with one adapter
//! per system, chosen by [`native`]:
//!
//! - Windows: `Shell_NotifyIcon` on a thread of Pane's own ([`windows`]):
//!   the notification area holds the icon, whose menu a right click
//!   opens and whose left click summons the launcher. No permission is
//!   needed. The icon is added again when Explorer restarts, keeps one
//!   identity per program path, and follows the taskbar's light or dark
//!   theme.
//! - macOS: an `NSStatusItem` in the system status area of the menu bar
//!   ([`macos`]), showing Pane's mark as a template image, whose menu
//!   AppKit shows when the item is clicked. It must be made and changed
//!   on the main thread, as the hotkey adapter must.
//! - Linux: no entry yet. The desktop's tray speaks StatusNotifierItem
//!   over DBus, which Pane does not speak yet, so there the adapter
//!   explains that the entry is unavailable — the preference stays
//!   recorded, never represented as a working toggle.
//!
//! An adapter reports each selection of the native menu to a
//! [`SelectionSender`]; the application reads them from the matching
//! [`Selections`]. Hiding the entry removes the native resource but keeps
//! the adapter alive, so showing it again needs no restart; dropping the
//! adapter removes the entry for good, as quitting Pane does.
//!
//! What this module does not do: it adds no other taskbar or desktop
//! behavior, changes no desktop setting, and claims no entry where the
//! system has none — an adapter that cannot provide one says why through
//! [`Tray::unavailable`], and a change that fails says why through
//! [`TrayError`], so a preference is kept only when the native state
//! really changed.

use std::fmt;
use std::sync::Arc;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsTray;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacTray;

/// One selection of the native menu: what the entry's menu did. The menu
/// and its labels belong to the adapter — platform-appropriate, in the
/// system's own terms — while what a selection *does* belongs to the
/// application, so the two cannot diverge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrayAction {
    /// Open Pane was chosen: the launcher is summoned — shown and
    /// focused, never hidden, whatever state it was in.
    OpenPane,
    /// Settings was chosen: the one Settings window is opened or focused.
    Settings,
    /// Quit was chosen: Pane ends, removing its native entry and its
    /// global hotkey registrations on the way out.
    Quit,
}

/// Why showing or hiding the native entry failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayError {
    /// The entry cannot be used on this system at all, with the
    /// explanation to show: an adapter with no native integration (Linux
    /// today), or one whose entry cannot be reached.
    Unavailable(String),
    /// The system refused the change, with its reason.
    Refused(String),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrayError::Unavailable(reason) => f.write_str(reason),
            // The same wording the global hotkeys' refusal uses, so a
            // page that shows both reads alike.
            TrayError::Refused(reason) => write!(f, "the system refused it: {reason}"),
        }
    }
}

/// Shows and hides Pane's tray or menu-bar entry, whose menu offers Open
/// Pane, Settings and Quit. The launcher calls it on the window's thread
/// (macOS changes the item on the main run loop, as it registers hotkeys
/// there); the adapter is kept for the whole process, so the preference's
/// every change finds the same native entry. Pan Pane's own code — the
/// General page and the quit path — hides the entry explicitly; the
/// adapter also removes it when dropped, and the system removes it when
/// the process ends, but only the explicit path is deterministic.
pub trait Tray: Send + Sync + 'static {
    /// Why the entry cannot be used here at all — an adapter with no
    /// native integration for this system; `None` when it can.
    fn unavailable(&self) -> Option<String>;

    /// Shows the native entry, or hides it, taking effect at once. Both
    /// directions may fail: `Err` explains why, and the entry stays as it
    /// was, so a preference is kept only for a change that really
    /// happened. Repeating the state in effect succeeds.
    fn set_visible(&self, visible: bool) -> Result<(), TrayError>;
}

/// Where an adapter reports the native menu's selections.
#[derive(Clone)]
pub struct SelectionSender(tokio::sync::mpsc::UnboundedSender<TrayAction>);

impl SelectionSender {
    /// Reports `action` as chosen in the native menu. Once the
    /// [`Selections`] are gone, it is dropped.
    pub fn send(&self, action: TrayAction) {
        let _ = self.0.send(action);
    }
}

/// The selections an adapter reports, in order.
pub struct Selections(tokio::sync::mpsc::UnboundedReceiver<TrayAction>);

impl Selections {
    /// The next selection, waiting for it; `None` once no adapter can
    /// send any.
    pub async fn next(&mut self) -> Option<TrayAction> {
        self.0.recv().await
    }

    /// A selection already reported, without waiting; `None` when none
    /// is waiting. Test support, as the hotkey presses' own peek is.
    #[cfg(test)]
    #[doc(hidden)]
    pub fn try_next(&mut self) -> Option<TrayAction> {
        self.0.try_recv().ok()
    }
}

/// A channel from an adapter to the application.
pub fn channel() -> (SelectionSender, Selections) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (SelectionSender(sender), Selections(receiver))
}

/// This system's adapter, reporting the menu's selections to
/// `selections`. On macOS it must be made on the main thread.
pub fn native(selections: SelectionSender) -> Arc<dyn Tray> {
    #[cfg(target_os = "windows")]
    {
        match WindowsTray::start(selections) {
            Ok(tray) => Arc::new(tray),
            Err(problem) => Arc::new(Unavailable(format!(
                "Not available: Pane could not make its tray entry: {problem}"
            ))),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match MacTray::start(selections) {
            Ok(tray) => Arc::new(tray),
            Err(problem) => Arc::new(Unavailable(format!(
                "Not available: Pane could not make its menu bar entry: {problem}"
            ))),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = selections;
        Arc::new(Unavailable(
            "Not available on Linux: the desktop's tray speaks StatusNotifierItem over DBus, \
             which Pane does not speak yet, so this Pane shows no tray entry. Quit Pane from \
             the launcher's window, and reach Settings from its footer menu."
                .into(),
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = selections;
        Arc::new(Unavailable(format!(
            "Not available on this system ({}): Pane has a tray or menu-bar entry only on \
             Windows, macOS and Linux",
            std::env::consts::OS
        )))
    }
}

/// An adapter for where the entry cannot be used, saying why.
pub struct Unavailable(pub String);

impl Tray for Unavailable {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn set_visible(&self, _visible: bool) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(self.0.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Tray, TrayAction, TrayError, Unavailable, channel};

    #[test]
    fn selections_arrive_in_order() {
        let (sender, mut selections) = channel();
        sender.send(TrayAction::OpenPane);
        sender.send(TrayAction::Settings);
        sender.send(TrayAction::Quit);
        // The channel is synchronous enough for a bounded check: each send
        // landed before the next, so the first three receives are them.
        assert_eq!(selections.try_next(), Some(TrayAction::OpenPane));
        assert_eq!(selections.try_next(), Some(TrayAction::Settings));
        assert_eq!(selections.try_next(), Some(TrayAction::Quit));
    }

    #[test]
    fn the_unavailable_adapter_carries_its_explanation() {
        let tray = Unavailable("Not available here".into());
        assert_eq!(tray.unavailable().as_deref(), Some("Not available here"));
        assert_eq!(
            tray.set_visible(true),
            Err(TrayError::Unavailable("Not available here".into()))
        );
        // The error *is* the explanation, as a refusal of the change is.
        assert_eq!(
            tray.set_visible(false).unwrap_err().to_string(),
            "Not available here"
        );
        assert_eq!(
            TrayError::Refused("the tray is full".into()).to_string(),
            "the system refused it: the tray is full"
        );
    }

    #[test]
    fn a_dropped_receiver_is_not_an_error_to_report_to() {
        let (sender, selections) = channel();
        drop(selections);
        // No receiver: the send is quietly dropped, as the hotkey
        // presses' are.
        sender.send(TrayAction::Quit);
    }
}
