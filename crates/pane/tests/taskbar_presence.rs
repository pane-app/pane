//! The launcher window has no taskbar button and no Alt+Tab entry, as
//! Raycast's has none, while the Settings window keeps its button (#458),
//! read back from real windows on Windows: the launcher's window, opened as
//! the binary opens it (`pane::open_launcher_window`), is a tool window
//! (`WS_EX_TOOLWINDOW`, without `WS_EX_APPWINDOW`, which is what the
//! taskbar and Alt+Tab leave out) from its first show, and still after it
//! is dismissed and summoned again through the tray's Open Pane; the
//! Settings window the tray's Settings opens is an ordinary application
//! window. The windows are GPUI's real Windows windows in this process, so
//! this needs a desktop session: it runs only where
//! `PANE_TEST_REAL_WINDOWS=1` is set (CI's Windows runner sets it), and
//! without it the test passes without looking. It injects no input and
//! closes what it opened. The macOS activation policy and the X11 window
//! manager hints are checked by hand (see `docs/launcher-appearance.md`).

#[cfg(target_os = "windows")]
mod windows {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use gpui::{AnyWindowHandle, App, AsyncApp, prelude::*};
    use pane::{DismissLauncher, LauncherWindow, SettingsWindow};
    use pane_core::tray::TrayAction;
    use pane_core::{Launcher, Runtime};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, IsWindowVisible, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    /// How long a window is given to show or hide: GPUI shows and hides a
    /// Windows window on its next turn of the event loop.
    const PATIENCE: Duration = Duration::from_secs(10);

    fn opted_in() -> bool {
        std::env::var("PANE_TEST_REAL_WINDOWS").is_ok_and(|value| value == "1")
    }

    /// What a window's native state says about its place on the taskbar.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Seen {
        visible: bool,
        tool_window: bool,
        app_window: bool,
    }

    /// A step of the test, and the window's state read after it.
    type Step = (&'static str, Option<Seen>);

    fn seen(window: AnyWindowHandle, cx: &mut AsyncApp) -> Option<Seen> {
        window
            .update(cx, |_, window, _| {
                // `Window` also has an inherent `window_handle`, so the
                // raw-window-handle trait is named.
                let handle = HasWindowHandle::window_handle(window).ok()?;
                let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                    return None;
                };
                let hwnd = HWND(handle.hwnd.get() as *mut _);
                // SAFETY: `hwnd` is this live window's; both calls only read.
                let (style, visible) =
                    unsafe { (GetWindowLongPtrW(hwnd, GWL_EXSTYLE), IsWindowVisible(hwnd)) };
                Some(Seen {
                    visible: visible.as_bool(),
                    tool_window: style & WS_EX_TOOLWINDOW.0 as isize != 0,
                    app_window: style & WS_EX_APPWINDOW.0 as isize != 0,
                })
            })
            .ok()
            .flatten()
    }

    /// The window's state once it is `visible` (or as it was when the
    /// patience ran out).
    async fn once(window: AnyWindowHandle, visible: bool, cx: &mut AsyncApp) -> Option<Seen> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let state = seen(window, cx);
            if state.is_some_and(|state| state.visible == visible) || Instant::now() > deadline {
                return state;
            }
            cx.background_executor()
                .timer(Duration::from_millis(20))
                .await;
        }
    }

    #[test]
    fn the_launcher_stays_off_the_taskbar_and_settings_keeps_its_button() {
        if !opted_in() {
            eprintln!("skipped: set PANE_TEST_REAL_WINDOWS=1 to open real windows");
            return;
        }
        let report: Rc<RefCell<Vec<Step>>> = Rc::default();
        let record = report.clone();
        gpui_platform::application().run(move |cx: &mut App| {
            let launcher = Launcher::new(Runtime::start(), Vec::new());
            let window = pane::open_launcher_window(cx, |window, cx| {
                cx.new(|cx| LauncherWindow::new(launcher, window, cx))
            })
            .expect("the launcher's window opens");
            cx.spawn(async move |cx| {
                let launcher: AnyWindowHandle = window.into();
                let first = once(launcher, true, cx).await;
                record.borrow_mut().push(("first shown", first));
                window
                    .update(cx, |_, window, cx| {
                        window.dispatch_action(Box::new(DismissLauncher), cx)
                    })
                    .ok();
                let hidden = once(launcher, false, cx).await;
                record.borrow_mut().push(("dismissed", hidden));
                window
                    .update(cx, |launcher, window, cx| {
                        launcher.tray_selected(TrayAction::OpenPane, window, cx)
                    })
                    .ok();
                let again = once(launcher, true, cx).await;
                record.borrow_mut().push(("summoned again", again));
                window
                    .update(cx, |launcher, window, cx| {
                        launcher.tray_selected(TrayAction::Settings, window, cx)
                    })
                    .ok();
                let settings = cx.update(|cx| {
                    cx.windows()
                        .into_iter()
                        .find(|window| window.downcast::<SettingsWindow>().is_some())
                });
                let settings = match settings {
                    Some(settings) => once(settings, true, cx).await,
                    None => None,
                };
                record.borrow_mut().push(("settings", settings));
                // The windows close before Pane quits, as quitting does
                // through the launcher's own window, so nothing they hold
                // is left when the application ends.
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
        let state = |step: &str| {
            report
                .iter()
                .find(|(name, _)| *name == step)
                .and_then(|(_, state)| *state)
                .unwrap_or_else(|| panic!("no window state was read {step}: {report:?}"))
        };
        let off_taskbar = |visible| Seen {
            visible,
            tool_window: true,
            app_window: false,
        };
        assert_eq!(state("first shown"), off_taskbar(true), "{report:?}");
        assert_eq!(state("dismissed"), off_taskbar(false), "{report:?}");
        assert_eq!(state("summoned again"), off_taskbar(true), "{report:?}");
        assert_eq!(
            state("settings"),
            Seen {
                visible: true,
                tool_window: false,
                app_window: true,
            },
            "{report:?}"
        );
    }
}
