//! The selected text on Windows (see the parent module): UI Automation
//! first, in a worker process of Pane's own program — started with the
//! internal argument, held in a tree of its own (a Job Object) so it ends
//! when Pane does however Pane ends, asked one request at a time over its
//! standard input and output, waiting [`WORKER_WAIT`] and replaced with a
//! fresh one when it takes longer — and, wherever UI Automation gives
//! nothing or fails, a simulated copy: the clipboard is held, the target
//! is brought back to the front, a tagged Ctrl+C is sent, the clipboard's
//! sequence number is waited for, the text read and the clipboard put
//! back, tagged, with the changes of that window kept from Pane's own
//! history — Windows' own history may keep the target's copy, which Pane
//! cannot mark.
//!
//! The worker reads the focused element of the target's own thread — the
//! target is behind Pane's window when a command asks for its selection —
//! through the Text pattern of the UI Automation element; a window met
//! for the first time has its accessibility tree woken first (a
//! Chromium-based application builds its tree only once something asks
//! the window for its accessibility object), and a read that then finds
//! nothing is tried again shortly after, once. "Nothing is selected" is
//! an answer, distinct from a failure, which counts toward the pause:
//! three failures within ten minutes pause the UI Automation reads for
//! ten minutes, the simulated copy carrying them meanwhile.

use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ::windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use ::windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use ::windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_C, VK_CONTROL,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetGUIThreadInfo, GetWindowThreadProcessId, IsHungAppWindow, IsWindow,
    OBJID_CLIENT, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETOBJECT,
};

use crate::clipboard::ignoring::IGNORED;
use crate::clipboard::windows::{hold_clip, read_clip, restore_clip};
use crate::process_tree::ProcessTree;
use crate::system::front::windows::Recorded;
use crate::system::windows::{bring_to_front, elevated};
use crate::system::{Clip, MAX_CLIPBOARD_TEXT, SystemError};
use crate::util::lock;
use crate::windows_shell::Com;

use super::{
    Answer, AskError, Asking, Pause, Reading, WORKER_ARGUMENT, WORKER_WAIT, Waking, request_line,
    requested, retry_after,
};

/// How long the copy waits for the clipboard's sequence number to change
/// after the keys are sent (proposed 500 ms, #125), and how often it looks
/// while it waits.
const SEQUENCE_WAIT: Duration = Duration::from_millis(500);
const LOOK_EVERY: Duration = Duration::from_millis(10);

/// How long the caller waits for the copy worker as a whole, bounding
/// even a call into a window that never answers: as long as the paste's
/// wait is.
const COPYING_WAIT: Duration = Duration::from_secs(10);

/// How long the message that wakes a window's accessibility tree waits
/// for a window that does not answer.
const WAKE_WAIT: Duration = Duration::from_millis(200);

/// The selected-text read on Windows. One read runs at a time: each is
/// asked for off the runtime's thread (`runtime/system_functions.rs`) and
/// bounded, however slow the application in front is, so reading never
/// freezes Pane.
pub(in crate::system) struct Selected {
    /// How the UI Automation read is asked for: the worker process of
    /// Pane's own program, or this process on a thread (the real-input
    /// adapter test's).
    asking: Box<dyn Asking>,
    /// The failure pause of the UI Automation reads.
    pause: Mutex<Pause>,
    /// Held while a read runs, so reads answer one at a time.
    reading: Mutex<()>,
}

impl Selected {
    /// Reads through the worker process of Pane's own program.
    pub(in crate::system) fn worker() -> Selected {
        Selected::with(Box::new(Worker {
            running: Mutex::new(None),
        }))
    }

    /// Reads in this process, on a thread of its own: the real-input
    /// adapter test's, which cannot start Pane's program as its worker.
    /// The read it asks for is the code the worker runs.
    pub(in crate::system) fn reading_here() -> Selected {
        Selected::with(Box::new(Reading::new(Arc::new(|window: usize| {
            let mut waking = Waking::default();
            selection(window, &mut waking)
        }))))
    }

    fn with(asking: Box<dyn Asking>) -> Selected {
        Selected {
            asking,
            pause: Mutex::new(Pause::default()),
            reading: Mutex::new(()),
        }
    }

