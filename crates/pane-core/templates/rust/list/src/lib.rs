//! __TITLE__, started from `pane-ext`'s `list` template: a command that
//! opens a list of items, each running an action when chosen. `pane-ext
//! dev` builds it with `cargo build --release --target wasm32-wasip2`,
//! hands each build to the running Pane, and builds it again after each
//! save, so an edit shows at once; see the README beside this file and
//! Pane's development-mode documentation.
//!
//! One component (the WebAssembly file `pane.json` names) serves every
//! command the package declares: Pane passes the command's id with each
//! call, so `render` and `run` below match on it. `pane-ext new command .`
//! adds one.
#![no_std]

// pane-ext new command adds a command's module here.

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, FieldValue, FormError, Item, List, NoCustomView};

/// The package's own type, whose command the SDK exports.
struct __STRUCT__;

pane_extension::export!(__STRUCT__);

/// What the "Say hello" item shows in a toast.
const GREETING: &str = "Hello from __TITLE__";

impl Command for __STRUCT__ {
    type CustomView = NoCustomView;

    /// The command's list: what shows at Enter on its row in root search.
    /// Pane asks for it again after each item's action runs.
    async fn render() -> Result<List, String> {
        match pane_extension::commands::current().command.as_str() {
            "__NAME__" => Ok(List::new("__TITLE__").items([
                Item::new("greet", "Say hello")
                    .subtitle("Shows a toast, as an item's action does")
                    .on_action(|| async {
                        show_toast(Toast::success(GREETING));
                        Ok(())
                    }),
                Item::new("edit", "Edit this list")
                    .subtitle("The items are here in render; save an edit and Pane reloads it"),
            ])),
            // pane-ext new command adds a view command's arm here.
            other => Err(format!("unknown command: {other}")),
        }
    }

    /// A command that opens no screen would run here instead; the
    /// `no-view` template starts from one.
    async fn run(command: String, _launch: pane_extension::LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            // pane-ext new command adds a no-view command's arm here.
            other => Err(format!("`{other}` opens a screen; it has no run entry point")),
        }
    }

    /// A command whose items open forms would answer them here; the `form`
    /// template starts from one.
    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        match pane_extension::commands::current().command.as_str() {
            // pane-ext new command adds a form command's arm here.
            other => Err(FormError {
                field: None,
                message: format!("`{other}` has no forms: {item_id}"),
            }),
        }
    }
}
