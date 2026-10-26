//! How long the user's keyboard and pointer have been still, system-wide:
//! what an attention launch waits on before the launcher is shown over what
//! the user is typing (ADR 0053, see `pane_core`'s `attention`).
//!
//! - Windows: `GetLastInputInfo`, the tick of the last input event any
//!   application received, against the tick count now.
//! - macOS: Core Graphics' `CGEventSourceSecondsSinceLastEventType` for any
//!   input event of the login session (#559), which needs no permission.
//! - Linux on X11: the MIT-SCREEN-SAVER extension's idle time, the time
//!   since the X server last saw the user's input (#559), on one
//!   connection kept for the purpose.
//! - Elsewhere (Wayland, which offers no such measure to an ordinary
//!   client, or an X server without the extension) Pane cannot tell:
//!   `None`, which holds nothing back.

use std::time::Duration;

/// How long the keyboard and the pointer have been still; `None` where the
/// system cannot tell.
#[cfg(target_os = "windows")]
pub(crate) fn input_idle() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut last = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `last` is a valid, writable LASTINPUTINFO whose `cbSize`
    // says its size, as the call requires; it only writes `dwTime`.
    if !unsafe { GetLastInputInfo(&mut last) }.as_bool() {
        return None;
    }
    // SAFETY: GetTickCount takes nothing and cannot fail.
    let now = unsafe { GetTickCount() };
    // Both are the same 32-bit tick count, which wraps after 49.7 days.
    Some(Duration::from_millis(u64::from(
        now.wrapping_sub(last.dwTime),
    )))
}

/// How long the keyboard and the pointer have been still; `None` where the
/// system cannot tell.
#[cfg(target_os = "macos")]
pub(crate) fn input_idle() -> Option<Duration> {
    /// `kCGEventSourceStateCombinedSessionState`: every input source of
    /// the login session.
    const COMBINED_SESSION: i32 = 0;
    /// `kCGAnyInputEventType`.
    const ANY_INPUT: u32 = !0;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }

    // SAFETY: a plain query of two constants, answering seconds.
    let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION, ANY_INPUT) };
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// How long the keyboard and the pointer have been still; `None` where the
/// system cannot tell.
#[cfg(target_os = "linux")]
pub(crate) fn input_idle() -> Option<Duration> {
    use std::sync::OnceLock;

    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::screensaver;
    use x11rb::rust_connection::RustConnection;

    /// The connection the idle time is asked on, and the screen's root
    /// window; `None` on Wayland, with no X server, or with no
    /// MIT-SCREEN-SAVER extension.
    static X11: OnceLock<Option<(RustConnection, u32)>> = OnceLock::new();
    let (connection, root) = X11
        .get_or_init(|| {
            // Under Wayland an X server is XWayland, which sees only its
            // own clients' input: its idle time would mislead.
            if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty()) {
                return None;
            }
            let (connection, screen) = x11rb::connect(None).ok()?;
            connection
                .extension_information(screensaver::X11_EXTENSION_NAME)
                .ok()??;
            let root = connection.setup().roots.get(screen)?.root;
            Some((connection, root))
        })
        .as_ref()?;
    let info = screensaver::query_info(connection, *root)
        .ok()?
        .reply()
        .ok()?;
    Some(Duration::from_millis(u64::from(info.ms_since_user_input)))
}

/// How long the keyboard and the pointer have been still; `None` where the
/// system cannot tell.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub(crate) fn input_idle() -> Option<Duration> {
    None
}
