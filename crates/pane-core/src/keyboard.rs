//! The bounded set of in-app navigation actions whose keys the Keyboard
//! page lets the user rebind.
//!
//! These are the *host's* navigation keys — the ones Pane's own windows
//! bind for moving through root search's results and leaving screens —
//! not any extension's actions and not the standard text-editing keys a
//! focused field owns. The set is closed and small by decision
//! ([#70](https://github.com/hoangvu12/pane/issues/70),
//! [#77](https://github.com/hoangvu12/pane/issues/77)): arbitrary
//! rebinding of every key is explicitly not promised.
//!
//! Nothing here knows the renderer. A [`Binding`] is a keystroke as a
//! plain value — the modifiers held and one key, written `ctrl-alt-b` —
//! with its own grammar for the records that keep it, so the same rules
//! apply wherever a binding is read: the host settings' record, the
//! Keyboard page's recorder, and the checks that keep one binding from
//! silently swallowing another. The window layer turns the [`Keyboard`]
//! set into its own key bindings; this module decides only what a
//! binding *is* and which sets of them are valid.
//!
//! The defaults are the specification's provisional synthesis, not
//! separately confirmed product decisions: Up/Down for selection, Enter
//! for invocation, Escape for back, Cmd+Esc on macOS / Shift+Esc
//! elsewhere for return to root, Cmd+W / Ctrl+W for dismissing the
//! launcher, and Cmd+, / Ctrl+, for Settings.

use std::collections::BTreeMap;

/// The platform modifier's name in a binding's id: `cmd` on every
/// system, as the window layer's keystroke grammar writes it.
const PLATFORM_MODIFIER: &str = "cmd";

/// The modifiers a binding can hold, in the order their ids are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Modifiers {
    control: bool,
    alt: bool,
    shift: bool,
    platform: bool,
    function: bool,
}

impl Modifiers {
    /// The modifiers of a binding id's prefix, as [`Binding::parse`]
    /// reads them. Synonyms name the same modifier: `control` is Ctrl,
    /// `option` is Alt, and `super`, `win` and `command` are the
    /// platform modifier. An unrecognized part holds nothing.
    fn of(parts: &[&str]) -> Modifiers {
        let mut modifiers = Modifiers::default();
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.control = true,
                "alt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "super" | "win" | "cmd" | "command" => modifiers.platform = true,
                "fn" => modifiers.function = true,
                _ => {}
            }
        }
        modifiers
    }

    /// Whether no modifier but Shift is held.
    fn only_shift(&self) -> bool {
        !self.control && !self.alt && !self.platform && !self.function
    }

    /// The modifiers in id order, joined by `-`, or empty.
    fn id(&self) -> String {
        let mut parts = Vec::new();
        for (held, name) in [
            (self.control, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.platform, PLATFORM_MODIFIER),
            (self.function, "fn"),
        ] {
            if held {
                parts.push(name);
            }
        }
        parts.join("-")
    }

    /// The modifiers as the user reads them on this system, joined by
    /// `+`: Control, Option, Shift, Command and Fn on macOS; Ctrl, Alt,
    /// Shift, Win and Fn elsewhere.
    fn display(&self) -> String {
        let names: [&str; 5] = if cfg!(target_os = "macos") {
            ["Control", "Option", "Shift", "Command", "Fn"]
        } else {
            ["Ctrl", "Alt", "Shift", "Win", "Fn"]
        };
        [
            self.control,
            self.alt,
            self.shift,
            self.platform,
            self.function,
        ]
        .into_iter()
        .zip(names)
        .filter_map(|(held, name)| held.then_some(name))
        .collect::<Vec<_>>()
        .join("+")
    }
}

/// One in-app navigation action the Keyboard page rebinds, and the
/// record's name for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyboardAction {
    /// Moves the selection to the previous result.
    PreviousResult,
    /// Moves the selection to the next result.
    NextResult,
    /// Opens the selected result — the footer's primary action.
    InvokeSelectedAction,
    /// Leaves the open screen, one level at a time, clearing a search's
    /// text on the way.
    Back,
    /// Returns to root search from wherever the launcher is.
    ReturnToRoot,
    /// Hides the launcher, keeping Pane running in the background.
    DismissLauncher,
    /// Opens or focuses the Settings window.
    OpenSettings,
    /// Opens or closes the selected result's Actions panel.
    OpenActions,
}

impl KeyboardAction {
    /// The bounded set, in the order the Keyboard page lists it.
    pub const ALL: [KeyboardAction; 8] = [
        KeyboardAction::PreviousResult,
        KeyboardAction::NextResult,
        KeyboardAction::InvokeSelectedAction,
        KeyboardAction::OpenActions,
        KeyboardAction::Back,
        KeyboardAction::ReturnToRoot,
        KeyboardAction::DismissLauncher,
        KeyboardAction::OpenSettings,
    ];

