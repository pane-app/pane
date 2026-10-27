//! The launcher hides once another application takes the focus (#571), as
//! Raycast's does, read back from real windows on Windows: the launcher's
//! window, opened as the binary opens it (`pane::open_launcher_window`),
//! brought forward by the tray's Open Pane and by an attention launch
//! (ADR 0053), hides when the taskbar (`Shell_TrayWnd`, Explorer's) is made
//! the foreground window, as clicking its clock does; the Settings window,
//! Pane's own, taking the focus leaves it shown. The windows are GPUI's
//! real Windows windows in this process and the taskbar is the desktop's,
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
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gpui::{App, AsyncApp, WindowHandle, prelude::*};
    use pane::{LauncherWindow, SettingsWindow};
    use pane_core::develop::Toolchains;
    use pane_core::tray::TrayAction;
    use pane_core::{LaunchType, Launcher, PackageIdentity, Runtime, Screen};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
        SetForegroundWindow,
    };
    use ::windows::core::w;

    use super::packages::assembled_package;

    /// How long a window is given to show, come forward or hide.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// How long the launcher is watched to stay shown.
    const STAYS: Duration = Duration::from_millis(1500);

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
    /// front, then attached to the input of the thread in front.
    fn take_front(window: HWND) {
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
    }

    /// A step of the test, and the launcher's state read after it.
    type Step = (&'static str, Seen);

    #[test]
    fn the_launcher_hides_when_the_taskbar_takes_the_focus() {
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_WINDOWS=1 to open real windows");
            return;
        }
        // SAFETY: a lookup by class name; the handle is only compared and
        // made the foreground window.
        let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
            .expect("this desktop has a taskbar (Shell_TrayWnd)");
        let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let folder = assembled_package("sample-no-view", &sources.path().join("alarm"));
        let alarm = PackageIdentity::local(&folder).unwrap();
        let extensions = data.path().join("extensions");
        let report: Rc<RefCell<Vec<Step>>> = Rc::default();
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
                let step = |name, state| record.borrow_mut().push((name, state));
                let summon = |cx: &mut AsyncApp| {
                    window
                        .update(cx, |launcher, window, cx| {
                            launcher.tray_selected(TrayAction::OpenPane, window, cx)
                        })
                        .ok();
                };

                // Summoned, then the taskbar takes the focus.
                summon(cx);
                step("summoned", once(launcher, Seen::InFront, cx).await);
                take_front(taskbar);
                step(
                    "the taskbar took the focus from the summoned launcher",
                    once(launcher, Seen::Hidden, cx).await,
                );

                // Shown by an attention launch, then the taskbar takes the
                // focus.
                let launched = core.launch_as(
                    &alarm,
                    None,
                    "show",
                    LaunchType::UserInitiated,
                    Some(r#"{"alarm":"07:00"}"#),
                    false,
                );
                assert!(launched.is_ok(), "the launch is accepted: {launched:?}");
                step(
                    "shown by an attention launch",
                    once(launcher, Seen::InFront, cx).await,
                );
                take_front(taskbar);
                step(
                    "the taskbar took the focus from the attention-launched launcher",
                    once(launcher, Seen::Hidden, cx).await,
                );

                // Summoned, then the Settings window takes the focus: the
                // launcher stays, behind it.
                summon(cx);
                step("summoned again", once(launcher, Seen::InFront, cx).await);
                window
                    .update(cx, |launcher, window, cx| {
                        launcher.tray_selected(TrayAction::Settings, window, cx)
                    })
                    .ok();
                let started = Instant::now();
                let mut settings_state = seen(launcher);
                while started.elapsed() < STAYS && settings_state != Seen::Hidden {
                    cx.background_executor()
                        .timer(Duration::from_millis(20))
                        .await;
                    settings_state = seen(launcher);
                }
                let settings_open = cx.update(|cx| {
                    cx.windows()
                        .iter()
                        .any(|window| window.downcast::<SettingsWindow>().is_some())
                });
                assert!(settings_open, "the Settings window opened");
                step("the Settings window took the focus", settings_state);

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

        let report = report.borrow();
        let state = |name: &str| {
            report
                .iter()
                .find(|(step, _)| *step == name)
                .map(|(_, state)| *state)
                .unwrap_or_else(|| panic!("the test never reached {name:?}: {report:?}"))
        };
        assert_eq!(state("summoned"), Seen::InFront, "{report:?}");
        assert_eq!(
            state("the taskbar took the focus from the summoned launcher"),
            Seen::Hidden,
            "{report:?}"
        );
        assert_eq!(
            state("shown by an attention launch"),
            Seen::InFront,
            "{report:?}"
        );
        assert_eq!(
            state("the taskbar took the focus from the attention-launched launcher"),
            Seen::Hidden,
            "{report:?}"
        );
        assert_eq!(state("summoned again"), Seen::InFront, "{report:?}");
        assert_eq!(
            state("the Settings window took the focus"),
            Seen::Behind,
            "{report:?}"
        );
    }
}
