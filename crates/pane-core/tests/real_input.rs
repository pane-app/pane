//! The front application and paste against Windows' real input (#253):
//! the Windows system adapter pastes into a window the test owns — a
//! form with a text field that a script of the test's runs in a process
//! of its own, since a window of the test's own process is never the
//! target — and the clipboard is put back, while a clipboard listener of
//! the test's own sees only writes tagged as not to be kept. A target
//! that is gone or not responding fails, saying so, with the text
//! staying on the clipboard, still tagged.
//!
//! The selected text (#262) is read the same way: through UI Automation
//! from a test-owned edit control with a selection and with none
//! ("nothing is selected", which is not a failure), and through the
//! simulated copy from a test-owned window with no text pattern, which
//! copies when the tagged Ctrl+C reaches it — the clipboard put back
//! after, with the changes of the window kept from every history: Pane's
//! own clipboard watch of the test's, running beside the read, reports
//! nothing while it happens. A target that is not responding fails,
//! saying so, and nothing is sent to it. The reads run in this process
//! (the test binary cannot serve as Pane's worker), on the same code the
//! worker runs, asked the same way with the same timeout.
//!
//! This needs a session with real input, so the test runs only where
//! `PANE_TEST_REAL_INPUT=1` is set (CI's Windows runner sets it); without
//! it the test passes without looking. It touches nothing but the
//! windows, the processes and the clipboard writes it owns, and it
//! brings nothing to the front that it did not create.

