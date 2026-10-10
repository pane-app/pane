//! The system functions on Windows (see the parent module): the clipboard
//! through the clipboard adapter's own window, opening with
//! `ShellExecuteExW`, File Explorer's selection with
//! `SHOpenFolderAndSelectItems`, and the Recycle Bin with
//! `SHFileOperationW`. COM is initialized on the calling thread (one of
//! Pane's own, for this call) while the shell is used.
//!
//! The Windows power features' halves (#125) are here too. The front
//! application is the window the watcher records (given to this adapter
//! when Pane starts, `crate::system::native`): its name and icon are
//! resolved best-effort through the window's AppUserModelID, its
//! program's path or its process's package identity against the
//! installed applications. Paste brings that window back to the front on
//! a worker with a timeout, waiting until it really is in front, then
//! sends it a tagged Ctrl+V (`SendInput`, every key Pane injects carries
//! Pane's own tag) — skipping a window that is gone, not responding or
//! running as administrator (detected from its process's elevation
//! before any key is sent, since Windows would drop them), each answered
//! with a failure that leaves what was put on the clipboard there, still
//! tagged, for the user to paste by hand. The clipboard's earlier
//! contents are put back after a short delay, unless something else was
//! copied meanwhile: every format it held in ordinary memory, tagged as
//! a concealed copy is, so neither Pane's history nor Windows' own keeps
//! the paste or the restore. The selected text is read through
//! `selected` (#262): UI Automation in a worker process of Pane's own
//! program and, where that gives nothing, a simulated copy that leaves
//! the clipboard as it was.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ::windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, PROPERTYKEY};
use ::windows::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use ::windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToString};
use ::windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use ::windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use ::windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use ::windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    ILCreateFromPathW, ILFree, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
    SHFILEOPSTRUCTW, SHFileOperationW, SHOpenFolderAndSelectItems, ShellExecuteExW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsHungAppWindow, IsWindow,
    SW_SHOWNORMAL, SetForegroundWindow,
};
use ::windows::core::{GUID, HRESULT, PCWSTR};

use super::front::windows::{Recorded, Watcher};
use super::front::{NO_TARGET, PasteRefusal, Target, front_application as resolve};
use super::selected::Selected;
use super::{Clip, FrontApplication, MAX_CLIPBOARD_TEXT, NotTrashed, System, SystemError, missing};
use crate::applications::identity::Catalog;
use crate::applications::{Discovery, StartMenu};
use crate::clipboard::windows::{Held, hold_clip, put_clip, read_clip, restore_clip};
use crate::util::wide;
use crate::windows_shell::Com;

/// The system functions on Windows.
pub(super) struct WindowsSystem {
    /// The watcher of the application in front (#125), started when Pane
    /// starts and given to this adapter; `None` when its hook could not
    /// be installed, and the front application and paste answer that.
    front: Option<Watcher>,
    /// The paste worker's bookkeeping, so a newer request supersedes an
    /// older one still working.
    pasting: Pasting,
    /// What the clipboard held when a paste was asked about
    /// ([`System::can_paste`]), to put back once the paste is done.
    held: Mutex<Option<Held>>,
    /// The installed applications, scanned once, best-effort, for the
    /// front application's name and icon.
    installed: OnceLock<Catalog>,
    /// The selected-text read (#262).
    selected: Selected,
}

impl WindowsSystem {
    /// The system functions, with `front` watching the application in
    /// front (see [`crate::system::native`]).
    pub(super) fn new(front: Option<Watcher>) -> WindowsSystem {
        WindowsSystem::with_selected(front, Selected::worker())
    }

    /// The system functions whose selected-text reads run in this
    /// process (see [`crate::system::native_selected_text_in_process`]):
    /// the real-input adapter test's.
    pub(super) fn reading_selected_here(front: Option<Watcher>) -> WindowsSystem {
        WindowsSystem::with_selected(front, Selected::reading_here())
    }

    fn with_selected(front: Option<Watcher>, selected: Selected) -> WindowsSystem {
        WindowsSystem {
            front,
            pasting: Pasting::default(),
            held: Mutex::new(None),
            installed: OnceLock::new(),
            selected,
        }
    }

    /// The installed applications, scanned once, best-effort: the Start
    /// menu, the Desktops, the taskbar's pins and the Apps folder, as
    /// the applications module finds them. What is installed after the
    /// first scan is named by its window's title until Pane restarts
    /// (#124 refines this).
    fn installed(&self) -> &Catalog {
        self.installed.get_or_init(|| {
            let sources = StartMenu::from_env().sources().unwrap_or_default();
            Catalog::new(sources)
        })
    }
}

