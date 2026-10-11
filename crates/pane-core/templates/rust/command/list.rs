//! The `__NAME__` command: a list of items, each running an action when
//! chosen, as the `list` template's own command is. Added to the package
//! by `pane-ext new command`; the entry file (`src/lib.rs`) calls its
//! `render`.

use pane_extension::alloc::string::String;
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Item, List};

/// What the "Say hello" item shows in a toast.
const GREETING: &str = "Hello from __TITLE__";

/// The command's list: what shows at Enter on its row in root search. Pane
/// asks for it again after each item's action runs.
pub(crate) async fn render() -> Result<List, String> {
    Ok(List::new("__TITLE__").items([
        Item::new("greet", "Say hello")
            .subtitle("Shows a toast, as an item's action does")
            .on_action(|| async {
                show_toast(Toast::success(GREETING));
                Ok(())
            }),
        Item::new("edit", "Edit this list")
            .subtitle("The items are here; save an edit and Pane reloads the command"),
    ]))
}
