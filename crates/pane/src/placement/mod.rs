//! Launcher placement: the display the launcher window opens on, as the
//! Launcher page's choice resolves against the display layout the platform
//! reports.
//!
//! The choice is a host setting (the record's `openingMonitor` field); the
//! behavior half that is the same everywhere — the resolution, with its
//! fallbacks — is [`pane_core::placement`]. What this module owns is the
//! platform half: the layout, read from the system's own display list, and
//! the move of a window that already exists, which GPUI CE has no API for
//! (it places a window once, when the window is made). One small trait,
//! [`Placement`], with one adapter per system, chosen by [`native`]:
//!
//! - Windows: the monitors the GDI API lists, each with its bounds and its
//!   work area, the primary one, the pointer's position from `GetCursorPos`
//!   and the foreground window's monitor from `MonitorFromWindow`; moving
//!   a window is `SetWindowPos` on the window's handle ([`windows`]);
//! - macOS: the displays CoreGraphics lists with their bounds in points,
//!   the main one, and the pointer's position from a fresh event; the
//!   display of the active window is not told — macOS shows no window of
//!   another application without a permission Pane does not ask for — so
//!   that choice is explained, not offered. Moving a window is
//!   `setFrameTopLeftPoint` on the AppKit window behind the view
//!   ([`macos`]);
//! - Linux on X11: the monitors RandR lists, the pointer's position from
//!   the root window, and the active window's monitor from the window
//!   manager's `_NET_ACTIVE_WINDOW`; moving a window is a configure
//!   request ([`x11`]). Wayland places windows itself, so there the whole
//!   choice is explained rather than offered.
//!
//! The tests replace the adapter through [`init`], with a layout they
//! choose and a record of every move they were asked for, so the placement
//! is exercised at the same boundary a user sees it — the window's
//! opening — without owning the machine's real displays.
//!
//! [`windows`]: self::windows
//! [`macos`]: self::macos
//! [`x11`]: self::x11

use std::rc::Rc;

use gpui::{App, Global, Window};
use pane_core::placement::{DisplayLayout, Rect};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::WindowsDisplays;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::MacDisplays;

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use x11::X11Displays;

/// The placement seam: the display layout as the platform reports it, and
/// the move of the launcher window onto the display the choice resolved
/// to. The layout is in the platform's own screen units — physical pixels
/// on Windows and X11, points on macOS — the same units [`Placement::place`]
/// applies, so the two halves cannot disagree about a coordinate.
///
/// What the platform cannot tell is `None`, never a guess: the Launcher
/// page explains the choice whose answer is missing rather than offering
/// it, and the resolution falls back to the primary display. A platform
/// that cannot choose the launcher's display at all — Wayland, whose
/// compositor places windows itself — says so through
/// [`Placement::unavailable`], and the page offers nothing.
pub trait Placement {
    /// Why the launcher's display cannot be chosen on this platform, if it
    /// cannot be. The page explains this and offers no choices.
    fn unavailable(&self) -> Option<String>;

    /// The display layout as it is now. Reading it never fails: what the
    /// platform does not tell is `None` in the layout, and a platform that
    /// cannot list displays at all reports an empty one.
    fn layout(&self) -> DisplayLayout;

    /// Moves the launcher window so that its top left sits at `bounds`'s
    /// origin, in the layout's units, keeping the window's size. The
    /// Settings window never goes through here: only the launcher window
    /// is placed, wherever the user left the other one. `Err` names why
    /// the window could not be moved, which the launcher says rather than
    /// letting an opening that went nowhere pass as one that did. A
    /// window with no platform handle — GPUI's test platform — is not an
    /// error: there is no real window to move.
    fn place(&self, window: &mut Window, bounds: Rect) -> Result<(), String>;
}

/// The one placement, as the app's global. The first [`shared`] call makes
/// it the platform's own; [`init`] installs another, before anything reads
/// it, so a test drives the placement through its own.
struct Shared(Rc<dyn Placement>);

impl Global for Shared {}

/// Installs `placement` as the placement this app uses, before any window
/// reads it. The tests call this with their own; nothing else needs to,
/// since [`ensure`] falls back to the platform's.
pub fn init(placement: Rc<dyn Placement>, cx: &mut App) {
    cx.set_global(Shared(placement));
}

/// The placement every window places the launcher and explains the choices
/// through, ensuring it exists: the global [`init`] made, or — for a
/// window built without a startup step, as the tests are — the platform's
/// own. Each window's constructor calls this, as it does the host
/// settings' own ensure, so whatever reads the placement later finds one.
pub(crate) fn ensure(cx: &mut App) -> Rc<dyn Placement> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let native = native();
    cx.set_global(Shared(native.clone()));
    native
}

/// The placement, as the windows' renders and the pages' searches read it.
/// Requires a window's constructor to have called [`ensure`].
pub(crate) fn shared(cx: &App) -> Rc<dyn Placement> {
    cx.global::<Shared>().0.clone()
}

/// How many of the layout's units one of the window's logical pixels is:
/// physical pixels on Windows and X11 — the units the layout's displays
/// are measured in there, and the size a window keeps as it moves between
/// them — and AppKit points on macOS, where the layout is in points and a
/// window's logical size already is.
pub(crate) fn units_per_pixel(window: &Window) -> f32 {
    #[cfg(target_os = "macos")]
    {
        // The layout is in AppKit points, and a window's logical size
        // already is: one unit per pixel.
        let _ = window;
        1.
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.scale_factor()
    }
}

/// The placement of the platform Pane runs on.
fn native() -> Rc<dyn Placement> {
    #[cfg(target_os = "windows")]
    return Rc::new(WindowsDisplays);
    #[cfg(target_os = "macos")]
    return Rc::new(MacDisplays);
    #[cfg(target_os = "linux")]
    {
        // Wayland's compositor places windows itself; an application
        // cannot choose a display for one, so the choice is explained
        // rather than offered — as the hotkeys are there.
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty()) {
            return Rc::new(Unavailable(
                "Not available on Linux with Wayland: the compositor places windows itself, and \
                 Pane cannot choose the display the launcher opens on. Run Pane on X11, or move \
                 the launcher window yourself."
                    .into(),
            ));
        }
        match X11Displays::connect() {
            Ok(displays) => Rc::new(displays),
            Err(problem) => Rc::new(Unavailable(format!(
                "Not available: Pane could not reach the X display: {problem}"
            ))),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return Rc::new(Unavailable(format!(
        "Not available on {}: Pane cannot choose the display the launcher opens on.",
        std::env::consts::OS
    )));
}

/// The placement of a system that cannot choose the launcher's display at
/// all: the reason, an empty layout, and no move. Constructed on the
/// platforms whose placement can be refused — Linux without an X display,
/// or any system Pane has no placement for; the platforms whose adapters
/// answer always never make one.
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
struct Unavailable(String);

impl Placement for Unavailable {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn layout(&self) -> DisplayLayout {
        DisplayLayout::default()
    }

    fn place(&self, _window: &mut Window, _bounds: Rect) -> Result<(), String> {
        Err(self.0.clone())
    }
}
