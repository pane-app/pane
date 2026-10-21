//! The front application and paste against Windows' real input (#253):
//! the Windows system adapter pastes into a window the test owns — a
//! window with a text field, in a process of its own, since a window of
//! the test's own process is never the target — and the clipboard is put
//! back, while a clipboard listener of the test's own sees only writes
//! tagged as not to be kept. A target that is gone or not responding
//! fails, saying so, with the text staying on the clipboard, still
//! tagged.
//!
//! The selected text (#262) is read the same way: through UI Automation
//! from a test-owned text box with a selection and with none
//! ("nothing is selected", which is not a failure), and through the
//! simulated copy from a test-owned window with no text pattern, which
//! copies when the tagged Ctrl+C reaches it — the clipboard put back
//! after, with the changes of the window kept from every history: Pane's
//! own clipboard watch of the test's, running beside the read, reports
//! nothing while it happens. A target that is not responding fails,
//! saying so, and nothing is sent to it. A text control focused in an
//! application's window is read by UI Automation from behind Pane's
//! window too (#432). The reads run in this process
//! (the test binary cannot serve as Pane's worker), on the same code the
//! worker runs, asked the same way with the same timeout.
//!
//! The targets are plain Win32 windows of the test binary itself, started
//! again as a process of its own ([`target_process`]): no runtime of
//! another language starts before them. PowerShell's Windows Forms and
//! WPF windows, the targets before, sometimes stalled 0.7–2.5 s on a
//! fresh CI runner, which failed a try now and then (#433).
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
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
        GetMessageW, GetWindowThreadProcessId, IsHungAppWindow, PeekMessageW, PostMessageW,
        PostThreadMessageW, RegisterClassW, SetForegroundWindow,
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

    /// The titles of the target's window and of the hung target's.
    const TARGET: &str = "Pane real input paste target";
    const HUNG: &str = "Pane real input hung target";

    /// The titles of the selection targets' windows: one whose text box
    /// has a selection, one with none, and one with no text control at
    /// all, which copies when Ctrl+C reaches it.
    const SELECTED: &str = "Pane real input selection target";
    const EMPTY: &str = "Pane real input empty selection target";
    const COPYING: &str = "Pane real input copy target";

    /// The title of the window whose text control is read from behind
    /// Pane's window.
    const EDITOR: &str = "Pane real input editor";

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

    /// A paste arrives while Pane's own keyboard hook is installed, as it
    /// is on a fresh Windows install, whose Open Pane hotkey is a lone tap
    /// of the Windows key (#268): the hook passes Pane's tagged Ctrl+V
    /// through. The Windows smoke's paste into Notepad never arrived with
    /// the hook installed, and did without it.
    #[test]
    fn a_paste_arrives_while_pane_s_keyboard_hook_is_installed() {
        use pane_core::hotkeys::{Hotkeys, Shortcut, WindowsHotkeys, channel};
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 to paste with real input");
            return;
        }
        let _serial = serial();
        let folder = tempfile::tempdir().unwrap();
        // Pane's hotkeys, in this process as in Pane's: the tap takes the
        // keyboard hook.
        let (sender, _presses) = channel();
        let hotkeys = WindowsHotkeys::start(sender).expect("the hotkeys adapter");
        let tap = Shortcut::parse("tap:win").unwrap();
        hotkeys.register(&tap).expect("the hook takes the tap");
        assert!(hotkeys.hook_handle().is_some(), "the hook is installed");

        let system = native();
        let own = own_window();
        let target = Target::paste(folder.path()).expect("the paste target");
        take_front(target.window());
        wait("the target to be recorded", || named_for(system.as_ref()));
        system
            .copy(&Clip::Text(BEFORE.into()), true)
            .expect("the setup copy");
        // Pane's own window in front, as the launcher is when the user
        // chooses Paste.
        take_front(own);
        system.can_paste().expect("a target to paste into");
        system
            .copy(&Clip::Text(PASTED.into()), true)
            .expect("the copy to paste");
        system
            .paste_clipboard()
            .expect("the paste with Pane's hook installed");
        wait("the paste to bring the target back", || {
            front_is(target.window())
        });
        // The target reads the paste as it pumps its messages.
        let pasted = Instant::now();
        wait("a moment for the target to read the paste", || {
            pasted.elapsed() > Duration::from_secs(2)
        });
        take_front(own);
        assert_eq!(
            target.close().as_deref(),
            Some(PASTED),
            "the paste arrived with Pane's keyboard hook installed"
        );
        hotkeys.unregister(&tap);
    }

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
        let target = Target::paste(folder.path()).expect("the paste target");

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
        // The paste honestly refuses a target of a higher integrity level
        // than its own — an elevated application, to a Pane that is not —
        // since Windows would silently drop the keys (the spec's own
        // rule). The test and its target run at the same level, elevated
        // on a CI runner's administrator session or not on a user's
        // desktop, so the paste reaches it.
        system
            .paste_clipboard()
            .expect("the paste into a target at the test's own level");

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
        assert_eq!(target.close().as_deref(), Some(PASTED));
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
        let hung = Target::hung(folder.path()).expect("the hung target");
        take_front(hung.window());
        wait("the watcher to record the hung target", || {
            named_for(system.as_ref())
        });
        hung.hang();
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

        drop(hung);
        listener.stop();
    }

    /// Whether the application in front is one of the test's targets,
    /// which run the test's own program: its icon names that program.
    fn named_for(system: &dyn System) -> bool {
        let icon = match system.front_application() {
            Ok(Some(front)) => front.icon,
            _ => None,
        };
        let icon = icon.unwrap_or_default().to_lowercase();
        let program = std::env::current_exe().expect("the test's own program");
        let program = program.to_string_lossy().to_lowercase();
        icon == program
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

    /// The environment variable that makes the test binary one of the
    /// tests' targets, naming which kind (see [`target::run`]).
    const TARGET_KIND: &str = "PANE_REAL_INPUT_TARGET";

    /// The test binary started again by [`Target::start`], as a target
    /// process of the tests: it runs this alone, with [`TARGET_KIND`] set,
    /// and makes the target's windows. Ignored by every ordinary run, and
    /// does nothing without the variable.
    #[test]
    #[ignore = "the real-input tests' target process, which they start themselves"]
    fn target_process() {
        if let Ok(kind) = std::env::var(TARGET_KIND) {
            target::run(&kind);
        }
    }

    /// A target the test owns: windows of the test's own program, started
    /// again in a process of its own (a window of the test's own process
    /// would never be the target, which Pane skips). It answers its
    /// windows through a ready mark once they are set up and its thread
    /// pumps its messages, so the test never reads a window still being
    /// made. Dropping it ends the process: a failed try leaves no window
    /// behind (#421).
    struct Target {
        /// The target's top-level window, which the paste and the reads
        /// reach.
        window: HWND,
        /// Where the target writes its field's text when it closes.
        result: PathBuf,
        child: std::process::Child,
    }

    impl Target {
        /// A window with a text field, which writes the field's text to a
        /// file when it closes: the window the paste reaches.
        fn paste(folder: &Path) -> Result<Target, String> {
            Target::start(folder, "frame", TARGET, &[("PANE_FIELD_CLASS", "EDIT")])
        }

        /// A window that answers its messages until [`Target::hang`] tells
        /// it to stop: a target that is not responding, which a paste and
        /// a read must skip.
        fn hung(folder: &Path) -> Result<Target, String> {
            Target::start(folder, "frame", HUNG, &[])
        }

        /// A window whose whole content is a rich edit control (Windows'
        /// own `RICHEDIT50W`) holding [`SELECTION`], all of it selected
        /// when `select` says so. The rich edit serves UI Automation
        /// itself, with its Text pattern, and being the top-level window
        /// it is read whether it is in front or not. A plain Win32 edit
        /// control is read through Windows' MSAA proxy, which has no Text
        /// pattern (on CI's runner, #421), so the read would fall to the
        /// simulated copy.
        fn selection(folder: &Path, title: &str, select: bool) -> Result<Target, String> {
            let select = if select { "1" } else { "0" };
            Target::start(
                folder,
                "field",
                title,
                &[
                    ("PANE_FIELD_CLASS", "RICHEDIT50W"),
                    ("PANE_FIELD_TEXT", SELECTION),
                    ("PANE_SELECT_ALL", select),
                ],
            )
        }

        /// An application's window with a rich edit control inside it,
        /// as WordPad's is, holding [`SELECTION`], all of it selected; the
        /// window gives the control the focus whenever it has it. The
        /// window itself has no text for UI Automation to read: only the
        /// control does.
        fn editor(folder: &Path) -> Result<Target, String> {
            Target::start(
                folder,
                "frame",
                EDITOR,
                &[
                    ("PANE_FIELD_CLASS", "RICHEDIT50W"),
                    ("PANE_FIELD_TEXT", SELECTION),
                    ("PANE_SELECT_ALL", "1"),
                ],
            )
        }

        /// A window with no control — no text pattern for UI Automation
        /// to read — which copies [`COPIED`] when Ctrl+C reaches it, and
        /// marks `mark` that it did.
        fn copy(folder: &Path, mark: &Path) -> Result<Target, String> {
            let mark = mark.to_string_lossy().into_owned();
            Target::start(
                folder,
                "frame",
                COPYING,
                &[("PANE_COPIED_TEXT", COPIED), ("PANE_COPIED_MARK", &mark)],
            )
        }

        /// Starts the test binary as a target of `kind` titled `title`,
        /// with `envs` set for it, and waits for it to be ready.
        fn start(
            folder: &Path,
            kind: &str,
            title: &str,
            envs: &[(&str, &str)],
        ) -> Result<Target, String> {
            use std::os::windows::process::CommandExt;
            let ready = folder.join(format!("{title}.ready"));
            let result = folder.join(format!("{title}.result"));
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            let mut command = std::process::Command::new(program);
            command
                .args([
                    "real_input::windows::target_process",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .env(TARGET_KIND, kind)
                .env("PANE_TARGET_TITLE", title)
                .env("PANE_READY_MARK", &ready)
                .env("PANE_TARGET_RESULT", &result)
                // The harness's own report is not wanted; a panic's message
                // still reaches the test's output.
                .stdout(std::process::Stdio::null());
            for (name, value) in envs {
                command.env(name, value);
            }
            // CREATE_NO_WINDOW: no console of its own.
            command.creation_flags(0x0800_0000);
            let child = command
                .spawn()
                .map_err(|error| format!("the target process: {error}"))?;
            let mut target = Target {
                window: HWND::default(),
                result,
                child,
            };
            let deadline = Instant::now() + WAIT;
            loop {
                if let Some(window) = target::ready(&ready) {
                    target.window = window;
                    return Ok(target);
                }
                if let Ok(Some(status)) = target.child.try_wait() {
                    return Err(format!("the target ended before it was ready: {status}"));
                }
                if Instant::now() >= deadline {
                    return Err(format!("timed out waiting for the target {title}"));
                }
                pump();
            }
        }

        /// The target's top-level window.
        fn window(&self) -> HWND {
            self.window
        }

        /// Tells the target to stop answering its messages, as a hung
        /// application does: its thread sleeps in the message it is sent,
        /// pumping nothing, until its process is ended.
        fn hang(&self) {
            // SAFETY: the target's own window and a plain message.
            let _ = unsafe { PostMessageW(Some(self.window), target::HANG, WPARAM(0), LPARAM(0)) };
        }

        /// Closes the target and waits for its process to end, answering
        /// the text its field ended with, if it has a field.
        fn close(mut self) -> Option<String> {
            // SAFETY: the target's own window, closed by its own handler.
            let _ = unsafe { PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            let deadline = Instant::now() + WAIT;
            while !matches!(self.child.try_wait(), Ok(Some(_))) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for the target to end"
                );
                std::thread::sleep(LOOK);
            }
            std::fs::read_to_string(&self.result).ok()
        }
    }

    impl Drop for Target {
        /// Ends the target's process, if it is still running, and reaps
        /// it: a failed try leaves no window behind for the next (#421).
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// The target process's side: plain Win32 windows on its main thread,
    /// with no runtime of another language to start.
    mod target {
        use std::cell::RefCell;
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use ::windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
        use ::windows::Win32::System::Threading::GetCurrentThreadId;
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_CONTROL};
        use ::windows::Win32::UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, ES_AUTOHSCROLL,
            ES_MULTILINE, GetMessageW, GetWindowTextW, IsWindow, MSG, PostThreadMessageW,
            RegisterClassW, SW_SHOW, SendMessageW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
            WINDOW_STYLE, WM_APP, WM_CLOSE, WM_KEYDOWN, WM_SETFOCUS, WM_SETTEXT, WNDCLASSW,
            WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };
        use ::windows::core::{HSTRING, PCWSTR, w};

        /// The message that tells a target's window to stop answering.
        pub(super) const HANG: u32 = WM_APP + 3;

        /// The thread message a target posts itself before its loop starts:
        /// handled, it writes the ready mark, the thread now pumping.
        const READY: u32 = WM_APP + 4;

        /// An edit control's message selecting a range of its text.
        const EM_SETSEL: u32 = 0x00B1;

        /// What the target's window procedure works with, on its thread.
        #[derive(Default)]
        struct Setup {
            /// The text field inside the frame, if it has one.
            field: Option<HWND>,
            /// Where the field's text is written when the frame closes.
            result: Option<PathBuf>,
            /// What Ctrl+C copies, and the file marked when it does.
            copies: Option<(String, PathBuf)>,
            /// The clipboard's owner of the last copy, kept while the
            /// process runs.
            owner: Option<pane_core::clipboard::testing::Owner>,
        }

        thread_local! {
            static SETUP: RefCell<Setup> = RefCell::new(Setup::default());
        }

        /// The target's windows, made from its environment, then its
        /// message loop until its top-level window is gone. `kind` is
        /// `frame` — a plain window, with a text field of the class
        /// `PANE_FIELD_CLASS` filling it if that is set — or `field` — a
        /// top-level text field of that class.
        pub(super) fn run(kind: &str) {
            let variable = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
            let title = variable("PANE_TARGET_TITLE").unwrap_or_default();
            let ready = variable("PANE_READY_MARK").map(PathBuf::from);
            let class = variable("PANE_FIELD_CLASS");
            if class.as_deref() == Some("RICHEDIT50W") {
                // SAFETY: a system library, which registers the class.
                unsafe { LoadLibraryW(w!("msftedit.dll")) }.expect("the rich edit's library");
            }
            let (window, field) = match kind {
                "frame" => {
                    let frame = frame(&title);
                    let field = class.as_deref().map(|class| field_in(frame, class));
                    (frame, field)
                }
                "field" => {
                    let class = class.as_deref().expect("the field's class");
                    (top_level_field(class, &title), None)
                }
                other => panic!("no target of the kind {other}"),
            };
            let text_field = field.unwrap_or(window);
            if let Some(text) = variable("PANE_FIELD_TEXT") {
                let text = HSTRING::from(text.as_str());
                // SAFETY: the field is this thread's; the text is
                // NUL-terminated and outlives the call.
                unsafe {
                    SendMessageW(
                        text_field,
                        WM_SETTEXT,
                        None,
                        Some(LPARAM(text.as_ptr() as isize)),
                    )
                };
            }
            if variable("PANE_SELECT_ALL").as_deref() == Some("1") {
                // SAFETY: the field is this thread's; 0 to -1 is all of it.
                unsafe { SendMessageW(text_field, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1))) };
            }
            SETUP.with(|setup| {
                let mut setup = setup.borrow_mut();
                setup.field = field;
                setup.result = variable("PANE_TARGET_RESULT").map(PathBuf::from);
                setup.copies = variable("PANE_COPIED_TEXT")
                    .zip(variable("PANE_COPIED_MARK").map(PathBuf::from));
            });
            // Shown once set up: a window shown as it is made may be
            // activated then, before its frame knows the field to give the
            // focus to, and it would keep the focus itself.
            // SAFETY: this thread's own window.
            let _ = unsafe { ShowWindow(window, SW_SHOW) };
            // SAFETY: no arguments; the thread has its queue, its windows
            // made.
            let thread = unsafe { GetCurrentThreadId() };
            // SAFETY: this thread's own id and a plain message.
            let _ = unsafe { PostThreadMessageW(thread, READY, WPARAM(0), LPARAM(0)) };
            let mut message = MSG::default();
            loop {
                // SAFETY: `message` is writable for the call.
                if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
                    break;
                }
                if message.hwnd.is_invalid() && message.message == READY {
                    if let Some(ready) = &ready {
                        mark_ready(ready, window);
                    }
                    continue;
                }
                // SAFETY: a message this thread's queue returned; the keys
                // are made into characters, which the fields type and
                // paste from.
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                // SAFETY: plain values.
                if !unsafe { IsWindow(Some(window)) }.as_bool() {
                    break;
                }
            }
        }

        /// Writes the ready mark: the top-level window, as a number,
        /// written whole before the mark appears.
        fn mark_ready(ready: &Path, window: HWND) {
            let written = (window.0 as usize).to_string();
            let partial = ready.with_extension("partial");
            std::fs::write(&partial, written).expect("the ready mark");
            std::fs::rename(&partial, ready).expect("the ready mark");
        }

        /// The window a target's ready mark names, once it is there.
        pub(super) fn ready(ready: &Path) -> Option<HWND> {
            let written = std::fs::read_to_string(ready).ok()?;
            let window = written.trim().parse::<usize>().ok()?;
            Some(HWND(window as *mut _))
        }

        /// A plain top-level window of the target's own class, hidden.
        fn frame(title: &str) -> HWND {
            let class = w!("PaneRealInputTarget");
            // SAFETY: no arguments; the module is this process's.
            let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }.expect("the module");
            let registered = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance.into(),
                lpszClassName: class,
                ..WNDCLASSW::default()
            };
            // SAFETY: `registered` is fully initialized, and its name and
            // instance live as long as the process.
            unsafe { RegisterClassW(&registered) };
            let title = HSTRING::from(title);
            // SAFETY: the class is registered; a window with no menu or
            // creation data, shown once it is set up.
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    &title,
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    400,
                    200,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
            }
            .expect("the target's window")
        }

        /// A text field of `class` filling `frame`.
        fn field_in(frame: HWND, class: &str) -> HWND {
            let class = HSTRING::from(class);
            let style = WS_CHILD | WS_VISIBLE | WS_BORDER | style_of(&[ES_AUTOHSCROLL]);
            // SAFETY: a system class; a child of this thread's window.
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    &class,
                    PCWSTR::null(),
                    style,
                    0,
                    0,
                    380,
                    150,
                    Some(frame),
                    None,
                    None,
                    None,
                )
            }
            .expect("the target's field")
        }

        /// A top-level text field of `class`, titled `title`, hidden.
        fn top_level_field(class: &str, title: &str) -> HWND {
            let class = HSTRING::from(class);
            let title = HSTRING::from(title);
            let style = WS_OVERLAPPEDWINDOW | style_of(&[ES_MULTILINE]);
            // SAFETY: a system class; a window with no menu or creation
            // data, shown once it is set up.
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    &class,
                    &title,
                    style,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    400,
                    200,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .expect("the target's field")
        }

        /// An edit control's styles as a window's style.
        fn style_of(styles: &[i32]) -> WINDOW_STYLE {
            WINDOW_STYLE(styles.iter().fold(0, |all, style| all | *style as u32))
        }

        /// The frame's window procedure: the focus goes to its field, its
        /// field's text is written as it closes, Ctrl+C copies where it is
        /// to, and [`HANG`] stops its thread.
        extern "system" fn procedure(
            window: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            match message {
                WM_SETFOCUS => {
                    if let Some(field) = SETUP.with(|setup| setup.borrow().field) {
                        // SAFETY: the frame's own field.
                        let _ = unsafe { SetFocus(Some(field)) };
                        return LRESULT(0);
                    }
                }
                WM_CLOSE => SETUP.with(|setup| {
                    let setup = setup.borrow();
                    if let (Some(field), Some(result)) = (setup.field, &setup.result) {
                        std::fs::write(result, text_of(field)).expect("the field's text");
                    }
                }),
                WM_KEYDOWN if wparam.0 == usize::from(b'C') && control_down() => {
                    let copied = SETUP.with(|setup| {
                        let mut setup = setup.borrow_mut();
                        let Some((text, mark)) = setup.copies.clone() else {
                            return false;
                        };
                        let owner = pane_core::clipboard::testing::set_text(&text, &[])
                            .expect("the copy target's copy");
                        setup.owner = Some(owner);
                        std::fs::write(mark, "copied").expect("the copy mark");
                        true
                    });
                    if copied {
                        return LRESULT(0);
                    }
                }
                HANG => std::thread::sleep(Duration::from_secs(300)),
                _ => {}
            }
            // SAFETY: the arguments are those this procedure was called with.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }

        /// Whether Ctrl is down as the key being handled was pressed.
        fn control_down() -> bool {
            // SAFETY: a plain value.
            let state = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) };
            state < 0
        }

        /// The text of `field`, a window of this thread.
        fn text_of(field: HWND) -> String {
            let mut text = vec![0u16; 4096];
            // SAFETY: `text` is writable for its length.
            let read = unsafe { GetWindowTextW(field, &mut text) };
            String::from_utf16_lossy(&text[..read.clamp(0, text.len() as i32) as usize])
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

        // UI Automation reads a text box's selection, without touching
        // the clipboard.
        let selected =
            Target::selection(folder.path(), SELECTED, true).expect("the selection target");
        take_front(selected.window());
        wait("the selection target to be recorded", || {
            named_for(system.as_ref())
        });
        // The read honestly refuses a target of a higher integrity level
        // than its own (Windows would not deliver its keys, so the
        // simulated copy could not reach it). The test and its target run
        // at the same level, elevated on a CI runner or not on a user's
        // desktop, so the reads below reach it.
        assert_eq!(
            system.selected_text(),
            Ok(Some(SELECTION.into())),
            "UI Automation reads the text box's selection"
        );
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));
        assert!(listener.seen().is_empty(), "the read changed nothing");
        // Pane's own window in front, so closing the target changes no
        // foreground the watcher would record.
        take_front(own);
        selected.close();

        // Nothing selected is an answer, not a failure — read while
        // Pane's own window is in front, as a command's read is.
        let empty =
            Target::selection(folder.path(), EMPTY, false).expect("the empty selection target");
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
        // put back. Read while Pane's own window is in front, as a
        // command's read is: the copy brings the target back to the front
        // and waits for it to settle before it sends the keys.
        let mark = folder.path().join("copied.txt");
        let copying = Target::copy(folder.path(), &mark).expect("the copy target");
        take_front(copying.window());
        wait("the copy target to be recorded", || {
            named_for(system.as_ref())
        });
        take_front(own);
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
        let hung = Target::hung(folder.path()).expect("the hung target");
        take_front(hung.window());
        wait("the hung target to be recorded", || {
            named_for(system.as_ref())
        });
        hung.hang();
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

        drop(hung);
        listener.stop();
    }

    /// With Pane's window in front, as it is when a command asks for the
    /// selection, the target's thread names no focused window. The read
    /// once looked at the target's top-level window then, which has no
    /// text of its own in most applications, and fell to the simulated
    /// copy (#432). The control focused in the target before Pane came to
    /// the front is read instead, through UI Automation: the clipboard
    /// never changes, and the target stays behind Pane's window.
    #[test]
    fn the_control_focused_behind_pane_s_window_is_read_by_uia() {
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 to read selections with real input");
            return;
        }
        let _serial = serial();
        let folder = tempfile::tempdir().unwrap();
        let listener = Listener::start().expect("the clipboard listener");
        let own = own_window();
        let system = native_selected_text_in_process();
        system
            .copy(&Clip::Text(BEFORE.into()), false)
            .expect("the setup copy");
        wait("the listener to see the setup copy", || {
            !listener.seen().is_empty()
        });
        listener.clear();

        // The user was in the editor, its control focused, and opened
        // Pane over it.
        let editor = Target::editor(folder.path()).expect("the editor");
        take_front(editor.window());
        wait("the editor to be recorded", || named_for(system.as_ref()));
        wait("the editor's control to take the focus", || {
            focus_of(editor.window()).is_some_and(|focus| focus != editor.window())
        });
        take_front(own);

        assert_eq!(
            system.selected_text(),
            Ok(Some(SELECTION.into())),
            "the focused control's selection is read"
        );
        assert_eq!(system.read_clipboard(), Ok(Some(Clip::Text(BEFORE.into()))));
        assert!(
            listener.seen().is_empty(),
            "UI Automation read it, and no copy changed the clipboard"
        );
        assert!(front_is(own), "the read brought nothing to the front");

        editor.close();
        listener.stop();
    }

    /// The window the thread of `window` has focused, if it names one.
    fn focus_of(window: HWND) -> Option<HWND> {
        use ::windows::Win32::UI::WindowsAndMessaging::{GUITHREADINFO, GetGUIThreadInfo};
        // SAFETY: a window handle; the process id is not wanted.
        let thread = unsafe { GetWindowThreadProcessId(window, None) };
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is writable for its size.
        unsafe { GetGUIThreadInfo(thread, &mut info) }.ok()?;
        (!info.hwndFocus.is_invalid()).then_some(info.hwndFocus)
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