impl System for WindowsSystem {
    fn copy(&self, clip: &Clip, concealed: bool) -> Result<(), String> {
        put_clip(clip, concealed)
    }

    fn read_clipboard(&self) -> Result<Option<Clip>, String> {
        read_clip(MAX_CLIPBOARD_TEXT)
    }

    fn open(&self, target: &str, application: Option<&str>) -> Result<(), String> {
        match application {
            None => shell_execute(target, None),
            // The application (its program, its Start menu shortcut or its
            // `shell:AppsFolder` id) opens with the target as its one
            // argument, as "Open with" does.
            Some(application) => shell_execute(application, Some(&argument(target))),
        }
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        if let Some(missing) = missing(path) {
            return Err(missing);
        }
        let _com = Com::new()?;
        let wide = wide(path);
        // SAFETY: a NUL-terminated path that outlives the call; the list it
        // answers is freed below.
        let item = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
        if item.is_null() {
            return Err(format!("File Explorer cannot find {}", path.display()));
        }
        // SAFETY: `item` is the list made above; with no children given,
        // File Explorer opens its folder and selects it.
        let shown = unsafe { SHOpenFolderAndSelectItems(item, None, 0) };
        // SAFETY: made by ILCreateFromPathW above, freed once.
        unsafe { ILFree(Some(item.cast_const())) };
        shown.map_err(|error| format!("File Explorer did not show it: {}", error.message()))
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed> {
        paths
            .iter()
            .filter_map(|path| {
                recycle(path).err().map(|reason| NotTrashed {
                    path: path.clone(),
                    reason,
                })
            })
            .collect()
    }

    fn can_paste(&self) -> Result<(), SystemError> {
        let Some(watcher) = &self.front else {
            return Err(SystemError::Failed(NOT_WATCHING.into()));
        };
        if watcher.target().is_none() {
            return Err(SystemError::Failed(NO_TARGET.into()));
        }
        // Keep what the clipboard holds, to put back once the paste is
        // done: every format it holds in ordinary memory, read before the
        // paste's own copy replaces them. One that cannot be read now is
        // not put back then; what the paste put there stays.
        *crate::util::lock(&self.held) = hold_clip().ok();
        Ok(())
    }

    fn paste_clipboard(&self) -> Result<(), SystemError> {
        let Some(watcher) = &self.front else {
            return Err(SystemError::Failed(NOT_WATCHING.into()));
        };
        let Some(target) = watcher.target() else {
            return Err(SystemError::Failed(NO_TARGET.into()));
        };
        let generation = self.pasting.request();
        // What the clipboard holds now is what the target will paste; the
        // number of its change tells later whether anything else copied
        // meanwhile.
        // SAFETY: no arguments.
        let sequence = unsafe { GetClipboardSequenceNumber() };
        let held = crate::util::lock(&self.held).take();
        let pasting = self.pasting.clone();
        let (answer, answered) = mpsc::channel();
        // The worker of its own: bringing a window to the front can wait
        // on that window's process, which the caller must not.
        let worker = std::thread::Builder::new()
            .name("pane-paste".into())
            .spawn(move || {
                let done = paste_into(target, generation, &pasting, held, sequence);
                let _ = answer.send(done);
            });
        if worker.is_err() {
            return Err(SystemError::Failed(
                "Pane could not start the paste worker".into(),
            ));
        }
        let done = answered.recv_timeout(PASTE_WAIT);
        match done {
            Ok(done) => done,
            Err(_) => Err(refused(PasteRefusal::TookTooLong)),
        }
    }

    fn front_application(&self) -> Result<Option<FrontApplication>, SystemError> {
        let Some(watcher) = &self.front else {
            return Err(SystemError::Failed(NOT_WATCHING.into()));
        };
        let Some(recorded) = watcher.target() else {
            return Ok(None);
        };
        let window = HWND(recorded.window as *mut _);
        // The window may have closed since it was recorded: there is no
        // target then, which is not a failure.
        // SAFETY: a handle the watcher recorded; a gone window is checked.
        if !unsafe { IsWindow(Some(window)) }.as_bool() {
            return Ok(None);
        }
        let target = Target {
            title: window_title(window),
            aumid: app_user_model_id(window),
            program: recorded.program.clone(),
            family: recorded.family.clone(),
        };
        Ok(Some(resolve(&target, self.installed())))
    }

    // The selected text (#262): read through `selected`, whose module
    // documents the path UI Automation and the simulated copy take.
    fn selected_text(&self) -> Result<Option<String>, SystemError> {
        let Some(watcher) = &self.front else {
            return Err(SystemError::Failed(NOT_WATCHING.into()));
        };
        // No window has been tracked as the application the user was in:
        // no selection is there to read, which is not a failure, as the
        // front application answering none is not.
        let Some(recorded) = watcher.target() else {
            return Ok(None);
        };
        self.selected.read(&recorded)
    }
}

/// What the front application and paste answer when the watcher could
/// not be started: no target can be known.
const NOT_WATCHING: &str = "Pane could not follow the application in front, so it cannot \
     paste into it; restarting Pane may fix that";

/// Pane's own tag on the input it injects: "PANE" ([`crate::hotkeys::INJECTED_TAG"]).
/// Every key Pane sends carries it in its extra information, so its own
/// keyboard hook (#252) and other tools' know the keys are Pane's, not
/// the user's.
const INJECTED_TAG: usize = crate::hotkeys::INJECTED_TAG;

/// How long the paste worker waits for the target to come to the front —
/// after the plain foreground call, and again after it attaches to the
/// foreground thread's input — how long the target has to read the
/// clipboard before it is put back, and how long the caller waits for
/// the worker as a whole (#125's proposed timings).
const FRONT_WAIT: Duration = Duration::from_millis(500);
const READ_WAIT: Duration = Duration::from_millis(500);
const PASTE_WAIT: Duration = Duration::from_secs(10);
/// How often the worker looks while it waits.
const LOOK_EVERY: Duration = Duration::from_millis(10);

/// The paste worker's bookkeeping: which paste request is the newest, so
/// a newer one supersedes an older still working. Cloned into each
/// worker, which carries its own view of it.
#[derive(Clone, Default)]
struct Pasting {
    /// The generation of the newest request, grown by each.
    newest: Arc<AtomicU64>,
}

impl Pasting {
    /// Registers a new request, answering its generation.
    fn request(&self) -> u64 {
        self.newest.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Whether `generation`'s request has been superseded by a newer one.
    fn superseded(&self, generation: u64) -> bool {
        self.newest.load(Ordering::SeqCst) != generation
    }
}

/// The paste worker: brings the target's window to the front and has it
/// paste what the clipboard holds — a tagged Ctrl+V — then puts back what
/// the clipboard held before, after the target has had the time to read
/// it, unless something else was copied meanwhile or a newer request
/// superseded this one. The caller has put the content on the clipboard
/// and waits for this answer; a refusal leaves it there, still tagged,
/// for the user to paste by hand.
fn paste_into(
    target: Recorded,
    generation: u64,
    pasting: &Pasting,
    held: Option<Held>,
    sequence: u32,
) -> Result<(), SystemError> {
    let window = HWND(target.window as *mut _);
    // A window that is gone, not responding, or of a process running as
    // administrator — which would drop the keys Pane sends — stops the
    // paste before any key is sent.
    if pasting.superseded(generation) {
        return Err(refused(PasteRefusal::Superseded));
    }
    // SAFETY: a handle the watcher recorded.
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        return Err(refused(PasteRefusal::Gone));
    }
    // SAFETY: as above.
    if unsafe { IsHungAppWindow(window) }.as_bool() {
        return Err(refused(PasteRefusal::NotResponding));
    }
    if elevated(target.process) {
        return Err(refused(PasteRefusal::Elevated));
    }
    if pasting.superseded(generation) {
        return Err(refused(PasteRefusal::Superseded));
    }
    bring_to_front(window).map_err(refused)?;
    if pasting.superseded(generation) {
        return Err(refused(PasteRefusal::Superseded));
    }
    send_paste_keys()?;
    // Let the target read the clipboard, then put back what it held —
    // unless anything was copied meanwhile, which its number says.
    std::thread::sleep(READ_WAIT);
    if !pasting.superseded(generation)
        // SAFETY: no arguments.
        && unsafe { GetClipboardSequenceNumber() } == sequence
        && let Some(held) = &held
    {
        // Putting it back is a courtesy: the paste itself is done, so a
        // failure here does not make it fail.
        let _ = restore_clip(held);
    }
    Ok(())
}

/// `refusal` as the command's answer.
fn refused(refusal: PasteRefusal) -> SystemError {
    SystemError::Failed(refusal.message())
}

/// The token of `process`, opened for reading, or `None` when Pane
/// cannot open the process or its token: an elevated process keeps its
/// token to itself, and a gone one has neither.
fn token_of(process: u32) -> Option<HANDLE> {
    // SAFETY: plain values; the handles are closed below.
    let handle: HANDLE =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut token = HANDLE::default();
    // SAFETY: `handle` was opened above; `token` is writable.
    let opened = unsafe { OpenProcessToken(handle, TOKEN_QUERY, &mut token) };
    // SAFETY: opened above.
    let _ = unsafe { CloseHandle(handle) };
    if opened.is_err() {
        return None;
    }
    Some(token)
}

/// Whether `process` is running elevated, as far as Pane can tell: the
/// elevation of its token — a process whose token Pane cannot read
/// counts as elevated, since the keys Pane would send it would be
/// dropped just the same. Read for the paste target, for each listed
/// window's record (#263), and for the selected-text read's simulated
/// copy (#262).
pub(crate) fn elevated(process: u32) -> bool {
    let Some(token) = token_of(process) else {
        return true;
    };
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0;
    // SAFETY: `token` is open; `elevation` is writable for its size.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    };
    // SAFETY: opened above.
    let _ = unsafe { CloseHandle(token) };
    read.is_ok() && elevation.TokenIsElevated != 0
}

/// Brings `window` to the front, waiting up to [`FRONT_WAIT`] for it to
/// really be there: the plain foreground call first, then, when another
/// thread's window is in front, attaching to that thread's input and
/// trying again, which is the way an application takes the foreground
/// where Windows would not let it. The paste worker follows this path
/// (#253), so does activating a window of the Switch Windows list
/// (#263), and so does the selected-text read's simulated copy (#262).
pub(crate) fn bring_to_front(window: HWND) -> Result<(), PasteRefusal> {
    // SAFETY: plain values.
    let _ = unsafe { SetForegroundWindow(window) };
    if waited_front(window) {
        return Ok(());
    }
    if let Some((ours, theirs)) = attached() {
        // SAFETY: plain thread ids, paired with the detach below.
        let _ = unsafe { AttachThreadInput(ours, theirs, true) };
        // SAFETY: plain values.
        let _ = unsafe { SetForegroundWindow(window) };
        // SAFETY: paired with the attach above.
        let _ = unsafe { AttachThreadInput(ours, theirs, false) };
        if waited_front(window) {
            return Ok(());
        }
    }
    Err(PasteRefusal::NotFront)
}

/// The calling thread and the thread of the window in front, when
/// attaching to it can help take the foreground: another thread's, not
/// this one.
fn attached() -> Option<(u32, u32)> {
    // SAFETY: no arguments.
    let front = unsafe { GetForegroundWindow() };
    if front.is_invalid() {
        return None;
    }
    // SAFETY: a window handle; the process id is not wanted.
    let theirs = unsafe { GetWindowThreadProcessId(front, None) };
    // SAFETY: no arguments.
    let ours = unsafe { GetCurrentThreadId() };
    (theirs != 0 && theirs != ours).then_some((ours, theirs))
}

/// Whether `window` is the window in front.
fn in_front(window: HWND) -> bool {
    // SAFETY: no arguments.
    let front = unsafe { GetForegroundWindow() };
    front == window
}

/// Waits until `window` is in front, at most [`FRONT_WAIT`], looking
/// every few moments.
fn waited_front(window: HWND) -> bool {
    let deadline = Instant::now() + FRONT_WAIT;
    while Instant::now() < deadline {
        if in_front(window) {
            return true;
        }
        std::thread::sleep(LOOK_EVERY);
    }
    in_front(window)
}

/// Sends the target a Ctrl+V, as the user pressing it would, every key
/// tagged as Pane's own.
fn send_paste_keys() -> Result<(), SystemError> {
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
                dwExtraInfo: INJECTED_TAG,
            },
        },
    };
    let keys = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    // SAFETY: `keys` is an array of INPUT of its own length.
    let sent = unsafe { SendInput(&keys, std::mem::size_of::<INPUT>() as i32) };
    if sent != keys.len() as u32 {
        return Err(SystemError::Failed(
            "Windows did not take the keys Pane sent; the text is still on the clipboard".into(),
        ));
    }
    Ok(())
}

