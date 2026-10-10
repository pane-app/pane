//! __TITLE__, started from `pane-ext`'s `detail` template: a command that
//! takes a query and shows the detail of the text root search sends it,
//! through its alias or as a fallback. `pane-ext dev` builds it with
//! `cargo build --release --target wasm32-wasip2`, hands each build to the
//! running Pane, and builds it again after each save, so an edit shows at
//! once; see the README beside this file and Pane's development-mode
//! documentation.
//!
//! One component (the WebAssembly file `pane.json` names) serves every
//! command the package declares: Pane passes the command's id with each
//! call, so `render` and `run` below match on it. `pane-ext new command .`
//! adds one.
#![no_std]

// pane-ext new command adds a command's module here.

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::system::{Clip, copy};
use pane_extension::{Command, FieldValue, FormError, Item, List, NoCustomView};

/// The package's own type, whose command the SDK exports.
struct __STRUCT__;

pane_extension::export!(__STRUCT__);

/// What the command shows when root search sent it nothing: it says how to
/// send it something.
const NOTHING: &str = "Nothing yet: give this command an alias, or make it a fallback, then \
                       type into root search";

impl Command for __STRUCT__ {
    type CustomView = NoCustomView;

    /// The command's detail: the text it was sent, its measures, and a
    /// copy action. `commands::current()` is the launch record of the
    /// screen being drawn, whose `fallback_text` is what root search sent.
    async fn render() -> Result<List, String> {
        match pane_extension::commands::current().command.as_str() {
            "__NAME__" => {
                let text = pane_extension::commands::current().fallback_text.clone();
                let detail = match text.as_deref() {
                    Some(text) => detail_of(text),
                    None => List::new("__TITLE__").item(
                        Item::new("nothing", NOTHING)
                            .subtitle("Root search sends text through an alias or a fallback"),
                    ),
                };
                Ok(detail)
            }
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

/// The detail of `text`: the text, its measures, and a copy action on its
/// row.
fn detail_of(text: &str) -> List {
    let characters = text.chars().count();
    let words = text.split_whitespace().count();
    List::new("__TITLE__").items([
        Item::new("text", text).subtitle("What root search sent"),
        Item::new("copy", "Copy the text")
            .subtitle("Puts what root search sent on the clipboard")
            .on_action(|| async {
                let text =
                    pane_extension::commands::current().fallback_text.clone().unwrap_or_default();
                copy(&Clip::Text(text), false)?;
                show_toast(Toast::success("Copied"));
                Ok(())
            }),
        Item::new("characters", format!("{characters} characters"))
            .subtitle("Counted as Unicode characters, not bytes"),
        Item::new("words", format!("{words} words")),
    ])
}
