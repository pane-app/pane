//! Bringing the launcher's window to the front, and keeping Windows'
//! system menu off it (#467).
//!
//! - Windows: the Open Pane hotkey's summon brings the launcher forward
//!   with the system's own calls rather than GPUI's `activate_window`,
//!   which injects a press and release of Alt to earn the right to take
//!   the foreground. That Alt reached whatever window was in front as it
//!   was processed — the application behind Pane, or the launcher itself
//!   once it was in front — and raced the call it was meant to permit:
//!   the launcher could show without the focus, so the next press brought
//!   it forward instead of hiding it. Here the launcher takes the
//!   foreground plainly, which Windows allows the process that received
//!   the hotkey, and otherwise by attaching to the input of the thread in
//!   front for the call, the way window-switching tools do; GPUI's way is
//!   kept only as the last resort.
//! - Windows: the launcher never opens its system menu (Restore, Move,
//!   Size, Minimize, Maximize, Close). With Alt+Space as the Open Pane
//!   hotkey, a Space still held as the launcher came forward reached it
//!   as Alt+Space, the system menu's own shortcut, and the arrows then
//!   moved through that menu instead of Pane's list. The window's
//!   procedure is wrapped so the keyboard's menu command (`SC_KEYMENU`)
//!   and the menu characters that lead to it (`WM_SYSCHAR`) end there;
//!   Alt+F4 is a different command (`SC_CLOSE`) and is let through.
//! - Windows: whether another application holds the focus the launcher
//!   lost (#571), which hides it, is the system's foreground window's
//!   process: the taskbar and the desktop are Explorer's, while the
//!   Settings window and a file dialog are Pane's own.
//! - Elsewhere GPUI's activation is used as it is: macOS's and X11's do
//!   not inject keys, and neither system has a menu of the kind.

use gpui::{App, Window};

/// Brings `window`, the launcher's, forward with the focus: on Windows
/// with the system's own calls, from a task of its own so the window's
/// activation messages reach GPUI outside this update, and only if
/// `wanted` still says so when the task runs (a press that hid the
/// launcher meanwhile wins), then `done`, the calls made; elsewhere
/// through GPUI, at once, and `done` is not called — the window's
/// activation, when it comes, says the same there.
pub(crate) fn bring_forward<V: gpui::Render>(
    window: &mut Window,
    cx: &mut App,
    wanted: impl FnOnce(&V) -> bool + 'static,
    done: impl FnOnce(&mut V) + 'static,
) {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = windows::hwnd(window)
        && let Some(handle) = window.window_handle().downcast::<V>()
    {
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            let wanted = handle
                .read_with(cx, |view, _| wanted(view))
                .unwrap_or(false);
            if wanted && !windows::take_foreground(hwnd.0) {
                // The system's calls were refused (an elevated window in
                // front, say): GPUI's own way is the last resort.
                let _ = handle.update(cx, |_, window, _| window.activate_window());
            }
            let _ = handle.update(cx, |view, _, _| done(view));
        })
        .detach();
        return;
    }
    let _ = (wanted, done);
    window.activate_window();
    cx.activate(true);
}

/// Whether `window` is the one in front with the keyboard: on Windows the
/// system's foreground window, which is what the user sees in front
/// whatever this thread's own active window says after an attached call
/// (#467); elsewhere GPUI's activation.
pub(crate) fn in_front(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = windows::hwnd(window) {
        return windows::in_front(hwnd.0);
    }
    window.is_window_active()
}

/// Whether another application holds the focus `window`, the launcher's,
/// lost (#571): on Windows, whether the system's foreground window is
/// another process's (the taskbar's, the desktop's, any application's) or
/// there is none, rather than one of Pane's own (the Settings window, a
/// file dialog Pane opened). Elsewhere, and for a window without a native
/// handle (GPUI's test platform's), no: the launcher stays shown there
/// when it loses the focus.
pub(crate) fn another_application_in_front(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    if windows::hwnd(window).is_some() {
        return windows::another_application_in_front();
    }
    let _ = window;
    false
}

