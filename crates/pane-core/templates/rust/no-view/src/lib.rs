//! __TITLE__, started from `pane-ext`'s `no-view` template: a command that
//! runs without opening a screen (`"mode": "no-view"` in `pane.json`),
//! telling the user what happened with a toast. Root search sends it text
//! through its alias or as a fallback, which `run` answers in the toast.
//! `pane-ext dev` builds it with `cargo build --release --target
//! wasm32-wasip2`, hands each build to the running Pane, and builds it
//! again after each save; see the README beside this file and Pane's
//! development-mode documentation.
//!
//! One component (the WebAssembly file `pane.json` names) serves every
//! command the package declares: Pane passes the command's id with each
//! call, so `render` and `run` below match on it. `pane-ext new command .`
//! adds one.
#![no_std]

// pane-ext new command adds a command's module here.

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, FieldValue, FormError, List, NoCustomView};

/// The package's own type, whose command the SDK exports.
struct __STRUCT__;

pane_extension::export!(__STRUCT__);

/// What the command says when root search sent it nothing: it says how to
/// send it something.
const NOTHING: &str = "Nothing yet: give this command an alias, or make it a fallback, then \
                       type into root search";

impl Command for __STRUCT__ {
    type CustomView = NoCustomView;

    /// A command that opens a screen would draw it here; the `list`,
    /// `detail` and `form` templates start from one.
    async fn render() -> Result<List, String> {
        match pane_extension::commands::current().command.as_str() {
            // pane-ext new command adds a view command's arm here.
            other => Err(format!("unknown command: {other}")),
        }
    }

    /// Runs the `__NAME__` command: it answers in a toast with the text
    /// root search sent it, if any. A command Pane or another command
    /// launches in the background does its work but shows nothing.
    async fn run(command: String, _launch: pane_extension::LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            "__NAME__" => the_run(_launch).await,
            // pane-ext new command adds a no-view command's arm here.
            other => Err(format!("unknown command: {other}")),
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

/// The command's work: what it says in its toast.
async fn the_run(launch: pane_extension::LaunchRecord) -> Result<(), String> {
    if launch.launch_type == pane_extension::LaunchType::Background {
        return Ok(());
    }
    let heard = match launch.fallback_text.as_deref() {
        Some(text) => format!("__TITLE__ heard “{text}”"),
        None => NOTHING.into(),
    };
    show_toast(Toast::success(heard));
    Ok(())
}
