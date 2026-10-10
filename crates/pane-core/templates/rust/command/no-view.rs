//! The `__NAME__` command: it runs without opening a screen and answers in
//! a toast, as the `no-view` template's own command does. Added to the
//! package by `pane-ext new command`; the entry file (`src/lib.rs`) calls
//! its `run`.

use pane_extension::alloc::{format, string::String};
use pane_extension::feedback::{Toast, show_toast};

/// What the command says when root search sent it nothing: it says how to
/// send it something.
const NOTHING: &str = "Nothing yet: give this command an alias, or make it a fallback, then \
                       type into root search";

/// Runs the command: it answers in a toast with the text root search sent
/// it, if any. A command Pane or another command launches in the
/// background does its work but shows nothing.
pub(crate) async fn run(launch: pane_extension::LaunchRecord) -> Result<(), String> {
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