/// Keeps Windows' system menu off `window`, the launcher's (see the
/// module's notes). Called once, as the window opens; on GPUI's test
/// platform and on the other systems it does nothing.
pub(crate) fn keep_system_menu_closed(window: &Window) {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = windows::hwnd(window) {
        windows::keep_system_menu_closed(hwnd.0);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(target_os = "windows")]
mod windows {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsHungAppWindow, IsIconic,
        IsWindowVisible, SC_KEYMENU, SW_RESTORE, SW_SHOWNA, SetForegroundWindow, ShowWindow,
        WM_SYSCHAR, WM_SYSCOMMAND,
    };

    /// A window's native handle, kept as a plain value a task can hold.
    #[derive(Clone, Copy)]
    pub(super) struct Hwnd(pub(super) HWND);

    /// The window's native handle, if it has one (GPUI's test platform's
    /// windows have none).
    pub(super) fn hwnd(window: &Window) -> Option<Hwnd> {
        // `Window` also has an inherent `window_handle` (GPUI's own
        // identifier), so the raw-window-handle trait is named.
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return None;
        };
        Some(Hwnd(HWND(handle.hwnd.get() as *mut _)))
    }

    /// Makes `hwnd` the window in front, shown and restored, and says
    /// whether it is: the plain call first, which Windows grants the
    /// process that received the hotkey or has the foreground already,
    /// then the same call with this thread attached to the input of the
    /// thread in front, unless that thread is not responding.
    pub(super) fn take_foreground(hwnd: HWND) -> bool {
        // SAFETY: `hwnd` is the launcher's live window, on this thread;
        // each call only reads or changes its show state and the
        // foreground, and the input attachment is undone before return.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            }
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            if GetForegroundWindow() != hwnd && !SetForegroundWindow(hwnd).as_bool() {
                attached(hwnd);
            }
            let front = GetForegroundWindow() == hwnd;
            if front {
                // This thread's own idea of its active window and focus,
                // which GPUI reads, follows: after an attached call the
                // thread in front may still hold them.
                let _ = SetActiveWindow(hwnd);
                let _ = SetFocus(Some(hwnd));
            }
            front
        }
    }

    /// The same call, with this thread attached to the input of the
    /// thread in front, unless that thread is not responding.
    unsafe fn attached(hwnd: HWND) {
        // SAFETY: as in `take_foreground`; the attachment is undone before
        // return.
        unsafe {
            let front = GetForegroundWindow();
            if front.is_invalid() || IsHungAppWindow(front).as_bool() {
                return;
            }
            let theirs = GetWindowThreadProcessId(front, None);
            let ours = GetCurrentThreadId();
            if theirs == 0 || theirs == ours {
                return;
            }
            if !AttachThreadInput(ours, theirs, true).as_bool() {
                return;
            }
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(ours, theirs, false);
        }
    }

    /// Whether `hwnd` is the window in front.
    pub(super) fn in_front(hwnd: HWND) -> bool {
        // SAFETY: no arguments; the handle is only compared.
        unsafe { GetForegroundWindow() == hwnd }
    }

    /// Whether the window in front is another process's, or there is none.
    pub(super) fn another_application_in_front() -> bool {
        // SAFETY: no arguments but the foreground window's handle, which
        // is only asked which process made it.
        unsafe {
            let front = GetForegroundWindow();
            if front.is_invalid() {
                return true;
            }
            let mut process = 0;
            GetWindowThreadProcessId(front, Some(&mut process));
            process != GetCurrentProcessId()
        }
    }

    /// The subclass's identifier on the launcher's window.
    const NO_SYSTEM_MENU: usize = 0x5061_6e65;

    /// Wraps `hwnd`'s procedure with [`no_system_menu`].
    pub(super) fn keep_system_menu_closed(hwnd: HWND) {
        // SAFETY: `hwnd` is the launcher's window, created on this thread,
        // which is the one a subclass must be set from; the procedure is
        // a plain function that lives as long as the program.
        let set = unsafe { SetWindowSubclass(hwnd, Some(no_system_menu), NO_SYSTEM_MENU, 0) };
        if !set.as_bool() {
            pane_core::diagnostic!(
                "Pane could not keep the launcher's system menu closed: Alt+Space may open it"
            );
        }
    }

    /// The launcher's procedure in front of GPUI's: the keyboard's menu
    /// command and the menu characters end here, so the system menu never
    /// opens and Alt alone never puts the window into menu mode;
    /// everything else, Alt+F4's close among it, goes on to GPUI.
    unsafe extern "system" fn no_system_menu(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        let menu = match message {
            WM_SYSCHAR => true,
            WM_SYSCOMMAND => (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU,
            _ => false,
        };
        if menu {
            return LRESULT(0);
        }
        // SAFETY: the arguments are those this procedure was called with,
        // passed on to the next procedure in the window's chain.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}
