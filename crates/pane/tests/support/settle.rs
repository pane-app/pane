//! Waiting, in the native window's tests on GPUI's test platform, for what
//! arrives from other threads (a guest's answer, a crash, a build) until
//! the window has drawn it. Shared by the test binaries of `pane`.
//!
//! The launcher's view can change on those threads before the window is
//! told to redraw: a package's third crash pauses it on the runtime thread,
//! before the call's answer reaches the window. A test that read the view
//! then would find the pause while the last frame still shows the running
//! command, so an element looked up with `debug_bounds` would be missing.
//! These wait until the last frame drew the launcher's view as it is, and
//! never make the window redraw themselves: a window that does not redraw
//! by itself fails the test.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use gpui::{Entity, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{LauncherView, Status};

/// Enters the launcher's extension-management flow, as Pane's Settings
/// window does when one of its extension pages runs an operation (#168),
/// and runs the window until it has drawn the list: the launcher window
/// has no row of its own that opens it ("Manage Extensions" opens
/// Settings), but it still draws the flow's screens while the launcher
/// holds them, and follows them with the keyboard as it follows any
/// change Settings makes.
pub fn enter_flow(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    window.update_in(cx, |window, w, cx| window.enter_extension_flow(w, cx));
    until(window, cx, |view| {
        matches!(view.screen, pane_core::Screen::Extensions { .. })
    })
}

/// Runs the window until the launcher is no longer running an action and
/// the window has drawn what it shows.
pub fn settle(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    until(window, cx, |view| {
        !matches!(view.status, Status::Running { .. })
    })
}

/// Runs the window, as [`settle`] does, until its last frame also drew
/// nothing over the launcher: no Actions panel, no confirmation. Keys go
/// where the last frame put them, so after Enter chose an action in the
/// panel, or answered a confirmation, a test waits for this before it
/// presses the next keys.
pub fn settle_bare(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (view, drawn, over) = cx.read_entity(window, |window, _| {
            (
                window.launcher().view(),
                window.drawn_view().cloned(),
                window.drawn_over(),
            )
        });
        if !matches!(view.status, Status::Running { .. }) && drawn.as_ref() == Some(&view) && !over
        {
            return view;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the launcher shows {view:?}, the window last drew {drawn:?} (over it: {over})"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window until `done` holds for the launcher's view and the
/// window's last frame drew that view.
pub fn until(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&LauncherView) -> bool,
) -> LauncherView {
    // Generous: opening a JavaScript command compiles a 4 MB component
    // first, and CI's runners are slow.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (view, drawn) = cx.read_entity(window, |window, _| {
            (window.launcher().view(), window.drawn_view().cloned())
        });
        if done(&view) && drawn.as_ref() == Some(&view) {
            return view;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the launcher shows {view:?}, the window last drew {drawn:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window until the launcher is no longer running an action, as
/// [`settle`] does, and answers what the user reads of the outcome, as the
/// status line said it before toasts (#141): the toast in the footer while
/// the status line is idle (a success as [`Status::Result`] with its
/// title, a failure as [`Status::Error`] with its title and message, work
/// in progress as [`Status::Progress`]), and the status line otherwise.
pub fn settle_shown(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Status {
    let view = settle(window, cx);
    if view.status != Status::Idle {
        return view.status;
    }
    let toast = cx.read_entity(window, |window, _| window.launcher().toast());
    match toast {
        None => Status::Idle,
        Some(shown) => match shown.toast.style {
            pane_core::ToastStyle::Success => Status::Result(shown.toast.text()),
            pane_core::ToastStyle::Failure => Status::Error(shown.toast.text()),
            pane_core::ToastStyle::Animated => Status::Progress(shown.toast.text()),
        },
    }
}
