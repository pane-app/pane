//! Global hotkeys on Windows: `RegisterHotKey` on a thread of Pane's own
//! for the chords Windows accepts, and a low-level keyboard hook of
//! Pane's own (`WH_KEYBOARD_LL`) for the chords Windows refuses — another
//! application has the shortcut, or Windows keeps it for itself — and
//! for the binding kinds no registration can express: a lone tap or a
//! double tap of a modifier, a side-specific modifier, a numpad key
//! (#260) — so a refused shortcut is never an error: the binding works
//! while Pane runs (ADR 0039). A conflict shows as
//! `ERROR_HOTKEY_ALREADY_REGISTERED`, which is the hook's cue rather than
//! the user's problem. No permission is needed.
//!
//! A hotkey belongs to the thread that registered it, so registering and
//! releasing are done by that thread: the caller queues the request, wakes
//! the thread with a thread message and waits for its answer. Dropping the
//! adapter ends the thread, which releases its hotkeys as it ends. The
//! thread is a `threads::windows::MessageThread`, like the clipboard
//! listener's.
//!
//! A recorder that listens asks for a recording session
//! ([`Hotkeys::recording`], #260): the hook then holds every key back
//! from the system — Escape and Tab excepted, so the recorder's own
//! cancellation keys work — and reports what the user presses to the
//! session, which lasts until the recorder stops listening, a window of
//! another process comes in front, or Pane quits.
//!
//! The hook lives on a thread of its own, started when the first binding
//! or session needs it and stopped when none does. Its callback runs at the
//! highest thread priority — not a raised process priority, so the rest of
//! Pane is unaffected — and only steps the recognizer (see `recognizer`)
//! and posts a message to the adapter's thread when a binding fires: it
//! never allocates, takes no lock another thread holds, and calls nothing
//! outside `user32`'s input machinery. While the Windows key is bound
//! alone, the release that completes its tap — and the release of a
//! Windows key whose chord Pane swallowed — carries the Start-menu mask
//! first: a tagged neutral key injected before the release reaches
//! Windows, so Explorer does not open the Start menu (which stays on the
//! taskbar's Start button and Ctrl+Esc). Its code and data pages are
//! locked in memory after the process's minimum working set is raised by
//! what they need, so a trimmed working set cannot fault the callback
//! past Windows' hook timeout; if Windows refuses, the diagnostic says
//! the pages are not pinned. A watchdog window that receives raw keyboard
//! input notices when key events stop reaching the hook — Windows has
//! silently removed it — and installs it again, giving up with a
//! diagnostic after repeated failures within a short time. The
//! recognizer's modifiers are re-read from the system on a session
//! unlock, on a resume and when an event contradicts them, so a release
//! missed while the secure desktop had the keyboard never leaves a
//! modifier stuck down. Keys Pane injects carry [`INJECTED_TAG`], which
//! the recognizer answers by passing them through untouched.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ::windows::Win32::Foundation::{
    ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, LRESULT, WPARAM,
};
use ::windows::Win32::System::Memory::{
    GetProcessWorkingSetSizeEx, SETPROCESSWORKINGSETSIZEEX_FLAGS, SetProcessWorkingSetSizeEx,
    VirtualLock,
};
use ::windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use ::windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThread, GetCurrentThreadId,
    SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_NONAME, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_TAB,
};
use ::windows::Win32::UI::Input::{RAWINPUTDEVICE, RIDEV_INPUTSINK, RegisterRawInputDevices};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DefWindowProcW, GetForegroundWindow, GetWindowThreadProcessId, HHOOK,
    KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_UP, MSG, PBT_APMRESUMEAUTOMATIC,
    PBT_APMRESUMESUSPEND, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_HOTKEY, WM_INPUT, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
};
use ::windows::core::HRESULT;

use super::{
    Binding, Decision, HookHealth, HotkeyError, Hotkeys, KeyEvent, Kind, PressSender, Recognizer,
    Recorded, RecordingSession, Route, Shortcut, Side,
};
use crate::threads::windows::{
    MessageThread, WM_FIRED, WM_RECORDED, WM_RECORDED_ENDED, WM_WAKE, Window, WindowClass,
    stop_sent,
};

/// How many raw key events arrive without the hook reporting one before
/// the watchdog concludes that Windows removed the hook and installs it
/// again.
const RAW_BEFORE_REINSTALL: u32 = 6;

/// How long a run of reinstallations may last before Pane counts them
/// afresh: only failures within a short time give up.
const REINSTALL_WINDOW: Duration = Duration::from_secs(30);

/// How many reinstallations within [`REINSTALL_WINDOW`] Pane gives up
/// after, leaving the hook-based hotkeys dead until Pane starts again.
const REINSTALLS_BEFORE_GIVING_UP: u32 = 5;

/// How much the process's minimum working set is raised by, for the pages
/// the hook pins: a lock that would not fit the working set fails, so the
/// allowance covers the pages with headroom.
const PIN_ALLOWANCE: usize = 256 * 1024;

/// One memory page, as every Windows Pane runs on uses.
const PAGE: usize = 4096;

/// The `WM_WTSSESSION_CHANGE` parameter that says the session was
/// unlocked: the secure desktop below the lock screen had the keyboard,
/// so key events were not delivered to the hook.
const SESSION_UNLOCK: usize = 8;

enum Request {
    Register(Shortcut, mpsc::Sender<Result<Route, HotkeyError>>),
    Unregister(Shortcut, mpsc::Sender<()>),
    /// Starts a recording session, reporting what the user presses to
    /// `reports` (#260); the answer says the hook is holding the keys
    /// back, so the session is live when it is handed out.
    Record(PressSender, mpsc::Sender<()>),
    /// Ends the recording session (the recorder stopped listening); the
    /// answer says the hook stopped holding the keys back.
    EndRecording(mpsc::Sender<()>),
}

type Requests = Arc<Mutex<VecDeque<Request>>>;

/// How the adapter answers [`Hotkeys::route`]: each registered shortcut's
/// route, as the hotkey thread answered when it registered it.
type Routes = Arc<Mutex<HashMap<Shortcut, Route>>>;

/// A binding the hotkey thread asks the hook thread to watch: the
/// binding's id and the binding itself, as the recognizer takes it.
enum HookRequest {
    Add(u32, Binding, mpsc::Sender<Result<(), String>>),
    Remove(u32, mpsc::Sender<()>),
    /// Turns the recording mode on or off: while it is on, the hook holds
    /// the keys back from the system and reports what is pressed (#260).
    Record(bool, mpsc::Sender<()>),
}

type HookRequests = Arc<Mutex<VecDeque<HookRequest>>>;

/// The adapter: a thread that registers the hotkeys and receives their
/// presses, and the routes its bindings are dispatched by, for the rows
/// to say.
pub struct WindowsHotkeys {
    thread: MessageThread,
    requests: Requests,
    /// How each registered shortcut is dispatched, as the hotkey thread
    /// answered when it registered it.
    routes: Routes,
    /// The keyboard hook's state, shared with the hook's own thread.
    report: Arc<Mutex<Report>>,
}

