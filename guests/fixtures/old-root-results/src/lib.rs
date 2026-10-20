//! Test fixture: a root provider built against an older shape of the
//! previous extension API, `wit/` here — a copy of `wit/previous` whose
//! root results' `results-for` takes the query alone, without the moment
//! it was typed, as the default extensions released before 0.1 was frozen
//! did (ADR 0046). Its manifest names 0.1, so Pane routes it to the
//! previous world; the shape check there must refuse it when it is
//! installed, rather than Pane failing when root search asks it.
//!
//! It cannot use `pane-extension`, which binds the current contract, so
//! it supplies the allocator, panic handler and `cabi_realloc` itself.
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

wit_bindgen::generate!({ path: "wit", world: "old-root-results" });

use exports::pane::extension::command::{
    CustomView, FieldValue, FormError, Frame, Guest, GuestCustomView, LaunchRecord, ViewEvent,
};
use exports::pane::extension::root_results::{self, RootResult};

struct OldRootResults;
export!(OldRootResults);

/// A view type that is never opened.
enum NoView {}

impl GuestCustomView for NoView {
    async fn render(&self) -> Frame {
        match *self {}
    }

    async fn handle_event(&self, _event: ViewEvent) -> Result<(), String> {
        match *self {}
    }
}

/// A root provider: Pane never opens or runs it.
impl Guest for OldRootResults {
    type CustomView = NoView;

    async fn render(_launch: LaunchRecord) -> Result<String, String> {
        Err("a root provider has no screen".into())
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<String, String> {
        Err(format!("`{command}` is a root provider"))
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        Err(format!("unknown callback: {callback}"))
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        })
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

impl root_results::Guest for OldRootResults {
    async fn results_for(_query: String) -> Result<Vec<RootResult>, String> {
        Ok(Vec::new())
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
