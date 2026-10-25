//! A double click on the Settings window's titlebar maximizes it, and a
//! second restores it, as Windows does for its own titlebars and Raycast's
//! Settings window does (#541); a drag by it still moves the window. Read
//! back from a real Windows window.
//!
//! Two tests. The first works through the window's messages alone: the
//! titlebar answers the system's hit test with the caption (`HTCAPTION`,
//! so dragging, Aero Snap and the system menu are the system's own), a
//! press on it does not begin the system's move (which would take the
//! rest of a double click for itself), and the caption double click the
//! system sends maximizes and restores the window. It injects no input
//! (the messages go to the test's own window), so it runs where
//! `PANE_TEST_REAL_WINDOWS=1` is set. The second double-clicks the
//! titlebar with the real pointer, with the window's thread answering at
//! once and with all of the double click queued before it answers, and
//! then drags the window by it: it needs a session with real
//! input, so it runs only where `PANE_TEST_REAL_INPUT=1` is set too (CI's
//! Windows runner sets both), never on a desktop someone is using.
//! Without the variables the tests pass without looking. Both close what
//! they opened.
//!
//! The macOS double click follows the system's "Double-click a window's
//! title bar to" setting; no test covers it (see
//! `docs/launcher-appearance.md`).

#[cfg(target_os = "windows")]
mod windows {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use gpui::{AnyWindowHandle, App, AsyncApp, prelude::*};
    use pane::SettingsWindow;
    use pane_core::tray::TrayAction;
    use pane_core::{Launcher, Runtime};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use ::windows::Win32::Graphics::Gdi::ClientToScreen;
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEINPUT, SendInput,
    };
    use ::windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, HTCAPTION, IsWindowVisible, IsZoomed, SC_MOVE, SendMessageW, SetCursorPos,
        SetForegroundWindow, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_SYSCOMMAND,
    };

    /// How long a window is given to show: GPUI shows a Windows window on
    /// its next turn of the event loop.
    const PATIENCE: Duration = Duration::from_secs(10);

    fn opted_in(variable: &str) -> bool {
        std::env::var(variable).is_ok_and(|value| value == "1")
    }

    /// The window's native handle and its scale factor.
    fn native(window: AnyWindowHandle, cx: &mut AsyncApp) -> Option<(HWND, f32)> {
        window
            .update(cx, |_, window, _| {
                // `Window` also has an inherent `window_handle`, so the
                // raw-window-handle trait is named.
                let handle = HasWindowHandle::window_handle(window).ok()?;
                let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                    return None;
                };
                Some((HWND(handle.hwnd.get() as *mut _), window.scale_factor()))
            })
            .ok()
            .flatten()
    }

    /// The window's handle once it is visible.
    async fn shown(window: AnyWindowHandle, cx: &mut AsyncApp) -> Option<(HWND, f32)> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let found = native(window, cx);
            // SAFETY: `hwnd` is this live window's; the call only reads.
            if found.is_some_and(|(hwnd, _)| unsafe { IsWindowVisible(hwnd) }.as_bool())
                || Instant::now() > deadline
            {
                return found;
            }
            pause(cx, 20).await;
        }
    }

    async fn pause(cx: &mut AsyncApp, millis: u64) {
        cx.background_executor()
            .timer(Duration::from_millis(millis))
            .await;
    }

    fn lparam(point: POINT) -> LPARAM {
        LPARAM(((point.y as u16 as isize) << 16) | point.x as u16 as isize)
    }

    /// A point on the titlebar's drag region, in the window's client
    /// coordinates and on the screen: its middle, where the title is,
    /// half its 48 points down.
    fn titlebar_point(hwnd: HWND, scale: f32) -> (POINT, POINT) {
        let client = POINT {
            x: (430. * scale) as i32,
            y: (24. * scale) as i32,
        };
        let mut screen = client;
        // SAFETY: `hwnd` is this live window's; the call only reads.
        let _ = unsafe { ClientToScreen(hwnd, &mut screen) };
        (client, screen)
    }

    fn zoomed(hwnd: HWND) -> bool {
        // SAFETY: `hwnd` is this live window's; the call only reads.
        unsafe { IsZoomed(hwnd) }.as_bool()
    }

    /// Opens the launcher and the Settings window from the tray, runs
    /// `test` on the Settings window's handle and scale factor, then
    /// closes both and quits.
    fn with_settings<F>(test: F)
    where
        F: AsyncFnOnce(HWND, f32, &mut AsyncApp) + 'static,
    {
        gpui_platform::application().run(move |cx: &mut App| {
            let launcher = Launcher::new(Runtime::start(), Vec::new());
            let window = pane::open_launcher_window(cx, |window, cx| {
                cx.new(|cx| pane::LauncherWindow::new(launcher, window, cx))
            })
            .expect("the launcher's window opens");
            cx.spawn(async move |cx| {
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
                if let Some((hwnd, scale)) = match settings {
                    Some(settings) => shown(settings, cx).await,
                    None => None,
                } {
                    test(hwnd, scale, cx).await;
                }
                for window in cx.update(|cx| cx.windows()) {
                    window
                        .update(cx, |_, window, _| window.remove_window())
                        .ok();
                }
                pause(cx, 200).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    }

    thread_local! {
        /// The mouse button and system command messages the Settings
        /// window received, for the failure messages.
        static RECEIVED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn received() -> String {
        RECEIVED.with_borrow(|received| received.join("\n"))
    }

    /// The subclass that records [`RECEIVED`], set and removed by the
    /// tests on their window.
    const RECORD: usize = 0x7469_746c;

    /// Records the window's button and system command messages. With
    /// `data` set it also keeps the system's move from beginning, which a
    /// message-made press would leave waiting on a button that is not
    /// held.
    unsafe extern "system" fn record_messages(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        let name = match message {
            WM_NCLBUTTONDOWN => "WM_NCLBUTTONDOWN",
            WM_NCLBUTTONDBLCLK => "WM_NCLBUTTONDBLCLK",
            WM_NCLBUTTONUP => "WM_NCLBUTTONUP",
            WM_LBUTTONDOWN => "WM_LBUTTONDOWN",
            WM_LBUTTONDBLCLK => "WM_LBUTTONDBLCLK",
            WM_LBUTTONUP => "WM_LBUTTONUP",
            WM_SYSCOMMAND => "WM_SYSCOMMAND",
            _ => "",
        };
        if !name.is_empty() {
            RECEIVED.with_borrow_mut(|received| received.push(format!("{name} {:#x}", wparam.0)));
        }
        if data != 0 && message == WM_SYSCOMMAND && wparam.0 & 0xFFF0 == SC_MOVE as usize {
            return LRESULT(0);
        }
        // SAFETY: the arguments are those this procedure was called with,
        // passed on to the next procedure in the window's chain.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    /// Records the window's messages (see [`record_messages`]).
    fn record(hwnd: HWND, keep_moves: bool) {
        // SAFETY: `hwnd` is this live window's, created on this thread,
        // which is the one a subclass must be set from; the procedure is
        // a plain function that lives as long as the program.
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(record_messages), RECORD, keep_moves as usize);
        }
    }

    fn stop_recording(hwnd: HWND) {
        // SAFETY: as in `record`.
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(record_messages), RECORD);
        }
    }

    /// What the message test saw.
    #[derive(Debug, Default)]
    struct Messages {
        hit_test: Option<isize>,
        maximized: Option<bool>,
        restored: Option<bool>,
    }

    #[test]
    fn the_settings_titlebar_is_the_caption_and_its_double_click_maximizes() {
        if !opted_in("PANE_TEST_REAL_WINDOWS") {
            eprintln!("skipped: set PANE_TEST_REAL_WINDOWS=1 to open real windows");
            return;
        }
        let seen: Rc<RefCell<Messages>> = Rc::default();
        let into = seen.clone();
        with_settings(async move |hwnd, scale, cx| {
            record(hwnd, true);
            let send = |message: u32, wparam: usize, point: POINT| {
                // SAFETY: `hwnd` is this live window's, on this thread,
                // and nothing borrows the app while the message runs.
                unsafe { SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(lparam(point))) }
            };
            // A double click as the system delivers it on the caption: a
            // press, its release, then the second press marked as the
            // double click, and its release. First the pointer is over
            // the titlebar as far as GPUI knows, as it is when a real
            // pointer reaches it: the hit test answers from GPUI's last
            // pointer position.
            let caption = HTCAPTION as usize;
            let double_click = || {
                let (client, screen) = titlebar_point(hwnd, scale);
                send(WM_MOUSEMOVE, 0, client);
                let hit_test = send(WM_NCHITTEST, 0, screen).0;
                send(WM_NCLBUTTONDOWN, caption, screen);
                send(WM_NCLBUTTONUP, caption, screen);
                send(WM_NCLBUTTONDBLCLK, caption, screen);
                send(WM_NCLBUTTONUP, caption, screen);
                hit_test
            };
            into.borrow_mut().hit_test = Some(double_click());
            into.borrow_mut().maximized = Some(zoomed(hwnd));
            pause(cx, 200).await;
            double_click();
            into.borrow_mut().restored = Some(!zoomed(hwnd));
            // The presses' tasks see no button held and end.
            pause(cx, 100).await;
            stop_recording(hwnd);
        });

        let received = received();
        let seen = seen.borrow();
        assert_eq!(
            seen.hit_test,
            Some(HTCAPTION as isize),
            "the titlebar is the window's caption: {seen:?}"
        );
        assert!(
            !received.contains(&format!("WM_SYSCOMMAND {:#x}", SC_MOVE | HTCAPTION)),
            "a press alone began no move of the window:\n{received}"
        );
        assert_eq!(
            seen.maximized,
            Some(true),
            "maximized: {seen:?}\n{received}"
        );
        assert_eq!(seen.restored, Some(true), "restored: {seen:?}\n{received}");
    }

    /// One step of the real pointer: the button flags to send (none for
    /// a move alone), where to move first, and how long to wait after.
    type Step = (MOUSE_EVENT_FLAGS, Option<POINT>, u64);

    /// The real pointer: to `at`, then each of `steps`.
    fn pointer(at: POINT, steps: &[Step]) {
        // SAFETY: plain input calls on this session's desktop.
        unsafe {
            let _ = SetCursorPos(at.x, at.y);
        }
        std::thread::sleep(Duration::from_millis(150));
        for &(flags, to, wait) in steps {
            if let Some(to) = to {
                // SAFETY: as above.
                let _ = unsafe { SetCursorPos(to.x, to.y) };
            }
            if flags.0 != 0 {
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dwFlags: flags,
                            ..Default::default()
                        },
                    },
                };
                // SAFETY: one well-formed input of the declared size.
                unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
            }
            std::thread::sleep(Duration::from_millis(wait));
        }
    }

    /// Runs `steps` of the real pointer from a thread of its own while
    /// this thread's event loop keeps answering the window, then waits
    /// for things to settle. With `busy`, this thread first stays away
    /// from its messages until all the input is queued, as a long frame
    /// would keep it.
    async fn real(at: POINT, steps: Vec<Step>, busy: bool, cx: &mut AsyncApp) {
        let thread = std::thread::spawn(move || pointer(at, &steps));
        if busy {
            while !thread.is_finished() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        while !thread.is_finished() {
            pause(cx, 20).await;
        }
        pause(cx, 600).await;
    }

    /// What the real-input test saw: whether the window was maximized
    /// after each double click, and how far the drag moved it.
    #[derive(Debug, Default)]
    struct Pointer {
        maximized: Vec<bool>,
        moved_by: Option<(i32, i32)>,
    }

    #[test]
    fn a_real_double_click_on_the_settings_titlebar_maximizes_and_restores_it() {
        if !opted_in("PANE_TEST_REAL_INPUT") || !opted_in("PANE_TEST_REAL_WINDOWS") {
            eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 and PANE_TEST_REAL_WINDOWS=1");
            return;
        }
        let seen: Rc<RefCell<Pointer>> = Rc::default();
        let into = seen.clone();
        with_settings(async move |hwnd, scale, cx| {
            record(hwnd, false);
            // SAFETY: `hwnd` is this live window's; the window this test
            // opened comes to the front.
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
            pause(cx, 300).await;
            // Two clicks 60 ms apart, well inside the double-click time.
            let (down, up) = (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP);
            let double_click = vec![
                (down, None, 30),
                (up, None, 60),
                (down, None, 30),
                (up, None, 0),
            ];
            // Each double click toggles the window, whether this thread
            // answers at once or only once all of it is queued.
            for busy in [false, false, true, true] {
                RECEIVED.with_borrow_mut(|received| {
                    received.push(format!("-- double click, busy: {busy}"))
                });
                let (_, at) = titlebar_point(hwnd, scale);
                real(at, double_click.clone(), busy, cx).await;
                into.borrow_mut().maximized.push(zoomed(hwnd));
            }

            // Dragging by the titlebar still moves the window.
            RECEIVED.with_borrow_mut(|received| received.push("-- drag".into()));
            let mut before = RECT::default();
            // SAFETY: `hwnd` is this live window's; the call only reads.
            let _ = unsafe { GetWindowRect(hwnd, &mut before) };
            let (_, at) = titlebar_point(hwnd, scale);
            let to = |dx: i32, dy: i32| {
                Some(POINT {
                    x: at.x + dx,
                    y: at.y + dy,
                })
            };
            let none = MOUSE_EVENT_FLAGS(0);
            let drag = vec![
                (down, None, 100),
                (none, to(10, 5), 50),
                (none, to(40, 20), 50),
                (none, to(80, 40), 100),
                (up, None, 0),
            ];
            real(at, drag, false, cx).await;
            let mut after = RECT::default();
            // SAFETY: as above.
            let _ = unsafe { GetWindowRect(hwnd, &mut after) };
            into.borrow_mut().moved_by = Some((after.left - before.left, after.top - before.top));
            stop_recording(hwnd);
        });

        let received = received();
        let seen = seen.borrow();
        assert_eq!(
            seen.maximized,
            [true, false, true, false],
            "each double click maximized or restored the window: {seen:?}\n{received}"
        );
        assert_eq!(
            seen.moved_by,
            Some((80, 40)),
            "the drag moved the window with the pointer: {seen:?}\n{received}"
        );
    }
}