#[cfg(target_os = "windows")]
mod windows {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use pane_core::clipboard::{
        ClipboardSystem, Observation, Sink, Ticket, Watch, WindowsClipboard,
    };
    use pane_core::system::{Clip, System, SystemError, native, native_selected_text_in_process};

    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use ::windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, IsClipboardFormatAvailable, RegisterClipboardFormatW,
        RemoveClipboardFormatListener,
    };
    use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use ::windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
        GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, IsHungAppWindow, PeekMessageW,
        PostMessageW, PostThreadMessageW, RegisterClassW, SetForegroundWindow,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        HWND_MESSAGE, MSG, PM_NOREMOVE, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
        WM_CLIPBOARDUPDATE, WM_CLOSE, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use ::windows::core::{PCWSTR, w};

    /// Whether the real-input test may run here.
    fn opted_in() -> bool {
        std::env::var("PANE_TEST_REAL_INPUT").is_ok_and(|value| value == "1")
    }

    /// The real-input tests share the session's foreground window and
    /// clipboard, so they run one at a time: nextest runs each test in a
    /// process of its own, and a plain `cargo test` runs them as threads
    /// of one, which this serializes.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The titles of the target's window and of the hung target's, which
    /// the test finds them by.
    const TARGET: &str = "Pane real input paste target";
    const HUNG: &str = "Pane real input hung target";

    /// The titles of the selection targets' windows, which the test finds
    /// them by: one whose text box has a selection, one with none, and one
    /// with no text control at all, which copies when Ctrl+C reaches it.
    const SELECTED: &str = "Pane real input selection target";
    const EMPTY: &str = "Pane real input empty selection target";
    const COPYING: &str = "Pane real input copy target";

    /// What the selection targets' text boxes hold, selected and not.
    const SELECTION: &str = "Selected by the real-input test";

    /// What the copy target copies when Ctrl+C reaches it.
    const COPIED: &str = "Copied by the copy target";

    /// What the pastes put on the clipboard, and what it held before.
    const BEFORE: &str = "The user's own copy";
    const PASTED: &str = "Pasted by the real-input test";
    const GONE: &str = "Pasted at a gone target";
    const UNRESPONSIVE: &str = "Pasted at a target that is not responding";

    /// How long the test waits for anything it waits for, and how often
    /// it looks while it waits.
    const WAIT: Duration = Duration::from_secs(30);
    const LOOK: Duration = Duration::from_millis(10);

    #[test]
    fn paste_arrives_the_clipboard_is_put_back_and_only_tagged_writes_are_seen() {
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 to paste with real input");
            return;
        }
        let _serial = serial();
        let folder = tempfile::tempdir().unwrap();
        let system = native();
        let listener = Listener::start().expect("the clipboard listener");
        let own = own_window();
        let target = TargetForm::start(folder.path()).expect("the paste target");

        // The target comes to the front, and the watcher records it: the
        // application in front is the target's, named for its program.
        take_front(target.window());
        wait("the target to be recorded", || named_for(system.as_ref()));
        let front = system.front_application();
        let tracked = front.expect("the front application's answer");
        let tracked = tracked.expect("the application in front");
        assert!(!tracked.name.trim().is_empty(), "{tracked:?}");

        // The clipboard holds something of the user's, tagged as a
        // concealed copy is; the paste must put it back.
        let before = Clip::Text(BEFORE.into());
        system.copy(&before, true).expect("the setup copy");
        listener.clear();

        // A window of Pane's own process is never the target: with the
        // test's own window in front, the application in front is still
        // the target's.
        take_front(own);
        assert!(named_for(system.as_ref()));

        // The paste: asked for first (which also keeps what the clipboard
        // holds, to put it back), the content put on the clipboard
        // concealed, then the paste itself — the worker brings the target
        // back to the front from behind the test's own window, sends it a
        // tagged Ctrl+V, waits for it to read, and puts the clipboard back.
        system.can_paste().expect("a target to paste into");
        let pasted = Clip::Text(PASTED.into());
        system.copy(&pasted, true).expect("the copy to paste");
        // A CI runner's session runs elevated, so every process the test
        // can start — the target's included — is an administrator's, and
        // the paste honestly refuses an elevated target (the spec's own
        // rule: Windows would silently drop the keys). Where it does, the
        // refusal is what the test asserts; the arrival below needs a
        // session that is not elevated, which a runner never is and a
        // user's desktop is.
        if let Err(SystemError::Failed(why)) = system.paste_clipboard() {
            assert!(
                why.contains("running as administrator"),
                "the paste's refusal: {why}"
            );
            eprintln!(
                "skipped the arrival: the session is elevated, so the paste \n\
                 refused its administrator target — {why}"
            );
            return;
        }

        // The text arrived in the target's field, and the clipboard holds
        // what it held.
        wait("the paste to bring the target back", || {
            front_is(target.window())
        });
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));

        // A clipboard listener saw only tagged writes: the paste's and the
        // putting back's.
        wait("the listener to see the paste's writes", || {
            listener.seen().len() >= 2
        });
        let seen = listener.seen();
        assert!(seen.len() >= 2, "{seen:?}: the paste's and the restore's");
        assert!(seen.iter().all(|tagged| *tagged), "{seen:?}");

        // Close the target while the test's own window is in front, so no
        // window of another process comes to the front: the watcher keeps
        // the now gone target, and the front application answers that no
        // target exists.
        take_front(own);
        assert_eq!(target.close(), PASTED);
        wait("the gone target to answer none", || {
            system.front_application() == Ok(None)
        });

        // A gone target fails, saying so, with the text staying on the
        // clipboard, still tagged.
        system.can_paste().expect("the gone target to be asked");
        let gone = Clip::Text(GONE.into());
        system.copy(&gone, true).expect("the copy to paste");
        let answer = system.paste_clipboard();
        let refused = answer.expect_err("a gone target to refuse");
        let SystemError::Failed(why) = refused else {
            panic!("a failure, not {refused:?} (a gone target)");
        };
        assert!(why.contains("gone"), "{why}");
        assert!(why.contains("clipboard"), "{why}");
        assert_eq!(system.read_clipboard(), Ok(Some(gone)));

        // A target that is not responding fails the same way: its window
        // stops answering, and no key is sent to it.
        let hung = HungForm::start(folder.path()).expect("the hung target");
        take_front(hung.window());
        wait("the watcher to record the hung target", || {
            named_for(system.as_ref())
        });
        wait("the hung target to stop responding", || {
            // SAFETY: the hung target's own window.
            unsafe { IsHungAppWindow(hung.window()) }.as_bool()
        });
        system.can_paste().expect("the hung target to be asked");
        let unresponsive = Clip::Text(UNRESPONSIVE.into());
        system.copy(&unresponsive, true).expect("the copy to paste");
        let answer = system.paste_clipboard();
        let refused = answer.expect_err("a hung target to refuse");
        let SystemError::Failed(why) = refused else {
            panic!("a failure, not {refused:?} (a hung target)");
        };
        assert!(why.contains("not responding"), "{why}");
        assert!(why.contains("clipboard"), "{why}");
        assert_eq!(system.read_clipboard(), Ok(Some(unresponsive)));

        // The failures' writes were tagged too.
        let seen = listener.seen();
        assert!(seen.iter().all(|tagged| *tagged), "{seen:?}");

        hung.kill();
        listener.stop();
    }

    /// Whether the application in front is one of the test's targets,
    /// which run Windows PowerShell: its icon names their program.
    fn named_for(system: &dyn System) -> bool {
        let icon = match system.front_application() {
            Ok(Some(front)) => front.icon,
            _ => None,
        };
        let icon = icon.unwrap_or_default();
        icon.to_lowercase().contains("powershell")
    }

    /// Waits up to [`WAIT`] for `check`, panicking with `what` if it never
    /// holds.
    fn wait(what: &str, check: impl Fn() -> bool) {
        let deadline = Instant::now() + WAIT;
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            pump();
        }
    }

    /// Waits up to [`WAIT`] for `check` to answer something, answering what
    /// it said; panics with `what` if it never did.
    fn wait_for<T>(what: &str, check: impl Fn() -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(answer) = check() {
                return answer;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            pump();
        }
    }

    /// Pumps this thread's messages for a moment, keeping the windows it
    /// owns responsive while the test waits.
    fn pump() {
        let mut message = MSG::default();
        // SAFETY: `message` is writable for the call; this thread's own
        // messages are taken and dispatched.
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            // SAFETY: a message this thread's queue returned.
            unsafe { DispatchMessageW(&message) };
        }
        std::thread::sleep(LOOK);
    }

    /// Brings `window` — one the test owns or made for itself — to the
    /// front, the way the paste does: the plain call, then attaching to
    /// the foreground thread's input and trying again.
    fn take_front(window: HWND) {
        // SAFETY: plain values.
        let _ = unsafe { SetForegroundWindow(window) };
        if front_is(window) {
            return;
        }
        // SAFETY: no arguments.
        let current = unsafe { GetForegroundWindow() };
        if !current.is_invalid() {
            // SAFETY: a window handle; the process id is not wanted.
            let theirs = unsafe { GetWindowThreadProcessId(current, None) };
            // SAFETY: no arguments.
            let ours = unsafe { GetCurrentThreadId() };
            if theirs != 0 && theirs != ours {
                // SAFETY: plain thread ids, paired with the detach below.
                let _ = unsafe { AttachThreadInput(ours, theirs, true) };
                // SAFETY: plain values.
                let _ = unsafe { SetForegroundWindow(window) };
                // SAFETY: paired with the attach above.
                let _ = unsafe { AttachThreadInput(ours, theirs, false) };
            }
        }
        wait("the window to come to the front", || front_is(window));
    }

    /// Whether `window` is the window in front.
    fn front_is(window: HWND) -> bool {
        // SAFETY: no arguments.
        let front = unsafe { GetForegroundWindow() };
        front == window
    }

    /// The top-level window titled `title`, if it exists.
    fn find_window(title: &str) -> Option<HWND> {
        let wide: Vec<u16> = title.encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated title; a plain class asks for any.
        match unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) } {
            Ok(window) => (!window.is_invalid()).then_some(window),
            Err(_) => None,
        }
    }

    /// The script `file` run by Windows PowerShell, with no console window
    /// of its own: the test owns the process and the windows it makes.
    fn powershell(file: &Path) -> std::process::Command {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("powershell.exe");
        command
            .arg("-NoProfile")
            .arg("-STA")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(file);
        // CREATE_NO_WINDOW: no console of its own.
        command.creation_flags(0x0800_0000);
        command
    }

    /// A form with a text field, which writes the field's text to a file
    /// when it closes: the window the paste reaches.
    struct TargetForm {
        window: HWND,
        /// Where the form's field's text is written on close.
        result: PathBuf,
        child: std::process::Child,
    }

    impl TargetForm {
        /// Starts the form in a process of its own: a window of the test's
        /// own process would never be the target, which Pane skips.
        fn start(folder: &Path) -> Result<TargetForm, String> {
            let result = folder.join("pasted.txt");
            let script = "\n\
Add-Type -AssemblyName System.Windows.Forms\n\
$form = New-Object System.Windows.Forms.Form\n\
$form.Text = 'Pane real input paste target'\n\
$box = New-Object System.Windows.Forms.TextBox\n\
$box.Dock = 'Fill'\n\
$form.Controls.Add($box)\n\
$form.ActiveControl = $box\n\
$form.Add_FormClosing({ param($sender, $event)\n\
  [System.IO.File]::WriteAllText($env:PANE_PASTE_RESULT, $box.Text) })\n\
[System.Windows.Forms.Application]::Run($form)\n";
            let file = folder.join("target.ps1");
            std::fs::write(&file, script).map_err(|error| error.to_string())?;
            let child = powershell(&file)
                .env("PANE_PASTE_RESULT", &result)
                .spawn()
                .map_err(|error| format!("powershell: {error}"))?;
            let window = wait_for("the target's window", || find_window(TARGET));
            Ok(TargetForm {
                window,
                result,
                child,
            })
        }

        /// The form's window, which the paste reaches.
        fn window(&self) -> HWND {
            self.window
        }

        /// Closes the form, answering the text its field ended with.
        fn close(mut self) -> String {
            // SAFETY: the form's own window, closed by its own handler,
            // which writes the field's text to the result file.
            let _ = unsafe { PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            let text = wait_for("the target to write its text", || {
                std::fs::read_to_string(&self.result).ok()
            });
            let _ = self.child.wait();
            text
        }
    }

    /// A form whose thread keeps its window responsive for a while and
    /// then stops answering messages: a target that is not responding,
    /// which a paste must skip.
    struct HungForm {
        window: HWND,
        child: std::process::Child,
    }

    impl HungForm {
        /// Starts the form in a process of its own.
        fn start(folder: &Path) -> Result<HungForm, String> {
            let script = "\n\
Add-Type -AssemblyName System.Windows.Forms\n\
$form = New-Object System.Windows.Forms.Form\n\
$form.Text = 'Pane real input hung target'\n\
$form.Show()\n\
$still = [DateTime]::UtcNow.AddSeconds(10)\n\
while ([DateTime]::UtcNow -lt $still) {\n\
  [System.Windows.Forms.Application]::DoEvents()\n\
  Start-Sleep -Milliseconds 100\n\
}\n\
Start-Sleep -Seconds 300\n";
            let file = folder.join("hung.ps1");
            std::fs::write(&file, script).map_err(|error| error.to_string())?;
            let child = powershell(&file).spawn();
            let child = child.map_err(|error| format!("powershell: {error}"))?;
            let window = wait_for("the hung target's window", || find_window(HUNG));
            Ok(HungForm { window, child })
        }

        /// Ends the hung target's process, which would never answer
        /// again.
        fn kill(mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }

        /// The form's window, which the paste would reach.
        fn window(&self) -> HWND {
            self.window
        }
    }

    /// A clipboard listener of the test's own: a message window on a
    /// thread of the test's, told of every clipboard change, recording
    /// whether each write was tagged as not to be kept — as the paste's
    /// and its putting back must be, so that no history keeps them.
    struct Listener {
        /// For each change seen: whether it was tagged.
        seen: Arc<Mutex<Vec<bool>>>,
        thread: Option<std::thread::JoinHandle<()>>,
        /// The listener thread's id, to tell it to stop.
        id: u32,
    }

    // The listener thread's recording, set while it listens.
    thread_local! {
        static RECORDING: RefCell<Option<Arc<Mutex<Vec<bool>>>>> =
            const { RefCell::new(None) };
    }

    impl Listener {
        /// Starts the listener on a thread of its own.
        fn start() -> Result<Listener, String> {
            let seen: Arc<Mutex<Vec<bool>>> = Arc::default();
            let recorded = seen.clone();
            let (started, answer) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("test-clipboard-listener".into())
                .spawn(move || {
                    let mut message = MSG::default();
                    // Makes the thread's queue, so nothing posted to it is
                    // lost.
                    // SAFETY: `message` is writable for the call.
                    let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
                    // SAFETY: no arguments.
                    let id = unsafe { GetCurrentThreadId() };
                    let window = listener_window().expect("the listener's window");
                    // SAFETY: this thread's own window.
                    let listening = unsafe { AddClipboardFormatListener(window) }.is_ok();
                    RECORDING.with(|recording| {
                        if listening {
                            *recording.borrow_mut() = Some(recorded);
                        }
                    });
                    let _ = started.send(listening.then_some(id));
                    if !listening {
                        // SAFETY: this thread's own window.
                        let _ = unsafe { DestroyWindow(window) };
                        return;
                    }
                    loop {
                        // SAFETY: `message` is writable for the call; with no
                        // window given it gets this thread's messages, the
                        // window's included.
                        if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
                            break;
                        }
                        if message.hwnd.is_invalid() && message.message == WM_APP + 1 {
                            break;
                        }
                        // SAFETY: a message this thread's queue returned.
                        unsafe { DispatchMessageW(&message) };
                    }
                    RECORDING.with(|recording| recording.borrow_mut().take());
                    // SAFETY: this thread's own window.
                    let _ = unsafe { RemoveClipboardFormatListener(window) };
                    // SAFETY: as above.
                    let _ = unsafe { DestroyWindow(window) };
                })
                .map_err(|error| error.to_string())?;
            match answer.recv() {
                // SAFETY: the listener answered its thread's id.
                Ok(Some(id)) => Ok(Listener {
                    seen,
                    thread: Some(thread),
                    id,
                }),
                _ => Err("the clipboard listener could not start".into()),
            }
        }

        /// The changes seen so far, and whether each was tagged.
        fn seen(&self) -> Vec<bool> {
            self.seen.lock().unwrap().clone()
        }

        /// Forgets the changes seen so far.
        fn clear(&self) {
            self.seen.lock().unwrap().clear();
        }

        /// Ends the listener's thread.
        fn stop(mut self) {
            // SAFETY: the listener thread's id and a plain message.
            let _ = unsafe { PostThreadMessageW(self.id, WM_APP + 1, WPARAM(0), LPARAM(0)) };
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// The listener's window procedure: records each change's
    /// tagged-ness.
    extern "system" fn listener(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_CLIPBOARDUPDATE {
            RECORDING.with(|recording| {
                if let Some(seen) = recording.borrow().as_ref() {
                    seen.lock().unwrap().push(tagged());
                }
            });
            return LRESULT(0);
        }
        // SAFETY: the arguments are those this procedure was called with.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    /// Whether the clipboard's latest write is tagged as not to be kept,
    /// as a concealed copy is: the formats that say so are there.
    fn tagged() -> bool {
        // SAFETY: valid NUL-terminated names.
        let markers = unsafe {
            [
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            ]
        };
        // SAFETY: plain values.
        markers
            .iter()
            .any(|marker| *marker != 0 && unsafe { IsClipboardFormatAvailable(*marker) }.is_ok())
    }

    /// The listener's message-only window, on the calling thread.
    fn listener_window() -> Option<HWND> {
        let class: Vec<u16> = "PaneRealInputListener".encode_utf16().chain([0]).collect();
        // SAFETY: no arguments; the module is this process's.
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }.ok()?;
        let registered = WNDCLASSW {
            lpfnWndProc: Some(listener),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..WNDCLASSW::default()
        };
        // SAFETY: `registered` is fully initialized, and its name and
        // instance live as long as the process.
        if unsafe { RegisterClassW(&registered) } == 0 {
            return None;
        }
        // SAFETY: the class is registered; a message-only window has no
        // title, size, menu or creation data.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
        }
        .ok()?;
        Some(window)
    }

    /// The test's own windows' procedure: nothing of their own to do.
    extern "system" fn plain(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the arguments are those this procedure was called with.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    /// A plain, visible window of this process, never a paste target:
    /// Pane skips its own windows, whatever they are.
    fn own_window() -> HWND {
        let class: Vec<u16> = "PaneRealInputOwn".encode_utf16().chain([0]).collect();
        // SAFETY: no arguments; the module is this process's.
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }.expect("the module");
        let registered = WNDCLASSW {
            lpfnWndProc: Some(plain),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..WNDCLASSW::default()
        };
        // SAFETY: `registered` is fully initialized, and its name and
        // instance live as long as the process.
        unsafe { RegisterClassW(&registered) };
        // SAFETY: the class is registered; a small, visible window with no
        // menu or creation data.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                w!("Pane's own test window"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                0,
                0,
                200,
                100,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .expect("the test's own window")
    }

    /// The script of a selection target: a form with a multiline text box
    /// — a Win32 edit control, whose selection UI Automation reads — its
    /// text selected when `PANE_SELECT_ALL` says so. Its title and text
    /// come from the environment.
    const SELECTION_FORM: &str = "\n\
Add-Type -AssemblyName System.Windows.Forms\n\
$form = New-Object System.Windows.Forms.Form\n\
$form.Text = $env:PANE_TARGET_TITLE\n\
$box = New-Object System.Windows.Forms.TextBox\n\
$box.Multiline = $true\n\
$box.Dock = 'Fill'\n\
$box.Text = $env:PANE_SELECTION_TEXT\n\
$form.Controls.Add($box)\n\
$form.ActiveControl = $box\n\
$form.Add_Shown({ param($sender, $event)\n\
  $box.Focus()\n\
  if ($env:PANE_SELECT_ALL -eq '1') { $box.SelectAll() } })\n\
[System.Windows.Forms.Application]::Run($form)\n";

    /// The script of a copy target: a form with no control — no text
    /// pattern for UI Automation to read — which copies the text
    /// `PANE_COPIED_TEXT` names when Ctrl+C reaches it, marking
    /// `PANE_COPIED_MARK` that it did. Its title comes from the
    /// environment.
    const COPY_FORM: &str = "\n\
Add-Type -AssemblyName System.Windows.Forms\n\
$form = New-Object System.Windows.Forms.Form\n\
$form.Text = $env:PANE_TARGET_TITLE\n\
$form.KeyPreview = $true\n\
$form.Add_KeyDown({ param($sender, $event)\n\
  if ($event.Control -and $event.KeyCode -eq 'C') {\n\
    try {\n\
      [System.Windows.Forms.Clipboard]::SetText($env:PANE_COPIED_TEXT)\n\
      [System.IO.File]::WriteAllText($env:PANE_COPIED_MARK, 'copied')\n\
    } catch { }\n\
    $event.Handled = $true\n\
    $event.SuppressKeyPress = $true\n\
  } })\n\
[System.Windows.Forms.Application]::Run($form)\n";

    #[test]
    fn the_selection_is_read_by_uia_and_by_a_copy_that_leaves_no_trace() {
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 to read selections with real input");
            return;
        }
        let _serial = serial();
        let folder = tempfile::tempdir().unwrap();
        // Pane's own clipboard watch of the test's, counting what it
        // reports: the ignore window of a simulated copy must keep every
        // change the read causes from reaching any history.
        let watching = Watched::start().expect("Pane's clipboard watch");
        // A listener of the test's own, which sees every change whatever
        // Pane does: what the read caused, and whether each was tagged.
        let listener = Listener::start().expect("the clipboard listener");
        let own = own_window();
        // The reads run in this process (the test binary cannot serve as
        // Pane's worker) on the same code the worker runs, asked the same
        // way with the same timeout.
        let system = native_selected_text_in_process();

        // The clipboard holds something of the user's; every read must
        // leave it there.
        let before = Clip::Text(BEFORE.into());
        system.copy(&before, false).expect("the setup copy");
        wait("Pane's watch to see the setup copy", || {
            watching.seen() == 1
        });
        wait("the listener to see the setup copy", || {
            !listener.seen().is_empty()
        });
        listener.clear();

        // UI Automation reads an edit control's selection, without
        // touching the clipboard.
        let selected = Form::start(
            folder.path(),
            SELECTION_FORM,
            SELECTED,
            &[
                ("PANE_TARGET_TITLE", SELECTED),
                ("PANE_SELECTION_TEXT", SELECTION),
                ("PANE_SELECT_ALL", "1"),
            ],
        )
        .expect("the selection target");
        take_front(selected.window());
        wait("the selection target to be recorded", || {
            named_for(system.as_ref())
        });
        // A CI runner's session runs elevated, so the target's process is
        // an administrator's, and the read honestly refuses an elevated
        // target (Windows would not deliver its keys, so the simulated
        // copy could not reach it). Where it does, the refusal is what
        // the test asserts; the reads below need a session that is not
        // elevated, which a runner never is and a user's desktop is.
        if let Err(SystemError::Failed(why)) = system.selected_text() {
            assert!(
                why.contains("running as administrator"),
                "the selection read's refusal: {why}"
            );
            eprintln!(
                "skipped the reads: the session is elevated, so the selection \n\
                 read refused its administrator target — {why}"
            );
            return;
        }
        assert_eq!(
            system.selected_text(),
            Ok(Some(SELECTION.into())),
            "UI Automation reads the edit control's selection"
        );
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));
        assert!(listener.seen().is_empty(), "the read changed nothing");
        // Pane's own window in front, so closing the form changes no
        // foreground the watcher would record.
        take_front(own);
        selected.close();

        // Nothing selected is an answer, not a failure — read while
        // Pane's own window is in front, as a command's read is.
        let empty = Form::start(
            folder.path(),
            SELECTION_FORM,
            EMPTY,
            &[
                ("PANE_TARGET_TITLE", EMPTY),
                ("PANE_SELECTION_TEXT", SELECTION),
                ("PANE_SELECT_ALL", "0"),
            ],
        )
        .expect("the empty selection target");
        take_front(empty.window());
        wait("the empty target to be recorded", || {
            named_for(system.as_ref())
        });
        take_front(own);
        assert_eq!(
            system.selected_text(),
            Ok(None),
            "nothing is selected, which is not a failure"
        );
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));
        take_front(own);
        empty.close();

        // Where UI Automation gives nothing — a window with no text
        // pattern — the simulated copy reads the selection: the target
        // copies when the tagged Ctrl+C reaches it, and the clipboard is
        // put back.
        let mark = folder.path().join("copied.txt");
        let marked = mark.to_string_lossy().into_owned();
        let copying = Form::start(
            folder.path(),
            COPY_FORM,
            COPYING,
            &[
                ("PANE_TARGET_TITLE", COPYING),
                ("PANE_COPIED_TEXT", COPIED),
                ("PANE_COPIED_MARK", marked.as_str()),
            ],
        )
        .expect("the copy target");
        take_front(copying.window());
        wait("the copy target to be recorded", || {
            named_for(system.as_ref())
        });
        assert_eq!(
            system.selected_text(),
            Ok(Some(COPIED.into())),
            "the simulated copy reads the selection"
        );
        // The clipboard is as it was, and the changes the read caused —
        // the target's own copy, untagged, and the tagged restore — never
        // reached a history: the ignore window kept them.
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));
        wait("the listener to see the read's changes", || {
            listener.seen().len() >= 2
        });
        let seen = listener.seen();
        assert!(
            seen.contains(&false),
            "{seen:?}: the target's copy, untagged, would have been kept"
        );
        assert!(seen.iter().any(|tagged| *tagged), "{seen:?}: the restore");
        assert_eq!(
            watching.seen(),
            1,
            "only the setup copy reached a history, not the read's changes"
        );
        assert!(mark.exists(), "the target answered the copy");
        take_front(own);
        copying.close();

        // A target that is not responding fails, saying so, and nothing
        // is sent to it.
        let hung = HungForm::start(folder.path()).expect("the hung target");
        take_front(hung.window());
        wait("the hung target to be recorded", || {
            named_for(system.as_ref())
        });
        wait("the hung target to stop responding", || {
            // SAFETY: the hung target's own window.
            unsafe { IsHungAppWindow(hung.window()) }.as_bool()
        });
        let answer = system.selected_text();
        let refused = answer.expect_err("a hung target to refuse");
        let SystemError::Failed(why) = refused else {
            panic!("a failure, not {refused:?} (a hung target)");
        };
        assert!(why.contains("not responding"), "{why}");
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));

        hung.kill();
        listener.stop();
    }

    /// A form the test owns, in a PowerShell process of its own (a window
    /// of the test's own process would never be the target, which Pane
    /// skips), found by its window's title and closed by asking it to.
    struct Form {
        window: HWND,
        child: std::process::Child,
    }

    impl Form {
        /// Runs `script` in a process of its own, with `envs` set for it,
        /// waiting for its window titled `title`.
        fn start(
            folder: &Path,
            script: &str,
            title: &str,
            envs: &[(&str, &str)],
        ) -> Result<Form, String> {
            let file = folder.join(format!("{}.ps1", title));
            std::fs::write(&file, script).map_err(|error| error.to_string())?;
            let mut command = powershell(&file);
            for (name, value) in envs {
                command.env(name, value);
            }
            let child = command
                .spawn()
                .map_err(|error| format!("powershell: {error}"))?;
            let window = wait_for("the form's window", || find_window(title));
            Ok(Form { window, child })
        }

        /// The form's window, which the read reaches.
        fn window(&self) -> HWND {
            self.window
        }

        /// Closes the form and waits for its process to end.
        fn close(mut self) {
            // SAFETY: the form's own window, closed by its own handler.
            let _ = unsafe { PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            let _ = self.child.wait();
        }
    }

    /// Pane's own clipboard watch of the test's, counting the changes it
    /// reports: an ignored window's changes reach no history, so the
    /// count says whether they did.
    struct Watched {
        /// How many changes were reported.
        seen: Arc<Mutex<usize>>,
        /// The watch, which stops when it is dropped at the test's end.
        _watch: Watch,
    }

    impl Watched {
        /// Starts the watch, with a sink of the test's counting what it
        /// is told.
        fn start() -> Result<Watched, String> {
            let seen: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
            let counting = Counting(seen.clone());
            let watch = WindowsClipboard.watch(Arc::new(counting))?;
            Ok(Watched {
                seen,
                _watch: watch,
            })
        }

        /// How many changes were reported.
        fn seen(&self) -> usize {
            *self.seen.lock().unwrap()
        }
    }

    /// [`Watched`]'s sink: counts what it is told.
    struct Counting(Arc<Mutex<usize>>);

    impl Sink for Counting {
        fn reading(&self) -> Ticket {
            Ticket::default()
        }

        fn observed(&self, _ticket: Ticket, _observation: Observation) {
            *self.0.lock().unwrap() += 1;
        }
    }
}
