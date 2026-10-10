//! The application in front on Windows (see the parent module): a
//! system foreground event hook — `SetWinEventHook` for
//! `EVENT_SYSTEM_FOREGROUND`, out of process (`WINEVENT_OUTOFCONTEXT`)
//! and cheap, delivered through the message queue of the thread that
//! installed it, whichever process the window belongs to — on a thread
//! of Pane's own (`threads::windows::MessageThread`), started when Pane
//! starts and ended when it ends. Each window that comes to the front
//! is classified by the pure rules beside this; the last one that is a
//! paste target is recorded, with its process, that process's program
//! path and package family — what the front application and the paste
//! read of it. Its title and its AppUserModelID are read when they are
//! asked for, since the window may be gone by then if it closed. The
//! reading of a window's class, ownership, styles, program path and
//! package family is shared with the Switch Windows host functions
//! (#263), which list the windows the same way.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use ::windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, HWND,
};
use ::windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use ::windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use ::windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WS_EX_TOOLWINDOW,
};
use ::windows::core::PWSTR;

use super::{WindowFacts, is_target};
use crate::threads::windows::MessageThread;

/// The watcher of the application in front: its thread's hook is called
/// for each window that comes to the front, and the last one that is a
/// paste target is what [`Watcher::target`] answers.
pub(in crate::system) struct Watcher {
    /// What the hook thread recorded, shared with whoever asks.
    target: Arc<Mutex<Option<Recorded>>>,
    thread: MessageThread,
}

/// A window the watcher recorded as the paste target.
#[derive(Clone)]
pub(in crate::system) struct Recorded {
    /// The window, as an address: a handle is not `Send`.
    pub(in crate::system) window: usize,
    /// The process that owns the window.
    pub(in crate::system) process: u32,
    /// The full path of that process's program, if it could be read.
    pub(in crate::system) program: Option<String>,
    /// That process's package family name, if it has one.
    pub(in crate::system) family: Option<String>,
}

impl Watcher {
    /// Starts the watcher: Pane's own thread, whose message loop the
    /// system calls the hook through. Nothing else is needed of it.
    pub(in crate::system) fn start() -> Result<Watcher, String> {
        let target: Arc<Mutex<Option<Recorded>>> = Arc::default();
        let recorded = target.clone();
        let thread = MessageThread::spawn(
            "pane-front",
            move || watch(recorded),
            // The watcher serves no requests on its thread; its stop
            // ends its loop.
            |_, _| {},
            |hook| {
                RECORDING.with(|recording| recording.borrow_mut().take());
                // SAFETY: `hook` is the hook this thread installed.
                let _ = unsafe { UnhookWinEvent(hook) };
            },
        )?;
        Ok(Watcher { target, thread })
    }

    /// The window in front last recorded as a paste target, with what
    /// the watcher read of it.
    pub(in crate::system) fn target(&self) -> Option<Recorded> {
        crate::util::lock(&self.target).clone()
    }
}

impl Drop for Watcher {
    /// Ends the watcher's thread, which uninstalls its hook as it ends.
    /// It never waits on another program, so this waits until it has.
    fn drop(&mut self) {
        self.thread.stop(None);
    }
}

// The hook thread's recording, set while it watches.
thread_local! {
    static RECORDING: RefCell<Option<Arc<Mutex<Option<Recorded>>>>> =
        const { RefCell::new(None) };
}

/// On the watcher's thread: installs the foreground hook, whose events
/// the thread's message loop delivers to [`foreground`] below.
fn watch(recording: Arc<Mutex<Option<Recorded>>>) -> Result<(HWINEVENTHOOK, Option<HWND>), String> {
    RECORDING.with(|recorded| *recorded.borrow_mut() = Some(recording));
    // SAFETY: no module to load the callback from, every process and
    // thread's events, out of process: the callback runs on this thread
    // as it pumps its messages, and never blocks the window that came to
    // the front.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        let why = ::windows::core::Error::from_thread().message();
        return Err(format!("Windows refused the foreground hook: {why}"));
    }
    Ok((hook, None))
}

