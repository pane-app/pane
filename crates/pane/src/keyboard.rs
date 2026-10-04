//! The launcher's in-app navigation bindings: the bounded set of host
//! actions the Keyboard page rebinds, applied to the window layer's key
//! bindings.
//!
//! [`pane_core::Keyboard`] is the renderer-independent value — one
//! [`pane_core::Binding`] per action, recorded in the host settings. This
//! module is where that value meets the keymap: [`bind_keys`] registers
//! each action under its effective binding, in the contexts the action
//! works in, and [`rebuild`] re-makes the whole keymap when a binding
//! changes, so the previous binding is replaced rather than piling up
//! beside the new one.
//!
//! The contexts, and why each action needs them:
//!
//! - **The launcher's window** (`"Launcher"`): every action of the set.
//!   They are window-local — the same keystroke may serve the Settings
//!   window, whose own keys bind in its context — and they sit *below*
//!   the deeper contexts of a focused field, an open form, a custom view
//!   or the footer menu, whose keys win while those controls hold focus.
//! - **The search field** (`"RootSearch > EditableText"`): previous and
//!   next result only, so the selection moves while the query field has
//!   focus instead of the field's own caret keys acting, as the fixed
//!   Up and Down always did. The other actions bubble to the window's
//!   context from the field, as Enter and Escape always did.
//!
//! Text editing and text composition stay owned by the focused field:
//! [`pane_core::Binding::protected`] refuses the keys that would swallow
//! them, and the recorder's own cancellation keys are bound deeper than
//! any action of the set, so recording never triggers the action being
//! rebound.

use gpui::{App, KeyBinding, Keystroke};
use pane_core::{Binding, Keyboard, KeyboardAction};

use crate::app::KEY_CONTEXT;
use crate::features::root_search;
use crate::{
    Back, Confirm, DismissLauncher, OpenSettings, ReturnToRoot, SelectNext, SelectPrevious,
};

/// Registers the navigation actions under their effective bindings in
/// [`Keyboard`], in the contexts above. Call after the shared text
/// editing keys, so the search field's selection keys take precedence
/// over the field's own.
pub(crate) fn bind_keys(cx: &mut App, keyboard: &Keyboard) {
    let field = root_search::field_context();
    let mut bindings = Vec::new();
    for action in KeyboardAction::ALL {
        let id = keyboard.binding(action).id();
        // The record's grammar is the keymap's own restricted to a single
        // keystroke, so every recorded binding parses; one that somehow
        // does not is skipped rather than panicking the app, leaving the
        // action without a key until the page resets it.
        if Keystroke::parse(&id).is_err() {
            continue;
        }
        bindings.push(launcher_binding(&id, action));
        // The selection keys also move the selection while the query
        // field has focus, above the field's own caret keys — the fixed
        // Up and Down's arrangement, kept for whatever keys replace them.
        if matches!(
            action,
            KeyboardAction::PreviousResult | KeyboardAction::NextResult
        ) {
            bindings.push(field_binding(&id, action, &field));
        }
    }
    cx.bind_keys(bindings);
}

/// `action`'s binding for the launcher window's context.
fn launcher_binding(id: &str, action: KeyboardAction) -> KeyBinding {
    match action {
        KeyboardAction::PreviousResult => KeyBinding::new(id, SelectPrevious, Some(KEY_CONTEXT)),
        KeyboardAction::NextResult => KeyBinding::new(id, SelectNext, Some(KEY_CONTEXT)),
        KeyboardAction::InvokeSelectedAction => KeyBinding::new(id, Confirm, Some(KEY_CONTEXT)),
        KeyboardAction::Back => KeyBinding::new(id, Back, Some(KEY_CONTEXT)),
        KeyboardAction::ReturnToRoot => KeyBinding::new(id, ReturnToRoot, Some(KEY_CONTEXT)),
        KeyboardAction::DismissLauncher => KeyBinding::new(id, DismissLauncher, Some(KEY_CONTEXT)),
        KeyboardAction::OpenSettings => KeyBinding::new(id, OpenSettings, Some(KEY_CONTEXT)),
    }
}

/// A selection action's binding for the query field's context.
fn field_binding(id: &str, action: KeyboardAction, context: &str) -> KeyBinding {
    match action {
        KeyboardAction::PreviousResult => KeyBinding::new(id, SelectPrevious, Some(context)),
        KeyboardAction::NextResult => KeyBinding::new(id, SelectNext, Some(context)),
        _ => unreachable!("only the selection keys bind in the field's context"),
    }
}

/// Re-makes the whole keymap over `keyboard`: the fixed bindings are
/// re-registered and the actions of the set take these bindings, so a
/// change replaces the binding it supersedes instead of adding a second
/// one. The caller passes the keyboard in force — the settings entity
/// hands the one it holds, since it cannot read itself back while its
/// own update is in flight. Safe wherever the settings change, including
/// mid-session: the keymap is data, and the windows redraw off the
/// effect the re-registration pushes.
pub(crate) fn rebuild(cx: &mut App, keyboard: &Keyboard) {
    cx.clear_key_bindings();
    crate::bind_keys_with(cx, keyboard);
}

/// The binding the keystroke of a key pressed names, for the Keyboard
/// page's recorder: the modifiers held and the key, as the window
/// reports them. `Err` explains a keystroke no binding can name (a
/// modifier held alone, or a key the grammar cannot write).
pub(crate) fn binding_of(keystroke: &Keystroke) -> Result<Binding, String> {
    let modifiers = keystroke.modifiers;
    Binding::new(
        modifiers.control,
        modifiers.alt,
        modifiers.shift,
        modifiers.platform,
        modifiers.function,
        &keystroke.key,
    )
}
