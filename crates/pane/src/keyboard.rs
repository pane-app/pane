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
//! It is also where a binding meets its presentation: [`binding_keys`]
//! turns an effective [`Binding`] into the [`KeySequence`] the shared
//! keycaps draw — so the shared visual layer never sees a core binding,
//! and a hint always shows the binding in force.
//!
//! Text editing and text composition stay owned by the focused field:
//! [`pane_core::Binding::protected`] refuses the keys that would swallow
//! them, and the recorder's own cancellation keys are bound deeper than
//! any action of the set, so recording never triggers the action being
//! rebound.

use gpui::{App, KeyBinding, Keystroke};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use pane_core::hotkeys::{Kind, Shortcut, Side};
use pane_core::{Binding, Keyboard, KeyboardAction, NavigationBindings};

use crate::app::KEY_CONTEXT;
use crate::features::actions_panel;
use crate::features::root_search;
use crate::ui::keycap::{Key, KeySequence};
use crate::{
    Back, Confirm, DismissLauncher, FocusNext, FocusPrevious, OpenActions, OpenSettings,
    ReturnToRoot, SelectNext, SelectNextFive, SelectNextSection, SelectPrevious,
    SelectPreviousFive, SelectPreviousSection,
};

/// Registers the navigation actions under their effective bindings in
/// [`Keyboard`], in the contexts above. Call after the shared text
/// editing keys, so the search field's selection keys take precedence
/// over the field's own.
///
/// The navigation bindings' Left and Right (Alt+B and Alt+F, Alt+H and
/// Alt+L) are the focus traversal Tab already is: they move between the
/// query and the argument fields (#258).
///
/// Back a level is the one action of the set with no keymap binding: a
/// focused field's own Backspace deletes text, and the keymap would
/// either swallow the key ahead of that or fire beside it. The window
/// follows the binding itself, ahead of the fields where the field has
/// nothing left to delete (see [`LauncherWindow::backspace_back_keys`]),
/// so the field's own key keeps its priority by construction.
pub(crate) fn bind_keys(cx: &mut App, keyboard: &Keyboard, navigation: NavigationBindings) {
    let field = root_search::field_context();
    let panel = format!("{} > {}", actions_panel::CONTEXT, DEFAULT_INPUT_CONTEXT);
    let mut bindings = Vec::new();
    // The extra selection keys the Keyboard page's navigation bindings
    // choose, first, so the set's own bindings registered after them win.
    // Wherever Up and Down move a list — the Actions panel's list is one
    // — the pair moves it too (#258), bound in the panel's own field to
    // its own selection action.
    if let Some((previous, next)) = navigation.bindings() {
        bindings.push(KeyBinding::new(
            previous,
            actions_panel::PreviousAction,
            Some(panel.as_str()),
        ));
        bindings.push(KeyBinding::new(
            next,
            actions_panel::NextAction,
            Some(panel.as_str()),
        ));
        for (id, action) in [
            (previous, KeyboardAction::PreviousResult),
            (next, KeyboardAction::NextResult),
        ] {
            bindings.push(launcher_binding(id, action));
            bindings.push(field_binding(id, action, &field));
        }
    }
    // Left and Right between the query and the argument fields: the
    // focus traversal, on the pair's keys.
    if let Some((left, right)) = navigation.left_right() {
        bindings.push(KeyBinding::new(left, FocusPrevious, Some(KEY_CONTEXT)));
        bindings.push(KeyBinding::new(right, FocusNext, Some(KEY_CONTEXT)));
    }
    for action in KeyboardAction::ALL {
        let id = keyboard.binding(action).id();
        // The record's grammar is the keymap's own restricted to a single
        // keystroke, so every recorded binding parses; one that somehow
        // does not is skipped rather than panicking the app, leaving the
        // action without a key until the page resets it.
        if Keystroke::parse(&id).is_err() {
            continue;
        }
        // Back a level is followed by the window itself, not the keymap
        // (see the module docs).
        if action == KeyboardAction::BackspaceBack {
            continue;
        }
        bindings.push(launcher_binding(&id, action));
        // The keys that move the selection also move it while the query
        // field has focus, above the field's own caret keys — the fixed
        // Up and Down's arrangement, kept for whatever keys replace
        // them — and above the caret keys macOS binds to Command and the
        // arrows (Command+Down is the caret to the text's end there).
        // The selection and section keys also take the Actions panel's
        // own field, so its list answers them there as it answers Up and
        // Down.
        if matches!(
            action,
            KeyboardAction::PreviousResult
                | KeyboardAction::NextResult
                | KeyboardAction::FiveRowsUp
                | KeyboardAction::FiveRowsDown
                | KeyboardAction::PreviousSection
                | KeyboardAction::NextSection
        ) {
            bindings.push(field_binding(&id, action, &field));
            if action != KeyboardAction::PreviousResult && action != KeyboardAction::NextResult {
                bindings.push(field_binding(&id, action, &panel));
            }
        }
    }
    cx.bind_keys(bindings);
}