/// The hook's state, as the adapter answers [`Hotkeys::hook_health`] and
/// the opt-in real-input tests: written on the hook's own thread (never
/// in its callback), read wherever Pane shows it.
#[derive(Default)]
struct Report {
    /// How many times Windows removed the hook and Pane installed it
    /// again.
    reinstalled: u32,
    /// Whether the hook's code and data pages are pinned in memory.
    pinned: bool,
    /// Why Pane gave up reinstalling the hook, if it did.
    given_up: Option<String>,
    /// The low-level hook's handle, for the tests to remove behind the
    /// adapter's back; 0 while none is installed.
    hook: usize,
    /// The watchdog's window, for the tests to deliver the raw key events
    /// the keyboard would; 0 until it is made.
    watchdog: usize,
}

/// Changes the shared report, which only the hook thread writes.
fn update(report: &Mutex<Report>, change: impl FnOnce(&mut Report)) {
    change(&mut report.lock().unwrap_or_else(|p| p.into_inner()));
}

/// The virtual-key code of `key` (see `Shortcut::key`), and whether it
/// is the numpad's form of a code the main keyboard shares — the
/// numpad's Enter (#260).
fn key_code(key: &str) -> Option<(u32, bool)> {
    if key == "numpad_enter" {
        // VK_RETURN, which the numpad's Enter shares with the main one;
        // the extended flag tells them apart, as the recognizer matches
        // it.
        return Some((0x0D, true));
    }
    Some((virtual_key(key)?, key.starts_with("numpad")))
}

/// The virtual-key code of `key` (see `Shortcut::key`).
fn virtual_key(key: &str) -> Option<u32> {
    match key.as_bytes() {
        [c @ b'a'..=b'z'] => Some(u32::from(c.to_ascii_uppercase())),
        [c @ b'0'..=b'9'] => Some(u32::from(*c)),
        [c] if ",./;'`[]\\-=".as_bytes().contains(c) => Some(match c {
            b',' => 0xBC,  // VK_OEM_COMMA
            b'.' => 0xBE,  // VK_OEM_PERIOD
            b'/' => 0xBF,  // VK_OEM_2
            b';' => 0xBA,  // VK_OEM_1
            b'\'' => 0xDE, // VK_OEM_7
            b'`' => 0xC0,  // VK_OEM_3
            b'[' => 0xDB,  // VK_OEM_4
            b']' => 0xDD,  // VK_OEM_6
            b'\\' => 0xDC, // VK_OEM_5
            b'-' => 0xBD,  // VK_OEM_MINUS
            _ => 0xBB,     // VK_OEM_PLUS, the '=' key
        }),
        _ if key == "space" => Some(0x20),
        _ if key == "enter" => Some(0x0D),
        _ if key == "tab" => Some(0x09),
        _ if key == "left" => Some(0x25),
        _ if key == "right" => Some(0x27),
        _ if key == "up" => Some(0x26),
        _ if key == "down" => Some(0x28),
        _ if key == "home" => Some(0x24),
        _ if key == "end" => Some(0x23),
        _ if key == "pageup" => Some(0x21),
        _ if key == "pagedown" => Some(0x22),
        _ if key == "insert" => Some(0x2D),
        _ if key == "delete" => Some(0x2E),
        _ if key.starts_with("numpad") => match key.strip_prefix("numpad")? {
            // VK_NUMPAD0 is 0x60, and the arithmetic keys follow their
            // own codes.
            "0" => Some(0x60),
            "1" => Some(0x61),
            "2" => Some(0x62),
            "3" => Some(0x63),
            "4" => Some(0x64),
            "5" => Some(0x65),
            "6" => Some(0x66),
            "7" => Some(0x67),
            "8" => Some(0x68),
            "9" => Some(0x69),
            "add" => Some(0x6B),
            "decimal" => Some(0x6E),
            "divide" => Some(0x6F),
            "multiply" => Some(0x6A),
            "subtract" => Some(0x6D),
            _ => None,
        },
        _ => {
            let number: u32 = key.strip_prefix('f')?.parse().ok()?;
            // VK_F1 is 0x70; F13, the first of the extended key set's,
            // is 0x7C (#260).
            (1..=24).contains(&number).then(|| 0x70 + number - 1)
        }
    }
}

/// The letters as the record writes them, for a virtual-key code's
/// name: `a` to `z`, whose codes run 0x41 to 0x5A.
const LETTERS: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];

/// The digits as the record writes them, for a virtual-key code's name:
/// `0` to `9`, whose codes run 0x30 to 0x39.
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// The key a virtual-key code names, as the record writes it: the
/// reverse of [`key_code`], for what a recording session recognized
/// (#260).
fn key_name_of(key: u32, numpad: bool) -> Option<&'static str> {
    if (0x41..=0x5A).contains(&key) {
        // The letters, as their own lowercase.
        return Some(LETTERS[(key - 0x41) as usize]);
    }
    if (0x30..=0x39).contains(&key) {
        return Some(DIGITS[(key - 0x30) as usize]);
    }
    if (0x70..=0x87).contains(&key) {
        return Some(match key - 0x70 + 1 {
            1 => "f1",
            2 => "f2",
            3 => "f3",
            4 => "f4",
            5 => "f5",
            6 => "f6",
            7 => "f7",
            8 => "f8",
            9 => "f9",
            10 => "f10",
            11 => "f11",
            12 => "f12",
            13 => "f13",
            14 => "f14",
            15 => "f15",
            16 => "f16",
            17 => "f17",
            18 => "f18",
            19 => "f19",
            20 => "f20",
            21 => "f21",
            22 => "f22",
            23 => "f23",
            _ => "f24",
        });
    }
    if (0x60..=0x69).contains(&key) {
        return Some(match key - 0x60 {
            0 => "numpad0",
            1 => "numpad1",
            2 => "numpad2",
            3 => "numpad3",
            4 => "numpad4",
            5 => "numpad5",
            6 => "numpad6",
            7 => "numpad7",
            8 => "numpad8",
            _ => "numpad9",
        });
    }
    Some(match (key, numpad) {
        (0x0D, true) => "numpad_enter",
        (0x0D, false) => "enter",
        (0x20, _) => "space",
        (0x09, _) => "tab",
        (0x25, _) => "left",
        (0x27, _) => "right",
        (0x26, _) => "up",
        (0x28, _) => "down",
        (0x24, _) => "home",
        (0x23, _) => "end",
        (0x21, _) => "pageup",
        (0x22, _) => "pagedown",
        (0x2D, _) => "insert",
        (0x2E, _) => "delete",
        (0xBC, _) => ",",
        (0xBE, _) => ".",
        (0xBF, _) => "/",
        (0xBA, _) => ";",
        (0xDE, _) => "'",
        (0xC0, _) => "`",
        (0xDB, _) => "[",
        (0xDD, _) => "]",
        (0xDC, _) => "\\",
        (0xBD, _) => "-",
        (0xBB, _) => "=",
        (0x6A, _) => "numpad_multiply",
        (0x6B, _) => "numpad_add",
        (0x6D, _) => "numpad_subtract",
        (0x6E, _) => "numpad_decimal",
        (0x6F, _) => "numpad_divide",
        _ => return None,
    })
}