/// `window`'s title, as the system shows it, if it has one: read for the
/// front application, and for each listed window (#263).
pub(crate) fn window_title(window: HWND) -> String {
    let mut title = vec![0u16; 512];
    // SAFETY: `window` is a window handle; `title` is writable for its
    // length.
    let read = unsafe { GetWindowTextW(window, &mut title) };
    let end = read.clamp(0, title.len() as i32) as usize;
    String::from_utf16_lossy(&title[..end])
}

/// `PKEY_AppUserModel_ID`.
const APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

/// `window`'s own AppUserModelID, if it has one, read through the shell's
/// property store for the window: what the taskbar groups and launches
/// the window by, which a packaged application or a web app has and a
/// plain desktop program usually does not. Read for the front
/// application's resolution, and for each listed window's (#263).
pub(crate) fn app_user_model_id(window: HWND) -> Option<String> {
    // The shell's property store needs COM on the calling thread; without
    // it there is simply no AppUserModelID to read.
    let _com = Com::new().ok()?;
    // SAFETY: plain values; the store is released with it.
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(window) }.ok()?;
    // SAFETY: the store's own method, on this thread; `value` is filled.
    let mut value = unsafe { store.GetValue(&APP_USER_MODEL_ID) }.ok()?;
    let mut text = vec![0u16; 1024];
    // SAFETY: `value` is the PROPVARIANT the store filled; `text` is
    // writable for its length.
    let read = unsafe { PropVariantToString(&value, &mut text) };
    // SAFETY: `value` holds no resources once cleared.
    let _ = unsafe { PropVariantClear(&mut value) };
    read.ok()?;
    let end = text
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(text.len());
    let aumid = String::from_utf16_lossy(&text[..end]).trim().to_owned();
    (!aumid.is_empty()).then_some(aumid)
}