    /// The action's id in the record, such as `next-result`.
    pub fn id(self) -> &'static str {
        match self {
            KeyboardAction::PreviousResult => "previous-result",
            KeyboardAction::NextResult => "next-result",
            KeyboardAction::InvokeSelectedAction => "invoke-selected-action",
            KeyboardAction::Back => "back",
            KeyboardAction::ReturnToRoot => "return-to-root",
            KeyboardAction::DismissLauncher => "dismiss-launcher",
            KeyboardAction::OpenSettings => "open-settings",
            KeyboardAction::OpenActions => "open-actions",
        }
    }

    /// The action of a record's `id`, if it is one of the set.
    pub fn of(id: &str) -> Option<KeyboardAction> {
        KeyboardAction::ALL
            .into_iter()
            .find(|action| action.id() == id)
    }

    /// The action's name, as the Keyboard page's row and its messages
    /// say it: "Next result", "Open Settings".
    pub fn title(self) -> &'static str {
        match self {
            KeyboardAction::PreviousResult => "Previous result",
            KeyboardAction::NextResult => "Next result",
            KeyboardAction::InvokeSelectedAction => "Invoke selected action",
            KeyboardAction::Back => "Back",
            KeyboardAction::ReturnToRoot => "Return to root",
            KeyboardAction::DismissLauncher => "Dismiss launcher",
            KeyboardAction::OpenSettings => "Open Settings",
            KeyboardAction::OpenActions => "Open actions",
        }
    }

    /// What the action does, as the page's rows describe it and a
    /// collision's message names the other action: "moves to the next
    /// result".
    pub fn does(self) -> &'static str {
        match self {
            KeyboardAction::PreviousResult => "moves to the previous result",
            KeyboardAction::NextResult => "moves to the next result",
            KeyboardAction::InvokeSelectedAction => "invokes the selected action",
            KeyboardAction::Back => "goes back",
            KeyboardAction::ReturnToRoot => "returns to root",
            KeyboardAction::DismissLauncher => "dismisses the launcher",
            KeyboardAction::OpenSettings => "opens Settings",
            KeyboardAction::OpenActions => "opens the selected result's actions",
        }
    }

    /// The action's defaults on this system, in order of preference: the
    /// first is its default; a record that already gives that keystroke to
    /// another action gives this action the next one free (an action added
    /// after the record was written).
    fn defaults(self) -> &'static [&'static str] {
        let macos = cfg!(target_os = "macos");
        match (self, macos) {
            (KeyboardAction::PreviousResult, _) => &["up"],
            (KeyboardAction::NextResult, _) => &["down"],
            (KeyboardAction::InvokeSelectedAction, _) => &["enter"],
            (KeyboardAction::Back, _) => &["escape"],
            (KeyboardAction::ReturnToRoot, true) => &["cmd-escape"],
            (KeyboardAction::ReturnToRoot, false) => &["shift-escape"],
            (KeyboardAction::DismissLauncher, true) => &["cmd-w"],
            (KeyboardAction::DismissLauncher, false) => &["ctrl-w"],
            (KeyboardAction::OpenSettings, true) => &["cmd-,"],
            (KeyboardAction::OpenSettings, false) => &["ctrl-,"],
            // Ctrl+K deletes to the end of the line in a macOS field.
            (KeyboardAction::OpenActions, true) => &["cmd-k", "cmd-shift-k"],
            (KeyboardAction::OpenActions, false) => &["ctrl-k", "ctrl-shift-k"],
        }
    }
}

/// The keys a binding's grammar knows as modifiers, for the message that
/// refuses a modifier held alone.
const MODIFIER_KEYS: [&str; 5] = ["ctrl", "alt", "shift", "cmd", "fn"];

/// One keystroke: the modifiers held while one key is pressed. The id —
/// [`Binding::id`], and the record's text — is the modifiers in a fixed
/// order and the key joined by `-`, such as `ctrl-alt-b`, `shift-escape`
/// or plain `up`.
///
/// The grammar is the window layer's keystroke grammar for a single
/// keystroke, restricted to what a binding may use: modifiers from
/// `ctrl`, `alt`, `shift`, `cmd` and `fn` (with their synonyms) and one
/// key that is not itself a modifier and holds no separator. Whether a
/// keystroke may *take over* an action is a separate rule —
/// [`Binding::protected`] — so a plain navigation key like `up` is a
/// valid binding while a plain typing key is refused for what it would
/// swallow.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    modifiers: Modifiers,
    key: String,
}

