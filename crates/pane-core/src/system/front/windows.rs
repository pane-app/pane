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
//! asked for, since the window may be gone by then if it closed. A
//! second hook, for `EVENT_OBJECT_FOCUS`, records the control focused
//! inside the recorded window as the focus moves (#432): once Pane's
//! window is in front, the target's thread names no focused window, and
//! the selected-text read still has to know which control to read. The
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
    EVENT_OBJECT_FOCUS, EVENT_SYSTEM_FOREGROUND, GA_ROOT, GUITHREADINFO, GW_OWNER, GWL_EXSTYLE,
    GetAncestor, GetClassNameW, GetGUIThreadInfo, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsWindow, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WS_EX_TOOLWINDOW,
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
    /// The control last focused inside the window, as an address, if
    /// the watcher saw one: what the selected-text read reads once Pane's
    /// window is in front and the window's thread names no focus (#432).
    pub(in crate::system) focus: Option<usize>,
}

impl Recorded {
    /// The control last focused inside the window, if it is still there:
    /// a window, and inside this one, since a closed control's address
    /// may have been given to another window since.
    pub(in crate::system) fn focused(&self) -> Option<HWND> {
        let focus = HWND(self.focus? as *mut _);
        // SAFETY: plain values; a gone window answers false.
        let alive = unsafe { IsWindow(Some(focus)) }.as_bool();
        (alive && root_of(focus) == self.window).then_some(focus)
    }
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
            |hooks: [HWINEVENTHOOK; 2]| {
                RECORDING.with(|recording| recording.borrow_mut().take());
                for hook in hooks {
                    // SAFETY: `hook` is a hook this thread installed.
                    let _ = unsafe { UnhookWinEvent(hook) };
                }
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

/// On the watcher's thread: installs the foreground hook and the focus
/// hook, whose events the thread's message loop delivers to [`event`]
/// below.
fn watch(
    recording: Arc<Mutex<Option<Recorded>>>,
) -> Result<([HWINEVENTHOOK; 2], Option<HWND>), String> {
    RECORDING.with(|recorded| *recorded.borrow_mut() = Some(recording));
    let foreground = hook(EVENT_SYSTEM_FOREGROUND, "foreground")?;
    let focus = match hook(EVENT_OBJECT_FOCUS, "focus") {
        Ok(focus) => focus,
        Err(why) => {
            // SAFETY: the hook installed just above.
            let _ = unsafe { UnhookWinEvent(foreground) };
            return Err(why);
        }
    };
    Ok(([foreground, focus], None))
}

/// Installs the hook of `kind` alone, named `what` in its refusal.
fn hook(kind: u32, what: &str) -> Result<HWINEVENTHOOK, String> {
    // SAFETY: no module to load the callback from, every process and
    // thread's events, out of process: the callback runs on this thread
    // as it pumps its messages, and never blocks the window the event is
    // about.
    let hook =
        unsafe { SetWinEventHook(kind, kind, None, Some(event), 0, 0, WINEVENT_OUTOFCONTEXT) };
    if hook.is_invalid() {
        let why = ::windows::core::Error::from_thread().message();
        return Err(format!("Windows refused the {what} hook: {why}"));
    }
    Ok(hook)
}

/// The hooks' callback, on the watcher's thread: called once for each
/// window that comes to the front, and for each control, in any process,
/// that takes the focus.
unsafe extern "system" fn event(
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
        } else if event == EVENT_OBJECT_FOCUS && !window.is_invalid() {
            focused(window);
        }
    });
}

/// Records `control`, which took the focus, as the recorded window's
/// focus if it is inside that window. Focus anywhere else, Pane's own
/// window above all, leaves the recorded focus as it was.
fn focused(control: HWND) {
    let root = root_of(control);
    RECORDING.with(|recording| {
        if let Some(recording) = recording.borrow().as_ref()
            && let Some(recorded) = crate::util::lock(recording).as_mut()
            && recorded.window == root
        {
            recorded.focus = Some(control.0 as usize);
        }
    });
}

/// The top-level window `control` is inside, as an address.
fn root_of(control: HWND) -> usize {
    // SAFETY: plain values; a window that is gone answers none.
    unsafe { GetAncestor(control, GA_ROOT) }.0 as usize
}

/// The control the thread of `window` has focused now, if it is inside
/// `window`: the focus as the window comes to the front, before any focus
/// event moves it.
fn focus_in(window: HWND) -> Option<usize> {
    // SAFETY: a window handle; the process id is not wanted.
    let thread = unsafe { GetWindowThreadProcessId(window, None) };
    if thread == 0 {
        return None;
    }
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is writable for its size; `thread` is `window`'s.
    unsafe { GetGUIThreadInfo(thread, &mut info) }.ok()?;
    let focus = info.hwndFocus;
    let inside = !focus.is_invalid() && root_of(focus) == window.0 as usize;
    inside.then_some(focus.0 as usize)
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
        focus: focus_in(window),
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