/// `text` as one argument on a Windows command line, quoted as
/// `CommandLineToArgvW` reads it back: a quote is escaped, and the
/// backslashes before a quote (or the closing one) are doubled.
fn argument(text: &str) -> String {
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in text.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        let escapes = if character == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.push_str(&"\\".repeat(escapes));
        backslashes = 0;
        quoted.push(character);
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Opens `file` (a URL, a path or an application's id) as Explorer does,
/// with `parameters` as its command line when it is a program, waiting
/// only until the shell has started it.
fn shell_execute(file: &str, parameters: Option<&str>) -> Result<(), String> {
    let file = wide(file);
    let parameters = parameters.map(wide);
    let _com = Com::new()?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait until the shell has started it (this thread ends next), and
        // report a failure here instead of in a dialog.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: parameters
            .as_ref()
            .map_or(PCWSTR::null(), |parameters| PCWSTR(parameters.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size, and its strings are
    // NUL-terminated and outlive the call.
    unsafe { ShellExecuteExW(&mut info) }
        .map_err(|error| format!("Windows did not open it: {}", error.message()))
}

/// Moves `path` to the Recycle Bin, or says why it did not. A path that
/// cannot be recycled (on a drive without a Recycle Bin, or too large for
/// it) is not deleted for good without the user's say: Windows asks first.
fn recycle(path: &Path) -> Result<(), String> {
    if let Some(missing) = missing(path) {
        return Err(missing);
    }
    // The list of paths ends with an empty one.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let flags =
        FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOF_WANTNUKEWARNING;
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        // The flags fit the field's 16 bits.
        fFlags: flags.0 as u16,
        ..Default::default()
    };
    // SAFETY: `operation` names a double-NUL-terminated list that outlives
    // the call, and no other pointer.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if operation.fAnyOperationsAborted.as_bool() {
        return Err("Moving it was cancelled".into());
    }
    if result != 0 {
        let message = HRESULT::from_win32(result as u32).message();
        return Err(format!(
            "Windows did not move it to the Recycle Bin: {message} (code {result:#x})"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_one_quoted_argument() {
        assert_eq!(argument("https://example.com"), r#""https://example.com""#);
        assert_eq!(argument(r"C:\My Notes\a.txt"), r#""C:\My Notes\a.txt""#);
        // Backslashes before the closing quote are doubled.
        assert_eq!(argument(r"C:\My Notes\"), r#""C:\My Notes\\""#);
        // A quote is escaped, with the backslashes before it doubled.
        assert_eq!(argument(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(argument(r#"a\"b"#), r#""a\\\"b""#);
    }
}
