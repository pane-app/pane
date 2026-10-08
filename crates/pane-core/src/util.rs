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
