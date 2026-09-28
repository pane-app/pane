//! Guest-side bindings for Pane's `pane:extension` contract.
//!
//! An extension implements [`Guest`] and calls [`export!`]. The crate is
//! `no_std` so the component imports only WASI 0.3 interfaces; it supplies the
//! allocator and a panic handler that traps, which the host reports as a
//! runtime error.
#![no_std]

pub extern crate alloc;

wit_bindgen::generate!({
    path: "../../wit",
    world: "extension",
    pub_export_macro: true,
    default_bindings_module: "pane_guest",
});

pub use exports::pane::extension::command::{Guest, Item, View};

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

/// Canonical-ABI allocation entry point, normally supplied by `std`.
///
/// # Safety
/// Called only by the component runtime with valid allocation metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cabi_realloc(
    old: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, realloc};
    if new_size == 0 {
        return align as *mut u8;
    }
    let result = if old_size == 0 {
        unsafe { alloc(Layout::from_size_align_unchecked(new_size, align)) }
    } else {
        unsafe {
            realloc(
                old,
                Layout::from_size_align_unchecked(old_size, align),
                new_size,
            )
        }
    };
    if result.is_null() {
        core::arch::wasm32::unreachable();
    }
    result
}