impl Binding {
    /// The binding of the modifiers held and `key`, as the window
    /// reports a key press: a letter, a digit, a named key such as
    /// `escape`, `enter`, `up` or `f5`, or a single character, in any
    /// case. A key that is itself a modifier is explained — a binding
    /// ends with a key, not with a modifier held alone — and so is one
    /// the grammar cannot write.
    pub fn new(
        control: bool,
        alt: bool,
        shift: bool,
        platform: bool,
        function: bool,
        key: &str,
    ) -> Result<Binding, String> {
        let key = key.trim().to_ascii_lowercase();
        let modifier = ["control", "option", "command", "super", "win"]
            .into_iter()
            .any(|name| key == name)
            || MODIFIER_KEYS.contains(&key.as_str());
        if modifier {
            return Err(format!(
                "Pane cannot use {key} in a shortcut: hold one more key with it"
            ));
        }
        if key.is_empty() || key.contains('-') || key.chars().any(char::is_whitespace) {
            return Err(format!("Pane cannot use “{key}” as a key in a shortcut"));
        }
        Ok(Binding {
            modifiers: Modifiers {
                control,
                alt,
                shift,
                platform,
                function,
            },
            key,
        })
    }

    /// Reads a binding as [`Binding::id`] writes it, such as
    /// `ctrl-alt-b`, `shift-escape` or `up`. The modifiers may name
    /// their synonyms (`control`, `option`, `super`, `win`, `command`);
    /// the key is whatever ends the id. A key that is itself a modifier —
    /// a modifier held alone — or one the grammar cannot write is
    /// explained.
    pub fn parse(id: &str) -> Result<Binding, String> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err("a shortcut names no key".into());
        }
        let parts: Vec<&str> = trimmed.split('-').collect();
        let key = parts.last().copied().unwrap_or_default();
        let modifiers = Modifiers::of(&parts[..parts.len() - 1]);
        Binding::new(
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
            modifiers.function,
            key,
        )
    }

    /// The binding's key, as the window layer's keystroke grammar writes
    /// it: `escape`, `enter`, `b`, `,`.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The binding's modifiers, as booleans in the order
    /// [`Binding::new`] takes them.
    pub fn modifiers(&self) -> (bool, bool, bool, bool, bool) {
        (
            self.modifiers.control,
            self.modifiers.alt,
            self.modifiers.shift,
            self.modifiers.platform,
            self.modifiers.function,
        )
    }

    /// The same text on every system, for records: the modifiers in a
    /// fixed order, then the key, joined by `-`.
    pub fn id(&self) -> String {
        let prefix = self.modifiers.id();
        if prefix.is_empty() {
            self.key.clone()
        } else {
            format!("{prefix}-{}", self.key)
        }
    }

    /// Whether this binding is exactly `key` with no modifier but Shift.
    fn plain_or_shift(&self, key: &str) -> bool {
        self.modifiers.only_shift() && self.key == key
    }

    /// Whether this binding is protected for a focused field — the
    /// Keyboard page refuses it, so text editing, text composition and
    /// focus traversal stay owned by the focused control and a navigation
    /// binding cannot silently swallow them. Returns the field's name for
    /// what the key does there, for the refusal's message.
    ///
    /// The keys that type are any single character and Space; the keys
    /// that edit are Backspace and Delete; the keys that move within a
    /// field are the arrows, Home and End; Tab, with no modifier but
    /// Shift, traverses. With the platform's editing modifier held
    /// (Command on macOS, Ctrl elsewhere) the field selects, copies,
    /// pastes, cuts, undoes and moves by words —
    /// [`text_editing_action`] below knows that platform's set.
    pub fn protected(&self) -> Option<&'static str> {
        if self.plain_or_shift("space") {
            return Some("types a space");
        }
        if self.plain_or_shift("tab") {
            return Some("moves the focus");
        }
        if self.plain_or_shift("backspace") {
            return Some("deletes text");
        }
        if self.plain_or_shift("delete") {
            return Some("deletes text");
        }
        for key in ["left", "right", "home", "end"] {
            if self.plain_or_shift(key) {
                return Some("moves within the text");
            }
        }
        if self.key.chars().count() == 1 && self.modifiers.only_shift() {
            return Some("types a character");
        }
        text_editing_action(self)
    }
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let modifiers = self.modifiers.display();
        if modifiers.is_empty() {
            f.write_str(&key_name(&self.key))
        } else {
            write!(f, "{modifiers}+{}", key_name(&self.key))
        }
    }
}

/// The key as the user reads it: the named keys with their names, a
/// single character as its uppercase, anything else capitalized.
fn key_name(key: &str) -> String {
    match key {
        "escape" => "Escape".into(),
        "enter" => "Enter".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "Page Up".into(),
        "pagedown" => "Page Down".into(),
        "space" => "Space".into(),
        "tab" => "Tab".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        other => other.to_uppercase(),
    }
}

