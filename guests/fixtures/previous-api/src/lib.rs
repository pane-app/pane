//! Test fixture: a guest built against the previous version of the
//! extension API, `wit/previous` here — the 0.1 contract this Pane still
//! runs through the host's adapter while the window lasts (ADR 0046), so
//! a component built for it is served and called through it, its
//! behaviour indistinguishable from the current world's path.
//!
//! Its command answers a list whose one item's action calls `show-hud` —
//! the one function whose style the 0.2 breaking change retyped: here it
//! still takes a `toast-style`, the shape the adapter converts to the
//! host's drawing. Everything else it does is a shape the two worlds
//! share, as most of the contract is.
//!
//! It cannot use `pane-extension`, which binds the current contract, so
//! it supplies the allocator, panic handler and `cabi_realloc` itself.
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

wit_bindgen::generate!({ path: "../../../wit/previous", world: "extension" });

use exports::pane::extension::command::{
    CustomView, FieldValue, FormError, Frame, Guest, GuestCustomView, LaunchRecord, ViewEvent,
};
use pane::extension::feedback::{ToastStyle, show_hud};

/// The one item's action's callback id.
const HUD: &str = "cb-hud";

struct PreviousApi;
export!(PreviousApi);

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

impl Guest for PreviousApi {
    type CustomView = NoView;

    async fn render(_launch: LaunchRecord) -> Result<String, String> {
        // A tree of the current shape, as the 0.1 contract's was: the
        // envelope `render` and `handle-event` answer (ADR 0036).
        Ok(
            "{\"version\":1,\"view\":{\"type\":\"list\",\"title\":\"Previous API\",\
             \"items\":[{\"id\":\"hud\",\"title\":\"Show a HUD\",\
             \"actions\":[{\"title\":\"Run\",\"onAction\":\"cb-hud\"}]}]}}"
                .into(),
        )
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<String, String> {
        Err(format!("`{command}` opens a screen"))
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        match callback.as_str() {
            HUD => show_hud("Hello from the previous API", ToastStyle::Success),
            other => return Err(format!("unknown callback: {other}")),
        }
        // Pane shows nothing of an answer: the HUD is what the user sees.
        Ok("{}".into())
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
