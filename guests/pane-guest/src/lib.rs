//! Guest-side bindings for Pane's `pane:extension` contract.
//!
//! An extension implements [`Guest`] and calls [`export!`]. It may keep
//! values between runs with [`settings`], compute results from root
//! search's query with [`root`], call operations other packages publish with
//! [`operations::call`] and serve those its own package publishes with
//! [`publish`]. The crate is
//! `no_std` so the component imports only WASI 0.3 interfaces; it supplies the
//! allocator and a panic handler that traps, which the host reports as a
//! runtime error.
//!
//! Without `std` no libc is linked, so the crate also supplies what the
//! compiler and the component runtime call into libc or `std` for:
//! `memcmp` and `bcmp`, which the compiler emits for byte and string
//! comparisons (`==` on `str`, `starts_with`, ...) as soon as a guest compares
//! strings, and the canonical-ABI `cabi_realloc`.
#![no_std]

pub extern crate alloc;

use core::ffi::c_void;

wit_bindgen::generate!({
    path: "../../wit",
    world: "extension-with-settings",
    pub_export_macro: true,
    default_bindings_module: "pane_guest",
});

pub use exports::pane::extension::command::{
    Choice, CustomView, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form,
    FormError, Frame, Guest, GuestCustomView, Item, Key, Platform, Point, Rect, Shape, Text,
    TextField, View, ViewEvent,
};
pub use pane::extension::{operations, settings};

impl operations::CallErrorKind {
    /// The kind's WIT name, such as `not-found`, as JavaScript sees it too.
    pub fn name(&self) -> &'static str {
        use operations::CallErrorKind::*;
        match self {
            NotFound => "not-found",
            Disabled => "disabled",
            Incompatible => "incompatible",
            Unavailable => "unavailable",
            Failed => "failed",
            Crashed => "crashed",
            Refused => "refused",
        }
    }
}

impl operations::CallError {
    /// `<kind>: <message>`, such as "failed: a name is needed", to show
    /// people.
    pub fn explain(&self) -> alloc::string::String {
        alloc::format!("{}: {}", self.kind.name(), self.message)
    }
}

/// Serving the operations a package publishes
/// (`pane:extension/published-operations`). The component its `pane.json`
/// names under `operations` implements [`publish::Guest`] too and calls
/// [`publish::export!`](crate::publish::export) beside [`export!`]:
///
/// ```ignore
/// pane_guest::export!(Greeter);
/// pane_guest::publish::export!(Greeter);
/// ```
pub mod publish {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "operations-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::publish",
    });

    pub use exports::pane::extension::published_operations::Guest;
}

/// Results a command computes from root search's query
/// (`pane:extension/root-results`), such as a calculator's answer. A command
/// whose `pane.json` entry sets `"rootResults": true` implements
/// [`root::Guest`] too and calls [`root::export!`](crate::root::export)
/// beside [`export!`]:
///
/// ```ignore
/// pane_guest::export!(Calculator);
/// pane_guest::root::export!(Calculator);
/// ```
pub mod root {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "root-results-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::root",
    });

    pub use exports::pane::extension::root_results::{Guest, RootAction, RootResult};
}

/// The custom view type of a command that has none: `type CustomView =
/// NoCustomView;` in its `Guest` implementation, with an `open_view` that
/// returns `Err`. It has no values, so no view of it can be opened.
pub enum NoCustomView {}

impl GuestCustomView for NoCustomView {
    async fn render(&self) -> Frame {
        match *self {}
    }

    async fn handle_event(&self, _event: ViewEvent) -> Result<(), alloc::string::String> {
        match *self {}
    }
}

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

/// Byte comparison, normally supplied by libc. The compiler emits calls to it
/// for slice and string comparisons (`==` on `str`, `starts_with`, ...).
///
/// # Safety
/// `a` and `b` must be valid for reads of `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    for i in 0..n {
        // Byte by byte through volatile reads, so that the compiler cannot
        // turn this loop back into a call to memcmp.
        let (x, y) = unsafe { (a.add(i).read_volatile(), b.add(i).read_volatile()) };
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}

/// Equality-only form of [`memcmp`], which the compiler may call instead.
///
/// # Safety
/// `a` and `b` must be valid for reads of `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
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