/// The platform's editing combinations a binding could swallow in a
/// focused field: Command on macOS, Ctrl elsewhere, and macOS's Option
/// and Ctrl ones. Returns what the combination does there, for the
/// refusal's message.
fn text_editing_action(binding: &Binding) -> Option<&'static str> {
    let (control, alt, _shift, platform, _function) = binding.modifiers();
    let key = binding.key();
    // The editing modifier on this system: Command on macOS, Ctrl
    // elsewhere.
    let editing = if cfg!(target_os = "macos") {
        platform
    } else {
        control
    };
    if editing {
        return match key {
            "a" => Some("selects all text"),
            "c" => Some("copies"),
            "v" => Some("pastes"),
            "x" => Some("cuts"),
            "z" => Some("undoes"),
            "space" => Some("shows the character palette"),
            "backspace" | "delete" => Some("deletes a word"),
            "left" | "right" => Some("moves by a word"),
            "home" | "end" | "up" | "down" => Some("moves through the text"),
            _ => None,
        };
    }
    if cfg!(target_os = "macos") {
        if alt {
            return match key {
                "backspace" => Some("deletes a word"),
                "left" | "right" => Some("moves by a word"),
                _ => None,
            };
        }
        if control && key == "k" {
            return Some("deletes to the end of the line");
        }
    }
    None
}

fn default_binding(id: &str) -> Binding {
    Binding::parse(id).expect("a default is a valid binding")
}

/// The set of bindings for the bounded actions: one binding per action,
/// valid together. Held in the host settings' record and applied by the
/// window layer; [`Keyboard::default_for_this_system`] is what a record
/// with no keyboard field means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyboard {
    bindings: BTreeMap<KeyboardAction, Binding>,
}

impl Default for Keyboard {
    fn default() -> Keyboard {
        Keyboard::default_for_this_system()
    }
}

impl Keyboard {
    /// The provisional defaults: Up/Down for selection, Enter for
    /// invocation, Cmd+K on macOS / Ctrl+K elsewhere for the Actions
    /// panel, Escape for back, Cmd+Esc / Shift+Esc for return to root,
    /// Cmd+W / Ctrl+W for dismissing the launcher, and Cmd+, / Ctrl+, for
    /// Settings.
    pub fn default_for_this_system() -> Keyboard {
        let bindings = KeyboardAction::ALL
            .into_iter()
            .map(|action| (action, default_binding(action.defaults()[0])))
            .collect();
        Keyboard { bindings }
    }

    /// The binding `action` has.
    pub fn binding(&self, action: KeyboardAction) -> &Binding {
        self.bindings
            .get(&action)
            .expect("every action of the set has a binding")
    }

    /// Whether `action` still has its default binding.
    pub fn is_default(&self, action: KeyboardAction) -> bool {
        *self.binding(action) == *Keyboard::default_for_this_system().binding(action)
    }

    /// Reads the keyboard the record holds: `fields` maps each action's
    /// id to its binding's id, as [`Keyboard::recorded`] writes them.
    /// An unknown action id, a binding that is not one, one that is
    /// protected for a focused field, or two of the record's actions
    /// sharing one binding is `Err` with the problem, so the record fails
    /// whole rather than half-loading.
    ///
    /// The record's bindings are read first, against each other only, so
    /// a binding moved from one action to another loads whatever order
    /// the fields come in. Then each action the record doesn't name — one
    /// added after it was written — takes its first default no recorded
    /// action holds.
    pub fn parse(fields: &BTreeMap<String, String>) -> Result<Keyboard, String> {
        let mut keyboard = Keyboard {
            bindings: BTreeMap::new(),
        };
        for (id, binding) in fields {
            let action = KeyboardAction::of(id).ok_or_else(|| {
                format!("its keyboard names “{id}”, which is not one of the actions")
            })?;
            let binding = Binding::parse(binding).map_err(|problem| {
                format!("its binding for {} is not one: {problem}", action.title())
            })?;
            keyboard.check(action, &binding).map_err(|problem| {
                format!(
                    "its binding for {} cannot be used: {problem}",
                    action.title()
                )
            })?;
            keyboard.bindings.insert(action, binding);
        }
        for action in KeyboardAction::ALL {
            if keyboard.bindings.contains_key(&action) {
                continue;
            }
            let free = action
                .defaults()
                .iter()
                .map(|id| default_binding(id))
                .find(|binding| keyboard.check(action, binding).is_ok())
                .ok_or_else(|| {
                    format!(
                        "its keyboard leaves {} no free binding: its defaults are taken",
                        action.title()
                    )
                })?;
            keyboard.bindings.insert(action, free);
        }
        Ok(keyboard)
    }

