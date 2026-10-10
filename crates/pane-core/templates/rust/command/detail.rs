//! The `__NAME__` command: the detail of the text root search sends it,
//! as the `detail` template's own command shows it. Added to the package
//! by `pane-ext new command`; the entry file (`src/lib.rs`) calls its
//! `render`.

use pane_extension::alloc::{format, string::String};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::system::{Clip, copy};
use pane_extension::{Item, List};

/// What the command shows when root search sent it nothing: it says how to
/// send it something.
const NOTHING: &str = "Nothing yet: give this command an alias, or make it a fallback, then \
                       type into root search";

/// The command's detail: the text it was sent, its measures, and a copy
/// action on its row. `commands::current()` is the launch record of the
/// screen being drawn, whose `fallback_text` is what root search sent.
pub(crate) async fn render() -> Result<List, String> {
    let detail = match pane_extension::commands::current().fallback_text.as_deref() {
        Some(text) => detail_of(text),
        None => List::new("__TITLE__").item(
            Item::new("nothing", NOTHING)
                .subtitle("Root search sends text through an alias or a fallback"),
        ),
    };
    Ok(detail)
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
