//! What a command tells the user through the launcher (#141): its toast in
//! the footer, which replaced the status line's answered text, and a
//! window fake that records what the launcher had the window do (hide, show
//! a HUD, draw a confirmation, #146). Shared by the test binaries that
//! drive the launcher.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use pane_core::feedback::WindowRequest;
use pane_core::{Hud, Launcher, Status, ToastStyle, WindowControl};

/// What the user reads of the last outcome, as the status line said it
/// before toasts: the toast in the footer while the status line is idle (a
/// success as [`Status::Result`] with its title, a failure as
/// [`Status::Error`] with its title and message, work in progress as
/// [`Status::Progress`]), and the status line otherwise. An error a command
/// answered with is a failure toast that reads as the status line read it
/// ("The extension reported an error: …").
pub fn shown(launcher: &Launcher) -> Status {
    let status = launcher.view().status;
    if status != Status::Idle {
        return status;
    }
    match launcher.toast() {
        None => Status::Idle,
        Some(shown) => match shown.toast.style {
            ToastStyle::Success => Status::Result(shown.toast.text()),
            ToastStyle::Failure => Status::Error(shown.toast.text()),
            ToastStyle::Animated => Status::Progress(shown.toast.text()),
        },
    }
}

/// The title of the toast in the footer, if one shows.
pub fn toast_title(launcher: &Launcher) -> Option<String> {
    launcher.toast().map(|shown| shown.toast.title)
}

/// A window that records what the launcher has it do, in order.
#[derive(Default)]
pub struct RecordingWindow {
    requests: Mutex<Vec<WindowRequest>>,
}

impl RecordingWindow {
    /// A recording window attached to `launcher`.
    pub fn attach(launcher: &Launcher) -> Arc<RecordingWindow> {
        let window = Arc::new(RecordingWindow::default());
        launcher.attach_window(window.clone());
        window
    }

    /// What the launcher had the window do so far, in order, forgotten
    /// once read.
    pub fn take(&self) -> Vec<WindowRequest> {
        std::mem::take(&mut *self.requests.lock().unwrap())
    }

    /// The HUDs the window was asked to show so far, forgotten with
    /// [`RecordingWindow::take`].
    pub fn huds(&self) -> Vec<Hud> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|request| match request {
                WindowRequest::Hud(hud) => Some(hud.clone()),
                WindowRequest::Hide | WindowRequest::Confirmation => None,
            })
            .collect()
    }

    /// How many times the window was asked to draw a confirmation (showing
    /// itself first while hidden) so far.
    pub fn confirmations(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| **request == WindowRequest::Confirmation)
            .count()
    }

    /// How many times the window was asked to hide so far.
    pub fn hides(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| **request == WindowRequest::Hide)
            .count()
    }
}

impl WindowControl for RecordingWindow {
    fn hide(&self) {
        self.requests.lock().unwrap().push(WindowRequest::Hide);
    }

    fn show_hud(&self, hud: &Hud) {
        self.requests
            .lock()
            .unwrap()
            .push(WindowRequest::Hud(hud.clone()));
    }

    fn confirmation(&self) {
        self.requests
            .lock()
            .unwrap()
            .push(WindowRequest::Confirmation);
    }
}
