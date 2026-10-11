//! The taskbar on Windows: shown while the launcher is open by turning
//! the taskbar's own auto-hide off, and put back by turning it on again,
//! through the documented AppBar message (`SHAppBarMessage` with
//! `ABM_SETSTATE`) on the taskbar's own window (#268, ADR 0039). The
//! state the user's taskbar had is kept while Pane shows it — the
//! always-on-top flag with the auto-hide, both of them the user's, not
//! Pane's to choose — and put back exactly as it was. A taskbar that does
//! not hide itself is on screen already, and nothing is taken; a taskbar
//! that cannot be found (a shell that is restarting) is nothing shown,
//! and the next showing tries again.

use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::core::{PCWSTR, w};

use super::Taskbar;

/// The Windows taskbar: the state the user's own taskbar setting had
/// before Pane showed it, kept to put back; `None` while Pane shows
/// nothing.
pub(super) struct WindowsTaskbar {
    before: Mutex<Option<u32>>,
}

impl WindowsTaskbar {
    pub(super) fn new() -> WindowsTaskbar {
        WindowsTaskbar {
            before: Mutex::new(None),
        }
    }
}

impl Taskbar for WindowsTaskbar {
    fn show_while_open(&self) {
        let mut before = self
            .before
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if before.is_some() {
            // Pane already shows it: nothing changes.
            return;
        }
        let Some(state) = state() else {
            return;
        };
        if state & ABS_AUTOHIDE == 0 {
            // The user's taskbar does not hide itself: it is on screen
            // already, and nothing is taken to put back.
            return;
        }
        // Auto-hide off, everything else as it was, so the taskbar stays
        // on screen while the launcher is open.
        if set(state & !ABS_AUTOHIDE) {
            *before = Some(state);
        }
    }

    fn restore(&self) {
        let mut before = self
            .before
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(state) = before.take() {
            set(state);
        }
    }
}

/// The taskbar's own state, as its AppBar answers: the auto-hide and
/// always-on-top flags. `None` where the taskbar's window cannot be
/// found.
fn state() -> Option<u32> {
    let taskbar = window()?;
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: taskbar,
        ..APPBARDATA::default()
    };
    // SAFETY: `data` is an `APPBARDATA` of the size its `cbSize` names,
    // as the API requires, holding the taskbar's own window.
    Some(unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) } as u32)
}

/// Sets the taskbar's state to `state`, answering whether it took. A
/// refusal is reported as a diagnostic, not raised: the choice is a
/// nicety beside the hotkeys, and the next opening tries again.
fn set(state: u32) -> bool {
    let Some(taskbar) = window() else {
        return false;
    };
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: taskbar,
        lParam: LPARAM(state as isize),
        ..APPBARDATA::default()
    };
    // SAFETY: as `state`'s own call, with the new state in `lParam`.
    let applied = unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
    if applied == 0 {
        pane_core::diagnostic!("Windows refused the taskbar's state change");
    }
    applied != 0
}

/// The taskbar's own window, `Shell_TrayWnd`; `None` where it cannot be
/// found (a shell that is restarting, or not running).
fn window() -> Option<HWND> {
    match unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) } {
        Ok(taskbar) => Some(taskbar),
        Err(why) => {
            pane_core::diagnostic!("Pane could not find the taskbar to show it: {why}");
            None
        }
    }
}
