//! The free-space probe behind the low-disk safety valve (#126 "Priority
//! and safety valves", #176): how much room the volume holding Pane's cache
//! has, read from the system, and the floor under which indexing stops
//! writing.

use std::path::Path;
use std::sync::Arc;

/// The free space under which indexing stops writing (#126's proposed
/// value of 1 GiB, written in the decimal units Pane shows sizes in, so the
/// File search page says "1 GB"): on the volume holding Pane's cache.
pub const FREE_SPACE_FLOOR: u64 = 1_000_000_000;

/// Reads the free space of the volume holding a folder (see
/// [`super::Valves::free_space`]).
pub type FreeSpace = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

/// The space free to this user on the volume holding `path` (or its
/// nearest existing folder above it); `None` when the system does not say.
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|folder| folder.exists())?;
    free_space_at(existing)
}

#[cfg(windows)]
fn free_space_at(path: &Path) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;
    let wide = crate::util::wide(path);
    let mut available = 0u64;
    // SAFETY: `wide` is NUL-terminated and `available` is a u64 the call
    // writes.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }.ok()?;
    Some(available)
}

#[cfg(unix)]
fn free_space_at(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: a plain C structure, for which all zeroes is a valid value.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and a structure of the call's own type.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    // The fields' widths differ between systems.
    #[allow(clippy::unnecessary_cast)]
    Some((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

#[cfg(not(any(windows, unix)))]
fn free_space_at(_path: &Path) -> Option<u64> {
    None
}
