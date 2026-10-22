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

/// Runs the window until the launcher is no longer running an action,
/// the keys the window held for the query's list have been applied, and
/// the window has drawn what it shows: a settle that returns while keys
/// are still held reports the list before the publication they wait for,
/// and the keys the test presses next are swallowed by the hold (#203).
pub fn settle(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (view, drawn, held) = cx.read_entity(window, |window, _| {
            (
                window.launcher().view(),
                window.drawn_view().cloned(),
                window.held_keys(),
            )
        });
        if !matches!(view.status, Status::Running { .. })
            && held.is_empty()
            && drawn.as_ref() == Some(&view)
        {
            return view;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the launcher shows {view:?}, the window last drew {drawn:?} \
             (keys still held: {held:?})"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window, as [`settle`] does, until the launcher is no longer
/// running an action and the keys the window held for the query's list
/// have been applied — but reads the launcher's state alone, never
/// demanding that the window's last frame drew it. The window's redraw
/// of a change is the frame bracket's question, not the waiting test's:
/// a settle that demands the drawn frame match false-fails when it lags
/// the launcher's state, as it does on this platform — the answer a
/// thread computed reaches the launcher while the frame it would appear
/// in never comes, and the state the test means to settle on is already
/// there (the fix-forward rounds' evidence, #363 and #374).
///
/// The wait's end moves the clock the window's timers run on past the
/// settle timers the platform arms — the announcer's typing settle, a
/// held key's deadline, both 300 ms — before it returns, as time passes
/// for a user reading what settled before acting again: a window whose
/// clock never moved has starved them, the redraw they ask for never
/// happens, and the keys the test presses next arrive nowhere. Tests
/// that measure the clock itself while their work settles drive their
/// own clock and read the state with a poll of their own.
pub fn settle_state(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (view, held) = cx.read_entity(window, |window, _| {
            (window.launcher().view(), window.held_keys())
        });
        if !matches!(view.status, Status::Running { .. }) && held.is_empty() {
            // The settle horizon: a late answer can still change the
            // state under it, so the read is taken again after.
            cx.executor().advance_clock(SETTLED_PAST);
            cx.run_until_parked();
            let (view, held) = cx.read_entity(window, |window, _| {
                (window.launcher().view(), window.held_keys())
            });
            if !matches!(view.status, Status::Running { .. }) && held.is_empty() {
                return view;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the launcher shows {view:?} (keys still held: {held:?})"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window until `done` holds for the launcher's view, read from
/// the launcher's state alone: the window's last frame is not demanded.
/// The window's redraw of a change is the frame bracket's question, not
/// the waiting test's — a wait that demands the drawn frame match
/// false-fails when it lags the launcher's state, as it does on this
/// platform (the fix-forward rounds' evidence, #363 and #374: the rows
/// are read from the launcher's state with an inline poll, matching the
/// pane-core harness's wait shape) — and what the test asserts of the
/// state is the launcher's, whichever frame shows it.
///
/// The wait's end moves the clock the window's timers run on past the
/// settle timers the platform arms, as [`settle_state`] does: the keys
/// the test types after it land. A test that measures the clock while
/// its answer settles — a loading bar's lateness, a fade's leaving —
/// drives its own clock and reads the state with a poll of its own
/// instead, unmoved by this.
pub fn until_state(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&LauncherView) -> bool,
) -> LauncherView {
    // Generous: opening a JavaScript command compiles a 4 MB component
    // first, and CI's runners are slow.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if done(&view) {
            // The settle horizon, as [`settle_state`] takes it: a late
            // answer can still change the state under the predicate, so
            // it is read again after the clock has moved.
            cx.executor().advance_clock(SETTLED_PAST);
            cx.run_until_parked();
            let view = cx.read_entity(window, |window, _| window.launcher().view());
            if done(&view) {
                return view;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the launcher shows {view:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// How far past the settle timers a wait's end moves the clock: the
/// announcer's settle and a held key's deadline are 300 ms, and the
/// horizon clears both.
const SETTLED_PAST: Duration = Duration::from_millis(310);

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

/// Runs the window until the list for the query typed is published and
/// the window has drawn it (#201): until then the rows shown stay the
/// previous query's, while the field shows what was typed. A query that
/// asks no provider is published at once, so this never waits for one.
pub fn published(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    // Generous: a provider answering within the budget still takes a
    // moment of real time, and CI's runners are slow.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if cx.read_entity(window, |window, _| window.launcher().list_published()) {
            return settle(window, cx);
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the query's list to be published"
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