    /// Reads the selection of the application `target` stands for: UI
    /// Automation first, unless three failures within ten minutes have
    /// paused its reads, and, wherever UI Automation gives nothing or
    /// fails, the simulated copy. Nothing selected answers `Ok(None)`,
    /// which is not a failure: a command is told so, rather than given an
    /// empty string silently.
    pub(in crate::system) fn read(&self, target: &Recorded) -> Result<Option<String>, SystemError> {
        let _one_at_a_time = lock(&self.reading);
        if !lock(&self.pause).paused(crate::util::now_ms()) {
            match self.asking.ask(target.window) {
                Ok(Answer::Text { text }) => {
                    // An empty selection is no selection.
                    return Ok((!text.is_empty()).then_some(text));
                }
                Ok(Answer::Nothing) => return Ok(None),
                // UI Automation gives nothing: the copy is next.
                Ok(Answer::GivesNothing) => {}
                // The read failed, or the worker took too long and was
                // replaced: the failure counts toward the pause, and the
                // copy carries the read.
                Ok(Answer::Failed { .. }) | Err(_) => {
                    lock(&self.pause).noted(crate::util::now_ms());
                }
            }
        }
        copied(target)
    }
}

/// The worker's own main, entered when Pane's program is started with the
/// internal argument (see `pane`'s `main`): answers one request at a time
/// — the window whose selection is asked for — until Pane ends it or its
/// input closes, and never returns. COM and the UI Automation object live
/// on this thread, in this process, where a slow target can hold only
/// the worker, which Pane replaces.
pub fn serve() -> ! {
    let stdin = std::io::stdin();
    // First contact with a window wakes its accessibility tree, so the
    // worker remembers the windows it has woken across the reads it is
    // asked for.
    let mut waking = Waking::default();
    loop {
        let mut line = String::new();
        // Pane ended the worker, or its input closed: it is done.
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let answer = match requested(&line) {
            Ok(window) => selection(window, &mut waking),
            Err(why) => Answer::Failed { why },
        };
        if writeln!(std::io::stdout().lock(), "{}", answer.line()).is_err() {
            break;
        }
    }
    std::process::exit(0)
}

/// The worker process of Pane's own program, asked for one read at a
/// time: started with the internal argument and held in a tree of its
/// own, so it ends when Pane does however Pane ends; replaced when it
/// takes too long or ends, the next ask starting a fresh one. Its
/// standard input carries the request lines; a thread of Pane's reads
/// the answer lines from its standard output.
struct Worker {
    /// The worker running now: `None` once it has been replaced or
    /// ended, until the next ask starts a fresh one.
    running: Mutex<Option<Running>>,
}

impl Asking for Worker {
    fn ask(&self, window: usize) -> Result<Answer, AskError> {
        let mut running = lock(&self.running);
        if running.is_none() {
            match Running::start() {
                Ok(started) => *running = Some(started),
                Err(why) => {
                    crate::diagnostics::report_line(&why);
                    return Err(AskError::Ended);
                }
            }
        }
        let worker = running.as_mut().expect("a worker is running");
        // One small request line, which the pipe carries even while a
        // worker that stopped reading is being replaced.
        if writeln!(worker.requests, "{}", request_line(window)).is_err()
            || worker.requests.flush().is_err()
        {
            *running = None;
            return Err(AskError::Ended);
        }
        match worker.answers.recv_timeout(WORKER_WAIT) {
            Ok(line) => match Answer::parse(&line) {
                Ok(answer) => Ok(answer),
                Err(why) => {
                    crate::diagnostics::report_line(&why);
                    *running = None;
                    Err(AskError::Ended)
                }
            },
            // The worker is replaced: this one is ended, and the next ask
            // starts a fresh one.
            Err(mpsc::RecvTimeoutError::Timeout) => {
                *running = None;
                Err(AskError::TimedOut)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                *running = None;
                Err(AskError::Ended)
            }
        }
    }
}