/// The name of the modifier at `at` — control, alt, shift, the Windows
/// key — as the record writes it for the tap kinds, with the side prefix
/// where the session recognized one side (#260).
fn modifier_name(at: usize, side: Side) -> String {
    let name = ["ctrl", "alt", "shift", "win"][at];
    match side {
        Side::Any => name.into(),
        Side::Left => format!("l{name}"),
        Side::Right => format!("r{name}"),
    }
}

/// What a recording session recognized, as the binding's text: the
/// recognizer's report, built by [`Shortcut::parse`] (#260).
fn recorded_text(report: Recorded) -> Option<String> {
    Some(match report {
        Recorded::Chord {
            modifiers,
            key,
            numpad,
        } => {
            let mut parts: Vec<String> = modifiers
                .iter()
                .enumerate()
                .filter_map(|(at, side)| {
                    side.map(|side| match side {
                        Side::Any => ["ctrl", "alt", "shift", "super"][at].into(),
                        _ => modifier_name(at, side),
                    })
                })
                .collect();
            parts.push(key_name_of(key, numpad)?.into());
            parts.join("+")
        }
        Recorded::Tap { modifier, side } => format!("tap:{}", modifier_name(modifier, side)),
        Recorded::Double { modifier, side } => {
            format!("double:{}", modifier_name(modifier, side))
        }
    })
}

/// Packs what a recording session recognized into a posted message's
/// word: the kind, and for a chord the modifiers' sides and the key, for
/// a tap or a double tap the modifier and its side. The hook's callback
/// may not allocate, so the report crosses the threads packed (#260).
fn pack(report: Recorded) -> usize {
    let kind = match report {
        Recorded::Chord { .. } => 0,
        Recorded::Tap { .. } => 1,
        Recorded::Double { .. } => 2,
    };
    match report {
        Recorded::Chord {
            modifiers,
            key,
            numpad,
        } => {
            let sides = modifiers.iter().enumerate().fold(0, |packed, (at, side)| {
                let side = match side {
                    None => 0,
                    Some(Side::Any) => 1,
                    Some(Side::Left) => 2,
                    Some(Side::Right) => 3,
                };
                packed | (side << (2 + 2 * at))
            });
            kind | sides | ((numpad as usize) << 10) | ((key as usize) << 16)
        }
        Recorded::Tap { modifier, side } | Recorded::Double { modifier, side } => {
            let side = match side {
                Side::Any => 1,
                Side::Left => 2,
                Side::Right => 3,
            };
            kind | (modifier << 4) | (side << 8)
        }
    }
}

/// Unpacks what a recording session recognized from a posted message's
/// word, as [`pack`] wrote it.
fn unpack(word: usize) -> Recorded {
    match word & 0xF {
        0 => {
            let mut modifiers = [None; 4];
            for (at, side) in modifiers.iter_mut().enumerate() {
                *side = match (word >> (2 + 2 * at)) & 3 {
                    0 => None,
                    1 => Some(Side::Any),
                    2 => Some(Side::Left),
                    _ => Some(Side::Right),
                };
            }
            Recorded::Chord {
                modifiers,
                key: ((word >> 16) & 0xFFFF) as u32,
                numpad: (word >> 10) & 1 != 0,
            }
        }
        1 => Recorded::Tap {
            modifier: (word >> 4) & 3,
            side: unpacked_side((word >> 8) & 3),
        },
        _ => Recorded::Double {
            modifier: (word >> 4) & 3,
            side: unpacked_side((word >> 8) & 3),
        },
    }
}

/// The side a packed tap or double tap names.
fn unpacked_side(word: usize) -> Side {
    match word {
        2 => Side::Left,
        3 => Side::Right,
        _ => Side::Any,
    }
}

fn modifiers(shortcut: &Shortcut) -> HOT_KEY_MODIFIERS {
    // Holding the keys reports one press, not one per repeat.
    let mut modifiers = MOD_NOREPEAT;
    if shortcut.control() {
        modifiers |= MOD_CONTROL;
    }
    if shortcut.alt() {
        modifiers |= MOD_ALT;
    }
    if shortcut.shift() {
        modifiers |= MOD_SHIFT;
    }
    if shortcut.super_key() {
        modifiers |= MOD_WIN;
    }
    modifiers
}

/// The binding `shortcut` names, as the recognizer takes it (#260): the
/// kind, the modifiers with their sides, and the key's code with its
/// numpad flag.
fn hook_binding(shortcut: &Shortcut) -> Option<Binding> {
    let (key, numpad) = match shortcut.kind() {
        Kind::Chord => key_code(shortcut.key())?,
        _ => (0, false),
    };
    Some(Binding {
        kind: shortcut.kind(),
        modifiers: shortcut.sides(),
        key,
        numpad,
    })
}

/// Whether `shortcut` is a binding `RegisterHotKey` cannot express, so
/// Pane's own keyboard hook takes it before the system is asked (#260):
/// a lone tap or a double tap of a modifier, a side-specific modifier,
/// or a numpad key, which the registration cannot tell from its
/// counterpart.
fn hook_only(shortcut: &Shortcut) -> bool {
    shortcut.kind() != Kind::Chord
        || shortcut
            .sides()
            .iter()
            .any(|side| matches!(side, Some(Side::Left | Side::Right)))
        || shortcut.key().starts_with("numpad")
}

impl WindowsHotkeys {
    /// Starts the hotkey thread. The keyboard hook's thread is started
    /// only when the first binding Windows refuses needs it, and stopped
    /// when none does.
    pub fn start(presses: PressSender) -> Result<WindowsHotkeys, String> {
        let requests: Requests = Arc::default();
        let served = requests.clone();
        let routes: Routes = Arc::default();
        let report = Arc::new(Mutex::new(Report::default()));
        let told = report.clone();
        let thread = MessageThread::spawn(
            "pane-hotkeys",
            move || {
                // SAFETY: it only reads the calling thread's id.
                let adapter = unsafe { GetCurrentThreadId() };
                Ok((Registered::new(adapter, told), None))
            },
            move |registered, message| registered.serve(message, &served, &presses),
            Registered::release_all,
        )?;
        Ok(WindowsHotkeys {
            thread,
            requests,
            routes,
            report,
        })
    }

    /// Queues `request` and wakes the thread; false if it is gone.
    fn send(&self, request: Request) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(request);
        self.thread.post(WM_WAKE)
    }

    /// The low-level keyboard hook's handle, for the opt-in real-input
    /// tests (`PANE_TEST_REAL_INPUT`) to remove behind the adapter's
    /// back, so the watchdog has a removal to notice and repair. `None`
    /// while no binding needs the hook.
    pub fn hook_handle(&self) -> Option<usize> {
        let report = self.report.lock().unwrap_or_else(|p| p.into_inner());
        (report.hook != 0).then_some(report.hook)
    }

    /// The watchdog's window, for the opt-in real-input tests to deliver
    /// the raw key events the keyboard would: a test's injected keys are
    /// a tool's, not the keyboard's, so the watchdog cannot see them any
    /// other way. `None` while no binding needs the hook.
    pub fn watchdog_window(&self) -> Option<usize> {
        let report = self.report.lock().unwrap_or_else(|p| p.into_inner());
        (report.watchdog != 0).then_some(report.watchdog)
    }
}