/// The hook's callback, on the watcher's thread: called once for each
/// window that comes to the front.
unsafe extern "system" fn foreground(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _moment: u32,
) {
    // A panic must not unwind into Windows, which would end Pane.
    let _ = std::panic::catch_unwind(|| {
        if event == EVENT_SYSTEM_FOREGROUND
            && object == OBJID_WINDOW.0
            && child == 0
            && let Some(recorded) = recorded(window)
        {
            RECORDING.with(|recording| {
                if let Some(recording) = recording.borrow_mut().as_mut() {
                    *crate::util::lock(recording) = Some(recorded);
                }
            });
        }
    });
}

/// What the watcher reads of `window` if it is a paste target: its
/// process, that process's program path and package family. A window of
/// Pane's own, or one of the shell's surfaces, answers none.
fn recorded(window: HWND) -> Option<Recorded> {
    let mut process = 0u32;
    // SAFETY: `window` is a window handle; `process` is writable.
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut process)) };
    if process == 0 {
        return None;
    }
    let facts = WindowFacts {
        class: class_name(window),
        program: program_path(process),
        // SAFETY: no arguments.
        own: process == unsafe { GetCurrentProcessId() },
        owned: owned(window),
        tool: tool(window),
    };
    if !is_target(&facts) {
        return None;
    }
    Some(Recorded {
        window: window.0 as usize,
        process,
        program: facts.program,
        family: family(process),
    })
}

/// `window`'s class name, if it has one: read for the front
/// application's rules, and for the windows' (#263).
pub(crate) fn class_name(window: HWND) -> String {
    let mut name = vec![0u16; 256];
    // SAFETY: `window` is a window handle; `name` is writable for its
    // length.
    let read = unsafe { GetClassNameW(window, &mut name) };
    let end = read.clamp(0, name.len() as i32) as usize;
    String::from_utf16_lossy(&name[..end])
}

/// Whether another top-level window owns `window`.
pub(crate) fn owned(window: HWND) -> bool {
    // SAFETY: plain values; a window with no owner answers none.
    matches!(unsafe { GetWindow(window, GW_OWNER) }, Ok(owner) if !owner.is_invalid())
}

/// Whether `window` has the tool-window style.
pub(crate) fn tool(window: HWND) -> bool {
    // SAFETY: plain values.
    let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) };
    style & WS_EX_TOOLWINDOW.0 as isize != 0
}

/// The full path of the program of `process`, if it can be read: read
/// for the front application's resolution, and for the windows' (#263).
pub(crate) fn program_path(process: u32) -> Option<String> {
    // SAFETY: plain values; the handle is closed below.
    let handle: HANDLE =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut name = [0u16; 1024];
    let mut length = name.len() as u32;
    // SAFETY: `handle` is open; `name` is writable for `length` units.
    let queried = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(name.as_mut_ptr()),
            &mut length,
        )
    };
    // SAFETY: opened above.
    let _ = unsafe { CloseHandle(handle) };
    queried.ok()?;
    let path = String::from_utf16_lossy(&name[..length as usize]);
    (!path.trim().is_empty()).then_some(path)
}

/// The package family name of `process`, if it has one (a packaged
/// application's): read for the front application's resolution, and for
/// the windows' (#263).
pub(crate) fn family(process: u32) -> Option<String> {
    // SAFETY: plain values; the handle is closed below.
    let handle: HANDLE =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut length = 0u32;
    // SAFETY: `handle` is open; asking for the length passes no buffer.
    let asked = unsafe { GetPackageFamilyName(handle, &mut length, None) };
    if asked != ERROR_SUCCESS && asked != ERROR_INSUFFICIENT_BUFFER {
        // SAFETY: opened above.
        let _ = unsafe { CloseHandle(handle) };
        return None;
    }
    let mut name = vec![0u16; length as usize];
    let buffer = PWSTR(name.as_mut_ptr());
    // SAFETY: `handle` is open and closed below; `buffer` is writable for
    // `length` units.
    let read = unsafe { GetPackageFamilyName(handle, &mut length, Some(buffer)) };
    // SAFETY: opened above.
    let _ = unsafe { CloseHandle(handle) };
    let family = String::from_utf16_lossy(&name[..length as usize]);
    (read == ERROR_SUCCESS && !family.trim().is_empty()).then_some(family)
}
