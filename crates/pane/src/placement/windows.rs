//! The displays on Windows: the monitors the GDI API lists, each with its
//! whole bounds and its work area (where a window's controls stay
//! reachable, clear of the taskbar), the primary one, the pointer's
//! position from `GetCursorPos`, and the monitor of the foreground window
//! — the one the user is working in, whatever application owns it — from
//! `MonitorFromWindow`. The layout is in physical pixels, the units the
//! GDI APIs answer in, and [`super::Placement::place`] applies the same
//! units through `SetWindowPos` on the window's own handle: only the
//! origin is set, so a window keeps its size, frame and shadow as it
//! moves.

use gpui::Window;
use pane_core::placement::{Display, DisplayId, DisplayLayout, Point, Rect, Size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, MONITORINFOF_PRIMARY, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowPos,
};
use windows::core::BOOL;

use super::Placement;

/// The Windows placement: the monitors, the primary, the pointer and the
/// foreground window's monitor, read fresh each time the launcher opens.
pub(super) struct WindowsDisplays;

impl Placement for WindowsDisplays {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn layout(&self) -> DisplayLayout {
        let mut displays = Vec::new();
        let mut primary = None;
        // The monitors this session has, as the GDI API lists them.
        let monitors = monitors();
        for handle in monitors {
            let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            // SAFETY: `handle` is a monitor the enumeration above handed
            // us, and `info` is a `MONITORINFO` of the size its `cbSize`
            // names, as the API requires.
            let read = unsafe { GetMonitorInfoW(handle, &mut info as *mut MONITORINFO) };
            if !read.as_bool() {
                continue;
            }
            let id = DisplayId(handle.0 as u64);
            if info.dwFlags & MONITORINFOF_PRIMARY != 0 {
                primary = Some(id);
            }
            displays.push(Display {
                id,
                bounds: rect(&info.rcMonitor),
                usable: rect(&info.rcWork),
            });
        }
        DisplayLayout {
            displays,
            primary,
            pointer: pointer(),
            active: active(),
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
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Ok(());
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        // SAFETY: `hwnd` is this window, which GPUI created before the
        // handle was read, and the move keeps the window's size and
        // stacking order while asking for no activation of its own.
        let moved = unsafe {
            SetWindowPos(
                hwnd,
                None,
                bounds.origin.x as i32,
                bounds.origin.y as i32,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        moved.map_err(|error| format!("Pane could not move the launcher: {error}"))
    }
}

/// The monitors this session has. The enumeration is safe — the callback
/// only collects what it is handed.
fn monitors() -> Vec<HMONITOR> {
    let mut monitors: Vec<HMONITOR> = Vec::with_capacity(4);
    // SAFETY: the callback is `monitor_enum_proc`, which pushes the handle
    // it is given into the list `data` names, as the API's contract
    // requires.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut monitors as *mut _ as _),
        );
    }
    monitors
}

/// The enumeration's callback: the handle it is handed goes into the list
/// `data` names.
unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _place: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = data.0 as *mut Vec<HMONITOR>;
    // SAFETY: `data` is the list `monitors` passed as the enumeration's
    // own context, and the enumeration calls this callback on the same
    // thread, before `EnumDisplayMonitors` returns.
    unsafe { (*monitors).push(hmonitor) };
    BOOL(1)
}

/// The pointer's position in the layout's units (physical pixels), as the
/// system reports it; `None` when it does not.
fn pointer() -> Option<Point> {
    let mut point = POINT::default();
    // SAFETY: `point` is a `POINT` for the API to fill in.
    let read = unsafe { GetCursorPos(&mut point) };
    read.ok().map(|_| Point {
        x: point.x as f32,
        y: point.y as f32,
    })
}

/// The monitor of the foreground window — the window the user is working
/// in, whatever application owns it — matched against the layout's own
/// identities; `None` when there is no foreground window or no monitor
/// answers for it.
fn active() -> Option<DisplayId> {
    // SAFETY: `GetForegroundWindow` takes no arguments.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    // SAFETY: `window` is the handle `GetForegroundWindow` returned.
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    Some(DisplayId(monitor.0 as u64))
}

/// A GDI rectangle, in physical pixels, as the layout's own rectangle.
fn rect(place: &RECT) -> Rect {
    Rect {
        origin: Point {
            x: place.left as f32,
            y: place.top as f32,
        },
        size: Size {
            width: (place.right - place.left) as f32,
            height: (place.bottom - place.top) as f32,
        },
    }
}
