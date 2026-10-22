//! Small helpers the core's modules share, each defined once.

use std::sync::{Mutex, MutexGuard};

/// Locks `mutex`, taking it over if a thread panicked while holding it:
/// the runtime thread may crash while a lock is held (see `supervisor`),
/// a walker or an icon reader may panic on a malformed file, and Pane
/// carries on with what the lock guarded.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `text` (a path, a name, any string) as the NUL-terminated UTF-16
/// string Windows' wide (`…W`) functions take.
#[cfg(windows)]
pub(crate) fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.as_ref().encode_wide().chain([0]).collect()
}

/// Now, in milliseconds since the Unix epoch, as clipboard history and
/// the selected-text read count time; 0 if the system's clock is before
/// the epoch. The pure halves of those take the number as a parameter,
/// so the tests give one of their own. Reaching it from them is Windows'
/// work, which is why it is allowed to sit unused elsewhere.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}