impl Hotkeys for WindowsHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        let (answer, answered) = mpsc::channel();
        if !self.send(Request::Register(shortcut.clone(), answer)) {
            return Err(HotkeyError::Refused("the hotkey thread stopped".into()));
        }
        let registered = answered
            .recv()
            .unwrap_or_else(|_| Err(HotkeyError::Refused("the hotkey thread stopped".into())));
        if let Ok(route) = &registered {
            self.routes
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(shortcut.clone(), *route);
        }
        registered.map(|_| ())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(shortcut);
        let (answer, answered) = mpsc::channel();
        if self.send(Request::Unregister(shortcut.clone(), answer)) {
            let _ = answered.recv();
        }
    }

    fn route(&self, shortcut: &Shortcut) -> Route {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(shortcut)
            .copied()
            .unwrap_or(Route::System)
    }

    fn hook_health(&self) -> Option<HookHealth> {
        let hooked = self
            .routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .any(|route| *route == Route::Hook);
        if !hooked {
            // No binding needs the hook, so none is installed.
            return None;
        }
        let report = self.report.lock().unwrap_or_else(|p| p.into_inner());
        Some(HookHealth {
            reinstalls: report.reinstalled,
            pinned: report.pinned,
            given_up: report.given_up.clone(),
        })
    }

    // The binding kinds #260 adds all work here: the ones no
    // registration can express go through Pane's own keyboard hook.
    fn kind_unavailable(&self, _shortcut: &Shortcut) -> Option<String> {
        None
    }

    fn recording(&self) -> Option<RecordingSession> {
        // The session's reports: the hotkey thread unpacks what the hook
        // recognizes, builds the binding and sends it here; dropping the
        // session (or its stop half) ends it. The answer says the hook is
        // holding the keys back, so the session is live when this hands
        // it out.
        let (reports, presses) = super::channel();
        let (answer, answered) = mpsc::channel();
        if !self.send(Request::Record(reports, answer)) {
            return None;
        }
        if answered.recv().is_err() {
            return None;
        }
        let requests = self.requests.clone();
        let thread = self.thread.id();
        Some(RecordingSession::of(presses, move || {
            let (answer, answered) = mpsc::channel();
            queue(&requests, thread, Request::EndRecording(answer));
            // The hook stops holding the keys back before the drop ends,
            // or at least soon after: a thread that is gone answers
            // nothing, and the keys are gone with it.
            let _ = answered.recv_timeout(Duration::from_secs(1));
        }))
    }
}

/// Queues `request` for the hotkey thread and wakes it, trying again for
/// a moment while its queue is full; false if it could not. The stop of a
/// recording session calls this without the thread's handle, so it posts
/// to the thread's id itself.
fn queue(requests: &Requests, thread: u32, request: Request) -> bool {
    requests
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push_back(request);
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
        // SAFETY: plain values, posted to the hotkey thread, which pumps
        // its messages.
        if unsafe { PostThreadMessageW(thread, WM_WAKE, WPARAM(0), LPARAM(0)) }.is_ok() {
            return true;
        }
    }
    false
}

impl Drop for WindowsHotkeys {
    /// Ends the thread, which releases every hotkey and stops the
    /// keyboard hook as it ends. It never waits on another program, so
    /// this waits until it has.
    fn drop(&mut self) {
        self.thread.stop(None);
    }
}

/// The hotkeys the thread registered, by their id.
struct Registered {
    shortcuts: HashMap<i32, Shortcut>,
    last_id: i32,
    /// The bindings the keyboard hook watches, by their id.
    hooked: HashMap<u32, Shortcut>,
    next_hook: u32,
    /// The keyboard hook's thread, while a binding or a recording
    /// session needs it.
    hook: Option<Hook>,
    /// This thread's id, which the hook posts the presses it recognizes
    /// to.
    adapter: u32,
    /// The hook's state, shared with its thread and the adapter.
    report: Arc<Mutex<Report>>,
    /// Where a recording session's reports go, while one listens (#260);
    /// dropped when it ends, which ends the reports.
    recording: Option<PressSender>,
}

impl Registered {
    fn new(adapter: u32, report: Arc<Mutex<Report>>) -> Registered {
        Registered {
            shortcuts: HashMap::new(),
            last_id: 0,
            hooked: HashMap::new(),
            next_hook: 0,
            hook: None,
            adapter,
            report,
            recording: None,
        }
    }

    fn release(&mut self, id: i32) {
        // SAFETY: `id` was registered by this thread with no window.
        let _ = unsafe { UnregisterHotKey(None, id) };
        self.shortcuts.remove(&id);
    }

    /// The route of a shortcut this thread already holds, if it does.
    fn route_of(&self, shortcut: &Shortcut) -> Option<Route> {
        if self.shortcuts.values().any(|done| *done == *shortcut) {
            return Some(Route::System);
        }
        self.hooked
            .values()
            .any(|done| *done == *shortcut)
            .then_some(Route::Hook)
    }