    /// The keyboard as the record holds it: every action's binding id.
    pub fn recorded(&self) -> BTreeMap<String, String> {
        self.bindings
            .iter()
            .map(|(action, binding)| (action.id().to_owned(), binding.id()))
            .collect()
    }

    /// Sets `action` to `binding`, checked as the page records one: a
    /// binding protected for a focused field is refused, and so is one
    /// another action of the set already has — their contexts overlap in
    /// the same window, so two actions on one keystroke cannot be told
    /// apart. `Err` names the problem; nothing changes then.
    pub fn checked_set(&mut self, action: KeyboardAction, binding: Binding) -> Result<(), String> {
        self.check(action, &binding)?;
        self.bindings.insert(action, binding);
        Ok(())
    }

    /// The one reason `action` cannot take `binding`, if it cannot:
    /// protected for a focused field, or already another action's. Every
    /// collision of a set is caught here as the set is built (each field
    /// is compared with all the map holds), so no two actions of a valid
    /// set ever share a binding.
    fn check(&self, action: KeyboardAction, binding: &Binding) -> Result<(), String> {
        if let Some(protected) = binding.protected() {
            return Err(format!(
                "{binding} is protected: it {protected} in a field, so it cannot {}",
                KeyboardAction::does(action)
            ));
        }
        if let Some(other) = self
            .bindings
            .iter()
            .find(|(other, held)| **other != action && *held == binding)
        {
            return Err(format!(
                "{binding} already {}",
                KeyboardAction::does(*other.0)
            ));
        }
        Ok(())
    }
}

/// The modifier the launcher's fixed pin keys hold: Command on macOS,
/// Ctrl elsewhere.
const PIN_MODIFIER: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

/// The launcher's key that toggles a pin: Ctrl+Shift+F (Command+Shift+F
/// on macOS). Window-local and fixed, not one of the actions the Keyboard
/// page rebinds.
pub fn pin_key() -> Binding {
    default_binding(&format!("{PIN_MODIFIER}-shift-f"))
}

/// The launcher's keys that move a focused quick slot one place among the
/// pins: Ctrl+Alt (Command+Option on macOS) and Up or Left moves it
/// `earlier`, Down or Right later; Up or Down first. Window-local and
/// fixed, as [`pin_key`] is.
pub fn move_pin_keys(earlier: bool) -> [Binding; 2] {
    let (vertical, horizontal) = if earlier {
        ("up", "left")
    } else {
        ("down", "right")
    };
    [vertical, horizontal].map(|arrow| default_binding(&format!("{PIN_MODIFIER}-alt-{arrow}")))
}

/// The local chord that picks what number `number` (0 to 9) names in the
/// launcher: Ctrl and the digit. Fixed, as [`pin_key`] is.
pub fn number_key(number: usize) -> Binding {
    default_binding(&format!("ctrl-{number}"))
}

/// The key that runs the action at `index` of the selected item in a
/// command's list without opening the Actions panel, for the second and
/// third (#137): Ctrl+Enter and Ctrl+Shift+Enter, the same chords on every
/// system (macOS names Ctrl "Control"). The first is the invoke binding the
/// Keyboard page rebinds ([`KeyboardAction::InvokeSelectedAction`]); `None`
/// for it and for any later action.
pub fn action_key(index: usize) -> Option<Binding> {
    match index {
        1 => Some(default_binding("ctrl-enter")),
        2 => Some(default_binding("ctrl-shift-enter")),
        _ => None,
    }
}

/// Pane's own effective keys in the launcher: what an extension's action
/// shortcut may never take (#137). An action whose shortcut is one of them
/// keeps its place in the Actions panel, without the shortcut.
///
/// They are the Keyboard page's bindings as the user has them (rebinds
/// included) and the navigation bindings it adds, and the launcher's fixed
/// keys: Escape, Ctrl+K (whatever Open actions is bound to), Up and Down,
/// Tab, Enter and the action chords ([`action_key`]), Ctrl and a digit,
/// and root search's pin keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneKeys {
    keys: Vec<(Binding, String)>,
}

impl Default for PaneKeys {
    fn default() -> PaneKeys {
        PaneKeys::new(
            &Keyboard::default_for_this_system(),
            crate::host_settings::NavigationBindings::default(),
        )
    }
}

