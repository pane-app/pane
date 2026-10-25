//! A press on the Settings window's titlebar, where the platform's own
//! titlebar is hidden (#541): a double click maximizes the window and a
//! second restores it, as Windows and Raycast do, and a drag still moves
//! it.
//!
//! - Windows: the titlebar's drag region answers the system's hit test
//!   with the caption (GPUI's `WindowControlArea::Drag`, `HTCAPTION`), so
//!   the double click, the drag, Aero Snap and the system menu are the
//!   system's own. One thing is held back: the system's move begins at
//!   the press, and while it runs it takes the mouse's messages for
//!   itself. It could take the release and the second press of a double
//!   click too (seen now and then with real input on CI's Windows
//!   runner, and every time on the Windows 11 desktop #541 was found
//!   on): the window never saw
//!   the double click, and nothing happened. So the press does not reach
//!   the system; the move begins once the pointer has moved
//!   past the distance the system's own drag detection uses, with the
//!   button still held, and a double click — a press
//!   the system marks as the second of two — goes on to the system,
//!   which maximizes or restores the window.
//! - macOS: AppKit leaves a titlebar it does not draw to the view under
//!   it, so the double click is asked of the system here
//!   (`titlebar_double_click`), which does what "Double-click a window's
//!   title bar to" says: zoom, minimize or nothing. The drag is AppKit's.
//!
//! Linux keeps the window manager's frame, which has no drag region of
//! Pane's, so the module is not built there.

use gpui::{App, MouseDownEvent, Window};

/// Called for a left press on the titlebar's drag region (see the
/// module's notes).
pub(crate) fn pressed(event: &MouseDownEvent, window: &mut Window, cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        let _ = cx;
        if event.click_count == 2 {
            window.titlebar_double_click();
        }
    }
    #[cfg(target_os = "windows")]
    windows::pressed(event, window, cx);
}

#[cfg(target_os = "windows")]
mod windows {
    use std::time::Duration;

    use gpui::{App, MouseDownEvent, Window};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetKeyState, ReleaseCapture, VK_LBUTTON, VK_RBUTTON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetMessagePos, GetSystemMetrics, HTCAPTION, PostMessageW, SC_MOVE, SM_CXDRAG,
        SM_CYDRAG, SM_SWAPBUTTON, WM_SYSCOMMAND,
    };

    /// How often a held press looks at the pointer: about a frame.
    const LOOK: Duration = Duration::from_millis(10);

    /// A window's native handle, kept as a plain value a task can hold.
    #[derive(Clone, Copy)]
    struct Hwnd(HWND);

    pub(super) fn pressed(event: &MouseDownEvent, window: &mut Window, cx: &mut App) {
        // The second press of a double click (and the fourth, of a second
        // double click) is the system's: it maximizes or restores the window.
        if event.click_count.is_multiple_of(2) {
            return;
        }
        // GPUI's test platform's windows have no native handle; nothing
        // is held back there.
        let Some(hwnd) = hwnd(window) else {
            return;
        };
        // The press stops here, so the system's move does not begin with
        // it (see the module's notes).
        cx.stop_propagation();
        let from = press_point();
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            while held() {
                if moved_past_drag_distance(from) {
                    begin_move(hwnd, from);
                    return;
                }
                cx.background_executor().timer(LOOK).await;
            }
        })
        .detach();
    }

    /// The window's native handle, if it has one.
    fn hwnd(window: &Window) -> Option<Hwnd> {
        // `Window` also has an inherent `window_handle` (GPUI's own
        // identifier), so the raw-window-handle trait is named.
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return None;
        };
        Some(Hwnd(HWND(handle.hwnd.get() as *mut _)))
    }

    /// Where the pointer was at the press: the message being handled is
    /// the press's own.
    fn press_point() -> POINT {
        // SAFETY: no arguments; only reads this thread's last message.
        let at = unsafe { GetMessagePos() };
        POINT {
            x: (at & 0xFFFF) as i16 as i32,
            y: (at >> 16) as i16 as i32,
        }
    }

    /// Whether the press is still held: this thread has handled no
    /// release since it, and the button is down now. The second keeps a
    /// release over another window, which this thread never sees, from
    /// leaving the press held. The physical button is the one the
    /// system's swap of the buttons makes the primary.
    fn held() -> bool {
        // SAFETY: plain reads of the keyboard and mouse state and the
        // system's metrics.
        unsafe {
            let physical = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
                VK_RBUTTON
            } else {
                VK_LBUTTON
            };
            GetKeyState(VK_LBUTTON.0 as i32) < 0 && GetAsyncKeyState(physical.0 as i32) < 0
        }
    }

    /// Whether the pointer is further from `from` than the system's drag
    /// distance, the one its own drag detection uses.
    fn moved_past_drag_distance(from: POINT) -> bool {
        let mut now = POINT::default();
        // SAFETY: `now` is a live point to write; the rest only read the
        // system's metrics.
        unsafe {
            if GetCursorPos(&mut now).is_err() {
                return false;
            }
            (now.x - from.x).abs() > GetSystemMetrics(SM_CXDRAG) / 2
                || (now.y - from.y).abs() > GetSystemMetrics(SM_CYDRAG) / 2
        }
    }

    /// Begins the system's move of the window by its caption, from the
    /// press's point, as the press itself would have: posted, so the
    /// move's own message loop runs outside GPUI's update.
    fn begin_move(hwnd: Hwnd, from: POINT) {
        let point = LPARAM((((from.y as u16) as isize) << 16) | (from.x as u16) as isize);
        // SAFETY: `hwnd` is the Settings window's; a release of a capture
        // this thread may not hold does nothing, and a post to a window
        // closed meanwhile fails harmlessly.
        unsafe {
            let _ = ReleaseCapture();
            let _ = PostMessageW(
                Some(hwnd.0),
                WM_SYSCOMMAND,
                WPARAM((SC_MOVE | HTCAPTION) as usize),
                point,
            );
        }
    }
}