/// `action`'s binding for the launcher window's context.
fn launcher_binding(id: &str, action: KeyboardAction) -> KeyBinding {
    match action {
        KeyboardAction::PreviousResult => KeyBinding::new(id, SelectPrevious, Some(KEY_CONTEXT)),
        KeyboardAction::NextResult => KeyBinding::new(id, SelectNext, Some(KEY_CONTEXT)),
        KeyboardAction::FiveRowsUp => KeyBinding::new(id, SelectPreviousFive, Some(KEY_CONTEXT)),
        KeyboardAction::FiveRowsDown => KeyBinding::new(id, SelectNextFive, Some(KEY_CONTEXT)),
        KeyboardAction::PreviousSection => {
            KeyBinding::new(id, SelectPreviousSection, Some(KEY_CONTEXT))
        }
        KeyboardAction::NextSection => KeyBinding::new(id, SelectNextSection, Some(KEY_CONTEXT)),
        KeyboardAction::InvokeSelectedAction => KeyBinding::new(id, Confirm, Some(KEY_CONTEXT)),
        KeyboardAction::Back => KeyBinding::new(id, Back, Some(KEY_CONTEXT)),
        KeyboardAction::BackspaceBack => {
            unreachable!("Back a level is followed by the window, not the keymap")
        }
        KeyboardAction::ReturnToRoot => KeyBinding::new(id, ReturnToRoot, Some(KEY_CONTEXT)),
        KeyboardAction::DismissLauncher => KeyBinding::new(id, DismissLauncher, Some(KEY_CONTEXT)),
        KeyboardAction::OpenSettings => KeyBinding::new(id, OpenSettings, Some(KEY_CONTEXT)),
        KeyboardAction::OpenActions => KeyBinding::new(id, OpenActions, Some(KEY_CONTEXT)),
    }
}

