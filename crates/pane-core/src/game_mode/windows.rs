//! The foreground source on Windows (see the parent module): a
//! `SetWinEventHook` consumer of the system's own foreground event —
//! out of process (`WINEVENT_OUTOFCONTEXT`) and cheap, delivered
//! through the message queue of the thread that installed it, whichever
//! process the window belongs to — on a thread of Pane's own
//! (`threads::windows::MessageThread`), started when the source is made
//! and ended when it is dropped. This mirrors the front application
//! watcher (#253) rather than sharing it: a second hook of Pane's own
//! keeps that watcher's behavior untouched, and this one needs
//! different facts anyway — every window that comes to the front, not
//! only the paste targets, and the shell's notification state, which
//! says a full-screen Direct3D application is in front. Each window is
//! reported to what [`ForegroundSource::watch`] was given, with the
//! full path of its process's program and whether that state is
//! reported, from this thread.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use ::windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use ::windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use ::windows::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use ::windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetWindowThreadProcessId, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
};
use ::windows::core::PWSTR;

use super::{Foreground, ForegroundSource, ForegroundTold};
use crate::threads::windows::MessageThread;

/// The watcher of the windows that come to the front, for game mode:
/// its thread's hook is called for each one, and what the launcher
/// subscribed ([`Watcher`]'s `watch`) is told of it.
pub(in crate::game_mode) struct Watcher {
    /// What the hook thread reports to, once the launcher subscribes;
    /// `None` until then, so events that arrive before it are dropped.
    told: Arc<Mutex<Option<ForegroundTold>>>,
    thread: MessageThread,
}

impl Watcher {
    /// Starts the watcher: Pane's own thread, whose message loop the
    /// system calls the hook through. Watching begins at once and ends
    /// when the watcher is dropped.
    pub(in crate::game_mode) fn start() -> Result<Watcher, String> {
        let told: Arc<Mutex<Option<ForegroundTold>>> = Arc::default();
        let shared = told.clone();
        let thread = MessageThread::spawn(
            "pane-game",
            move || watch(shared),
            // The watcher serves no requests on its thread; its stop ends
            // its loop.
            |_, _| {},
            |hook| {
                REPORTING.with(|reporting| reporting.borrow_mut().take());
                // SAFETY: `hook` is the hook this thread installed.
                let _ = unsafe { UnhookWinEvent(hook) };
            },
        )?;
        Ok(Watcher { told, thread })
    }
}

impl ForegroundSource for Watcher {
    fn watch(&self, told: ForegroundTold) {
        *crate::util::lock(&self.told) = Some(told);
    }
}

impl Drop for Watcher {
    /// Ends the watcher's thread, which uninstalls its hook as it ends.
    /// It never waits on another program, so this waits until it has.
    fn drop(&mut self) {
        self.thread.stop(None);
    }
}

// What the hook thread reports to, placed by its start and dropped with
// it: the slot the watcher shares with whoever subscribed.
thread_local! {
    static REPORTING: RefCell<Option<Arc<Mutex<Option<ForegroundTold>>>>> =
        const { RefCell::new(None) };
}

/// On the watcher's thread: installs the foreground hook, whose events
/// the thread's message loop delivers to [`foreground`] below.
fn watch(
    reporting: Arc<Mutex<Option<ForegroundTold>>>,
) -> Result<(HWINEVENTHOOK, Option<HWND>), String> {
    REPORTING.with(|slot| *slot.borrow_mut() = Some(reporting));
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
/// window that comes to the front, which it reports to what the
/// launcher subscribed, from this thread.
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
        if event == EVENT_SYSTEM_FOREGROUND && object == OBJID_WINDOW.0 && child == 0 {
            let front = Foreground {
                program: program_path(window),
                full_screen: full_screen(),
            };
            REPORTING.with(|reporting| {
                if let Some(slot) = reporting.borrow().as_ref()
                    && let Some(told) = crate::util::lock(slot).as_ref()
                {
                    told.front(&front);
                }
            });
        }
    });
}

/// Whether the shell reports a full-screen Direct3D application in
/// front: the documented notification state game mode recognizes a
/// full-screen game by (`QUNS_RUNNING_D3D_FULL_SCREEN`). A state that
/// cannot be read says no, so the window is decided by its program
/// alone.
fn full_screen() -> bool {
    // SAFETY: it only reads the shell's state.
    let state = unsafe { SHQueryUserNotificationState() };
    state.is_ok_and(|state| state == QUNS_RUNNING_D3D_FULL_SCREEN)
}

/// The full path of the program of the window in front's process, if it
/// can be read.
fn program_path(window: HWND) -> Option<String> {
    let mut process = 0u32;
    // SAFETY: `window` is a window handle; `process` is writable.
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut process)) };
    if process == 0 {
        return None;
    }
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