/// A worker process running. Dropping it ends the worker, however it
/// answered, and reaps it.
struct Running {
    child: Child,
    /// The worker's process tree: its Job Object ends the worker when
    /// Pane ends, however Pane ends, and whatever the worker left
    /// running with it.
    _tree: ProcessTree,
    /// The request lines are written to its standard input.
    requests: ChildStdin,
    /// Its answer lines, from the thread reading its standard output.
    answers: mpsc::Receiver<String>,
}

impl Running {
    /// Starts a worker: Pane's own program, with the internal argument,
    /// in a tree of its own.
    fn start() -> Result<Running, String> {
        let program = std::env::current_exe()
            .map_err(|error| format!("Pane's own program could not be found: {error}"))?;
        let mut command = Command::new(program);
        command.arg(WORKER_ARGUMENT);
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        // The worker answers on its standard output and writes nothing of
        // its own anywhere else.
        command.stderr(Stdio::null());
        ProcessTree::prepare_program(&mut command, false);
        let mut child = command
            .spawn()
            .map_err(|error| format!("Pane could not start its selected-text worker: {error}"))?;
        let tree = match ProcessTree::adopt_program(&child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Pane could not hold its selected-text worker in a tree: {error}"
                ));
            }
        };
        let requests = child.stdin.take().expect("the worker's input is piped");
        let answers = child.stdout.take().expect("the worker's output is piped");
        let (sent, answering) = mpsc::channel();
        let reading = std::thread::Builder::new()
            .name("pane-selected-answers".into())
            .spawn(move || {
                let mut answers = std::io::BufReader::new(answers);
                loop {
                    let mut line = String::new();
                    // The worker's answers are one line each; its end
                    // closes the pipe, which ends this thread.
                    match answers.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if sent.send(line).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        if reading.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Pane could not read its selected-text worker's answers".into());
        }
        Ok(Running {
            child,
            _tree: tree,
            requests,
            answers: answering,
        })
    }
}

impl Drop for Running {
    /// Ends the worker and reaps it: one that already ended is only
    /// reaped, and its tree is dropped with it, ending whatever it left.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads the selection of `window`'s focused element through UI
/// Automation, giving `waking` the first contact with the window so its
/// accessibility tree is woken before it is read, and retrying shortly
/// after a read that finds nothing, once.
fn selection(window: usize, waking: &mut Waking) -> Answer {
    let window = HWND(window as *mut _);
    // COM on this thread, single-threaded, for the UI Automation object:
    // the read runs on the worker's own thread (or a thread of Pane's,
    // in the real-input test), so it is initialized here and ends with
    // it.
    let _com = match Com::new() {
        Ok(com) => com,
        Err(why) => return Answer::Failed { why },
    };
    // SAFETY: created on this thread, with COM initialized above, and
    // released with it.
    let uia: IUIAutomation =
        match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
            Ok(uia) => uia,
            Err(error) => {
                return Answer::Failed {
                    why: format!("UI Automation could not be started: {}", error.message()),
                };
            }
        };
    // The element to read is the one the target's own thread has focused
    // — the target is behind Pane's window when the command asks for its
    // selection — or the target's window itself when its thread names no
    // focus.
    let focused = focused_of(window).unwrap_or(window);
    if waking.first_contact(focused.0 as usize) {
        wake(focused);
    }
    let mut tried = 0;
    loop {
        tried += 1;
        if let Some(answer) = selection_of(&uia, focused) {
            return answer;
        }
        // A tree just woken needs the time to build: the read is tried
        // again shortly after, once.
        match retry_after(tried) {
            Some(wait) => std::thread::sleep(wait),
            None => break,
        }
    }
    Answer::GivesNothing
}

/// The window the thread of `window` has focused, or `None` when the
/// thread names no focus, and `window` itself is read.
fn focused_of(window: HWND) -> Option<HWND> {
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
    if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_err() {
        return None;
    }
    (!info.hwndFocus.is_invalid()).then_some(info.hwndFocus)
}

