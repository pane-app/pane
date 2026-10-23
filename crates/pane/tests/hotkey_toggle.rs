//! Every Open Pane press toggles the launcher, and the launcher never opens
//! Windows' system menu (#467), driven with real input on Windows: Pane's
//! real launcher window, opened as the binary opens it, with the system's
//! real hotkey adapter, and the hotkey pressed by `SendInput` while
//! another program's window is in front.
//!
//! Each test presses its binding at a person's pace, quickly, and in a
//! burst, and checks that every press toggled — shown means visible and
//! the window in front, hidden means not visible — and measures how long
//! each show and hide took from the key event. Then, with the binding
//! Alt+Space, it holds the Space down as the launcher comes forward (its
//! repeats reach the launcher with Alt held, the system menu's shortcut)
//! and presses the arrows: the list's selection moves and no menu opens.
//! The bindings are the user's Alt+Space, registered with the system;
//! Alt+Space held by another program first, so Pane's keyboard hook takes
//! it, as it does when another launcher has the shortcut; and the Windows
//! key alone, which only the hook recognizes.
//!
//! The windows are GPUI's real ones in this process, and the keys reach
//! the whole desktop, so this runs only where `PANE_TEST_REAL_INPUT=1` and
//! `PANE_TEST_REAL_WINDOWS=1` are set (CI's Windows runner sets both), one
//! test at a time (the nextest group of the real-input tests); without
//! them each test passes without looking. Never run it on a desktop in
//! use.

