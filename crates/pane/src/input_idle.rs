//! How long the user's keyboard and pointer have been still, system-wide:
//! what an attention launch waits on before the launcher is shown over what
//! the user is typing (ADR 0053, see `pane_core`'s `attention`).
//!
//! - Windows: `GetLastInputInfo`, the tick of the last input event any
//!   application received, against the tick count now.
//! - Elsewhere Pane cannot tell yet (#559): `None`, which holds nothing
//!   back.

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
#[cfg(not(target_os = "windows"))]
pub(crate) fn input_idle() -> Option<Duration> {
    None
}
