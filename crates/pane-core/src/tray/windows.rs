//! The tray entry on Windows: an icon in the notification area
//! (`Shell_NotifyIcon`), owned by a hidden window of a thread of Pane's
//! own whose message loop receives the shell's callback messages. A
//! right click on the icon opens the menu — Open Pane, Settings, Exit,
//! the platform's own labels — and a left click summons the launcher,
//! as the tray's primary action convention is. No permission is needed.
//!
//! Showing and hiding are done by that thread, as registering a hotkey
//! is: the caller queues the request, wakes the thread with a thread
//! message and waits for its answer, because the icon and its menu
//! belong to the thread that made them. Dropping the adapter ends the
//! thread, which deletes the icon and destroys its menu as it ends — so
//! the preference's every change, and the quit path, leave no icon
//! behind.
//!
//! The icon is the one the packaging embeds in Pane's own program (the
//! same resource the window icon loads); a build without one falls back
//! to the system's application icon, honestly plain — the native
//! validation records which one a run saw.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use ::windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFY_ICON_DATA_FLAGS, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, GDI_IMAGE_TYPE, GetCursorPos, HICON,
    HMENU, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED, LoadIconW, LoadImageW,
    MENU_ITEM_FLAGS, MF_STRING, MSG, PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP,
};
use ::windows::core::PCWSTR;

use super::{SelectionSender, Tray, TrayAction, TrayError};
use crate::threads::windows::{MessageThread, WM_WAKE, Window, WindowClass, stop_sent};

/// The icon's id, both in `Shell_NotifyIcon`'s terms and as the menu
/// window's.
const ICON_ID: u32 = 1;
/// The message the shell sends to the owning window when the icon is
/// used: in the `WM_APP` range, clear of the threads module's wake and
/// stop.
const TRAY_MESSAGE: u32 = 0x8000 + 3;
/// The menu's commands, as `TrackPopupMenuEx` reports them.
const OPEN_PANE: usize = 1;
const SETTINGS: usize = 2;
const EXIT: usize = 3;

/// The window class of the tray icon's owner: a hidden top-level window,
/// made by [`WindowClass::hidden_window`], because the shell addresses
/// the owner and its menus need a real window to belong to.
static TRAY_CLASS: WindowClass = WindowClass::new("PaneTray", procedure);

/// A change to apply, with where to answer it.
enum Request {
    Show(mpsc::Sender<Result<(), TrayError>>),
    Hide(mpsc::Sender<Result<(), TrayError>>),
}

type Requests = Arc<Mutex<VecDeque<Request>>>;

/// The adapter: the thread that owns the icon and its menu.
pub struct WindowsTray {
    thread: MessageThread,
    requests: Requests,
}

/// What the tray thread owns: the icon's window and menu, and where the
/// menu's selections go. Held in a thread-local the window procedure
/// reads, as the clipboard listener's is, on that thread only.
struct Entry {
    selections: SelectionSender,
    /// The icon's owner; destroyed when the entry is.
    window: Window,
    /// The menu a right click opens; destroyed with the entry.
    menu: HMENU,
    /// The icon as the notification area holds it.
    icon: HICON,
    /// Whether the icon is in the notification area now.
    shown: bool,
}

thread_local! {
    static ENTRY: std::cell::RefCell<Option<Entry>> = const { std::cell::RefCell::new(None) };
}

/// A NUL-terminated wide copy of `text`, as the shell's APIs take it.
fn wide(text: &str) -> Vec<u16> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    wide
}

/// The icon of Pane's own program, as the window icon loads it; a build
/// without an embedded icon falls back to the system's application icon.
/// Called on the tray thread.
fn icon() -> HICON {
    use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: no arguments; the module is this process's executable.
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) };
    if let Ok(module) = module
        // SAFETY: the module is this process's; the id names the icon it
        // embeds, and the shared load keeps the system's cache of it.
        && let Ok(image) = unsafe {
            LoadImageW(
                Some(module.into()),
                PCWSTR(ICON_ID as _),
                GDI_IMAGE_TYPE(IMAGE_ICON.0),
                0,
                0,
                LR_DEFAULTSIZE | LR_SHARED,
            )
        }
    {
        return HICON(image.0);
    }
    // SAFETY: the shared system icon, which no one owns.
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

