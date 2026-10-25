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

/// Whether `keystroke`, a key pressed, is `binding`. A binding with Shift
/// and a punctuation key also matches the press Windows reports for it:
/// the shifted character without Shift (Ctrl+Shift+, arrives as Ctrl+<
/// there), on the US layout, the one whose shifted characters are known
/// here.
pub(crate) fn pressed(keystroke: &Keystroke, binding: &Binding) -> bool {
    if binding_of(keystroke).is_ok_and(|pressed| pressed == *binding) {
        return true;
    }
    let (control, alt, shift, platform, function) = binding.modifiers();
    let held = keystroke.modifiers;
    shift
        && !held.shift
        && (held.control, held.alt, held.platform, held.function)
            == (control, alt, platform, function)
        && shifted(binding.key()).is_some_and(|key| keystroke.key == key)
}

/// The character Shift and `key` type on the US layout, for the
/// punctuation keys.
fn shifted(key: &str) -> Option<&'static str> {
    Some(match key {
        "," => "<",
        "." => ">",
        "/" => "?",
        ";" => ":",
        "'" => "\"",
        "[" => "{",
        "]" => "}",
        "\\" => "|",
        "-" => "_",
        "=" => "+",
        "`" => "~",
        _ => return None,
    })
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
/// platform: [`binding_keys_on`] for the system Pane runs on.
pub(crate) fn binding_keys(binding: &Binding) -> KeySequence {
    binding_keys_on(binding, cfg!(target_os = "macos"))
}

/// The keys `binding` is pressed with, as keycaps show them on `macos` or
/// elsewhere: every modifier its own cap, then the key. On Windows (and
/// Linux) the Windows key leads, as Windows writes its own shortcuts
/// ("Win+Alt+Left"), then Ctrl, Alt, Shift and Fn, as words. macOS keeps
/// its Control, Option, Shift, Command order and shows each modifier's
/// symbol (⌃ ⌥ ⇧ ⌘), as Raycast and macOS's own menus do (#117), so a long
/// chord still fits a narrow launcher's footer. Enter shows the return
/// symbol and the arrows their arrows; Escape shows "Esc", and macOS's
/// symbols for its other named keys ([`key_cap`]). Every cap keeps its
/// key's spelled-out name ("Control", "Page Down"), which the sequence's
/// name — what a hint announces to assistive technology — joins: the full
/// binding, modifiers included, so Shift+Enter is never read as Enter.
fn binding_keys_on(binding: &Binding, macos: bool) -> KeySequence {
    let (control, alt, shift, platform, function) = binding.modifiers();
    let modifiers: [(bool, &str, &str); 5] = if macos {
        [
            (control, "⌃", "Control"),
            (alt, "⌥", "Option"),
            (shift, "⇧", "Shift"),
            (platform, "⌘", "Command"),
            (function, "Fn", "Fn"),
        ]
    } else {
        [
            (platform, "Win", "Win"),
            (control, "Ctrl", "Ctrl"),
            (alt, "Alt", "Alt"),
            (shift, "Shift", "Shift"),
            (function, "Fn", "Fn"),
        ]
    };
    let mut keys: Vec<Key> = modifiers
        .into_iter()
        .filter(|(held, _, _)| *held)
        .map(|(_, cap, name)| Key::new(cap, name))
        .collect();
    let name = key_name(binding.key());
    let cap = key_cap(binding.key(), macos).map_or_else(|| name.clone(), str::to_owned);
    keys.push(Key::new(cap, name));
    KeySequence { keys }
}

/// The symbol a named key's cap shows on `macos` or elsewhere, where it
/// shows one rather than its name: Enter's return symbol and the arrows
/// everywhere, "Esc" for Escape off macOS, and on macOS the symbols its
/// menus and Raycast show — ⎋ Escape, ⇥ Tab, ⌫ Backspace (the key macOS
/// labels Delete), ⌦ Delete (forward delete), ⇞ Page Up, ⇟ Page Down,
/// ↖ Home and ↘ End. `None` for a key shown by its name ("Space", "V").
fn key_cap(key: &str, macos: bool) -> Option<&'static str> {
    Some(match (key, macos) {
        ("enter", _) => "↵",
        ("left", _) => "←",
        ("right", _) => "→",
        ("up", _) => "↑",
        ("down", _) => "↓",
        ("escape", false) => "Esc",
        ("escape", true) => "⎋",
        ("tab", true) => "⇥",
        ("backspace", true) => "⌫",
        ("delete", true) => "⌦",
        ("pageup", true) => "⇞",
        ("pagedown", true) => "⇟",
        ("home", true) => "↖",
        ("end", true) => "↘",
        _ => return None,
    })
}