/// A selection action's binding for the query field's context.
fn field_binding(id: &str, action: KeyboardAction, context: &str) -> KeyBinding {
    match action {
        KeyboardAction::PreviousResult => KeyBinding::new(id, SelectPrevious, Some(context)),
        KeyboardAction::NextResult => KeyBinding::new(id, SelectNext, Some(context)),
        KeyboardAction::FiveRowsUp => KeyBinding::new(id, SelectPreviousFive, Some(context)),
        KeyboardAction::FiveRowsDown => KeyBinding::new(id, SelectNextFive, Some(context)),
        KeyboardAction::PreviousSection => {
            KeyBinding::new(id, SelectPreviousSection, Some(context))
        }
        KeyboardAction::NextSection => KeyBinding::new(id, SelectNextSection, Some(context)),
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
pub(crate) fn rebuild(cx: &mut App, keyboard: &Keyboard, navigation: NavigationBindings) {
    cx.clear_key_bindings();
    crate::bind_keys_with(cx, keyboard, navigation);
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

/// The Escape key's cap: what closes the Actions panel and the footer
/// menu, whatever back is bound to.
pub(crate) fn escape_keys() -> KeySequence {
    binding_keys(&Binding::parse("escape").expect("escape is a binding"))
}

/// The local chord that invokes quick slot `number` (1 to 5, the numbered
/// pins) while root search has focus: Ctrl and the digit, the approved
/// Windows chord. A window-local binding of the root search field, never
/// registered with the system, and fixed — not one of the actions the
/// Keyboard page rebinds — so the hints the slots show are always the
/// chords that work.
pub(crate) fn quick_slot_binding(number: usize) -> Binding {
    pane_core::keyboard::number_key(number)
}

/// The caps of quick slot `number`'s chord ([`quick_slot_binding`]).
pub(crate) fn quick_slot_keys(number: usize) -> KeySequence {
    binding_keys(&quick_slot_binding(number))
}

/// The launcher's key that toggles a pin: Ctrl+Shift+F (Command+Shift+F
/// on macOS) pins root search's selected result, or unpins it once it is
/// pinned, and unpins a quick slot that has focus. Window-local, never
/// registered with the system, and fixed — not one of the actions the
/// Keyboard page rebinds — so the Actions panel's caps and the pin hint
/// always name a key that works.
pub(crate) fn toggle_pin_binding() -> Binding {
    pane_core::keyboard::pin_key()
}

/// The launcher's keys that move a focused quick slot one place among the
/// pins: Ctrl+Alt (Command+Option on macOS) and Up or Left moves it
/// `earlier`, Down or Right later. Window-local and fixed, as the pin key
/// is ([`toggle_pin_binding`]). Up or Down comes first, the vertical
/// layout's direction; Left or Right second, the strip's.
pub(crate) fn move_pin_bindings(earlier: bool) -> [Binding; 2] {
    pane_core::keyboard::move_pin_keys(earlier)
}

/// The keys `binding` is pressed with, as keycaps show them on this
/// platform: every modifier its own cap, then the key. On Windows (and
/// Linux) the Windows key leads, as Windows writes its own shortcuts
/// ("Win+Alt+Left"), then Ctrl, Alt, Shift and Fn; macOS keeps its
/// Control, Option, Shift, Command order. Enter shows the return symbol
/// and the arrows their arrows, under their names; Escape shows "Esc".
/// The sequence's name — what a hint announces — is the full binding,
/// modifiers included: Shift+Enter is never shown or read as Enter.
pub(crate) fn binding_keys(binding: &Binding) -> KeySequence {
    let (control, alt, shift, platform, function) = binding.modifiers();
    let modifiers: [(bool, &str); 5] = if cfg!(target_os = "macos") {
        [
            (control, "Control"),
            (alt, "Option"),
            (shift, "Shift"),
            (platform, "Command"),
            (function, "Fn"),
        ]
    } else {
        [
            (platform, "Win"),
            (control, "Ctrl"),
            (alt, "Alt"),
            (shift, "Shift"),
            (function, "Fn"),
        ]
    };
    let mut keys: Vec<Key> = modifiers
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| Key::new(name, name))
        .collect();
    let name = key_name(binding.key());
    let cap = match binding.key() {
        "enter" => "↵".to_owned(),
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        "escape" => "Esc".to_owned(),
        _ => name.clone(),
    };
    keys.push(Key::new(cap, name));
    KeySequence { keys }
}

/// The keys a command's global hotkey is pressed with, shown as
/// [`binding_keys`] shows a binding: a hotkey's keys are a binding's.
/// (Core's type for a global hotkey is `Shortcut`.) The binding kinds
/// #260 adds show as Windows names them: a lone tap is its modifier —
/// "Win", "Right Ctrl" — a double tap the name twice — "Ctrl Ctrl" —
/// and a chord with a named side carries it — "Right Alt+Space". The
/// numpad's keys keep their own names, distinct from their
/// counterparts'.
pub(crate) fn hotkey_keys(shortcut: &Shortcut) -> KeySequence {
    match shortcut.kind() {
        Kind::Chord => {
            let macos = cfg!(target_os = "macos");
            // The chord's modifiers, each with the side it names where one
            // is named (#260), in the order the platform writes them, then
            // the key — the caps of the binding of the key alone.
            let order: [usize; 4] = if macos { [0, 1, 2, 3] } else { [3, 0, 1, 2] };
            let mut keys: Vec<Key> = order
                .into_iter()
                .filter_map(|at| shortcut.sides()[at].map(|side| (at, side)))
                .map(|(at, side)| {
                    let name = format!("{}{}", side_prefix(side), modifier_name(at, macos));
                    Key::new(name.clone(), name)
                })
                .collect();
            match Binding::new(false, false, false, false, false, shortcut.key()) {
                Ok(alone) => keys.extend(binding_keys(&alone).keys),
                // Every hotkey key is a binding key; a future one that is
                // not is still shown, by its own text.
                Err(_) => keys.push(Key::new(shortcut.key().to_uppercase(), shortcut.key())),
            }
            KeySequence { keys }
        }
        Kind::Tap | Kind::Double => {
            // One cap: the binding as the user names it — "Win", "Right
            // Ctrl", "Ctrl Ctrl".
            let name = shortcut.to_string();
            KeySequence {
                keys: vec![Key::new(name.clone(), name)],
            }
        }
    }
}

/// How `side` prefixes a modifier's name as a hotkey's cap shows it
/// (#260): "Left ", "Right ", or nothing for either.
fn side_prefix(side: Side) -> &'static str {
    match side {
        Side::Any => "",
        Side::Left => "Left ",
        Side::Right => "Right ",
    }
}

/// The modifier at `at` — control, alt, shift, then the Windows key —
/// as a hotkey's cap names it on `macos` (Control, Option, Shift,
/// Command) and elsewhere (Win, Ctrl, Alt, Shift), as [`binding_keys`]
/// orders them.
fn modifier_name(at: usize, macos: bool) -> &'static str {
    match (at, macos) {
        (0, true) => "Control",
        (0, false) => "Ctrl",
        (1, true) => "Option",
        (1, false) => "Alt",
        (2, _) => "Shift",
        (_, true) => "Command",
        (_, false) => "Win",
    }
}