impl PaneKeys {
    /// Pane's keys with `keyboard`'s bindings and `navigation`'s extra
    /// selection keys in force.
    pub fn new(keyboard: &Keyboard, navigation: crate::host_settings::NavigationBindings) -> Self {
        let mut keys: Vec<(Binding, String)> = Vec::new();
        let mut add = |binding: Binding, does: &str| {
            if !keys.iter().any(|(held, _)| *held == binding) {
                keys.push((binding, does.to_owned()));
            }
        };
        for action in KeyboardAction::ALL {
            add(keyboard.binding(action).clone(), action.does());
        }
        if let Some((previous, next)) = navigation.bindings() {
            add(default_binding(previous), "moves to the previous result");
            add(default_binding(next), "moves to the next result");
        }
        add(
            default_binding("escape"),
            "closes the Actions panel and goes back",
        );
        add(default_binding("ctrl-k"), "opens the Actions panel");
        add(default_binding("up"), "moves to the previous result");
        add(default_binding("down"), "moves to the next result");
        add(default_binding("tab"), "moves the focus");
        add(default_binding("shift-tab"), "moves the focus");
        add(default_binding("enter"), "runs the primary action");
        for (index, does) in [
            (1, "runs the secondary action"),
            (2, "runs the third action"),
        ] {
            add(action_key(index).expect("an action chord"), does);
        }
        for number in 0..=9 {
            add(number_key(number), "picks a numbered result");
        }
        add(pin_key(), "pins the selected result");
        for earlier in [true, false] {
            for key in move_pin_keys(earlier) {
                add(key, "moves a pin");
            }
        }
        PaneKeys { keys }
    }

    /// What Pane does with `binding`, if it is one of its keys: "opens
    /// the Actions panel".
    pub fn taken(&self, binding: &Binding) -> Option<&str> {
        self.keys
            .iter()
            .find(|(held, _)| held == binding)
            .map(|(_, does)| does.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `id`, panicking on a problem, for the tests' fixtures.
    fn binding(id: &str) -> Binding {
        Binding::parse(id).unwrap()
    }

    #[test]
    fn ids_round_trip_through_parse() {
        for id in [
            "up",
            "down",
            "enter",
            "escape",
            "shift-escape",
            "cmd-escape",
            "ctrl-w",
            "cmd-w",
            "ctrl-,",
            "ctrl-alt-b",
            "f5",
            "pageup",
        ] {
            assert_eq!(binding(id).id(), id, "{id} round trips");
        }
        // Synonyms normalize to the one spelling; `option` is Alt, as
        // the window layer's grammar reads it.
        assert_eq!(binding("control-alt-command-b").id(), "ctrl-alt-cmd-b");
        assert_eq!(binding("win-super-option-b").id(), "alt-cmd-b");
        assert_eq!(binding("Control-B").id(), "ctrl-b");
    }

    #[test]
    fn a_modifier_alone_or_a_bad_key_is_explained() {
        for id in ["ctrl", "shift", "cmd", "fn", ""] {
            assert!(Binding::parse(id).is_err(), "{id} is not a binding");
        }
        assert!(Binding::new(false, false, false, false, false, "cmd").is_err());
        assert!(Binding::new(false, false, false, false, false, "").is_err());
        // The minus key holds the separator, so the grammar cannot write
        // it: the recorder explains rather than records it.
        let problem = Binding::parse("ctrl--").unwrap_err();
        assert!(problem.contains("as a key"), "{problem}");
    }

    #[test]
    fn bindings_are_displayed_as_the_user_reads_them() {
        assert_eq!(binding("up").to_string(), "Up");
        assert_eq!(binding("escape").to_string(), "Escape");
        assert_eq!(binding("enter").to_string(), "Enter");
        assert_eq!(binding("shift-escape").to_string(), "Shift+Escape");
        // The modifiers are named for this system, as Shortcut names them.
        let macos = cfg!(target_os = "macos");
        assert_eq!(
            binding("ctrl-alt-b").to_string(),
            if macos {
                "Control+Option+B"
            } else {
                "Ctrl+Alt+B"
            }
        );
        assert_eq!(
            binding("ctrl-,").to_string(),
            if macos { "Control+," } else { "Ctrl+," }
        );
        assert_eq!(
            binding("cmd-w").to_string(),
            if macos { "Command+W" } else { "Win+W" }
        );
        assert_eq!(binding("f5").to_string(), "F5");
    }

    #[test]
    fn the_defaults_are_the_specifications_proposal() {
        let defaults = Keyboard::default_for_this_system();
        assert_eq!(defaults.binding(KeyboardAction::PreviousResult).id(), "up");
        assert_eq!(defaults.binding(KeyboardAction::NextResult).id(), "down");
        assert_eq!(
            defaults.binding(KeyboardAction::InvokeSelectedAction).id(),
            "enter"
        );
        assert_eq!(defaults.binding(KeyboardAction::Back).id(), "escape");
        assert_eq!(
            defaults.binding(KeyboardAction::ReturnToRoot).id(),
            if cfg!(target_os = "macos") {
                "cmd-escape"
            } else {
                "shift-escape"
            }
        );
        assert_eq!(
            defaults.binding(KeyboardAction::DismissLauncher).id(),
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            }
        );
        assert_eq!(
            defaults.binding(KeyboardAction::OpenSettings).id(),
            if cfg!(target_os = "macos") {
                "cmd-,"
            } else {
                "ctrl-,"
            }
        );
        assert_eq!(
            defaults.binding(KeyboardAction::OpenActions).id(),
            if cfg!(target_os = "macos") {
                "cmd-k"
            } else {
                "ctrl-k"
            }
        );
        // Every default is a valid, distinct set, and it round trips.
        let recorded = defaults.recorded();
        assert_eq!(
            Keyboard::parse(&recorded).unwrap(),
            defaults,
            "the defaults round trip"
        );
        let ids: Vec<String> = KeyboardAction::ALL
            .into_iter()
            .map(|action| defaults.binding(action).id())
            .collect();
        let distinct: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(ids.len(), distinct.len(), "no two actions share a default");
    }

    fn record(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|(action, binding)| ((*action).to_owned(), (*binding).to_owned()))
            .collect()
    }

