//! Test fixture: a guest built against `wit/extension.wit` here, Pane's
//! contract with `form-error` lacking its `field` field. Every export has
//! the name Pane looks for; only a type differs. Pane's type check must
//! refuse it before any of its code runs.
//!
//! It cannot use `pane-extension`, which binds the current contract, so it
//! supplies the allocator, panic handler and `cabi_realloc` itself.
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

wit_bindgen::generate!({ path: "wit", world: "extension" });

use exports::pane::extension::command::{
    FieldValue, FormError, Guest, GuestView, LaunchRecord, Outcome, Rendered, UiEvent, View,
};

struct Mismatched;
export!(Mismatched);

/// A designed view type that is never opened.
enum NoView {}

impl GuestView for NoView {
    async fn render(&self, _context: String) -> Result<Rendered, String> {
        match *self {}
    }

    async fn handle_event(&self, _event: UiEvent) -> Result<Outcome, String> {
        match *self {}
    }
}

impl Guest for Mismatched {
    type View = NoView;

    async fn render(_launch: LaunchRecord) -> Result<String, String> {
        Ok(
            "{\"version\":1,\"view\":{\"type\":\"list\",\"title\":\"Mismatched\",\"items\":[]}}"
                .into(),
        )
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<String, String> {
        Err(command)
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        Err(callback)
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Ok(item_id)
    }

    async fn open_view(_command: String, _launch: LaunchRecord) -> Result<View, String> {
        Err("no designed view".into())
    }
}

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
