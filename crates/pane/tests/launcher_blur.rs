//! The launcher hides once another application takes the focus (#571), as
//! Raycast's does, read back from real windows on Windows: the launcher's
//! window, opened as the binary opens it (`pane::open_launcher_window`),
//! brought forward by the tray's Open Pane and by an attention launch
//! (ADR 0053), hides when the taskbar (`Shell_TrayWnd`, Explorer's) is made
//! the foreground window, as clicking its clock does. Where this desktop's
//! taskbar does not keep the foreground it is given (the test logs whether
//! it did), another program's window takes it instead: the test binary
//! started again, showing a plain window. The Settings window, Pane's own,
//! taking the focus leaves the launcher shown. The windows are GPUI's real
//! Windows windows in this process, and the foreground is the desktop's,
//! so this needs a desktop session: it runs only where
//! `PANE_TEST_REAL_WINDOWS=1` is set (CI's Windows runner sets it), and
//! without it the test passes without looking. It injects no input and
//! closes what it opened. The attention launch's command is the no-view
//! sample's "Show launch", from `cargo xtask guests`.

#[cfg(target_os = "windows")]
#[path = "support/packages.rs"]
mod packages;

#[cfg(target_os = "windows")]
mod windows {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gpui::{App, AsyncApp, WindowHandle, prelude::*};
    use pane::{LauncherWindow, SettingsWindow};
    use pane_core::develop::Toolchains;
    use pane_core::tray::TrayAction;
    use pane_core::{LaunchType, Launcher, PackageIdentity, Runtime, Screen};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use ::windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW,
        GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, IsWindowVisible, MSG,
        PostQuitMessage, RegisterClassW, SW_SHOW, SetForegroundWindow, ShowWindow,
        TranslateMessage, WINDOW_EX_STYLE, WM_DESTROY, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };
    use ::windows::core::{HSTRING, w};

    use super::packages::assembled_package;

    /// How long a window is given to show or come forward.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// How long the launcher is given to hide once another window has the
    /// foreground, and is watched to stay shown behind the Settings window.
    const AWAY: Duration = Duration::from_secs(3);

    /// How long a window must keep the foreground to count as having taken
    /// the focus: past the launcher's own look, 100 ms after it lost it.
    const HELD: Duration = Duration::from_millis(300);

    /// The environment variable that makes the test binary the test's
    /// other program (see [`other_program`]).
    const OTHER: &str = "PANE_BLUR_OTHER";

    fn opted_in() -> bool {
        std::env::var("PANE_TEST_REAL_WINDOWS").is_ok_and(|value| value == "1")
    }

    /// The launcher's state as the system has it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Seen {
        Hidden,
        /// Shown, another window in front.
        Behind,
        InFront,
    }

    fn seen(launcher: HWND) -> Seen {
        // SAFETY: both calls only read; `launcher` is the live window's.
        unsafe {
            if !IsWindowVisible(launcher).as_bool() {
                Seen::Hidden
            } else if GetForegroundWindow() == launcher {
                Seen::InFront
            } else {
                Seen::Behind
            }
        }
    }

    /// The launcher's state once it is `wanted` (or as it was when the
    /// patience ran out).
    async fn once(launcher: HWND, wanted: Seen, cx: &mut AsyncApp) -> Seen {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let state = seen(launcher);
            if state == wanted || Instant::now() > deadline {
                return state;
            }
            cx.background_executor()
                .timer(Duration::from_millis(20))
                .await;
        }
    }

    /// Makes `window` the foreground window, as a click on it would: the
    /// plain call, which this process may make while its launcher is in
    /// front, then attached to the input of the thread in front. On a
    /// thread of its own, as another program's click is, so the window
    /// thread hears of it through its messages.
    fn take_front(window: isize) {
        std::thread::spawn(move || {
            let window = HWND(window as *mut _);
            // SAFETY: plain values; the attachment is undone at once.
            unsafe {
                if !SetForegroundWindow(window).as_bool() {
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
        });
    }

    /// Gives `window` the foreground and waits up to [`AWAY`] for the
    /// launcher to hide: the launcher's state then, and whether `window`
    /// kept the foreground for [`HELD`] meanwhile.
    async fn away(window: isize, launcher: HWND, cx: &mut AsyncApp) -> (Seen, bool) {
        take_front(window);
        let deadline = Instant::now() + AWAY;
        let (mut since, mut reached) = (None, false);
        loop {
            // SAFETY: no arguments; the handle is only compared.
            if unsafe { GetForegroundWindow() }.0 as isize == window {
                let since = *since.get_or_insert_with(Instant::now);
                reached |= since.elapsed() >= HELD;
            } else {
                since = None;
            }
            let state = seen(launcher);
            if state == Seen::Hidden || Instant::now() > deadline {
                return (state, reached);
            }
            cx.background_executor()
                .timer(Duration::from_millis(20))
                .await;
        }
    }

    /// What became of the launcher once the focus went elsewhere: the
    /// taskbar's try, and the other program's where the taskbar's left it
    /// shown.
    #[derive(Debug, Default)]
    struct Left {
        taskbar: Option<(Seen, bool)>,
        other: Option<(Seen, bool)>,
    }

    impl Left {
        /// Whether the launcher hid once another application's window was
        /// seen in front, and never stayed shown behind one.
        fn hid(&self) -> bool {
            let hid_after = |taken: Option<(Seen, bool)>| match taken {
                Some((Seen::Hidden, _)) => Some(true),
                Some((_, true)) => Some(false),
                _ => None,
            };
            match (hid_after(self.taskbar), hid_after(self.other)) {
                (Some(false), _) | (_, Some(false)) => false,
                (Some(true), _) | (_, Some(true)) => true,
                (None, None) => false,
            }
        }
    }

    /// The taskbar takes the foreground, and, if the launcher is still
    /// shown, the other program's window.
    async fn focus_elsewhere(
        taskbar: Option<isize>,
        other: isize,
        launcher: HWND,
        cx: &mut AsyncApp,
    ) -> Left {
        let mut left = Left::default();
        if let Some(taskbar) = taskbar {
            left.taskbar = Some(away(taskbar, launcher, cx).await);
        }
        if seen(launcher) != Seen::Hidden {
            left.other = Some(away(other, launcher, cx).await);
        }
        left
    }

    /// What the test saw.
    #[derive(Debug, Default)]
    struct Report {
        summoned: Option<Seen>,
        summoned_left: Left,
        attention: Option<Seen>,
        attention_left: Left,
        summoned_again: Option<Seen>,
        behind_settings: Option<Seen>,
    }

    #[test]
    fn the_launcher_hides_when_the_taskbar_takes_the_focus() {
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_WINDOWS=1 to open real windows");
            return;
        }
        // SAFETY: a lookup by class name; the handle is only compared and
        // made the foreground window.
        let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
            .ok()
            .map(|taskbar| taskbar.0 as isize);
        let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let other = Other::start(data.path()).expect("the other program starts");
        let other_window = other.window;
        let folder = assembled_package("sample-no-view", &sources.path().join("alarm"));
        let alarm = PackageIdentity::local(&folder).unwrap();
        let extensions = data.path().join("extensions");
        let report: Rc<RefCell<Report>> = Rc::default();
        let record = report.clone();
        gpui_platform::application().run(move |cx: &mut App| {
            pane::settings::init_with_overrides(
                Some(data.path().to_owned()),
                pane::settings::Overrides::default(),
                cx,
            );
            pane::bind_keys(cx);
            let (changed, changes) = pane_core::changes::channel();
            let launcher = Launcher::with_packages(Runtime::start(), vec![], extensions)
                .with_development(Arc::new(Toolchains::from_env(None)), changed);
            let core = launcher.clone();
            let window: WindowHandle<LauncherWindow> =
                pane::open_launcher_window(cx, |window, cx| {
                    cx.new(|cx| {
                        let mut launcher = LauncherWindow::new(launcher, window, cx);
                        launcher.follow_changes(changes, window, cx);
                        // The user is not typing: an attention launch shows
                        // at once.
                        launcher.set_input_idle(|| Some(Duration::from_secs(5)));
                        launcher
                    })
                })
                .expect("the launcher's window opens");
            let hwnd = window
                .update(cx, |_, window, _| {
                    // `Window` also has an inherent `window_handle`, so the
                    // raw-window-handle trait is named.
                    let handle = HasWindowHandle::window_handle(window).ok()?;
                    match handle.as_raw() {
                        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                        _ => None,
                    }
                })
                .ok()
                .flatten()
                .expect("the launcher's native window");
            let launcher = HWND(hwnd as *mut _);
            cx.spawn(async move |cx| {
                core.install_package(&folder).await;
                while !matches!(core.view().screen, Screen::Root { .. }) {
                    core.back();
                }
                let summon = |cx: &mut AsyncApp| {
                    window
                        .update(cx, |launcher, window, cx| {
                            launcher.tray_selected(TrayAction::OpenPane, window, cx)
                        })
                        .ok();
                };

                // Summoned, then the focus goes elsewhere.
                summon(cx);
                let summoned = once(launcher, Seen::InFront, cx).await;
                record.borrow_mut().summoned = Some(summoned);
                let left = focus_elsewhere(taskbar, other_window, launcher, cx).await;
                record.borrow_mut().summoned_left = left;

                // Shown by an attention launch, then the focus goes
                // elsewhere.
                let launched = core.launch_as(
                    &alarm,
                    None,
                    "show",
                    LaunchType::UserInitiated,
                    Some(r#"{"alarm":"07:00"}"#),
                    false,
                );
                if launched.is_ok() {
                    let attention = once(launcher, Seen::InFront, cx).await;
                    record.borrow_mut().attention = Some(attention);
                    let left = focus_elsewhere(taskbar, other_window, launcher, cx).await;
                    record.borrow_mut().attention_left = left;
                }

                // Summoned, then the Settings window takes the focus: the
                // launcher stays, behind it.
                summon(cx);
                let again = once(launcher, Seen::InFront, cx).await;
                record.borrow_mut().summoned_again = Some(again);
                window
                    .update(cx, |launcher, window, cx| {
                        launcher.tray_selected(TrayAction::Settings, window, cx)
                    })
                    .ok();
                let started = Instant::now();
                let mut state = seen(launcher);
                while started.elapsed() < AWAY && state != Seen::Hidden {
                    cx.background_executor()
                        .timer(Duration::from_millis(20))
                        .await;
                    state = seen(launcher);
                }
                let settings_open = cx.update(|cx| {
                    cx.windows()
                        .iter()
                        .any(|window| window.downcast::<SettingsWindow>().is_some())
                });
                if settings_open {
                    record.borrow_mut().behind_settings = Some(state);
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
        });
        drop(other);

        let report = report.borrow();
        // (Seen, whether the window was seen in front): the taskbar's
        // try says whether this desktop's taskbar kept the foreground.
        eprintln!("launcher blur, taskbar found: {}", taskbar.is_some());
        eprintln!("  {report:#?}");
        assert_eq!(report.summoned, Some(Seen::InFront), "{report:#?}");
        assert!(
            report.summoned_left.hid(),
            "the summoned launcher hid once another application had the focus: {report:#?}"
        );
        assert_eq!(report.attention, Some(Seen::InFront), "{report:#?}");
        assert!(
            report.attention_left.hid(),
            "the attention-launched launcher hid once another application had the focus: \
             {report:#?}"
        );
        assert_eq!(report.summoned_again, Some(Seen::InFront), "{report:#?}");
        assert_eq!(report.behind_settings, Some(Seen::Behind), "{report:#?}");
    }

    /// The other program: the test binary started again, which shows a
    /// plain window. Dropping it ends the process.
    struct Other {
        window: isize,
        child: std::process::Child,
    }

    impl Other {
        fn start(folder: &Path) -> Result<Other, String> {
            let ready = folder.join("other.ready");
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            let child = std::process::Command::new(program)
                .args([
                    "launcher_blur::windows::other_program",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .env(OTHER, &ready)
                .stdout(std::process::Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())?;
            let mut other = Other { window: 0, child };
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Ok(text) = std::fs::read_to_string(&ready)
                    && let Ok(window) = text.trim().parse::<isize>()
                {
                    other.window = window;
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

    /// The test binary started again as the test's other program: a plain
    /// window, its messages pumped until the test ends the process.
    /// Ignored by every ordinary run, and does nothing without [`OTHER`].
    #[test]
    #[ignore = "the launcher blur test's other program, which it starts itself"]
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
                lpszClassName: w!("PaneBlurOther"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("PaneBlurOther"),
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
            std::fs::write(&ready, format!("{}", window.0 as isize)).unwrap();
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_DESTROY {
            // SAFETY: no arguments.
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        // SAFETY: the arguments this procedure was called with.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
}