/// The keys a command's global hotkey is pressed with, shown as
/// [`binding_keys`] shows a binding: a hotkey's keys are a binding's.
/// (Core's type for a global hotkey is `Shortcut`.) The binding kinds
/// #260 adds show as Windows names them: a lone tap is its modifier —
/// "Win", "Right Ctrl" — a double tap the name twice — "Ctrl Ctrl" —
/// and a chord with a named side carries it — "Right Alt+Space". The
/// numpad's keys keep their own names, distinct from their
/// counterparts'. On macOS a chord's caps show the modifiers' symbols,
/// as [`binding_keys`] does: "Right ⌥", named "Right Option".
pub(crate) fn hotkey_keys(shortcut: &Shortcut) -> KeySequence {
    hotkey_keys_on(shortcut, cfg!(target_os = "macos"))
}

/// [`hotkey_keys`] on `macos` or elsewhere. A tap or a double tap is
/// named as this system names it, whichever `macos` says: macOS takes
/// neither yet (`pane_core::hotkeys`), so its caps are Windows' words.
fn hotkey_keys_on(shortcut: &Shortcut, macos: bool) -> KeySequence {
    match shortcut.kind() {
        Kind::Chord => {
            // The chord's modifiers, each with the side it names where one
            // is named (#260), in the order the platform writes them, then
            // the key — the caps of the binding of the key alone.
            let order: [usize; 4] = if macos { [0, 1, 2, 3] } else { [3, 0, 1, 2] };
            let mut keys: Vec<Key> = order
                .into_iter()
                .filter_map(|at| shortcut.sides()[at].map(|side| (at, side)))
                .map(|(at, side)| {
                    let (cap, name) = modifier_cap(at, macos);
                    let side = side_prefix(side);
                    Key::new(format!("{side}{cap}"), format!("{side}{name}"))
                })
                .collect();
            match Binding::new(false, false, false, false, false, shortcut.key()) {
                Ok(alone) => keys.extend(binding_keys_on(&alone, macos).keys),
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
/// as a hotkey's cap shows it and names it on `macos` (⌃ Control,
/// ⌥ Option, ⇧ Shift, ⌘ Command) and elsewhere (Win, Ctrl, Alt, Shift,
/// shown as named), as [`binding_keys_on`] does.
fn modifier_cap(at: usize, macos: bool) -> (&'static str, &'static str) {
    match (at, macos) {
        (0, true) => ("⌃", "Control"),
        (0, false) => ("Ctrl", "Ctrl"),
        (1, true) => ("⌥", "Option"),
        (1, false) => ("Alt", "Alt"),
        (2, true) => ("⇧", "Shift"),
        (2, false) => ("Shift", "Shift"),
        (_, true) => ("⌘", "Command"),
        (_, false) => ("Win", "Win"),
    }
}

/// The tooltip of a dropdown beside a search field, `what` it does and
/// the chord that opens it from anywhere on the screen: "Filter by Type
/// (Ctrl+P)" (#497).
pub(crate) fn dropdown_hint(what: &str) -> String {
    let keys = binding_keys(&pane_core::keyboard::open_dropdown_key());
    format!("{what} ({})", keys.name())
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

    /// `binding`'s caps and name on `macos` or elsewhere.
    fn sequence_on(binding: &str, macos: bool) -> (Vec<String>, String) {
        let keys = binding_keys_on(&Binding::parse(binding).expect("the binding parses"), macos);
        (caps(&keys), keys.name())
    }

    /// `binding`'s caps and name off macOS.
    fn sequence(binding: &str) -> (Vec<String>, String) {
        sequence_on(binding, false)
    }

    /// `binding`'s caps and name on macOS.
    fn sequence_macos(binding: &str) -> (Vec<String>, String) {
        sequence_on(binding, true)
    }

    fn caps(keys: &KeySequence) -> Vec<String> {
        keys.keys.iter().map(|key| key.cap.to_string()).collect()
    }

    #[test]
    fn enter_alone_is_one_return_cap_named_enter() {
        for macos in [false, true] {
            assert_eq!(
                sequence_on("enter", macos),
                (vec!["↵".into()], "Enter".into())
            );
        }
    }

    #[test]
    fn every_modifier_of_an_enter_chord_gets_its_own_cap_and_name() {
        assert_eq!(
            sequence("shift-enter"),
            (vec!["Shift".into(), "↵".into()], "Shift+Enter".into())
        );
        assert_eq!(
            sequence("ctrl-enter"),
            (vec!["Ctrl".into(), "↵".into()], "Ctrl+Enter".into())
        );
        assert_eq!(
            sequence("ctrl-shift-p"),
            (
                vec!["Ctrl".into(), "Shift".into(), "P".into()],
                "Ctrl+Shift+P".into()
            )
        );
    }

    #[test]
    fn macos_caps_show_the_modifiers_symbols_under_their_names() {
        // Raycast's and macOS's own menus' symbols, in macOS's Control,
        // Option, Shift, Command order (#117); the names stay spelled out
        // for assistive technology (#93).
        assert_eq!(
            sequence_macos("shift-enter"),
            (vec!["⇧".into(), "↵".into()], "Shift+Enter".into())
        );
        assert_eq!(
            sequence_macos("cmd-k"),
            (vec!["⌘".into(), "K".into()], "Command+K".into())
        );
        assert_eq!(
            sequence_macos("ctrl-alt-shift-cmd-p"),
            (
                vec!["⌃".into(), "⌥".into(), "⇧".into(), "⌘".into(), "P".into()],
                "Control+Option+Shift+Command+P".into()
            )
        );
        assert_eq!(
            sequence_macos("fn-f5"),
            (vec!["Fn".into(), "F5".into()], "Fn+F5".into())
        );
    }

    #[test]
    fn macos_caps_show_the_named_keys_symbols_under_their_names() {
        for (binding, cap, name) in [
            ("escape", "⎋", "Escape"),
            ("tab", "⇥", "Tab"),
            ("backspace", "⌫", "Backspace"),
            ("delete", "⌦", "Delete"),
            ("pageup", "⇞", "Page Up"),
            ("pagedown", "⇟", "Page Down"),
            ("home", "↖", "Home"),
            ("end", "↘", "End"),
            ("left", "←", "Left"),
            ("right", "→", "Right"),
            ("up", "↑", "Up"),
            ("down", "↓", "Down"),
            ("space", "Space", "Space"),
        ] {
            assert_eq!(
                sequence_macos(binding),
                (vec![cap.into()], name.into()),
                "{binding}"
            );
        }
        // The chord the narrow footer's test binds (#117): four caps, one
        // character each.
        assert_eq!(
            sequence_macos("ctrl-alt-shift-pagedown"),
            (
                vec!["⌃".into(), "⌥".into(), "⇧".into(), "⇟".into()],
                "Control+Option+Shift+Page Down".into()
            )
        );
    }

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

    #[test]
    fn a_hotkey_shows_its_keys_as_a_binding_does() {
        let keys = hotkey_keys_on(&Shortcut::parse("ctrl+shift+v").unwrap(), false);
        assert_eq!(caps(&keys), ["Ctrl", "Shift", "V"]);
        assert_eq!(keys.name(), "Ctrl+Shift+V");
        let keys = hotkey_keys_on(&Shortcut::parse("super+alt+space").unwrap(), false);
        assert_eq!(keys.name(), "Win+Alt+Space");
        // macOS shows the symbols, under the names, as a binding does.
        let keys = hotkey_keys_on(&Shortcut::parse("ctrl+alt+g").unwrap(), true);
        assert_eq!(caps(&keys), ["⌃", "⌥", "G"]);
        assert_eq!(keys.name(), "Control+Option+G");
        let keys = hotkey_keys_on(&Shortcut::parse("super+shift+pageup").unwrap(), true);
        assert_eq!(caps(&keys), ["⇧", "⌘", "⇞"]);
        assert_eq!(keys.name(), "Shift+Command+Page Up");
    }

    #[test]
    fn the_binding_kinds_show_as_windows_names_them() {
        // A lone tap is one cap, the modifier it names — "Win",
        // "Right Ctrl" — and a double tap the name twice — "Ctrl Ctrl"
        // (#260): the cap, the announced name and the written-out binding
        // agree, as they do for a chord.
        let macos = cfg!(target_os = "macos");
        let keys = hotkey_keys(&Shortcut::parse("tap:win").unwrap());
        let name = keys.name();
        if macos {
            assert_eq!(name, "Command");
        } else if cfg!(target_os = "windows") {
            assert_eq!(name, "Win");
        } else {
            assert_eq!(name, "Super");
        }
        let keys = hotkey_keys(&Shortcut::parse("tap:rctrl").unwrap());
        let ctrl = if macos { "Control" } else { "Ctrl" };
        assert_eq!(caps(&keys), [format!("Right {ctrl}")]);
        assert_eq!(keys.name(), format!("Right {ctrl}"));
        let keys = hotkey_keys(&Shortcut::parse("double:ctrl").unwrap());
        assert_eq!(keys.name(), format!("{ctrl} {ctrl}"));
        assert_eq!(keys.keys.len(), 1);
        // A chord with a named side carries it, with the other modifiers
        // and the key as a binding shows them; macOS's cap is the symbol.
        let right_alt = Shortcut::parse("ralt+space").unwrap();
        let keys = hotkey_keys_on(&right_alt, false);
        assert_eq!(keys.name(), "Right Alt+Space");
        assert_eq!(caps(&keys), ["Right Alt", "Space"]);
        let keys = hotkey_keys_on(&right_alt, true);
        assert_eq!(keys.name(), "Right Option+Space");
        assert_eq!(caps(&keys), ["Right ⌥", "Space"]);
        // The numpad's keys keep their own names, distinct from their
        // counterparts' (#260).
        for (macos, ctrl) in [(false, "Ctrl"), (true, "⌃")] {
            let caps_of =
                |shortcut: &str| caps(&hotkey_keys_on(&Shortcut::parse(shortcut).unwrap(), macos));
            assert_eq!(caps_of("ctrl+numpad5"), [ctrl, "Num 5"]);
            assert_eq!(caps_of("ctrl+enter"), [ctrl, "\u{21b5}"]);
            assert_eq!(caps_of("ctrl+numpad_enter"), [ctrl, "Num Enter"]);
        }
    }

    #[test]
    fn named_keys_keep_their_names_for_assistive_technology() {
        assert_eq!(sequence("escape"), (vec!["Esc".into()], "Escape".into()));
        assert_eq!(
            sequence("ctrl-k"),
            (vec!["Ctrl".into(), "K".into()], "Ctrl+K".into())
        );
        assert_eq!(
            sequence("ctrl-pagedown"),
            (
                vec!["Ctrl".into(), "Page Down".into()],
                "Ctrl+Page Down".into()
            )
        );
        assert_eq!(
            sequence_macos("ctrl-pagedown"),
            (vec!["⌃".into(), "⇟".into()], "Control+Page Down".into())
        );
    }

    #[test]
    fn a_shifted_punctuation_chord_matches_as_windows_reports_it_too() {
        let press = |id: &str| Keystroke::parse(id).unwrap();
        let configure = Binding::parse("ctrl-shift-,").unwrap();
        // As the test platform and macOS report it, and as Windows does:
        // the shifted character, Shift dropped.
        assert!(pressed(&press("ctrl-shift-,"), &configure));
        assert!(pressed(&press("ctrl-<"), &configure));
        // Other modifiers, or the plain key, are other chords.
        assert!(!pressed(&press("ctrl-,"), &configure));
        assert!(!pressed(&press("ctrl-alt-<"), &configure));
        assert!(!pressed(&press("<"), &configure));
        // A chord without Shift never takes the shifted character.
        let extension = Binding::parse("ctrl-alt-,").unwrap();
        assert!(pressed(&press("ctrl-alt-,"), &extension));
        assert!(!pressed(&press("ctrl-alt-<"), &extension));
        let disable = Binding::parse("ctrl-alt-shift-d").unwrap();
        assert!(pressed(&press("ctrl-alt-shift-d"), &disable));
        assert!(!pressed(&press("ctrl-alt-d"), &disable));
    }
}