    /// Releases the hook bindings of `shortcut`, stopping the hook's
    /// thread when none is left: the hook is removed when no binding —
    /// and no recording session — needs it (#260).
    fn release_hooked(&mut self, shortcut: &Shortcut) {
        let ids: Vec<u32> = self
            .hooked
            .iter()
            .filter(|(_, done)| **done == *shortcut)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(hook) = &self.hook {
                hook.remove(id);
            }
            self.hooked.remove(&id);
        }
        if self.hooked.is_empty()
            && self.recording.is_none()
            && let Some(mut hook) = self.hook.take()
        {
            hook.stop();
        }
    }

    fn release_all(mut self) {
        // The hook's thread first: it stops watching the keyboard before
        // the registrations it stood in for go.
        if let Some(mut hook) = self.hook.take() {
            hook.stop();
        }
        let ids: Vec<i32> = self.shortcuts.keys().copied().collect();
        for id in ids {
            self.release(id);
        }
    }

    /// Reports a hotkey's press, or serves the queued requests.
    fn serve(&mut self, message: &MSG, requests: &Mutex<VecDeque<Request>>, presses: &PressSender) {
        match message.message {
            WM_HOTKEY => {
                if let Some(shortcut) = self.shortcuts.get(&(message.wParam.0 as i32)) {
                    presses.send(shortcut.clone());
                }
            }
            WM_FIRED => {
                // A chord the keyboard hook recognized: the binding the
                // hotkey thread holds for it.
                if let Some(shortcut) = self.hooked.get(&(message.wParam.0 as u32)) {
                    presses.send(shortcut.clone());
                }
            }
            WM_RECORDED => {
                // What a recording session recognized (#260): the binding
                // the session's report names, sent to the session's
                // reports.
                if let Some(text) = recorded_text(unpack(message.wParam.0))
                    && let Some(shortcut) = Shortcut::parse(&text).ok()
                    && let Some(reports) = self.recording.as_ref()
                {
                    reports.send(shortcut);
                }
            }
            WM_RECORDED_ENDED => {
                // The hook thread noticed the window lost the focus (#260):
                // the session ends, and the hook stops when no binding
                // needs it.
                self.end_recording();
            }
            WM_WAKE => loop {
                let request = requests
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .pop_front();
                match request {
                    None => break,
                    Some(Request::Register(shortcut, answer)) => {
                        let _ = answer.send(self.register(shortcut));
                    }
                    Some(Request::Unregister(shortcut, answer)) => {
                        let ids: Vec<i32> = self
                            .shortcuts
                            .iter()
                            .filter(|(_, done)| **done == shortcut)
                            .map(|(id, _)| *id)
                            .collect();
                        for id in ids {
                            self.release(id);
                        }
                        self.release_hooked(&shortcut);
                        let _ = answer.send(());
                    }
                    Some(Request::Record(reports, answer)) => {
                        self.recording = Some(reports);
                        self.ensure_hook();
                        if let Some(hook) = &self.hook {
                            hook.record(true);
                        }
                        let _ = answer.send(());
                    }
                    Some(Request::EndRecording(answer)) => {
                        self.end_recording();
                        let _ = answer.send(());
                    }
                }
            },
            _ => {}
        }
    }

    /// Starts the keyboard hook's thread if no binding or session has it
    /// (#260): a recording session needs it to hold the keys back, as a
    /// binding needs it to recognize the kinds no registration expresses.
    /// A hook that cannot start leaves the session's reports silent, with
    /// a diagnostic saying why.
    fn ensure_hook(&mut self) {
        if self.hook.is_some() {
            return;
        }
        let adapter = self.adapter;
        let report = self.report.clone();
        match Hook::start(adapter, report) {
            Ok(hook) => self.hook = Some(hook),
            Err(problem) => crate::diagnostics::report_line(&format!(
                "Pane's keyboard hook is not available: {problem}"
            )),
        }
    }

    /// Ends the recording session (#260): its reports end with their
    /// sender, and the hook stops when no binding needs it.
    fn end_recording(&mut self) {
        self.recording = None;
        if let Some(hook) = &self.hook {
            hook.record(false);
        }
        if self.hooked.is_empty()
            && let Some(mut hook) = self.hook.take()
        {
            hook.stop();
        }
    }

    /// Registers `shortcut` with the system: `RegisterHotKey` when Windows
    /// accepts the chord, and Pane's own keyboard hook when Windows
    /// refuses it — another application has the shortcut, or Windows keeps
    /// it — or when the binding is a kind the registration cannot express
    /// (a lone tap, a double tap, a side-specific modifier, a numpad key,
    /// #260), none of which is an error (ADR 0039): the binding works
    /// while Pane runs.
    fn register(&mut self, shortcut: Shortcut) -> Result<Route, HotkeyError> {
        if let Some(route) = self.route_of(&shortcut) {
            return Ok(route);
        }
        if hook_only(&shortcut) {
            return self.through_hook(shortcut);
        }
        let Some(key) = virtual_key(shortcut.key()) else {
            return Err(HotkeyError::Refused(format!(
                "{shortcut} has no Windows key"
            )));
        };
        let id = self.last_id + 1;
        let taken = HRESULT::from_win32(ERROR_HOTKEY_ALREADY_REGISTERED.0);
        // SAFETY: plain values; with no window the hotkey belongs to this
        // thread, whose loop receives it.
        match unsafe { RegisterHotKey(None, id, modifiers(&shortcut), key) } {
            Ok(()) => {
                self.last_id = id;
                self.shortcuts.insert(id, shortcut);
                Ok(Route::System)
            }
            Err(error) if error.code() == taken => self.through_hook(shortcut),
            Err(error) => Err(HotkeyError::Refused(error.message())),
        }
    }

    /// Recognizes `shortcut` through Pane's own keyboard hook instead:
    /// Windows refused the registration, or the binding is a kind the
    /// registration cannot express. The hook's thread starts when the
    /// first binding needs it.
    fn through_hook(&mut self, shortcut: Shortcut) -> Result<Route, HotkeyError> {
        let Some(binding) = hook_binding(&shortcut) else {
            return Err(HotkeyError::Refused(format!(
                "{shortcut} has no Windows key"
            )));
        };
        if self.hook.is_none() {
            self.hook = Some(Hook::start(self.adapter, self.report.clone()).map_err(
                |problem| {
                    HotkeyError::Refused(format!(
                        "Pane's keyboard hook is not available: {problem}"
                    ))
                },
            )?);
        }
        let id = self.next_hook + 1;
        if let Err(problem) = self.hook.as_mut().expect("just started").add(id, binding) {
            return Err(HotkeyError::Refused(format!(
                "Pane's keyboard hook could not take it: {problem}"
            )));
        }
        self.next_hook = id;
        self.hooked.insert(id, shortcut);
        Ok(Route::Hook)
    }
}

/// The keyboard hook's thread, as the hotkey thread drives it: it is
/// started when the first binding Windows refuses needs it and stopped
/// when none does, and its bindings are added and removed by request.
struct Hook {
    thread: MessageThread,
    requests: HookRequests,
}

impl Hook {
    /// Starts the hook's thread: the hook, the watchdog and the pinned
    /// pages are its start's work.
    fn start(adapter: u32, report: Arc<Mutex<Report>>) -> Result<Hook, String> {
        let requests: HookRequests = Arc::default();
        let served = requests.clone();
        let thread = MessageThread::spawn(
            "pane-keyboard-hook",
            move || start_hook(adapter, report),
            move |_, message| serve_hook(message, &served),
            finish_hook,
        )?;
        Ok(Hook { thread, requests })
    }

    /// Queues `request` and wakes the thread; false if it is gone.
    fn send(&self, request: HookRequest) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(request);
        self.thread.post(WM_WAKE)
    }

    /// Binds `binding` as binding `id`, which the hook reports when it is
    /// pressed.
    fn add(&self, id: u32, binding: Binding) -> Result<(), String> {
        let (answer, answered) = mpsc::channel();
        if !self.send(HookRequest::Add(id, binding, answer)) {
            return Err("the keyboard hook thread stopped".into());
        }
        answered
            .recv()
            .unwrap_or_else(|_| Err("the keyboard hook thread stopped".into()))
    }

    /// Turns the hook's recording mode on or off (#260), waiting for the
    /// hook thread to have done it.
    fn record(&self, on: bool) {
        let (answer, answered) = mpsc::channel();
        if self.send(HookRequest::Record(on, answer)) {
            let _ = answered.recv();
        }
    }

    /// Unbinds the binding `id`.
    fn remove(&self, id: u32) {
        let (answer, answered) = mpsc::channel();
        if self.send(HookRequest::Remove(id, answer)) {
            let _ = answered.recv();
        }
    }

    /// Stops the thread, which releases the hook and its watchdog as it
    /// ends. It never waits on another program, so this waits until it
    /// has.
    fn stop(&mut self) {
        self.thread.stop(None);
    }
}

/// The hook thread's finish: its state is dropped with the thread, which
/// unhooks the hook and destroys the watchdog window.
fn finish_hook(_: ()) {}