#[cfg(target_os = "windows")]
mod windows {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use gpui::{App, prelude::*};
    use pane::LauncherWindow;
    use pane_core::{CommandMatches, CommandRegistration, CommandWhen, Launcher, Runtime};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use ::windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MOD_ALT, MOD_NOREPEAT, MapVirtualKeyW, RegisterHotKey,
        SendInput, VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_LWIN, VK_MENU, VK_SPACE,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows,
        GUI_INMENUMODE, GUI_POPUPMENUMODE, GUI_SYSTEMMENUMODE, GUITHREADINFO, GetClassNameW,
        GetForegroundWindow, GetGUIThreadInfo, GetMessageW, GetWindowThreadProcessId,
        IsWindowVisible, MSG, PostQuitMessage, RegisterClassW, SC_KEYMENU, SW_SHOW,
        SetForegroundWindow, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_DESTROY,
        WM_SYSCOMMAND, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };
    use ::windows::core::{BOOL, HSTRING, w};

    /// How long a show or a hide is given before the press counts as
    /// missed.
    const PATIENCE: Duration = Duration::from_millis(1500);

    /// When each press reached the launcher's window, as the GPUI side
    /// forwarded it: the key event's way through the system and the
    /// adapter, told apart from the window's own work.
    static ARRIVED: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

    /// How many presses reached the window from `since` on, and when the
    /// first of them did.
    fn arrived(since: Instant) -> (usize, Option<Instant>) {
        let arrived = ARRIVED.lock().unwrap();
        let after: Vec<Instant> = arrived.iter().copied().filter(|at| *at >= since).collect();
        (after.len(), after.first().copied())
    }

    /// The environment variable that makes the test binary the tests'
    /// other program (see [`other_program`]).
    const OTHER: &str = "PANE_TOGGLE_OTHER";

    fn opted_in() -> bool {
        let set = |name| std::env::var(name).is_ok_and(|value| value == "1");
        set("PANE_TEST_REAL_INPUT") && set("PANE_TEST_REAL_WINDOWS")
    }

    /// The binding a test presses, and how Pane comes to dispatch it.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Binding {
        /// Alt+Space, registered with the system.
        AltSpace,
        /// Alt+Space while the other program holds its registration:
        /// Pane's keyboard hook takes it.
        AltSpaceHeldElsewhere,
        /// The Windows key alone, through the hook.
        WindowsKey,
    }

    impl Binding {
        fn record(self) -> &'static str {
            match self {
                Binding::AltSpace | Binding::AltSpaceHeldElsewhere => "alt+space",
                Binding::WindowsKey => "tap:win",
            }
        }
    }

    #[test]
    fn every_alt_space_press_toggles_the_launcher_and_no_system_menu_opens() {
        run(Binding::AltSpace);
    }

    #[test]
    fn every_alt_space_press_toggles_the_launcher_through_the_keyboard_hook() {
        run(Binding::AltSpaceHeldElsewhere);
    }

    #[test]
    fn every_windows_key_tap_toggles_the_launcher() {
        run(Binding::WindowsKey);
    }

    /// What the test saw: its log, printed whatever happens, and what
    /// failed.
    #[derive(Default)]
    struct Report {
        lines: Vec<String>,
        failures: Vec<String>,
        shows: Vec<Duration>,
        hides: Vec<Duration>,
    }

    impl Report {
        fn log(&mut self, line: String) {
            self.lines.push(line);
        }

        fn fail(&mut self, failure: String) {
            self.lines.push(format!("FAILED: {failure}"));
            self.failures.push(failure);
        }
    }

    fn run(binding: Binding) {
        if !opted_in() {
            eprintln!(
                "skipped: set PANE_TEST_REAL_INPUT=1 and PANE_TEST_REAL_WINDOWS=1 to press real keys"
            );
            return;
        }
        let data = tempfile::tempdir().expect("a data folder");
        std::fs::write(
            data.path().join("settings.json"),
            format!(
                "{{ \"version\": 2, \"open_pane\": \"{}\" }}",
                binding.record()
            ),
        )
        .unwrap();
        // The other program comes first: where it holds Alt+Space, Pane's
        // registration then finds it taken and the hook takes it.
        let other = Other::start(data.path(), binding == Binding::AltSpaceHeldElsewhere)
            .expect("the other program starts");
        let report = Arc::new(Mutex::new(Report::default()));
        let done = Arc::new(AtomicBool::new(false));
        let selected = Arc::new(AtomicI64::new(-1));
        let folder = data.path().to_owned();
        let other_window = other.window as isize;
        {
            let report = report.clone();
            gpui_platform::application().run(move |cx: &mut App| {
                open(cx, folder, other_window, binding, report, done, selected);
            });
        }
        drop(other);
        let report = report.lock().unwrap();
        let summary = |name: &str, all: &[Duration]| {
            let mut sorted = all.to_vec();
            sorted.sort();
            match (sorted.first(), sorted.get(sorted.len() / 2), sorted.last()) {
                (Some(least), Some(median), Some(most)) => format!(
                    "{name}: {} presses, least {least:?}, median {median:?}, most {most:?}",
                    sorted.len()
                ),
                _ => format!("{name}: none"),
            }
        };
        eprintln!("hotkey toggle, {binding:?}:");
        for line in &report.lines {
            eprintln!("  {line}");
        }
        eprintln!(
            "  {}",
            summary("show (key event to shown and in front)", &report.shows)
        );
        eprintln!("  {}", summary("hide (key event to hidden)", &report.hides));
        assert!(report.failures.is_empty(), "{:#?}", report.failures);
    }

    /// The GPUI side, as `main.rs` sets the launcher up: the settings
    /// record, the key bindings, the system's adapter and the launcher's
    /// window, its presses forwarded to the window. The presses are made
    /// on a thread of the test's own ([`drive`]), which reads the windows'
    /// native state; this side keeps the selection where it can read it,
    /// and quits once the thread is done.
    fn open(
        cx: &mut App,
        folder: PathBuf,
        other: isize,
        binding: Binding,
        report: Arc<Mutex<Report>>,
        done: Arc<AtomicBool>,
        selected: Arc<AtomicI64>,
    ) {
        pane::settings::init_with_overrides(
            Some(folder.clone()),
            pane::settings::Overrides::default(),
            cx,
        );
        pane::bind_keys(cx);
        let (sender, mut presses) = pane_core::hotkeys::channel();
        let launcher = Launcher::new(Runtime::start(), commands())
            .with_hotkeys(pane_core::hotkeys::native(sender));
        let core = launcher.clone();
        let window = pane::open_launcher_window(cx, |window, cx| {
            cx.new(|cx| LauncherWindow::new(launcher, window, cx))
        })
        .expect("the launcher's window opens");
        report.lock().unwrap().log(format!(
            "the Open Pane binding {} is dispatched through {:?}; problem: {:?}",
            binding.record(),
            core.open_pane_route(),
            core.open_pane_problem()
        ));
        cx.spawn(async move |cx| {
            while let Some(shortcut) = presses.next().await {
                ARRIVED.lock().unwrap().push(Instant::now());
                let pressed = window.update(cx, |launcher, window, cx| {
                    launcher.hotkey_pressed(&shortcut, window, cx)
                });
                if pressed.is_err() {
                    break;
                }
            }
        })
        .detach();
        let hwnd = window
            .update(cx, |_, window, _| {
                let handle = HasWindowHandle::window_handle(window).ok()?;
                match handle.as_raw() {
                    RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                    _ => None,
                }
            })
            .ok()
            .flatten()
            .expect("the launcher's native window");
        {
            let done = done.clone();
            let selected = selected.clone();
            std::thread::spawn(move || {
                let mut seen = Report::default();
                drive(
                    HWND(hwnd as *mut _),
                    HWND(other as *mut _),
                    binding,
                    &selected,
                    &mut seen,
                );
                let mut report = report.lock().unwrap();
                report.lines.append(&mut seen.lines);
                report.failures.append(&mut seen.failures);
                report.shows.append(&mut seen.shows);
                report.hides.append(&mut seen.hides);
                done.store(true, Ordering::SeqCst);
            });
        }
        cx.spawn(async move |cx| {
            while !done.load(Ordering::SeqCst) {
                let index = core.selected().map_or(-1, |index| index as i64);
                selected.store(index, Ordering::SeqCst);
                cx.background_executor()
                    .timer(Duration::from_millis(5))
                    .await;
            }
            // The windows close before Pane quits, as quitting does
            // through the launcher's own window.
            for window in cx.update(|cx| cx.windows()) {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }

    /// Twelve root commands, "Command 01" and so on, never run: rows for
    /// the arrows to move through.
    fn commands() -> Vec<CommandRegistration> {
        let component =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
        (1..=12)
            .map(|index| CommandRegistration {
                id: format!("command-{index:02}"),
                title: format!("Command {index:02}"),
                subtitle: None,
                component: component.clone(),
                takes_query: false,
                keywords: Vec::new(),
                when: CommandWhen::Always,
                matches: CommandMatches::Title,
            })
            .collect()
    }

    /// The presses and the checks, on the test's own thread.
    fn drive(
        launcher: HWND,
        other: HWND,
        binding: Binding,
        selected: &AtomicI64,
        report: &mut Report,
    ) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !visible(launcher) {
            if Instant::now() > deadline {
                report.fail("the launcher never showed as it opened".into());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !take_front(other) {
            report.fail("the other program's window did not come to the front".into());
            return;
        }
        // The launcher is shown behind the other program: the first press
        // brings it forward.
        let mut shown = toggle(launcher, binding, true, "summoned from behind", report);
        if !shown {
            return;
        }
        // A person's pace, then quick presses: each one toggles, the next
        // pressed once the last one's toggle is seen or after the gap,
        // whichever is later.
        for (pace, gap, presses) in [("paced", 800, 8), ("quick", 150, 10)] {
            for press in 0..presses {
                let started = Instant::now();
                let expected = !shown;
                let name = format!("{pace} press {press}");
                if toggle(launcher, binding, expected, &name, report) {
                    shown = expected;
                } else {
                    shown = in_front(launcher);
                }
                let gap = Duration::from_millis(gap);
                if let Some(rest) = gap.checked_sub(started.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        }
        // A burst, the presses 30 ms apart whatever the window does: an
        // odd number of them from hidden leaves the launcher shown, an
        // even number from shown leaves it shown.
        for presses in [5, 4] {
            let before = in_front(launcher);
            let started = Instant::now();
            for _ in 0..presses {
                press(binding);
                std::thread::sleep(Duration::from_millis(30));
            }
            let expected = before ^ (presses % 2 == 1);
            std::thread::sleep(Duration::from_millis(1500));
            let after = in_front(launcher);
            let hidden_now = !visible(launcher);
            report.log(format!(
                "burst of {presses} from {} ({} presses reached the window): {}",
                if before { "shown" } else { "hidden" },
                arrived(started).0,
                if after {
                    "shown"
                } else if hidden_now {
                    "hidden"
                } else {
                    "visible but not in front"
                }
            ));
            if after != expected || (!after && !hidden_now) {
                report.fail(format!(
                    "after a burst of {presses} presses the launcher was not {}",
                    if expected { "shown" } else { "hidden" }
                ));
            }
        }
        // Hidden, for the arrows' check, and left a moment as a person
        // would before pressing again.
        if in_front(launcher) && !toggle(launcher, binding, false, "hidden for the arrows", report)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(700));
        menu_and_arrows(launcher, binding, selected, report);
    }

    /// Opens the launcher with the binding's keys held as a person holds
    /// them — for Alt+Space, the Space kept down so its repeats reach the
    /// launcher with Alt held, the system menu's own shortcut — then
    /// presses Down twice: the selection moves by two, and no menu opens
    /// at any point.
    fn menu_and_arrows(
        launcher: HWND,
        binding: Binding,
        selected: &AtomicI64,
        report: &mut Report,
    ) {
        let thread = thread_of(launcher);
        let mut menu_seen = None;
        let look = |when: &str, seen: &mut Option<String>| {
            if seen.is_none()
                && let Some(menu) = menu_open(thread)
            {
                *seen = Some(format!("{menu} {when}"));
            }
        };
        let started = Instant::now();
        match binding {
            Binding::WindowsKey => press(binding),
            Binding::AltSpace | Binding::AltSpaceHeldElsewhere => {
                send(&[key(VK_MENU, false), key(VK_SPACE, false)]);
            }
        }
        let came = wait_for(|| in_front(launcher));
        if !came {
            send(&[key(VK_SPACE, true), key(VK_MENU, true)]);
            report.fail("the held hotkey did not bring the launcher forward".into());
            return;
        }
        report.log(format!("held press: shown after {:?}", started.elapsed()));
        if binding != Binding::WindowsKey {
            // The Space's repeats, with Alt still down.
            for _ in 0..4 {
                std::thread::sleep(Duration::from_millis(40));
                send(&[key(VK_SPACE, false)]);
                look("while Alt+Space was held", &mut menu_seen);
            }
            send(&[key(VK_SPACE, true), key(VK_MENU, true)]);
        }
        std::thread::sleep(Duration::from_millis(200));
        look("after the hotkey's keys were released", &mut menu_seen);
        let before = selected.load(Ordering::SeqCst);
        for _ in 0..2 {
            send(&[key(VK_DOWN, false), key(VK_DOWN, true)]);
            std::thread::sleep(Duration::from_millis(80));
            look("as Down was pressed", &mut menu_seen);
        }
        std::thread::sleep(Duration::from_millis(300));
        look("after the arrows", &mut menu_seen);
        let after = selected.load(Ordering::SeqCst);
        report.log(format!(
            "arrows: selection {before} -> {after}; menu: {}",
            menu_seen.as_deref().unwrap_or("none")
        ));
        if let Some(menu) = menu_seen {
            report.fail(format!("the launcher opened a menu: {menu}"));
            // Closed again, so nothing is left open.
            for _ in 0..2 {
                send(&[key(VK_ESCAPE, false), key(VK_ESCAPE, true)]);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if before < 0 || after != before + 2 {
            report.fail(format!(
                "two presses of Down moved the selection from {before} to {after}"
            ));
        }
        if !in_front(launcher) {
            report.fail("the launcher lost the front during the arrows".into());
        }
    }

    /// Presses the binding once and waits for the launcher to be shown
    /// and in front (`shown`) or hidden, logging how long it took; a
    /// press that changed nothing within [`PATIENCE`] fails the test.
    fn toggle(
        launcher: HWND,
        binding: Binding,
        shown: bool,
        name: &str,
        report: &mut Report,
    ) -> bool {
        let started = Instant::now();
        press(binding);
        let reached = wait_for(|| {
            if shown {
                in_front(launcher)
            } else {
                !visible(launcher)
            }
        });
        let took = started.elapsed();
        let wanted = if shown { "shown" } else { "hidden" };
        let (count, first) = arrived(started);
        let reached_pane = match first {
            Some(at) => format!("{:?}", at.duration_since(started)),
            None => "never".into(),
        };
        if reached {
            report.log(format!(
                "{name}: {wanted} after {took:?} (the press reached the window after {reached_pane})"
            ));
            if shown {
                report.shows.push(took);
            } else {
                report.hides.push(took);
            }
        } else {
            let state = if in_front(launcher) {
                "shown and in front"
            } else if visible(launcher) {
                "visible but not in front"
            } else {
                "hidden"
            };
            report.fail(format!(
                "{name}: the launcher was to be {wanted}, and was {state} {took:?} after the \
                 press ({count} presses reached the window, the first after {reached_pane})"
            ));
        }
        reached
    }

    /// Waits up to [`PATIENCE`] for `check`, looking every millisecond or
    /// so.
    fn wait_for(check: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if check() {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::yield_now();
        }
    }

    /// Presses and releases the binding's keys.
    fn press(binding: Binding) {
        match binding {
            Binding::AltSpace | Binding::AltSpaceHeldElsewhere => send(&[
                key(VK_MENU, false),
                key(VK_SPACE, false),
                key(VK_SPACE, true),
                key(VK_MENU, true),
            ]),
            Binding::WindowsKey => send(&[key(VK_LWIN, false), key(VK_LWIN, true)]),
        }
    }

    /// One key event, as a keyboard reports it (no tag: the hook takes it
    /// for another tool's, which fires the binding).
    fn key(key: VIRTUAL_KEY, up: bool) -> INPUT {
        // SAFETY: a plain conversion of a key code.
        let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) } as u16;
        let mut flags = if up {
            KEYEVENTF_KEYUP
        } else {
            KEYBD_EVENT_FLAGS(0)
        };
        if key == VK_LWIN || key == VK_DOWN {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(events: &[INPUT]) {
        // SAFETY: the events are this slice's.
        let sent = unsafe { SendInput(events, size_of::<INPUT>() as i32) };
        assert_eq!(sent as usize, events.len(), "the keys were injected");
    }

    fn visible(window: HWND) -> bool {
        // SAFETY: only reads the window's state.
        unsafe { IsWindowVisible(window) }.as_bool()
    }

    /// Whether `window` is shown and the window in front.
    fn in_front(window: HWND) -> bool {
        // SAFETY: no arguments.
        visible(window) && unsafe { GetForegroundWindow() } == window
    }

    fn thread_of(window: HWND) -> u32 {
        // SAFETY: only reads which thread made the window.
        unsafe { GetWindowThreadProcessId(window, None) }
    }

    /// The menu open on `thread`, if one is: the thread in a menu's mode,
    /// or a menu's window (`#32768`) of this process shown.
    fn menu_open(thread: u32) -> Option<String> {
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is sized for the call.
        if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_ok() {
            let modes = info.flags;
            if modes.contains(GUI_SYSTEMMENUMODE) {
                return Some("the system menu's mode".into());
            }
            if modes.contains(GUI_POPUPMENUMODE) {
                return Some("a pop-up menu's mode".into());
            }
            if modes.contains(GUI_INMENUMODE) {
                return Some("a menu's mode".into());
            }
        }
        let mut found = false;
        // SAFETY: the callback only reads each window's class and owner,
        // and writes through the pointer it is given to `found`.
        let _ = unsafe { EnumWindows(Some(menu_window), LPARAM(&mut found as *mut bool as isize)) };
        found.then(|| "a menu window shown".into())
    }

    unsafe extern "system" fn menu_window(window: HWND, found: LPARAM) -> BOOL {
        let mut class = [0u16; 16];
        // SAFETY: the buffer is the call's, with its length.
        let length = unsafe { GetClassNameW(window, &mut class) } as usize;
        let mut process = 0;
        // SAFETY: only reads the window's owner.
        unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
        if String::from_utf16_lossy(&class[..length]) == "#32768"
            && process == std::process::id()
            && visible(window)
        {
            // SAFETY: the pointer `menu_open` passed, to its live bool.
            unsafe { *(found.0 as *mut bool) = true };
            return BOOL(0);
        }
        BOOL(1)
    }

    /// Brings `window` to the front, as a person clicking it would: the
    /// plain call, then attached to the input of the thread in front.
    fn take_front(window: HWND) -> bool {
        // SAFETY: plain values; the attachment is undone at once.
        unsafe {
            let _ = SetForegroundWindow(window);
            if GetForegroundWindow() != window {
                let front = GetForegroundWindow();
                let theirs = GetWindowThreadProcessId(front, None);
                let ours = GetCurrentThreadId();
                if theirs != 0 && theirs != ours {
                    let _ = AttachThreadInput(ours, theirs, true);
                    let _ = SetForegroundWindow(window);
                    let _ = AttachThreadInput(ours, theirs, false);
                }
            }
        }
        wait_for(|| {
            // SAFETY: no arguments.
            let front = unsafe { GetForegroundWindow() };
            front == window
        })
    }

    /// The other program in front: the test binary started again, which
    /// shows a plain window and, where asked, holds Alt+Space's
    /// registration. Dropping it ends the process.
    struct Other {
        window: *mut core::ffi::c_void,
        child: std::process::Child,
    }

    impl Other {
        fn start(folder: &Path, hold: bool) -> Result<Other, String> {
            let ready = folder.join("other.ready");
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            let child = std::process::Command::new(program)
                .args([
                    "hotkey_toggle::windows::other_program",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .env(OTHER, &ready)
                .env("PANE_TOGGLE_HOLD", if hold { "1" } else { "0" })
                .stdout(std::process::Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())?;
            let mut other = Other {
                window: std::ptr::null_mut(),
                child,
            };
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Ok(text) = std::fs::read_to_string(&ready)
                    && let Some((window, held)) = text.trim().split_once(' ')
                {
                    if hold && held != "held" {
                        return Err(format!(
                            "the other program could not hold Alt+Space: {held}"
                        ));
                    }
                    other.window = window.parse::<isize>().map_err(|e| e.to_string())? as *mut _;
                    return Ok(other);
                }
                if let Ok(Some(status)) = other.child.try_wait() {
                    return Err(format!("the other program ended: {status}"));
                }
                if Instant::now() > deadline {
                    return Err("timed out waiting for the other program".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for Other {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// The test binary started again as the tests' other program: a
    /// plain window, Alt+Space registered where `PANE_TOGGLE_HOLD=1`, and
    /// its messages pumped until the test ends the process. Ignored by
    /// every ordinary run, and does nothing without [`OTHER`].
    #[test]
    #[ignore = "the hotkey toggle tests' other program, which they start themselves"]
    fn other_program() {
        let Ok(ready) = std::env::var(OTHER) else {
            return;
        };
        // SAFETY: a class and a window of this thread's own, whose
        // procedure is the plain function below; the messages are this
        // thread's.
        unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                lpszClassName: w!("PaneToggleOther"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("PaneToggleOther"),
                &HSTRING::from("The other program"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                640,
                480,
                None,
                None,
                None,
                None,
            )
            .expect("the other program's window");
            let _ = ShowWindow(window, SW_SHOW);
            let held = if std::env::var("PANE_TOGGLE_HOLD").as_deref() == Ok("1") {
                match RegisterHotKey(None, 1, MOD_ALT | MOD_NOREPEAT, u32::from(VK_SPACE.0)) {
                    Ok(()) => "held".to_owned(),
                    Err(error) => format!("refused:{}", error.code().0),
                }
            } else {
                "free".to_owned()
            };
            std::fs::write(&ready, format!("{} {held}", window.0 as isize)).unwrap();
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    /// The other program's window: an Alt pressed alone does not put it
    /// in its menu's mode, so what the test sees is Pane's doing alone.
    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU {
            return LRESULT(0);
        }
        if message == WM_DESTROY {
            // SAFETY: no arguments.
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        // SAFETY: the arguments this procedure was called with.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
}
