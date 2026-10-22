//! Shared editable-input bindings; screen actions remain with the caller.

use gpui::App;
use gpui_elements::editable_text::actions::{
    DEFAULT_INPUT_CONTEXT, Enter, Escape, Tab, default_bindings,
};

/// Proof that the editing keys of every editable text element, the form's
/// text fields and root search's query field alike, are bound: see
/// [`bind_text_editing`].
pub(crate) struct TextEditingKeys(());

/// Registers GPUI CE's editing keys for every editable text element, once,
/// except Tab, Enter and Escape: those are left to bubble to the launcher
/// (focus traversal, confirm or submit, and back) instead of being text
/// edits. The form's and root search's key bindings take the result, since
/// both rely on it: without it their fields would not edit, and with Enter
/// or Escape bound as text edits the fields would swallow them.
pub(crate) fn bind_text_editing(cx: &mut App) -> TextEditingKeys {
    let text_editing = default_bindings()
        .as_keybindings(Some(DEFAULT_INPUT_CONTEXT))
        .filter(|binding| {
            let action = binding.action();
            !(action.partial_eq(&Tab) || action.partial_eq(&Enter) || action.partial_eq(&Escape))
        })
        .collect::<Vec<_>>();
    cx.bind_keys(text_editing);
    TextEditingKeys(())
}