/// The hook thread's state: the recognizer, the hook, the watchdog window
/// and the watchdog's counters. On the hook thread only, which the
/// callback, the watchdog window and the request serving all share.
struct Hooked {
    /// The bound bindings and the keyboard state, stepped by the
    /// callback.
    recognizer: Recognizer,
    /// The low-level keyboard hook, replaced when Windows removes it.
    hook: HHOOK,
    /// The watchdog's window, which receives the raw keyboard input and
    /// the session and power notifications. Held here for as long as the
    /// state lives, so the window — and with it the raw-input registration
    /// and the session notification — ends with the hook; nothing else
    /// reads it.
    #[allow(dead_code)]
    window: Window,
    /// The adapter's thread, which a recognized chord is posted to.
    adapter: u32,
    /// The hook's state, shared with the adapter.
    report: Arc<Mutex<Report>>,
    /// Whether a recording session listens (#260): the callback holds
    /// the keys back and posts what it recognizes to the adapter's
    /// thread, which sends it to the session.
    recording: bool,
    /// The key events the hook last reported seeing, for the watchdog's
    /// comparison.
    last_seen: u64,
    /// The raw key events since the hook last reported seeing one.
    unseen: u32,
    /// When the first of the current run of reinstallations happened.
    first: Option<Instant>,
    /// How many times the hook has been reinstalled in the current run,
    /// which gives up after too many within a short time.
    attempts: u32,
    /// Whether Pane has given up reinstalling it.
    given_up: bool,
}

impl Drop for Hooked {
    /// Releases the hook; the raw-input registration and the session
    /// notification go with the window, and the handles the adapter
    /// answers for are gone.
    fn drop(&mut self) {
        // SAFETY: this thread's own hook.
        let _ = unsafe { UnhookWindowsHookEx(self.hook) };
        update(&self.report, |report| {
            report.hook = 0;
            report.watchdog = 0;
        });
    }
}

thread_local! {
    /// The hook thread's state, placed by its start and dropped with the
    /// thread.
    static HOOKED: std::cell::RefCell<Option<Hooked>> =
        const { std::cell::RefCell::new(None) };
}

/// The watchdog's window class.
static WATCHDOG_CLASS: WindowClass = WindowClass::new("PaneKeyboardWatchdog", watchdog);

/// The hook thread's start: its priority, the hook, the watchdog window
/// and its notifications, and the pinned pages.
fn start_hook(adapter: u32, report: Arc<Mutex<Report>>) -> Result<((), Option<HWND>), String> {
    // The highest thread priority, not a raised process priority, so the
    // rest of Pane is unaffected: the callback must return before
    // Windows' hook timeout.
    // SAFETY: the calling thread's own pseudo-handle, and plain values.
    let priority = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
    if priority.is_err() {
        crate::diagnostics::report_line(
            "Windows did not give Pane's keyboard hook thread its highest priority",
        );
    }
    let hook = install()?;
    let window = match WATCHDOG_CLASS.message_window() {
        Ok(window) => window,
        Err(problem) => {
            // SAFETY: this thread's own hook, just installed.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
            return Err(problem);
        }
    };
    let watchdog = window.handle();
    if let Err(problem) = watch_input(watchdog) {
        // SAFETY: this thread's own hook, just installed.
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        return Err(format!(
            "the watchdog could not watch the keyboard: {problem}"
        ));
    }
    // A session notification that fails is a lost resync, not a lost hook:
    // the resume and contradiction re-reads still cover it.
    // SAFETY: plain values; the notification reaches this window.
    let notified = unsafe { WTSRegisterSessionNotification(watchdog, NOTIFY_FOR_THIS_SESSION) };
    if notified.is_err() {
        crate::diagnostics::report_line(
            "Windows did not tell Pane's keyboard hook of session changes; its modifiers are \
             re-read on a resume and on a contradiction only",
        );
    }
    let handle = hook.0 as usize;
    HOOKED.with(|hooked| {
        hooked.borrow_mut().replace(Hooked {
            recognizer: Recognizer::default(),
            hook,
            window,
            adapter,
            report: report.clone(),
            recording: false,
            last_seen: 0,
            unseen: 0,
            first: None,
            attempts: 0,
            given_up: false,
        });
    });
    // The pages are pinned where the state now lives, in the thread's own
    // storage: it does not move again, so the lock holds.
    let mut pinned = true;
    HOOKED.with(|hooked| {
        if let Some(state) = hooked.borrow().as_ref()
            && let Err(problem) = pin(state as *const Hooked as usize)
        {
            pinned = false;
            crate::diagnostics::report_line(&format!(
                "Pane's keyboard hook pages are not pinned in memory: {problem}"
            ));
        }
    });
    update(&report, |report| {
        report.pinned = pinned;
        report.hook = handle;
        report.watchdog = watchdog.0 as usize;
    });
    Ok(((), Some(watchdog)))
}

/// Serves the hook thread's queued requests.
fn serve_hook(message: &MSG, requests: &Mutex<VecDeque<HookRequest>>) {
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
            Some(HookRequest::Add(id, binding, answer)) => {
                let _ = answer.send(bind(id, binding));
            }
            Some(HookRequest::Remove(id, answer)) => {
                unbind(id);
                let _ = answer.send(());
            }
            Some(HookRequest::Record(on, answer)) => {
                record(on);
                let _ = answer.send(());
            }
        }
    }
}

/// Adds the binding to the recognizer of the hook thread's state.
fn bind(id: u32, binding: Binding) -> Result<(), String> {
    HOOKED
        .try_with(|hooked| {
            let Ok(mut held) = hooked.try_borrow_mut() else {
                return Err("the hook's state is in use".into());
            };
            let Some(state) = held.as_mut() else {
                return Err("the hook's state is gone".into());
            };
            state.recognizer.add(id, binding);
            Ok(())
        })
        .unwrap_or_else(|_| Err("the hook's state is gone".into()))
}

/// Turns the recording mode of the hook thread's recognizer on or off
/// (#260): while it is on, the callback holds the keys back and reports
/// what is pressed.
fn record(on: bool) {
    let _ = HOOKED.try_with(|hooked| {
        let Ok(mut held) = hooked.try_borrow_mut() else {
            return;
        };
        if let Some(state) = held.as_mut() {
            state.recognizer.recording(on);
            state.recording = on;
        }
    });
}

/// Removes the binding from the recognizer of the hook thread's state.
fn unbind(id: u32) {
    let _ = HOOKED.try_with(|hooked| {
        let Ok(mut held) = hooked.try_borrow_mut() else {
            return;
        };
        if let Some(state) = held.as_mut() {
            state.recognizer.remove(id);
        }
    });
}

