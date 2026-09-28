//! Test fixture: a guest built against an older shape of the same extension
//! API version, `wit/extension.wit` here: from before #19 and #21, its `item`
//! has no `platforms` and it exports no custom views.
//! Pane's type check must refuse it before any of its code runs.
//!
//! It cannot use `pane-guest`, which binds the current contract, so it
//! supplies the allocator, panic handler and `cabi_realloc` itself.
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

wit_bindgen::generate!({ path: "wit", world: "extension" });

use exports::pane::extension::command::{FieldValue, FormError, Guest, Item, View};

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

struct OldApi;
export!(OldApi);

impl Guest for OldApi {
    async fn get_view() -> Result<View, String> {
        Ok(View {
            title: "Old".into(),
            items: vec![Item {
                id: "x".into(),
                title: "x".into(),
                subtitle: None,
                form: None,
            }],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        Ok(item_id)
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Ok(item_id)
    }
}