/// The selection `focused`'s element exposes through UI Automation's Text
/// pattern, or `None` when the element exposes none, or cannot be read:
/// [`Answer::Text`] when a selection is exposed, [`Answer::Nothing`] when
/// none is.
fn selection_of(uia: &IUIAutomation, focused: HWND) -> Option<Answer> {
    // SAFETY: `focused` is a window handle; the element it answers is
    // released with it.
    let element = unsafe { uia.ElementFromHandle(focused) }.ok()?;
    // SAFETY: the element's own method, on this thread; the pattern is
    // released with it. An element without the pattern answers an error,
    // which is no selection exposed.
    let pattern: IUIAutomationTextPattern =
        unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
    // SAFETY: the pattern's own method, on this thread; the ranges are
    // released with it.
    let ranges = unsafe { pattern.GetSelection() }.ok()?;
    // SAFETY: as above; the length of the array it answered.
    let count = unsafe { ranges.Length() }.ok()?;
    // A selection can be discontiguous: the texts of its ranges are
    // joined, one a line.
    let mut text = String::new();
    for index in 0..count {
        // SAFETY: as above; `index` is below the array's length.
        let range = unsafe { ranges.GetElement(index) }.ok()?;
        // SAFETY: the range's own method, on this thread; the whole of
        // its text is read, since a command is told when a selection is
        // too long rather than given a cut of it.
        let read = unsafe { range.GetText(-1) }.ok()?;
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf16_lossy(&read));
    }
    Some(if text.is_empty() {
        Answer::Nothing
    } else {
        Answer::Text { text }
    })
}

/// Wakes `window`'s accessibility tree, as a first contact with it: asks
/// the window for its accessibility object the way a screen reader's
/// first probe does (`WM_GETOBJECT`), which a Chromium-based application
/// answers by building its tree. Bounded, so a window that does not
/// answer cannot hold the read.
fn wake(window: HWND) {
    // SAFETY: plain values; the message's result is not wanted, and a
    // window that is hung answers nothing within the timeout.
    let _ = unsafe {
        SendMessageTimeoutW(
            window,
            WM_GETOBJECT,
            WPARAM(0),
            LPARAM(OBJID_CLIENT.0 as isize),
            SMTO_ABORTIFHUNG,
            WAKE_WAIT.as_millis() as u32,
            None,
        )
    };
}

/// The simulated copy of the selection: on a worker of its own, as the
/// paste is, because bringing a window to the front can wait on that
/// window's process, which the caller must not.
fn copied(target: &Recorded) -> Result<Option<String>, SystemError> {
    // The worker takes the target as its own: it ends after the caller
    // has stopped waiting for it.
    let target = target.clone();
    let (answer, answered) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("pane-selected".into())
        .spawn(move || {
            let _ = answer.send(copy_selection(&target));
        });
    if worker.is_err() {
        return Err(SystemError::Failed(
            "Pane could not start the worker that copies the selection".into(),
        ));
    }
    match answered.recv_timeout(COPYING_WAIT) {
        Ok(done) => done,
        // The worker is left to end on its own, as the paste's is, once
        // its call returns.
        Err(_) => Err(refused(CopyRefusal::TookTooLong)),
    }
}

/// The copy itself: brings the target back to the front (it must be, to
/// take the keys), saves what the clipboard holds, sends the target a
/// tagged Ctrl+C, waits for its copy to change the clipboard's sequence
/// number, reads the text and puts the clipboard back. The target's copy
/// is not Pane's and carries no marker Pane could give it, so the
/// clipboard's changes are ignored while the window is armed, and neither
/// the target's copy nor the restore reaches Pane's history; Windows' own
/// history may keep the target's copy, which Pane cannot mark (#125). A
/// copy that changed nothing copied nothing: no selection, which is not
/// a failure.
fn copy_selection(target: &Recorded) -> Result<Option<String>, SystemError> {
    let window = HWND(target.window as *mut _);
    // A window that is gone, not responding, or of a process running as
    // administrator — which would drop the keys Pane sends — stops the
    // copy before anything is sent, as the paste does.
    // SAFETY: a handle the watcher recorded.
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        return Err(refused(CopyRefusal::Gone));
    }
    // SAFETY: as above.
    if unsafe { IsHungAppWindow(window) }.as_bool() {
        return Err(refused(CopyRefusal::NotResponding));
    }
    if elevated(target.process) {
        return Err(refused(CopyRefusal::Elevated));
    }
    // What the clipboard holds, saved to be put back once the copy is
    // read; one that cannot be read now is not put back then, and the
    // copy leaves what it finds.
    let held = hold_clip().ok();
    // SAFETY: no arguments.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    IGNORED.around(|| {
        // The target must be in front to take the keys; the call answers
        // only that it did not come.
        if bring_to_front(window).is_err() {
            return Err(refused(CopyRefusal::NotFront));
        }
        send_copy_keys()?;
        let copied = waited_sequence(sequence);
        let text = copied
            .then(|| read_clip(MAX_CLIPBOARD_TEXT).ok().flatten())
            .flatten()
            .and_then(|clip| match clip {
                Clip::Text(text) => Some(text),
                // A copy of files is not a selection of text.
                Clip::File(_) => None,
            });
        // Put the clipboard back, tagged as a concealed copy is, unless
        // nothing was copied over it. A courtesy: the read is done, so a
        // failure here does not fail it.
        if copied && let Some(held) = &held {
            let _ = restore_clip(held);
        }
        Ok(text)
    })
}