/// The low-level keyboard hook's callback, called by the system on the
/// thread that installed it while it pumps its messages. It only steps
/// the recognizer and posts the press to the adapter's thread: it never
/// allocates, takes no lock another thread holds, or calls anything
/// outside the hook machinery. A panic must not unwind into Windows,
/// which would end Pane, so it catches one and lets the event through.
unsafe extern "system" fn hooked(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        // SAFETY: forwarded, as the hook contract asks for a code it does
        // not handle.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let decided = std::panic::catch_unwind(|| {
        // SAFETY: the system passes a KBDLLHOOKSTRUCT for a keyboard
        // event.
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        decide(event)
    });
    let (swallow, fired, mask) = decided.unwrap_or((false, None, false));
    if mask {
        // The Start-menu mask (#260): a tagged neutral key, injected
        // before the Windows key's release is passed along, so Windows
        // does not act on the key held alone. The injected events pass
        // through this hook untouched, being tagged.
        inject_neutral();
    }
    if let Some(binding) = fired
        && let Some(adapter) = adapter_thread()
    {
        // SAFETY: plain values, posted to the adapter's thread, which
        // pumps its messages.
        let _ =
            unsafe { PostThreadMessageW(adapter, WM_FIRED, WPARAM(binding as usize), LPARAM(0)) };
    }
    if swallow {
        LRESULT(1)
    } else {
        // SAFETY: forwarded, as an event Pane does not take must be.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// Injects the neutral key — `VK_NONAME`, a key no keyboard has — as a
/// press and a release, both tagged [`INJECTED_TAG`] (#260): the Start-menu
/// mask. Any key event between the Windows key's press and its release
/// keeps Windows from acting on the key held alone, so the mask is sent
/// before the release reaches the system, and its tag keeps Pane's own
/// hook from taking it.
fn inject_neutral() {
    let mut events = [INPUT::default(), INPUT::default()];
    for (event, up) in events.iter_mut().zip([false, true]) {
        event.r#type = INPUT_KEYBOARD;
        event.Anonymous.ki = KEYBDINPUT {
            wVk: VK_NONAME,
            wScan: 0,
            dwFlags: if up {
                KEYEVENTF_KEYUP
            } else {
                KEYBD_EVENT_FLAGS(0)
            },
            time: 0,
            dwExtraInfo: super::INJECTED_TAG,
        };
    }
    // SAFETY: the events are this slice's, both of them.
    let _ = unsafe { SendInput(&events, size_of::<INPUT>() as i32) };
}

/// Steps the recognizer with one keyboard event, saying whether to
/// swallow the event, which binding it fired, and whether the
/// Start-menu mask is to be injected first (#260). While a recording
/// session listens, the keys are held back from the system — except
/// Pane's own tagged ones, which pass untouched, and Escape and Tab,
/// which the recorder's own cancellation keys take — and what the
/// session recognizes is posted to the adapter's thread; the session
/// ends when a window of another process is in front, so the keys
/// belong to it and not to Pane's recorder.
fn decide(event: &KBDLLHOOKSTRUCT) -> (bool, Option<u32>, bool) {
    HOOKED
        .try_with(|hooked| {
            let mut held = hooked.try_borrow_mut().ok()?;
            let state = held.as_mut()?;
            let key = KeyEvent {
                key: event.vkCode,
                scan: event.scanCode,
                pressed: !event.flags.contains(LLKHF_UP),
                extended: event.flags.contains(LLKHF_EXTENDED),
                injected: event.flags.contains(LLKHF_INJECTED),
                tag: event.dwExtraInfo,
                time: u64::from(event.time),
            };
            if state.recording {
                // Escape and Tab pass, so the recorder's own cancellation
                // keys work as they do today; everything else is held
                // back, and what the session recognizes is posted.
                if key.key == VK_ESCAPE.0 as u32 || key.key == VK_TAB.0 as u32 {
                    return Some((false, None, false));
                }
                if !pane_in_front() {
                    // Another application's window is in front: the keys
                    // are the user's there, not the recorder's. The
                    // session ends, and this event passes.
                    state.recording = false;
                    state.recognizer.recording(false);
                    post(WM_RECORDED_ENDED, 0);
                    return Some((false, None, false));
                }
                Some(match state.recognizer.step(key) {
                    Decision::Recorded(report) => {
                        post(WM_RECORDED, pack(report));
                        (true, None, false)
                    }
                    Decision::Swallow => (true, None, false),
                    Decision::Pass => (false, None, false),
                    Decision::Resync => {
                        resync(&mut state.recognizer);
                        (false, None, false)
                    }
                    // The recording mode answers none of these.
                    _ => (false, None, false),
                })
            } else {
                Some(match state.recognizer.step(key) {
                    Decision::Fire(binding) => (true, Some(binding), false),
                    Decision::Injected(binding) => (false, Some(binding), false),
                    Decision::Tap { binding, mask } => (false, Some(binding), mask),
                    Decision::Mask => (false, None, true),
                    Decision::Swallow => (true, None, false),
                    Decision::Pass => (false, None, false),
                    Decision::Resync => {
                        // The event contradicts the state Pane holds:
                        // re-read the modifiers' real state, as an unlock
                        // or a resume does.
                        resync(&mut state.recognizer);
                        (false, None, false)
                    }
                    Decision::Recorded(_) => (false, None, false),
                })
            }
        })
        .ok()
        .flatten()
        .unwrap_or((false, None, false))
}

/// Posts `message` with `word` to the adapter's thread; a post that
/// fails leaves the recognizer's answer as it was, and the hotkey
/// thread will see the session's end when it ends.
fn post(message: u32, word: usize) {
    if let Some(adapter) = adapter_thread() {
        // SAFETY: plain values, posted to the adapter's thread, which
        // pumps its messages.
        let _ = unsafe { PostThreadMessageW(adapter, message, WPARAM(word), LPARAM(0)) };
    }
}

/// Whether a window of Pane's own process is the one in front, so a
/// recording session may keep the keys it would hold back (#260): a
/// window of any other process in front ends the session, whoever it
/// belongs to — the keys are that application's to take.
fn pane_in_front() -> bool {
    // SAFETY: no arguments; the front window's handle is only read.
    let front = unsafe { GetForegroundWindow() };
    if front.is_invalid() {
        return false;
    }
    let mut process = 0;
    // SAFETY: a window handle; the window's process id is written to it.
    // The call's answer is the thread that made the window, not the
    // process — the id the comparison needs is the one written.
    let thread = unsafe { GetWindowThreadProcessId(front, Some(&mut process)) };
    // SAFETY: no arguments; it reads this process's own id.
    let ours = unsafe { GetCurrentProcessId() };
    thread != 0 && process == ours
}

/// The adapter's thread, to post the presses to; `None` if the hook
/// thread's state is gone.
fn adapter_thread() -> Option<u32> {
    HOOKED
        .try_with(|hooked| {
            let held = hooked.try_borrow().ok()?;
            held.as_ref().map(|state| state.adapter)
        })
        .unwrap_or(None)
}

/// The watchdog's window: the raw keyboard input that tells the keyboard
/// is in use, the session notification of an unlock, the power
/// notification of a resume, and the stop message. A panic must not
/// unwind into Windows, which would end Pane, so it catches one.
extern "system" fn watchdog(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let handled = std::panic::catch_unwind(|| watched(window, message, wparam, lparam));
    match handled {
        Ok(result) => result,
        Err(_) => {
            crate::diagnostics::report_line(
                "Pane's keyboard hook watchdog failed on a message; it goes on watching",
            );
            LRESULT(0)
        }
    }
}

/// The watchdog's messages: the raw input, the session and power
/// notifications that re-read the modifiers, and the stop message.
fn watched(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if stop_sent(message) {
        return LRESULT(0);
    }
    if message == WM_INPUT {
        // The keyboard delivered an event; the hook must have seen the
        // ones before it. The count is the message itself — Pane reads
        // no raw input data — so zero is the whole handling of it.
        let _ = HOOKED.try_with(|hooked| {
            let Ok(mut held) = hooked.try_borrow_mut() else {
                return;
            };
            if let Some(state) = held.as_mut() {
                watch(state);
            }
        });
        return LRESULT(0);
    }
    // An unlock — the secure desktop had the keyboard, so releases were
    // missed — or a resume: the modifiers' real state is re-read.
    let unlocked = message == WM_WTSSESSION_CHANGE && wparam.0 == SESSION_UNLOCK;
    let resumed = message == WM_POWERBROADCAST
        && (wparam.0 == PBT_APMRESUMEAUTOMATIC as usize
            || wparam.0 == PBT_APMRESUMESUSPEND as usize);
    if unlocked || resumed {
        resync_hooked();
        return LRESULT(0);
    }
    def(window, message, wparam, lparam)
}

/// The default processing of a message the watchdog window does not
/// handle.
fn def(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the arguments are those the procedure was called with.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The watchdog's check on one raw key event: the hook sees every key
/// event the raw input also reports — the keyboard's own, another tool's
/// injected keys and Pane's own among them — so when several raw events
/// arrive with the hook seeing none, Windows has removed the hook and it
/// is installed again. The keys a tool injects reach the raw input as the
/// keyboard's do but are not the keyboard's, so counting only the
/// keyboard's own here would take a busy remapper for a removed hook.
fn watch(state: &mut Hooked) {
    if state.given_up {
        return;
    }
    let seen = state.recognizer.events();
    if seen != state.last_seen {
        state.last_seen = seen;
        state.unseen = 0;
    }
    state.unseen += 1;
    if state.unseen < RAW_BEFORE_REINSTALL {
        return;
    }
    state.unseen = 0;
    reinstall(state);
}

/// Installs the hook again: Windows has removed it. The attempts within a
/// short time are counted, and after too many Pane gives up and says so,
/// leaving the hook-based hotkeys dead until Pane starts again.
fn reinstall(state: &mut Hooked) {
    let now = Instant::now();
    if state
        .first
        .is_none_or(|first| now - first > REINSTALL_WINDOW)
    {
        state.first = Some(now);
        state.attempts = 0;
    }
    state.attempts += 1;
    if state.attempts > REINSTALLS_BEFORE_GIVING_UP {
        state.given_up = true;
        // SAFETY: this thread's own hook; Windows has removed it, so this
        // fails harmlessly and ends Pane's claim on it.
        let _ = unsafe { UnhookWindowsHookEx(state.hook) };
        let why = format!(
            "Windows removed Pane's keyboard hook {} times within {} seconds; Pane gave up \
             reinstalling it, and the hotkeys it dispatches do nothing until Pane starts again",
            state.attempts,
            REINSTALL_WINDOW.as_secs()
        );
        crate::diagnostics::report_line(&why);
        update(&state.report, |report| {
            report.hook = 0;
            report.given_up = Some(why);
        });
        return;
    }
    // SAFETY: this thread's own hook; Windows has removed it, so this
    // fails harmlessly and a fresh one takes its place either way.
    let _ = unsafe { UnhookWindowsHookEx(state.hook) };
    match install() {
        Ok(hook) => {
            state.hook = hook;
            update(&state.report, |report| {
                report.reinstalled += 1;
                report.hook = hook.0 as usize;
            });
            // The state went stale while the hook was gone: re-read the
            // modifiers, as a resume does.
            resync(&mut state.recognizer);
        }
        Err(problem) => {
            crate::diagnostics::report_line(&format!(
                "Windows would not reinstall Pane's keyboard hook: {problem}"
            ));
        }
    }
}

/// Re-reads the recognizer of the hook thread's state.
fn resync_hooked() {
    let _ = HOOKED.try_with(|hooked| {
        let Ok(mut held) = hooked.try_borrow_mut() else {
            return;
        };
        if let Some(state) = held.as_mut() {
            resync(&mut state.recognizer);
        }
    });
}

/// Takes the modifiers' real state into the recognizer: for a session
/// unlock, a resume, a reinstall, or the re-read a contradictory event
/// asked for.
fn resync(recognizer: &mut Recognizer) {
    recognizer.resync(&real());
}

/// The modifiers' real state, as the system reports it: the keys held,
/// by their virtual-key codes, whichever side of each modifier they
/// are.
fn real() -> Vec<u32> {
    let down = |key: VIRTUAL_KEY| {
        // SAFETY: the call only reads that key's state.
        unsafe { (GetAsyncKeyState(key.0 as i32) as u16) & 0x8000 != 0 }
    };
    [
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ]
    .into_iter()
    .filter(|key| down(*key))
    .map(|key| key.0 as u32)
    .collect()
}

/// Installs the low-level keyboard hook on this thread, which the system
/// calls from the thread's message loop.
fn install() -> Result<HHOOK, String> {
    // SAFETY: the procedure is this module's, kept for as long as the
    // process runs; a low-level hook needs no module handle and watches
    // every thread.
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hooked), None, 0) }
        .map_err(|error| error.message())
}