/// The tray's menu, with the platform's own labels. Called on the tray
/// thread.
fn menu() -> Result<HMENU, String> {
    // SAFETY: a new menu of this thread.
    let menu = unsafe { CreatePopupMenu() }.map_err(|error| error.message())?;
    for (command, label) in [
        (OPEN_PANE, "Open Pane"),
        (SETTINGS, "Settings"),
        (EXIT, "Exit"),
    ] {
        let label = wide(label);
        // SAFETY: the menu this thread made; the label is fully
        // initialized for the call.
        unsafe {
            AppendMenuW(
                menu,
                MENU_ITEM_FLAGS(MF_STRING.0),
                command,
                PCWSTR(label.as_ptr()),
            )
        }
        .map_err(|error| error.message())?;
    }
    Ok(menu)
}

/// Adds or deletes the icon in the notification area. Called on the tray
/// thread, with the entry the thread holds.
fn shell_notify(entry: &mut Entry, shown: bool) -> Result<(), TrayError> {
    if entry.shown == shown {
        // Repeating the state in effect succeeds, as the trait says.
        return Ok(());
    }
    let mut tip: [u16; 128] = [0; 128];
    let pane = wide("Pane");
    tip[..pane.len()].copy_from_slice(&pane);
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: entry.window.handle(),
        uID: ICON_ID,
        uFlags: NOTIFY_ICON_DATA_FLAGS(NIF_MESSAGE.0 | NIF_ICON.0 | NIF_TIP.0),
        uCallbackMessage: TRAY_MESSAGE,
        hIcon: entry.icon,
        szTip: tip,
        ..NOTIFYICONDATAW::default()
    };
    let message = if shown { NIM_ADD } else { NIM_DELETE };
    // SAFETY: `data` is fully initialized; the icon is this thread's own.
    let done = unsafe { Shell_NotifyIconW(message, &data) };
    if !done.as_bool() {
        // It reads the calling thread's last error, which the failed call
        // just left there.
        let why = ::windows::core::Error::from_thread().message();
        return Err(TrayError::Refused(why));
    }
    entry.shown = shown;
    Ok(())
}

/// Opens the entry's menu at the pointer and reports the command chosen
/// (0 when the menu was dismissed). Called on the tray thread, from the
/// window procedure.
fn open_menu(entry: &mut Entry) -> usize {
    let mut where_clicked = POINT::default();
    // SAFETY: writes this thread's own point. A failure leaves the
    // origin, which is as good a place as any the menu can open at when
    // no pointer position is there (no pointer attached).
    let _ = unsafe { GetCursorPos(&mut where_clicked) };
    // The two calls the platform's tray menus need around them: the
    // owning window takes the foreground so the menu dismisses when the
    // user clicks elsewhere, and a no-op message follows the menu so the
    // taskbar gives the foreground back.
    // SAFETY: this thread's own window. The platform's answer is not
    // ours to act on: a refusal leaves the menu to dismiss on its own.
    let _ = unsafe { SetForegroundWindow(entry.window.handle()) };
    let flags = TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0 | TPM_BOTTOMALIGN.0;
    // SAFETY: this thread's own menu and window; with `TPM_RETURNCMD`
    // the return value is the command chosen, not only whether it ran.
    let chosen = unsafe {
        TrackPopupMenuEx(
            entry.menu,
            flags,
            where_clicked.x,
            where_clicked.y,
            entry.window.handle(),
            None,
        )
    };
    // SAFETY: this thread's own window; a no-op message.
    unsafe { PostMessageW(Some(entry.window.handle()), WM_NULL, WPARAM(0), LPARAM(0)) }.ok();
    chosen.0 as usize
}

/// The tray window's procedure: the shell's callback messages for the
/// icon, and the stop message. A panic must not unwind into Windows,
/// which would end Pane.
extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = std::panic::catch_unwind(|| handle(window, message, wparam, lparam));
    match handled {
        Ok(result) => result,
        Err(_) => {
            log("Pane's tray entry failed on a click; it goes on listening");
            LRESULT(0)
        }
    }
}