    #[test]
    fn a_record_from_before_an_action_existed_gives_it_its_default() {
        // A record written before Open actions was an action of the set.
        let mut older = Keyboard::default_for_this_system().recorded();
        older.remove("open-actions");
        let keyboard = Keyboard::parse(&older).unwrap();
        assert_eq!(keyboard, Keyboard::default_for_this_system());
    }

    #[test]
    fn a_record_holding_a_new_actions_default_keeps_it_and_the_action_takes_its_alternate() {
        let (taken, alternate) = if cfg!(target_os = "macos") {
            ("cmd-k", "cmd-shift-k")
        } else {
            ("ctrl-k", "ctrl-shift-k")
        };
        let keyboard = Keyboard::parse(&record(&[("back", taken)])).unwrap();
        assert_eq!(keyboard.binding(KeyboardAction::Back).id(), taken);
        assert_eq!(
            keyboard.binding(KeyboardAction::OpenActions).id(),
            alternate
        );
    }

    #[test]
    fn a_record_that_moved_a_binding_between_actions_loads() {
        // Dismiss moved off its default, then Back took that default: a
        // valid set, whatever order its fields are read in.
        let (dismiss, moved) = if cfg!(target_os = "macos") {
            ("cmd-w", "cmd-q")
        } else {
            ("ctrl-w", "ctrl-q")
        };
        let fields = record(&[("back", dismiss), ("dismiss-launcher", moved)]);
        let keyboard = Keyboard::parse(&fields).unwrap();
        assert_eq!(keyboard.binding(KeyboardAction::Back).id(), dismiss);
        assert_eq!(
            keyboard.binding(KeyboardAction::DismissLauncher).id(),
            moved
        );
        // Escape, Back's default, is free again; nothing else took it.
        assert_eq!(Keyboard::parse(&keyboard.recorded()).unwrap(), keyboard);
    }

    #[test]
    fn a_record_whose_bindings_collide_still_fails_whole() {
        let fields = record(&[("back", "ctrl-q"), ("dismiss-launcher", "ctrl-q")]);
        assert!(Keyboard::parse(&fields).is_err());
    }

    #[test]
    fn actions_read_from_their_ids() {
        assert_eq!(
            KeyboardAction::of("next-result"),
            Some(KeyboardAction::NextResult)
        );
        assert_eq!(KeyboardAction::of("made-up"), None);
    }

    #[test]
    fn protected_keys_are_refused_and_navigation_keys_are_not() {
        for id in [
            "b",
            "B",
            "shift-b",
            "5",
            ",",
            "space",
            "tab",
            "shift-tab",
            "backspace",
            "left",
        ] {
            assert!(
                binding(id).protected().is_some(),
                "{id} is protected for a focused field"
            );
        }
        for id in [
            "up",
            "down",
            "pageup",
            "enter",
            "escape",
            "shift-escape",
            "ctrl-b",
            "alt-5",
            "cmd-tab",
            "ctrl-j",
        ] {
            assert!(
                binding(id).protected().is_none(),
                "{id} is a navigation key"
            );
        }
    }

    #[test]
    fn a_set_is_refused_when_two_actions_share_a_binding() {
        let mut keyboard = Keyboard::default_for_this_system();
        keyboard
            .checked_set(KeyboardAction::Back, binding("ctrl-b"))
            .unwrap();
        // Now Ctrl+B is Back's; taking it for the next result is refused
        // with the other action named.
        let refused = keyboard
            .checked_set(KeyboardAction::NextResult, binding("ctrl-b"))
            .unwrap_err();
        assert!(refused.contains("goes back"), "{refused}");
        // The refusal left what was held in place.
        assert_eq!(keyboard.binding(KeyboardAction::NextResult).id(), "down");

        // A set is also invalid when read whole from a record: the
        // field's check names the other action.
        let mut fields = keyboard.recorded();
        fields.insert("next-result".into(), "ctrl-b".into());
        let problem = Keyboard::parse(&fields).unwrap_err();
        let ctrl_b = binding("ctrl-b").to_string();
        assert!(
            problem.contains(&format!("cannot be used: {ctrl_b} already goes back")),
            "{problem}"
        );
    }

