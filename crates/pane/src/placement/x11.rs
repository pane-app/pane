//! The displays on Linux under X11: the monitors RandR lists, each with
//! its bounds (the whole monitor: X11 has no per-monitor work area, so the
//! window manager's panels are the placement's honest limit, recorded
//! natively), the primary one, the pointer's position from a query on the
//! root window, and the monitor of the active window, as the window
//! manager's EWMH `_NET_ACTIVE_WINDOW` names it — `None` under a window
//! manager that does not keep that property, which the page explains
//! rather than guessing. The layout is in physical pixels, the units the
//! X server answers in, and [`super::Placement::place`] applies the same
//! units with a configure request on the window's own X window.

use gpui::Window;
use pane_core::placement::{Display, DisplayId, DisplayLayout, Point, Rect, Size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::randr::{ConnectionExt as _, MonitorInfo};
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, Window as XWindow,
};
use x11rb::rust_connection::RustConnection;

use super::Placement;

/// The X11 placement over one display connection.
pub(super) struct X11Displays {
    connection: RustConnection,
    /// The root window of the default screen, whose monitors are the
    /// displays and whose properties name the active window.
    root: XWindow,
}

impl X11Displays {
    /// Connects to the display the environment names, taking the default
    /// screen's root as the placement's root window. `Err` names why the
    /// display could not be reached, which the page explains.
    pub(super) fn connect() -> Result<X11Displays, String> {
        let (connection, screen) =
            RustConnection::connect(None).map_err(|error| error.to_string())?;
        let root = connection.setup().roots[screen].root;
        Ok(X11Displays { connection, root })
    }
}

impl Placement for X11Displays {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn layout(&self) -> DisplayLayout {
        // What this read cannot answer is simply absent, as the layout's
        // own contract says: the monitors, the primary, the pointer and
        // the active window, one read, in one coordinate space.
        let monitors = self
            .connection
            .randr_get_monitors(self.root, true)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.monitors)
            .unwrap_or_default();
        let displays: Vec<Display> = monitors
            .iter()
            .enumerate()
            .map(|(index, monitor)| display(index as u64, monitor))
            .collect();
        // The identity is the monitor's place in this list: it only has to
        // match within the one layout the choice resolves against, which
        // the same read built.
        let primary = monitors
            .iter()
            .position(|monitor| monitor.primary)
            .map(|index| DisplayId(index as u64));
        DisplayLayout {
            pointer: self.pointer(),
            active: self.active(&monitors),
            displays,
            primary,
        }
    }

    fn place(&self, window: &mut Window, bounds: Rect) -> Result<(), String> {
        // `Window` also has an inherent `window_handle` (GPUI's own
        // identifier), so the raw-window-handle trait is named rather than
        // called through the receiver.
        let handle = match HasWindowHandle::window_handle(window) {
            Ok(handle) => handle,
            Err(_) => {
                // No platform window exists to move — GPUI's test
                // platform; the placement's decision is what its tests
                // observe.
                return Ok(());
            }
        };
        let RawWindowHandle::Xcb(handle) = handle.as_raw() else {
            return Ok(());
        };
        // A configure request: plain X11, on the window id GPUI's own
        // handle names.
        self.connection
            .configure_window(
                handle.window.get(),
                &ConfigureWindowAux::new()
                    .x(bounds.origin.x as i32)
                    .y(bounds.origin.y as i32),
            )
            .map_err(|error| format!("Pane could not move the launcher: {error}"))?;
        Ok(())
    }
}

impl X11Displays {
    /// The pointer's position in the layout's units (physical pixels), as
    /// the server reports it for the root window; `None` when it does not
    /// answer.
    fn pointer(&self) -> Option<Point> {
        let reply = self
            .connection
            .query_pointer(self.root)
            .ok()?
            .reply()
            .ok()?;
        Some(Point {
            x: reply.root_x as f32,
            y: reply.root_y as f32,
        })
    }

    /// The monitor of the active window, as the window manager's
    /// `_NET_ACTIVE_WINDOW` property names it, placed by its origin;
    /// `None` when no window manager property answers, which the page
    /// explains rather than guessing.
    fn active(&self, monitors: &[MonitorInfo]) -> Option<DisplayId> {
        let name = self
            .connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let reply = self
            .connection
            .get_property(false, self.root, name, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        // The property holds one window id, as four bytes.
        let bytes: [u8; 4] = reply.value.get(0..4)?.try_into().ok()?;
        let active = u32::from_le_bytes(bytes);
        // Where the active window sits in root coordinates: its own
        // origin, which is enough to name the monitor it is on.
        let translated = self
            .connection
            .translate_coordinates(active, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        monitors
            .iter()
            .position(|monitor| {
                translated.dst_x >= monitor.x
                    && translated.dst_y >= monitor.y
                    && translated.dst_x < monitor.x + monitor.width as i16
                    && translated.dst_y < monitor.y + monitor.height as i16
            })
            .map(|index| DisplayId(index as u64))
    }
}

/// One RandR monitor, as the layout's own display: its place in the list
/// is its identity, its whole bounds its usable area (X11 names no
/// per-monitor work area).
fn display(id: u64, monitor: &MonitorInfo) -> Display {
    let bounds = Rect {
        origin: Point {
            x: monitor.x as f32,
            y: monitor.y as f32,
        },
        size: Size {
            width: monitor.width as f32,
            height: monitor.height as f32,
        },
    };
    Display {
        id: DisplayId(id),
        bounds,
        usable: bounds,
    }
}