/// The key's own name, as the binding's text names it ("Enter", "Page
/// Down", "V"): the binding of the key alone, written out.
fn key_name(key: &str) -> String {
    Binding::new(false, false, false, false, false, key)
        .map(|alone| alone.to_string())
        .unwrap_or_else(|_| key.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ctrl's cap and name on this platform: macOS writes it out.
    const CTRL: &str = if cfg!(target_os = "macos") {
        "Control"
    } else {
        "Ctrl"
    };

    fn sequence(binding: &str) -> (Vec<String>, String) {
        let keys = binding_keys(&Binding::parse(binding).expect("the binding parses"));
        let caps = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        (caps, keys.name())
    }

    #[test]
    fn enter_alone_is_one_return_cap_named_enter() {
        assert_eq!(sequence("enter"), (vec!["↵".into()], "Enter".into()));
    }

    #[test]
    fn every_modifier_of_an_enter_chord_gets_its_own_cap_and_name() {
        assert_eq!(
            sequence("shift-enter"),
            (vec!["Shift".into(), "↵".into()], "Shift+Enter".into())
        );
        assert_eq!(
            sequence("ctrl-enter"),
            (vec![CTRL.into(), "↵".into()], format!("{CTRL}+Enter"))
        );
        assert_eq!(
            sequence("ctrl-shift-p"),
            (
                vec![CTRL.into(), "Shift".into(), "P".into()],
                format!("{CTRL}+Shift+P")
            )
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_windows_key_leads_and_arrows_show_as_arrows() {
        assert_eq!(
            sequence("win-alt-left"),
            (
                vec!["Win".into(), "Alt".into(), "←".into()],
                "Win+Alt+Left".into()
            )
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_hotkey_shows_its_keys_as_a_binding_does() {
        let keys = hotkey_keys(&Shortcut::parse("ctrl+shift+v").unwrap());
        let caps: Vec<_> = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        assert_eq!(caps, ["Ctrl", "Shift", "V"]);
        assert_eq!(keys.name(), "Ctrl+Shift+V");
        let keys = hotkey_keys(&Shortcut::parse("super+alt+space").unwrap());
        assert_eq!(keys.name(), "Win+Alt+Space");
    }

    #[test]
    fn the_binding_kinds_show_as_windows_names_them() {
        // A lone tap is one cap, the modifier it names — "Win",
        // "Right Ctrl" — and a double tap the name twice — "Ctrl Ctrl"
        // (#260): the cap, the announced name and the written-out binding
        // agree, as they do for a chord.
        let keys = hotkey_keys(&Shortcut::parse("tap:win").unwrap());
        let name = keys.name();
        if cfg!(target_os = "macos") {
            assert_eq!(name, "Command");
        } else if cfg!(target_os = "windows") {
            assert_eq!(name, "Win");
        } else {
            assert_eq!(name, "Super");
        }
        let keys = hotkey_keys(&Shortcut::parse("tap:rctrl").unwrap());
        let caps: Vec<_> = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        let ctrl = if cfg!(target_os = "macos") {
            "Control"
        } else {
            "Ctrl"
        };
        assert_eq!(caps, [format!("Right {ctrl}").as_str()]);
        assert_eq!(keys.name(), format!("Right {ctrl}"));
        let keys = hotkey_keys(&Shortcut::parse("double:ctrl").unwrap());
        assert_eq!(keys.name(), format!("{ctrl} {ctrl}"));
        assert_eq!(keys.keys.len(), 1);
        // A chord with a named side carries it, with the other modifiers
        // and the key as a binding shows them.
        let keys = hotkey_keys(&Shortcut::parse("ralt+space").unwrap());
        let alt = if cfg!(target_os = "macos") {
            "Option"
        } else {
            "Alt"
        };
        assert_eq!(keys.name(), format!("Right {alt}+Space"));
        let caps: Vec<_> = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        assert_eq!(caps, [format!("Right {alt}").as_str(), "Space"]);
        // The numpad's keys keep their own names, distinct from their
        // counterparts' (#260).
        let keys = hotkey_keys(&Shortcut::parse("ctrl+numpad5").unwrap());
        let caps: Vec<_> = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        assert_eq!(caps, [ctrl, "Num 5"]);
        let keys = hotkey_keys(&Shortcut::parse("ctrl+enter").unwrap());
        let caps: Vec<_> = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        assert_eq!(caps, [ctrl, "\u{21b5}"]);
        let keys = hotkey_keys(&Shortcut::parse("ctrl+numpad_enter").unwrap());
        let caps: Vec<_> = keys.keys.iter().map(|key| key.cap.to_string()).collect();
        assert_eq!(caps, [ctrl, "Num Enter"]);
    }

    #[test]
    fn named_keys_keep_their_names_for_assistive_technology() {
        assert_eq!(sequence("escape"), (vec!["Esc".into()], "Escape".into()));
        assert_eq!(
            sequence("ctrl-k"),
            (vec![CTRL.into(), "K".into()], format!("{CTRL}+K"))
        );
        assert_eq!(
            sequence("ctrl-pagedown"),
            (
                vec![CTRL.into(), "Page Down".into()],
                format!("{CTRL}+Page Down")
            )
        );
    }
}
