//! For tests: the most memory a thread holds while it runs something, so
//! that a test can check that reading hostile input takes no more than Pane
//! counts for it. Every allocation of the tests goes through
//! [`Counting`], which counts it for the thread that makes it, so tests
//! running at once do not count each other's memory.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// Bytes this thread allocated and did not free; below zero when it
    /// frees what another thread allocated.
    static HELD: Cell<isize> = const { Cell::new(0) };
    /// The most [`HELD`] has been since [`peak_while`] started.
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn count(change: isize) {
    // `try_with`: nothing is counted while the thread's own storage is
    // being torn down.
    let _ = HELD.try_with(|held| {
        let now = held.get().saturating_add(change);
        held.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
}

/// The system's allocator, counting what each thread holds.
struct Counting;

// SAFETY: each call is passed on to the system allocator as it is; besides,
// only this thread's counters change, which neither allocates nor frees.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's guarantees are the system allocator's.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            count(layout.size() as isize);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            count(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as for `alloc`.
        unsafe { System.dealloc(ptr, layout) };
        count(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            // Both are held while the contents move.
            count(new_size as isize);
            count(-(layout.size() as isize));
        }
        new
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Runs `f`, and returns what it returned and the most memory this thread
/// held at once while it ran, beyond what it held before.
pub(crate) fn peak_while<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = HELD.with(Cell::get);
    PEAK.with(|peak| peak.set(before));
    let result = f();
    let peak = PEAK.with(Cell::get) - before;
    (result, peak.max(0) as usize)
}
