//! Test fixture: a command whose tree and answers are JSON written by hand
//! against Pane's current contract (`wit/extension.wit`), not by pane-guest,
//! so that Pane's reading of them is checked on its own (see
//! `crates/pane-core/tests/list_tree.rs`).
//!
//! Its list, titled "Drawn <n> times", names each item's action by a
//! callback id of its own (`cb-<item id>`), not the item's id as the SDKs
//! do, and its trees carry fields Pane does not know at every level.
//! Choosing an item answers "Handled <callback> with <details>", then:
//!
//! - "Reverse the list" lists the items in the other order from then on;
//! - "Remove this item" leaves itself out from then on;
//! - "Draw a newer tree" answers a tree naming version 2 from then on;
//! - "Draw a grid" answers a version 2 tree whose view is a grid, which
//!   Pane cannot show, once, then the list again;
//! - "Draw an unreadable tree" answers a tree that is not JSON once, then
//!   the list again;
//! - "Answer unreadably" answers with something that is not JSON;
//! - "Answer nothing" answers an object with no text to show, only a field
//!   Pane does not know yet.
//!
//! It cannot use `pane-guest`, which writes the tree itself, so it supplies
//! the allocator, panic handler, byte comparisons and `cabi_realloc`.
#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use core::ffi::c_void;

wit_bindgen::generate!({ path: "../../../wit", world: "extension" });

use exports::pane::extension::command::{
    CustomView, FieldValue, FormError, Frame, Guest, GuestCustomView, ViewEvent,
};

/// What the next drawing answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    /// The list, as a version 1 tree.
    List,
    /// The list, as a version 2 tree.
    Newer,
    /// A grid, once.
    Grid,
    /// Text that is not JSON, once.
    Unreadable,
}

/// The fixture's state, kept in the instance.
struct State {
    drawn: Cell<u32>,
    reversed: Cell<bool>,
    removed: Cell<bool>,
    next: Cell<Next>,
}

// SAFETY: a component's code runs on one thread.
unsafe impl Sync for State {}

static STATE: State = State {
    drawn: Cell::new(0),
    reversed: Cell::new(false),
    removed: Cell::new(false),
    next: Cell::new(Next::List),
};

/// The items, (id, title), in their first order.
const ITEMS: [(&str, &str); 8] = [
    ("first", "First"),
    ("reverse", "Reverse the list"),
    ("remove", "Remove this item"),
    ("newer", "Draw a newer tree"),
    ("grid", "Draw a grid"),
    ("unreadable", "Draw an unreadable tree"),
    ("bad-answer", "Answer unreadably"),
    ("quiet", "Answer nothing"),
];

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut json = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            other => json.push(other),
        }
    }
    json.push('"');
    json
}

/// The list as a tree naming `version`, with fields Pane does not know at
/// the root, in the view, in each item and in each action.
fn list(version: u32) -> String {
    let mut items: Vec<(&str, &str)> = ITEMS
        .iter()
        .copied()
        .filter(|&(id, _)| !(id == "remove" && STATE.removed.get()))
        .collect();
    if STATE.reversed.get() {
        items.reverse();
    }
    let items: Vec<String> = items
        .iter()
        .map(|&(id, title)| {
            format!(
                "{{\"id\":{},\"title\":{},\"subtitle\":\"Callback cb-{id}\",\"icon\":\"star\",\
                 \"accessories\":[{{\"text\":\"new\"}}],\
                 \"actions\":[{{\"title\":\"Run\",\"onAction\":\"cb-{id}\",\"shortcut\":\"ctrl+r\"}}]}}",
                quoted(id),
                quoted(title),
            )
        })
        .collect();
    format!(
        "{{\"version\":{version},\"renderAfter\":1000,\"view\":{{\"type\":\"list\",\
         \"title\":\"Drawn {} times\",\"dropdown\":{{}},\"items\":[{}]}}}}",
        STATE.drawn.get(),
        items.join(","),
    )
}

struct Trees;
export!(Trees);

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

impl Guest for Trees {
    type CustomView = NoView;

    async fn render() -> Result<String, String> {
        STATE.drawn.set(STATE.drawn.get() + 1);
        let next = STATE.next.get();
        Ok(match next {
            Next::List => list(1),
            Next::Newer => list(2),
            Next::Grid => {
                STATE.next.set(Next::List);
                "{\"version\":2,\"view\":{\"type\":\"grid\",\"columns\":3,\"items\":[]}}".into()
            }
            Next::Unreadable => {
                STATE.next.set(Next::List);
                "{\"version\":1,\"view\":".into()
            }
        })
    }

    async fn handle_event(callback: String, details: String) -> Result<String, String> {
        match callback.as_str() {
            "cb-reverse" => STATE.reversed.set(!STATE.reversed.get()),
            "cb-remove" => STATE.removed.set(true),
            "cb-newer" => STATE.next.set(Next::Newer),
            "cb-grid" => STATE.next.set(Next::Grid),
            "cb-unreadable" => STATE.next.set(Next::Unreadable),
            "cb-bad-answer" => return Ok("Handled, but not as JSON".into()),
            "cb-quiet" => return Ok("{\"toast\":{\"title\":\"Not shown yet\"}}".into()),
            "cb-first" => {}
            other => return Err(format!("unknown callback: {other}")),
        }
        let status = format!("Handled {callback} with {details}");
        Ok(format!(
            "{{\"status\":{},\"unknown\":true}}",
            quoted(&status)
        ))
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

/// Byte comparison, normally supplied by libc; the compiler emits calls to
/// it for string comparisons.
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