fn handle(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if stop_sent(message) {
        return LRESULT(0);
    }
    if message == TRAY_MESSAGE {
        // With the icon's version 0 semantics, `lParam` is the mouse
        // message and `wParam` the icon's id.
        let mouse = (lparam.0 & 0xFFFF) as u32;
        ENTRY.with(|entry| {
            // Not borrowed already: nothing a click waits on calls back
            // into it.
            let Ok(mut held) = entry.try_borrow_mut() else {
                return;
            };
            let Some(entry) = held.as_mut() else {
                return;
            };
            let action = match mouse {
                WM_LBUTTONUP => Some(TrayAction::OpenPane),
                WM_RBUTTONUP | WM_CONTEXTMENU => match open_menu(entry) {
                    OPEN_PANE => Some(TrayAction::OpenPane),
                    SETTINGS => Some(TrayAction::Settings),
                    EXIT => Some(TrayAction::Quit),
                    _ => None,
                },
                _ => None,
            };
            if let Some(action) = action {
                entry.selections.send(action);
            }
        });
        return LRESULT(0);
    }
    // SAFETY: the arguments are those this procedure was called with.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Writes `message` to standard error, if there is one. Unlike
/// `eprintln!`, it cannot panic.
fn log(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
}

impl WindowsTray {
    /// Starts the tray thread: its window, the icon's owner, and the
    /// menu a right click opens. Returns once they are made, with the
    /// reason they could not be.
    pub fn start(selections: SelectionSender) -> Result<WindowsTray, String> {
        let requests: Requests = Arc::default();
        let served = requests.clone();
        let thread = MessageThread::spawn(
            "pane-tray",
            move || {
                let window = TRAY_CLASS.hidden_window()?;
                let menu = menu()?;
                let icon = icon();
                let owner = window.handle();
                ENTRY.with(|entry| {
                    entry.replace(Some(Entry {
                        selections,
                        window,
                        menu,
                        icon,
                        shown: false,
                    }))
                });
                Ok(((), Some(owner)))
            },
            move |_, message| serve(message, &served),
            |()| finish(),
        )?;
        Ok(WindowsTray { thread, requests })
    }

    /// Queues `request` and wakes the thread; false if it is gone.
    fn send(&self, request: Request) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(request);
        self.thread.post(WM_WAKE)
    }
}

/// Serves the queued show and hide requests. Runs on the tray thread.
fn serve(message: &MSG, requests: &Requests) {
    if message.message != WM_WAKE {
        return;
    }
    loop {
        let request = requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front();
        match request {
            None => break,
            Some(Request::Show(answer)) => {
                let _ = answer.send(change(true));
            }
            Some(Request::Hide(answer)) => {
                let _ = answer.send(change(false));
            }
        }
    }
}

/// Applies `shown` to the entry this thread holds. Runs on the tray
/// thread.
fn change(shown: bool) -> Result<(), TrayError> {
    ENTRY.with(|entry| {
        let Ok(mut held) = entry.try_borrow_mut() else {
            return Err(TrayError::Refused(
                "the tray entry was busy on a click".to_string(),
            ));
        };
        match held.as_mut() {
            Some(entry) => shell_notify(entry, shown),
            None => Err(TrayError::Refused(
                "the tray entry was not made".to_string(),
            )),
        }
    })
}

/// Deletes the icon, destroys the menu and the window that owned them.
/// Runs on the tray thread, as it ends.
fn finish() {
    ENTRY.with(|entry| {
        if let Ok(mut held) = entry.try_borrow_mut()
            && let Some(mut entry) = held.take()
        {
            // The icon goes first, while the window it belongs to is
            // still there for the shell to find.
            let _ = shell_notify(&mut entry, false);
            // SAFETY: this thread's own menu.
            unsafe { DestroyMenu(entry.menu) }.ok();
            // The window goes with the entry: its own drop destroys it,
            // on this thread.
        }
    });
}

impl Tray for WindowsTray {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        let (answer, answered) = mpsc::channel();
        let request = if visible {
            Request::Show(answer)
        } else {
            Request::Hide(answer)
        };
        if !self.send(request) {
            return Err(TrayError::Refused("the tray thread stopped".into()));
        }
        answered
            .recv()
            .unwrap_or_else(|_| Err(TrayError::Refused("the tray thread stopped".into())))
    }
}

impl Drop for WindowsTray {
    /// Ends the thread, which deletes the icon and destroys the menu and
    /// the window as it ends. It never waits on another program, so this
    /// waits until it has.
    fn drop(&mut self) {
        self.thread.stop(None);
    }
}