    #[test]
    fn a_binding_protected_for_a_field_is_refused() {
        let mut keyboard = Keyboard::default_for_this_system();
        let refused = keyboard
            .checked_set(KeyboardAction::NextResult, binding("j"))
            .unwrap_err();
        assert!(refused.contains("types a character"), "{refused}");
        // A record holding such a binding is invalid whole.
        let mut fields = keyboard.recorded();
        fields.insert("next-result".into(), "j".into());
        assert!(Keyboard::parse(&fields).is_err());
    }

    #[test]
    fn platform_editing_combinations_are_named() {
        let what = |id: &str| text_editing_action(&binding(id)).map(str::to_owned);
        if cfg!(target_os = "macos") {
            assert_eq!(what("cmd-a").as_deref(), Some("selects all text"));
            assert_eq!(what("alt-left").as_deref(), Some("moves by a word"));
            assert_eq!(
                what("ctrl-k").as_deref(),
                Some("deletes to the end of the line")
            );
            assert_eq!(what("ctrl-a"), None, "Ctrl+A is free on macOS");
        } else {
            assert_eq!(what("ctrl-a").as_deref(), Some("selects all text"));
            assert_eq!(what("ctrl-backspace").as_deref(), Some("deletes a word"));
            assert_eq!(what("ctrl-z").as_deref(), Some("undoes"));
            assert_eq!(what("alt-a"), None, "Alt+A is free elsewhere");
        }
        // The defaults never collide with the editing combinations.
        let defaults = Keyboard::default_for_this_system();
        for action in KeyboardAction::ALL {
            let binding = defaults.binding(action);
            assert!(
                binding.protected().is_none(),
                "{binding} is protected for a field"
            );
        }
    }

    #[test]
    fn a_record_with_missing_actions_defaults_and_unknown_ones_fail() {
        // Missing: the default.
        let mut fields = BTreeMap::new();
        fields.insert("back".into(), "ctrl-b".into());
        let keyboard = Keyboard::parse(&fields).unwrap();
        assert_eq!(keyboard.binding(KeyboardAction::Back).id(), "ctrl-b");
        assert_eq!(keyboard.binding(KeyboardAction::NextResult).id(), "down");

        // Unknown action: the whole set fails.
        fields.insert("launch".into(), "ctrl-l".into());
        assert!(Keyboard::parse(&fields).is_err());

        // A binding that is not one: the whole set fails.
        let mut fields = BTreeMap::new();
        fields.insert("back".into(), "not a binding".into());
        assert!(Keyboard::parse(&fields).is_err());
    }

    #[test]
    fn panes_keys_hold_the_fixed_keys_and_the_bindings_in_force() {
        use crate::host_settings::NavigationBindings;

        let keys = PaneKeys::default();
        for fixed in [
            "escape",
            "ctrl-k",
            "up",
            "down",
            "enter",
            "ctrl-enter",
            "ctrl-shift-enter",
            "ctrl-1",
            "ctrl-0",
        ] {
            assert!(keys.taken(&binding(fixed)).is_some(), "{fixed}");
        }
        assert!(keys.taken(&pin_key()).is_some());
        assert_eq!(keys.taken(&binding("ctrl-shift-k")), None);
        assert_eq!(keys.taken(&binding("ctrl-shift-c")), None);

        // A rebound action's new key is Pane's; its old one is free again
        // unless it is a fixed key.
        let mut keyboard = Keyboard::default_for_this_system();
        let dismiss = keyboard.binding(KeyboardAction::DismissLauncher).clone();
        keyboard
            .checked_set(KeyboardAction::DismissLauncher, binding("ctrl-shift-y"))
            .unwrap();
        let keys = PaneKeys::new(&keyboard, NavigationBindings::None);
        assert_eq!(
            keys.taken(&binding("ctrl-shift-y")),
            Some("dismisses the launcher")
        );
        assert_eq!(keys.taken(&dismiss), None);

        // The navigation bindings' extra keys.
        let keys = PaneKeys::new(&keyboard, NavigationBindings::Emacs);
        assert!(keys.taken(&binding("ctrl-n")).is_some());
    }

    #[test]
    fn the_action_chords_are_ctrl_enter_and_ctrl_shift_enter() {
        assert_eq!(action_key(0), None);
        assert_eq!(action_key(1).unwrap().id(), "ctrl-enter");
        assert_eq!(action_key(2).unwrap().id(), "ctrl-shift-enter");
        assert_eq!(action_key(3), None);
    }
}
