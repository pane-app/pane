//! The launcher window has no taskbar button and no entry in the system's
//! window switcher, as Raycast's has none (#458): Pane is reached by its
//! hotkey and its tray or menu-bar entry, and the launcher is never on the
//! taskbar — there is no setting. The Settings window is an ordinary
//! application window and keeps its button; a HUD is a GPUI pop-up, which
//! no system lists anyway.
//!
//! - Windows: the launcher is a tool window (`WS_EX_TOOLWINDOW`, without
//!   `WS_EX_APPWINDOW`), which the taskbar and Alt+Tab leave out. Windows
//!   decides whether a window gets a taskbar button as it is shown, so
//!   [`crate::open_launcher_window`] opens it hidden, makes it a tool
//!   window, and only then shows it. GPUI changes no window's extended
//!   style after creating it, so the style holds across every hide and
//!   show. (Raycast's window is also owned by a hidden window; the style
//!   alone is what keeps a window off both lists.)
//! - macOS: Pane is an accessory application (`main.rs` sets the
//!   activation policy before it finishes launching), so it has no Dock
//!   icon and no Cmd+Tab entry at all; nothing is done per window.
//! - Linux on X11: the window asks the window manager to skip the taskbar
//!   and the pager (EWMH's `_NET_WM_STATE_SKIP_TASKBAR` and
//!   `_NET_WM_STATE_SKIP_PAGER`). A window manager may forget the state
//!   when a window is withdrawn, so it is asked again before every show
//!   ([`keep_off_taskbar`]). A window manager without EWMH ignores it.
//! - Wayland has no protocol for it: whether the launcher is listed is up
//!   to the compositor.

use gpui::Window;

/// Keeps `window`, the launcher's, off the taskbar and out of the window
/// switcher where the system allows (see the module's notes). The
/// launcher calls it as its window opens and before every show; it is
/// cheap and changes nothing the second time. On GPUI's test platform,
/// which has no native window, it does nothing.
#[cfg(target_os = "windows")]
pub(crate) fn keep_off_taskbar(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    // `Window` also has an inherent `window_handle` (GPUI's own identifier),
    // so the raw-window-handle trait is named rather than called through
    // the receiver.
    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(_) => return,
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    // SAFETY: `hwnd` is this window, which GPUI created before the handle
    // was read; only its extended style changes.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let kept = (style | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        if kept != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, kept);
        }
    }
}

/// Keeps `window`, the launcher's, off the taskbar and the pager on X11
/// (see the Windows version): the state is written on the window itself
/// while it is unmapped, which the window manager reads as it maps it,
/// and asked of the window manager while it is mapped (or about to be),
/// as EWMH says. Wayland's windows, and GPUI's test platform's, are left
/// as they are.
#[cfg(target_os = "linux")]
pub(crate) fn keep_off_taskbar(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(_) => return,
    };
    let RawWindowHandle::Xcb(handle) = handle.as_raw() else {
        return;
    };
    if let Err(error) = x11::skip_taskbar(handle.window.get()) {
        pane_core::diagnostic!("Pane could not keep the launcher off the taskbar: {error}");
    }
}

/// Elsewhere the application's own policy keeps it out of the Dock and
/// the switcher (macOS), or nothing is available.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(crate) fn keep_off_taskbar(_window: &Window) {}

#[cfg(target_os = "linux")]
mod x11 {
    use std::sync::OnceLock;

    use x11rb::connection::Connection;
    use x11rb::errors::ReplyError;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, MapState, PropMode,
        Window as XWindow,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    /// EWMH's `_NET_WM_STATE_ADD`, the client message's first datum.
    const ADD: u32 = 1;
    /// EWMH's source indication for a normal application.
    const FROM_APPLICATION: u32 = 1;

    /// Pane's own connection to the X server for this, opened at the first
    /// show and kept, with the atoms it names: GPUI keeps its connection to
    /// itself. `None` when no X display could be reached.
    struct Session {
        connection: RustConnection,
        root: XWindow,
        state: Atom,
        skip_taskbar: Atom,
        skip_pager: Atom,
    }

    static SESSION: OnceLock<Option<Session>> = OnceLock::new();

    fn session() -> Option<&'static Session> {
        SESSION
            .get_or_init(|| match Session::connect() {
                Ok(session) => Some(session),
                Err(error) => {
                    pane_core::diagnostic!(
                        "Pane could not reach the X11 display to keep the launcher off the \
                         taskbar: {error}"
                    );
                    None
                }
            })
            .as_ref()
    }

    impl Session {
        fn connect() -> Result<Session, String> {
            let (connection, screen) =
                RustConnection::connect(None).map_err(|error| error.to_string())?;
            let root = connection.setup().roots[screen].root;
            let atom = |name: &[u8]| -> Result<Atom, String> {
                Ok(connection
                    .intern_atom(false, name)
                    .map_err(|error| error.to_string())?
                    .reply()
                    .map_err(|error| error.to_string())?
                    .atom)
            };
            let state = atom(b"_NET_WM_STATE")?;
            let skip_taskbar = atom(b"_NET_WM_STATE_SKIP_TASKBAR")?;
            let skip_pager = atom(b"_NET_WM_STATE_SKIP_PAGER")?;
            Ok(Session {
                connection,
                root,
                state,
                skip_taskbar,
                skip_pager,
            })
        }
    }

    /// Asks that `window` be left off the taskbar and the pager.
    pub(super) fn skip_taskbar(window: XWindow) -> Result<(), String> {
        let Some(session) = session() else {
            return Ok(());
        };
        ask(session, window).map_err(|error| error.to_string())
    }

    fn ask(session: &Session, window: XWindow) -> Result<(), ReplyError> {
        let connection = &session.connection;
        let wanted = [session.skip_taskbar, session.skip_pager];
        let mapped = connection.get_window_attributes(window)?.reply()?.map_state;
        if mapped == MapState::UNMAPPED {
            // A withdrawn window's state is its own to write, and the window
            // manager reads it as the window is mapped: the states it holds
            // are kept, ours added. The request is checked, a round trip, so
            // the server has it before GPUI's map request on its own
            // connection follows.
            let held: Vec<Atom> = connection
                .get_property(false, window, session.state, AtomEnum::ATOM, 0, 64)?
                .reply()?
                .value32()
                .map(|atoms| atoms.collect())
                .unwrap_or_default();
            let mut states = held.clone();
            states.extend(wanted.iter().filter(|atom| !held.contains(atom)));
            connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    session.state,
                    AtomEnum::ATOM,
                    &states,
                )?
                .check()?;
        }
        // A mapped window's state is the window manager's: it is asked. A
        // window GPUI has just asked to map may not be managed yet, so it
        // is asked either way; a window manager that already read the
        // property changes nothing. Checked too, which also sends it now.
        let message = ClientMessageEvent::new(
            32,
            window,
            session.state,
            [ADD, wanted[0], wanted[1], FROM_APPLICATION, 0],
        );
        connection
            .send_event(
                false,
                session.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            )?
            .check()?;
        Ok(())
    }
}