/// Registers the watchdog window for the raw input of the keyboard, so it
/// receives `WM_INPUT` for every key event whatever window has focus: the
/// evidence the hook is still seeing them.
fn watch_input(window: HWND) -> Result<(), String> {
    let device = RAWINPUTDEVICE {
        // The generic desktop page, and the keyboard on it.
        usUsagePage: 0x01,
        usUsage: 0x06,
        // Received without focus: the hook watches the whole desktop, and
        // so must the watchdog.
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: window,
    };
    // SAFETY: one device, registered for this window.
    unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
        .map_err(|error| error.message())
}

/// Locks the pages the hook's callback needs in memory: those its code
/// and the recognizer's are on, and those of the state it reads, with a
/// page of margin — after raising the process's minimum working set by an
/// allowance for them, since a lock that would not fit it fails. If
/// Windows refuses, `Err` says the pages are not pinned.
fn pin(state: usize) -> Result<(), String> {
    raise_working_set(PIN_ALLOWANCE)?;
    // The callback and the recognizer's step are this module's code; the
    // pages around them, with a margin, are what runs on the hot path.
    let callback: unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT = hooked;
    let step: fn(&mut Recognizer, KeyEvent) -> Decision = Recognizer::step;
    lock_pages(callback as usize, 4)
        .and_then(|_| lock_pages(step as usize, 4))
        .and_then(|_| lock_pages(state, 3))
}

/// Locks the `pages` pages `address` is in the middle of, for as long as
/// the process runs.
fn lock_pages(address: usize, pages: usize) -> Result<(), String> {
    let base = (address / PAGE) * PAGE;
    // SAFETY: the range is this module's code or the hook thread's own
    // state, kept for as long as the process runs.
    unsafe { VirtualLock(base as *const _, (pages + 1) * PAGE) }.map_err(|error| error.message())
}

/// Raises the process's minimum working set by `by`, so the pages the
/// hook pins fit in it.
fn raise_working_set(by: usize) -> Result<(), String> {
    // SAFETY: the calling process's own pseudo-handle.
    let process = unsafe { GetCurrentProcess() };
    let (mut minimum, mut maximum, mut flags) = (0, 0, 0);
    // SAFETY: the sizes are writable out-parameters it reads.
    let read =
        unsafe { GetProcessWorkingSetSizeEx(process, &mut minimum, &mut maximum, &mut flags) };
    if !read.as_bool() {
        return Err("Windows would not say the working set".into());
    }
    // SAFETY: the sizes it read, raised by the allowance.
    let raised = unsafe {
        SetProcessWorkingSetSizeEx(
            process,
            minimum + by,
            maximum + by,
            SETPROCESSWORKINGSETSIZEEX_FLAGS(flags),
        )
    };
    raised.map_err(|error| error.message())
}