/// Waits until the clipboard's sequence number is no longer `before` —
/// the target has copied — at most [`SEQUENCE_WAIT`], looking every few
/// moments.
fn waited_sequence(before: u32) -> bool {
    let deadline = Instant::now() + SEQUENCE_WAIT;
    while Instant::now() < deadline {
        // SAFETY: no arguments.
        if unsafe { GetClipboardSequenceNumber() } != before {
            return true;
        }
        std::thread::sleep(LOOK_EVERY);
    }
    // SAFETY: no arguments.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    sequence != before
}

/// Sends the target a Ctrl+C, as the user pressing it would, every key
/// tagged as Pane's own, so its own keyboard hook and other tools' know
/// the keys are Pane's, not the user's.
fn send_copy_keys() -> Result<(), SystemError> {
    let key = |key: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS::default()
                },
                time: 0,
                dwExtraInfo: crate::hotkeys::INJECTED_TAG,
            },
        },
    };
    let keys = [
        key(VK_CONTROL, false),
        key(VK_C, false),
        key(VK_C, true),
        key(VK_CONTROL, true),
    ];
    // SAFETY: `keys` is an array of INPUT of its own length.
    let sent = unsafe { SendInput(&keys, std::mem::size_of::<INPUT>() as i32) };
    if sent != keys.len() as u32 {
        return Err(SystemError::Failed(
            "Windows did not take the keys Pane sent to copy the selection".into(),
        ));
    }
    Ok(())
}

/// Why a simulated copy of the selection did not happen: each answers the
/// command with a failure saying so, and nothing is sent to the target.
/// Unlike a paste, nothing stays anywhere for the user: the clipboard is
/// as it was.
enum CopyRefusal {
    /// The target's window is gone: its application closed.
    Gone,
    /// The target's window is not responding, and would not take the
    /// keys.
    NotResponding,
    /// The target's process is running as administrator, which would
    /// drop the keys Pane sends.
    Elevated,
    /// The target's window did not come back to the front, so no keys
    /// were sent.
    NotFront,
    /// The copy worker took longer than Pane waits for it.
    TookTooLong,
}

impl CopyRefusal {
    /// What the command is told, for the user.
    fn message(&self) -> String {
        match self {
            CopyRefusal::Gone => "The application that was in front is gone, so its \
                 selection cannot be read"
                .into(),
            CopyRefusal::NotResponding => "The application that was in front is not \
                 responding, so its selection cannot be read"
                .into(),
            CopyRefusal::Elevated => "The application that was in front is running as \
                 administrator, which Pane cannot read the selection of"
                .into(),
            CopyRefusal::NotFront => "The application that was in front did not come back \
                 to the front, so its selection cannot be read"
                .into(),
            CopyRefusal::TookTooLong => "Pane waited too long to read the selection of \
                 the application in front"
                .into(),
        }
    }
}

/// `refusal` as the command's answer.
fn refused(refusal: CopyRefusal) -> SystemError {
    SystemError::Failed(refusal.message())
}
