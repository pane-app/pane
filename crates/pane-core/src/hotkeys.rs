//! Global hotkeys: a shortcut the user assigns to an installed command,
//! which opens that command in Pane's window even while another application
//! has focus.
//!
//! The launcher decides which shortcuts are bound and which should be
//! registered; the system is reached through one small trait, [`Hotkeys`],
//! with one adapter per system, chosen by [`native`]:
//!
//! - Windows: `RegisterHotKey` on a thread of Pane's own for the chords
//!   Windows accepts, and a low-level keyboard hook of Pane's own
//!   (`WH_KEYBOARD_LL`) for the chords Windows refuses — another
//!   application has the shortcut, or Windows keeps it — and for the
//!   binding kinds no registration can express: a lone tap or a double
//!   tap of a modifier, a side-specific modifier, a numpad key (#260) —
//!   so a refused shortcut is never an error ([`windows`], ADR 0039);
//! - macOS: Carbon's `RegisterEventHotKey`, through the `global-hotkey`
//!   crate, which needs no Accessibility permission ([`macos`]);
//! - Linux on X11: a passive key grab on the root window (`XGrabKey`)
//!   ([`X11Hotkeys`]). Wayland has no way for an application to watch keys
//!   pressed in other applications except the desktop portal's global
//!   shortcuts, which Pane does not use yet, so there the adapter explains
//!   that hotkeys are unavailable.
//!
//! An adapter reports each press of a registered shortcut to a
//! [`PressSender`]; the window reads them from the matching [`Presses`] and
//! asks the launcher to open the bound command. How a shortcut is
//! dispatched — the system's registration or Pane's keyboard hook — is the
//! adapter's to say ([`Hotkeys::route`]), and the binding's row shows it
//! ([`Route::note_on`]). The hook's own state is the adapter's to say
//! too ([`Hotkeys::hook_health`]), and the Settings pages and Copy
//! Diagnostics show it ([`HookHealth::note`]).
//!
//! The binding kinds #260 adds — the tap kinds, named sides, the extended
//! key set — work wherever the adapter has a hook to recognize them, which
//! is Windows; the other systems explain them ([`kinds_unavailable`]), so
//! a record copied from a Windows machine is not mysterious. A recorder
//! that listens asks the adapter for a recording session
//! ([`Hotkeys::recording`]), which holds the keys back from the system
//! while it lasts, so those kinds can be recorded without the system
//! acting on them (the Start menu the Windows key alone opens). The
//! Windows key alone is also the fresh-install Open Pane default on
//! Windows, where the adapter can take it (ADR 0039, #268; see
//! [`Hotkeys::open_pane_fresh_default`]); where it cannot, the default
//! stays today's, with the reason.

use std::fmt;
use std::sync::Arc;

use crate::platform::Platform;

/// The tag Pane puts in the extra information of every key it injects
/// (SendInput's `dwExtraInfo`): "PANE". Pane's keyboard hook passes
/// tagged events through untouched, so Pane never reacts to its own
/// input — the Start-menu mask, a paste, a simulated copy — and the tools
/// that remap keys can tell Pane's injections from the user's. Keys
/// another tool injects carry that tool's marker, or none.
pub const INJECTED_TAG: usize = 0x50414E45;

/// The binding recognizer: the pure state machine that decides, from the
/// key events the Windows hook sees, whether one of the bound bindings was
/// pressed, and what a recording session recognized. Compiled on every
/// system, so its logic is tested everywhere.
mod recognizer;
pub use recognizer::{Binding, Decision, KeyEvent, Recognizer, Recorded};

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::X11Hotkeys;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsHotkeys;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacHotkeys;

/// The keys a hotkey can end with.
const FUNCTION_KEYS: u8 = 24;

/// The four modifiers a binding can name, as the record writes them, in
/// the order [`Shortcut`] holds them: the name a chord's id names the
/// modifier by when no side is named, and the base of its side forms
/// and of the tap kinds (`lctrl`, `ralt`, `tap:win`).
const MODIFIERS: [(&str, &str); 4] = [
    ("ctrl", "ctrl"),
    ("alt", "alt"),
    ("shift", "shift"),
    ("super", "win"),
];

/// The named keys a chord may end with (#260): Space, Enter, Tab, the
/// arrows, Home, End, Page Up, Page Down, Insert and Delete.
const NAMED_KEYS: [&str; 13] = [
    "space", "enter", "tab", "left", "right", "up", "down", "home", "end", "pageup", "pagedown",
    "insert", "delete",
];

/// The numpad's keys, distinct from their counterparts (#260): the
/// digits, the arithmetic keys, the decimal point and the numpad's own
/// Enter.
const NUMPAD_KEYS: [&str; 16] = [
    "numpad0",
    "numpad1",
    "numpad2",
    "numpad3",
    "numpad4",
    "numpad5",
    "numpad6",
    "numpad7",
    "numpad8",
    "numpad9",
    "numpad_add",
    "numpad_decimal",
    "numpad_divide",
    "numpad_enter",
    "numpad_multiply",
    "numpad_subtract",
];

/// The punctuation keys a chord may end with, by the character the US
/// layout's unshifted key types (#260).
const PUNCTUATION: &str = ",./;'`[]\\-=";

/// Which of a modifier's two keys a binding names (#260): either of
/// them, or one named side, where the binding must not take the other
/// key's presses too — Right Ctrl, Left Alt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// Either of the modifier's two keys.
    Any,
    /// The left key.
    Left,
    /// The right key.
    Right,
}

/// What a binding is (#260): a chord, or a lone tap or a double tap of
/// one modifier — the kinds no system registration can express, which
/// Pane's own keyboard hook recognizes (Windows, ADR 0039).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Modifier keys held while one other key is pressed.
    Chord,
    /// One modifier pressed and released with no other key between.
    Tap,
    /// One modifier pressed twice with nothing between.
    Double,
}

/// A key combination: a chord — modifier keys, each any side or a named
/// one, held while one other key is pressed — or a lone tap or a double
/// tap of one modifier (#260).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    kind: Kind,
    /// Whether control, alt, shift and the Windows key are part of the
    /// binding, and which side of each it names, in that order: `None`
    /// for not part. A tap or a double tap names exactly one.
    modifiers: [Option<Side>; 4],
    /// The key a chord ends with; empty for the tap kinds, which name
    /// their modifier in `modifiers`.
    key: String,
}

impl Shortcut {
    /// The shortcut of the modifiers held and `key`, as the window
    /// reports a key press: a letter, a digit, `f1` to `f24`, one of the
    /// named keys (Space, Enter, Tab, the arrows, Home, End, Page Up,
    /// Page Down, Insert, Delete), a punctuation key by its US-layout
    /// character, or a numpad key, in any case (#260). Another key is
    /// explained.
    pub fn new(
        control: bool,
        alt: bool,
        shift: bool,
        super_key: bool,
        key: &str,
    ) -> Result<Shortcut, String> {
        let named = key_of(&key.trim().to_ascii_lowercase()).ok_or_else(|| {
            format!(
                "Pane cannot use {} in a hotkey: end it with a letter, a digit, F1 to F24, \
                 Space, Enter, Tab, an arrow, a punctuation key, a navigation key or a numpad \
                 key",
                key_name(key)
            )
        })?;
        // Ctrl+Alt+Delete, the one shortcut the key set cannot express
        // that is refused for a reason of its own: Windows keeps it on
        // the secure screen, and no program can take it. On any system,
        // so the reason is tested everywhere.
        if named == "delete" && control && alt && !shift && !super_key {
            return Err("Ctrl+Alt+Delete is reserved: no program can intercept it".into());
        }
        Ok(Shortcut {
            kind: Kind::Chord,
            modifiers: [held(control), held(alt), held(shift), held(super_key)],
            key: named,
        })
    }

    /// The Open Pane default where the Windows key alone is not taken
    /// (#268, ADR 0039): Ctrl+Alt+Space on Windows and Linux, Option+Space
    /// on macOS. The parent specification records it as a synthesis
    /// default, not a separately confirmed product decision: it stays
    /// clear of the combinations each system keeps for itself (the
    /// Windows key, Spotlight, the window menu) and of plain typing, and
    /// the General page can change it. On Windows a fresh data folder
    /// starts with the Windows key alone instead, where the adapter's
    /// own keyboard hook can recognize the tap — that is
    /// [`Hotkeys::open_pane_fresh_default`]'s to decide; this remains
    /// the default where it cannot, and what an existing record keeps.
    pub fn open_pane_default() -> Shortcut {
        Shortcut::open_pane_default_on(Platform::current())
    }

    /// [`Shortcut::open_pane_default`] as `platform` names it, so every
    /// system's default is decided and tested on every system:
    /// Option+Space on macOS, Ctrl+Alt+Space elsewhere.
    fn open_pane_default_on(platform: Option<Platform>) -> Shortcut {
        if platform == Some(Platform::Macos) {
            Shortcut::parse("alt+space").expect("a valid default")
        } else {
            Shortcut::parse("ctrl+alt+space").expect("a valid default")
        }
    }

    /// The Windows key tapped alone — the fresh-install Open Pane default
    /// on Windows, where the adapter's own keyboard hook can recognize
    /// the tap (#268, ADR 0039), as Raycast's fresh installs are: Pane
    /// replaces the Start menu as the place the user starts everything.
    /// The General page's "Use the Windows key" choice sets it in one
    /// step; while it is bound, Win+E, Win+D, Win+L and the rest keep
    /// Windows' meaning, and the Start menu stays reachable from the
    /// taskbar's Start button and Ctrl+Esc.
    pub fn windows_key() -> Shortcut {
        Shortcut::parse("tap:win").expect("a valid default")
    }

    /// Reads a shortcut as [`Shortcut::id`] writes it: a chord such as
    /// `ctrl+alt+p`, with a side prefix where a side is named
    /// (`rctrl+alt+g`, `lwin+space`), or a tap or a double tap of one
    /// modifier (`tap:win`, `tap:rctrl`, `double:ctrl`) (#260). The
    /// modifiers may name their synonyms (`control`, `option`, `super`,
    /// `win`, `cmd`, `command`, each with an `l` or an `r` prefix for a
    /// side); the key is whatever ends the id.
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        let trimmed = text.trim();
        let lowered = trimmed.to_ascii_lowercase();
        if let Some(named) = lowered.strip_prefix("tap:") {
            return Shortcut::tapped(text, named, Kind::Tap);
        }
        if let Some(named) = lowered.strip_prefix("double:") {
            return Shortcut::tapped(text, named, Kind::Double);
        }
        let mut parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
        let key = parts.pop().filter(|key| !key.is_empty());
        let Some(key) = key else {
            return Err(format!("“{text}” names no key"));
        };
        let mut modifiers = [None; 4];
        for part in parts {
            let Some((at, side)) = modifier_of(part) else {
                return Err(format!("“{text}”: {part} is not a modifier key"));
            };
            if modifiers[at].is_some() {
                let name = match at {
                    0 => "control",
                    1 => "alt",
                    2 => "shift",
                    _ => "the Windows key",
                };
                return Err(format!("“{text}”: {name} is named twice"));
            }
            modifiers[at] = Some(side);
        }
        // The key takes `new`'s own checks, then the sides the text named
        // go back over the either-side modifiers `new` wrote.
        let chord = Shortcut::new(
            modifiers[0].is_some(),
            modifiers[1].is_some(),
            modifiers[2].is_some(),
            modifiers[3].is_some(),
            key,
        )?;
        Ok(Shortcut { modifiers, ..chord })
    }

    /// The tap or the double tap of the modifier `named`, which
    /// [`Shortcut::parse`] read after its `tap:` or `double:` prefix.
    fn tapped(text: &str, named: &str, kind: Kind) -> Result<Shortcut, String> {
        let Some((at, side)) = modifier_of(named) else {
            return Err(format!("“{text}”: {named} is not a modifier key"));
        };
        let mut modifiers = [None; 4];
        modifiers[at] = Some(side);
        Ok(Shortcut {
            kind,
            modifiers,
            key: String::new(),
        })
    }

    /// The same text on every system, for records: the modifiers in a
    /// fixed order — each by its name, or with an `l` or an `r` prefix
    /// where a side is named — then the key, such as `ctrl+alt+p` or
    /// `rctrl+alt+p`; or, for the tap kinds, `tap:win`, `tap:rctrl` or
    /// `double:ctrl` (#260).
    pub fn id(&self) -> String {
        let parts: Vec<String> = match self.kind {
            Kind::Chord => self
                .modifiers
                .iter()
                .zip(MODIFIERS)
                .filter_map(|(&side, (chord, base))| {
                    side.map(|side| match side {
                        Side::Any => chord.to_owned(),
                        _ => named_modifier(base, side),
                    })
                })
                .chain(std::iter::once(self.key.clone()))
                .collect(),
            kind => {
                let (at, side) = self.lone().expect("a tap names one modifier");
                vec![format!(
                    "{}:{}",
                    match kind {
                        Kind::Tap => "tap",
                        _ => "double",
                    },
                    named_modifier(MODIFIERS[at].1, side)
                )]
            }
        };
        parts.join("+")
    }

    pub fn control(&self) -> bool {
        self.modifiers[0].is_some()
    }

    pub fn alt(&self) -> bool {
        self.modifiers[1].is_some()
    }

    pub fn shift(&self) -> bool {
        self.modifiers[2].is_some()
    }

    /// Whether the Windows key (Windows), Command (macOS) or Super (Linux)
    /// is held.
    pub fn super_key(&self) -> bool {
        self.modifiers[3].is_some()
    }

    /// What the binding is: a chord, or a lone tap or a double tap of
    /// one modifier (#260).
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Whether each of control, alt, shift and the Windows key is part
    /// of the binding, and which side of it the binding names, in that
    /// order: `None` for not part.
    pub fn sides(&self) -> [Option<Side>; 4] {
        self.modifiers
    }

    /// The modifier a tap or a double tap names, and which side of it,
    /// as its position among the four [`Shortcut`] holds; `None` for a
    /// chord.
    pub(crate) fn lone(&self) -> Option<(usize, Side)> {
        if self.kind == Kind::Chord {
            return None;
        }
        let at = self.modifiers.iter().position(|side| side.is_some())?;
        Some((at, self.modifiers[at]?))
    }

    /// `a` to `z`, `0` to `9`, a punctuation key, a named key, `f1` to
    /// `f24` or a numpad key; empty for the tap kinds, which name their
    /// modifier.
    pub fn key(&self) -> &str {
        &self.key
    }

    fn modifiers(&self) -> [(Option<Side>, &'static str); 4] {
        [
            (self.modifiers[0], "ctrl"),
            (self.modifiers[1], "alt"),
            (self.modifiers[2], "shift"),
            (self.modifiers[3], "super"),
        ]
    }

    /// Why Pane does not bind this shortcut on this system, if it does
    /// not: it has no Ctrl, Alt or Super (Command) key, so it would take
    /// over typing, or no program can intercept it (Win+L on Windows,
    /// Ctrl+Alt+Delete everywhere). A lone or double tap of a modifier
    /// needs no such guard — it is a modifier, so it takes over no
    /// typing (#260). On Windows a shortcut the system keeps for itself
    /// is no longer refused: Pane's own keyboard hook takes it instead
    /// and its row warns what Windows does with it (#252, ADR 0039).
    pub fn refusal(&self) -> Option<String> {
        self.refusal_on(Platform::current())
    }

    /// Why Pane does not bind this shortcut on `platform`, if it does
    /// not; the rules are named by platform so every system's are tested
    /// on every system, and the shortcut is named as that platform names
    /// it (see [`Shortcut::display_on`]).
    fn refusal_on(&self, platform: Option<Platform>) -> Option<String> {
        // A lone or double tap of a modifier needs no guard: it is a
        // modifier, so it takes over no typing (#260).
        if self.kind == Kind::Chord && !(self.control() || self.alt() || self.super_key()) {
            let modifiers = match platform {
                Some(Platform::Macos) => "Control, Option or Command",
                Some(Platform::Windows) => "Ctrl, Alt or the Windows key",
                _ => "Ctrl, Alt or Super",
            };
            return Some(format!(
                "{} needs {modifiers}, so that it does not take over typing",
                self.display_on(platform)
            ));
        }
        reserved(platform)
            .iter()
            .find(|(id, _)| Shortcut::parse(id).is_ok_and(|reserved| reserved == *self))
            .map(|(_, why)| format!("{} is reserved: {why}", self.display_on(platform)))
            .or_else(|| self.editing_refusal(platform))
    }

    /// What the system does with this shortcut while Pane does not take
    /// it first, where `platform` keeps it for itself and Pane's own
    /// keyboard hook can take it (Windows, #252): the warning a
    /// hook-dispatched binding's row shows beside its route. `None` where
    /// the system does not keep it.
    fn kept_by(&self, platform: Option<Platform>) -> Option<&'static str> {
        kept(platform)
            .iter()
            .find(|(id, _)| Shortcut::parse(id).is_ok_and(|kept| kept == *self))
            .map(|(_, why)| *why)
    }

    /// Ctrl (Command on macOS) with only a letter or digit: every
    /// application uses those for its own commands, such as copying.
    fn editing_refusal(&self, platform: Option<Platform>) -> Option<String> {
        let primary = if platform == Some(Platform::Macos) {
            self.super_key() && !self.control()
        } else {
            self.control() && !self.super_key()
        };
        let single = primary && !self.alt() && !self.shift();
        let character = self.key.len() == 1;
        (single && character).then(|| {
            format!(
                "{} is used by applications for their own commands, such as copying; add Alt \
                 or Shift",
                self.display_on(platform)
            )
        })
    }
}

/// Shortcuts no program can intercept, which stay refused, each with why,
/// on `platform`.
fn reserved(platform: Option<Platform>) -> &'static [(&'static str, &'static str)] {
    match platform {
        Some(Platform::Windows) => &[(
            "super+l",
            "Windows locks the computer with it, and no program can intercept it",
        )],
        Some(Platform::Macos) => &[
            ("super+space", "macOS opens Spotlight with it"),
            ("ctrl+space", "macOS switches input sources with it"),
            ("super+alt+space", "macOS opens a Finder search with it"),
            ("shift+super+3", "macOS takes screenshots with it"),
            ("shift+super+4", "macOS takes screenshots with it"),
            ("shift+super+5", "macOS takes screenshots with it"),
            ("ctrl+super+q", "macOS locks the screen with it"),
        ],
        _ => &[
            ("alt+f4", "desktops close the active window with it"),
            ("super+l", "desktops lock the screen with it"),
            ("ctrl+alt+f1", "Linux switches to another console with it"),
            ("ctrl+alt+f2", "Linux switches to another console with it"),
            ("ctrl+alt+f3", "Linux switches to another console with it"),
            ("ctrl+alt+f4", "Linux switches to another console with it"),
            ("ctrl+alt+f5", "Linux switches to another console with it"),
            ("ctrl+alt+f6", "Linux switches to another console with it"),
            ("ctrl+alt+f7", "Linux switches to another console with it"),
            ("ctrl+alt+f8", "Linux switches to another console with it"),
            ("ctrl+alt+f9", "Linux switches to another console with it"),
            ("ctrl+alt+f10", "Linux switches to another console with it"),
            ("ctrl+alt+f11", "Linux switches to another console with it"),
            ("ctrl+alt+f12", "Linux switches to another console with it"),
        ],
    }
}

/// Shortcuts the system keeps for itself that Pane still takes through
/// its own keyboard hook (Windows, #252), each with what the system does
/// with the shortcut while Pane does not take it first — the warning a
/// hook-dispatched binding's row shows. Windows refuses to register these
/// for any application, but the hook recognizes the chord all the same,
/// so they are warnings rather than refusals. Empty everywhere but
/// Windows: the other systems' adapters have no hook, so their kept
/// shortcuts stay refused (see [`reserved`]).
fn kept(platform: Option<Platform>) -> &'static [(&'static str, &'static str)] {
    match platform {
        Some(Platform::Windows) => &[
            ("alt+f4", "Windows closes the active window with it"),
            ("super+d", "Windows shows the desktop with it"),
            ("super+e", "Windows opens File Explorer with it"),
            ("super+r", "Windows opens Run with it"),
            ("alt+space", "Windows opens the window menu with it"),
        ],
        _ => &[],
    }
}

/// People's name for `key`: `P`, `5`, `F5`, `Space`, `Page Up`,
/// `Num 5`, `,` (#260).
fn key_name(key: &str) -> String {
    match key {
        "space" => "Space".into(),
        "enter" => "Enter".into(),
        "tab" => "Tab".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "Page Up".into(),
        "pagedown" => "Page Down".into(),
        "insert" => "Insert".into(),
        "delete" => "Delete".into(),
        "numpad_add" => "Num +".into(),
        "numpad_decimal" => "Num .".into(),
        "numpad_divide" => "Num /".into(),
        "numpad_enter" => "Num Enter".into(),
        "numpad_multiply" => "Num *".into(),
        "numpad_subtract" => "Num -".into(),
        key if key.starts_with("numpad") => {
            let digit = key.strip_prefix("numpad").expect("the numpad's digit");
            format!("Num {digit}")
        }
        key if key.starts_with('f') && key.len() > 1 => key.to_ascii_uppercase(),
        key => {
            let mut chars = key.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

/// People's names on this system: "Ctrl+Alt+P" on Linux and Windows
/// (with "Super" or "Win"), "Control+Option+Command+P" on macOS; a named
/// side writes "Right Ctrl" or "Left Win"; a lone tap is its modifier
/// ("Win", "Right Ctrl") and a double tap the name twice ("Ctrl Ctrl")
/// (#260), as Windows names them.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_on(Platform::current()))
    }
}

impl Shortcut {
    /// The shortcut as `platform` names it — [`fmt::Display`]'s names
    /// with that platform's own for the Windows key — for a message that
    /// names a platform it is about, as the refusals and warnings a
    /// shortcut's row shows do: what Windows refuses reads "Win+L" on
    /// every system, not as the system running the message spells it.
    fn display_on(&self, platform: Option<Platform>) -> String {
        let names: [&str; 4] = match platform {
            Some(Platform::Macos) => ["Control", "Option", "Shift", "Command"],
            Some(Platform::Windows) => ["Ctrl", "Alt", "Shift", "Win"],
            _ => ["Ctrl", "Alt", "Shift", "Super"],
        };
        let mut shown = String::new();
        match self.kind {
            Kind::Chord => {
                for ((held, _), name) in self.modifiers().into_iter().zip(names) {
                    if let Some(side) = held {
                        shown.push_str(side_prefix(side));
                        shown.push_str(name);
                        shown.push('+');
                    }
                }
                shown.push_str(&key_name(&self.key));
            }
            Kind::Tap => {
                let (at, side) = self.lone().expect("a tap names one modifier");
                shown.push_str(side_prefix(side));
                shown.push_str(names[at]);
            }
            Kind::Double => {
                let (at, side) = self.lone().expect("a double tap names one modifier");
                shown.push_str(side_prefix(side));
                shown.push_str(names[at]);
                shown.push(' ');
                shown.push_str(side_prefix(side));
                shown.push_str(names[at]);
            }
        }
        shown
    }
}

/// The name of `side` as it prefixes a modifier's: "Left ", "Right ",
/// or nothing for either.
fn side_prefix(side: Side) -> &'static str {
    match side {
        Side::Any => "",
        Side::Left => "Left ",
        Side::Right => "Right ",
    }
}

/// The modifier `part` names and which side of it, as its position among
/// the four [`Shortcut`] holds and the side ([`Side`]): `ctrl` (either
/// side), `rctrl`, `lalt`, `super`, `win`, `rcommand` and the other names
/// and their synonyms, each with an `l` or an `r` prefix for a side
/// (#260). `None` for any other part.
fn modifier_of(part: &str) -> Option<(usize, Side)> {
    let part = part.trim().to_ascii_lowercase();
    for (prefix, side) in [("l", Side::Left), ("r", Side::Right)] {
        if let Some((at, _)) = part.strip_prefix(prefix).and_then(modifier_position) {
            return Some((at, side));
        }
    }
    modifier_position(&part).map(|(at, _)| (at, Side::Any))
}

/// The modifier `name` names, without a side prefix, as its position
/// among the four [`Shortcut`] holds and the name the record writes the
/// side forms and the tap kinds by.
fn modifier_position(name: &str) -> Option<(usize, &'static str)> {
    Some(match name {
        "ctrl" | "control" => (0, "ctrl"),
        "alt" | "option" => (1, "alt"),
        "shift" => (2, "shift"),
        "super" | "win" | "cmd" | "command" => (3, "win"),
        _ => return None,
    })
}

/// The record's name for `side` of the modifier the chord names `chord`
/// (such as `super`) when no side is named, and the tap kinds and side
/// forms name `base` (such as `win`): `super` itself, `win`, `lctrl`,
/// `ralt` (#260).
fn named_modifier(name: &str, side: Side) -> String {
    match side {
        Side::Any => name.to_owned(),
        Side::Left => format!("l{name}"),
        Side::Right => format!("r{name}"),
    }
}

/// The side `held` names: either of the modifier's two keys, or `None`
/// for not held.
fn held(held: bool) -> Option<Side> {
    held.then_some(Side::Any)
}

/// The key `key` names, as the record writes it: a letter, a digit, one
/// of the named keys, F1 to F24, a numpad key, or a punctuation key by
/// the character the US layout's unshifted key types — or by its name
/// (`comma`), as a record written by hand names it. A character a
/// punctuation key types only with Shift held reads as that key, so a
/// binding recorded with Shift held names the key rather than the
/// character. `None` for anything else (#260).
fn key_of(key: &str) -> Option<String> {
    if let Some(base) = unshifted(key) {
        return Some(base.to_owned());
    }
    if NAMED_KEYS.contains(&key) || NUMPAD_KEYS.contains(&key) {
        return Some(key.to_owned());
    }
    const NAMED_PUNCTUATION: [(&str, &str); 11] = [
        ("comma", ","),
        ("period", "."),
        ("slash", "/"),
        ("semicolon", ";"),
        ("apostrophe", "'"),
        ("grave", "`"),
        ("bracketleft", "["),
        ("bracketright", "]"),
        ("backslash", "\\"),
        ("minus", "-"),
        ("equal", "="),
    ];
    if let Some((_, character)) = NAMED_PUNCTUATION.iter().find(|(name, _)| *name == key) {
        return Some((*character).to_owned());
    }
    match key.as_bytes() {
        [c] if c.is_ascii_lowercase() || c.is_ascii_digit() => Some(key.to_owned()),
        [c] if PUNCTUATION.as_bytes().contains(c) => Some(key.to_owned()),
        _ => {
            let number: u32 = key.strip_prefix('f')?.parse().ok()?;
            if (1..=u32::from(FUNCTION_KEYS)).contains(&number) {
                Some(format!("f{number}"))
            } else {
                None
            }
        }
    }
}

/// The key the character `character` types only with Shift held: the US
/// layout's base key that types it with Shift (`<` is Shift+Comma), so a
/// binding names the key. `None` for a character the layout types
/// without Shift.
fn unshifted(character: &str) -> Option<&'static str> {
    Some(match character {
        "<" => ",",
        ">" => ".",
        "?" => "/",
        ":" => ";",
        "\"" => "'",
        "~" => "`",
        "{" => "[",
        "}" => "]",
        "|" => "\\",
        "_" => "-",
        "+" => "=",
        _ => return None,
    })
}

/// Why the binding kinds `shortcut` needs cannot be used on `platform`,
/// if they cannot: they need Pane's own keyboard hook, which only
/// Windows' adapter has (#260) — a lone tap or a double tap of a
/// modifier, a side-specific modifier, and a key beyond letters, digits,
/// F1 to F12 and Space, the numpad's among them. On Windows the answer
/// is `None`: the hook is that platform's own, and the [`Hotkeys`]
/// trait's [`Hotkeys::kind_unavailable`] default, which reads this with
/// the current system, agrees there — Windows' adapter overrides that
/// method to say nothing is outright all the same. The launcher checks
/// this before registering, and the binding's row shows the reason where
/// a record names a kind this system cannot take.
pub fn kinds_unavailable(shortcut: &Shortcut, platform: Option<Platform>) -> Option<String> {
    if platform == Some(Platform::Windows) {
        // Every kind works on Windows, whose adapter has the hook.
        return None;
    }
    let here = match platform {
        Some(platform) => platform.to_string(),
        None => format!("this system ({})", std::env::consts::OS),
    };
    match shortcut.kind() {
        Kind::Tap => Some(format!(
            "Not available on {here}: lone modifier taps work only on Windows for now"
        )),
        Kind::Double => Some(format!(
            "Not available on {here}: double-tap hotkeys work only on Windows for now"
        )),
        Kind::Chord => {
            let mut needs: Vec<String> = Vec::new();
            if shortcut
                .sides()
                .iter()
                .any(|side| matches!(side, Some(Side::Left | Side::Right)))
            {
                needs.push("side-specific modifiers".into());
            }
            if beyond_the_base_keys(shortcut.key()) {
                needs.push("keys beyond letters, digits, F1 to F12 and Space".into());
            }
            (!needs.is_empty()).then(|| {
                format!(
                    "Not available on {here}: {} work only on Windows for now",
                    crate::platform::join(&needs)
                )
            })
        }
    }
}

/// Whether `key` is one the base set a system's own registration
/// offered — a letter, a digit, F1 to F12, or Space — rather than one
/// the extended key set added (#260).
fn beyond_the_base_keys(key: &str) -> bool {
    if key == "space" {
        return false;
    }
    match key.as_bytes() {
        [c] if c.is_ascii_lowercase() || c.is_ascii_digit() => false,
        _ => match key
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
        {
            Some(number) => number > 12,
            None => true,
        },
    }
}

/// Why the system did not register a shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyError {
    /// Another application, or the system, already uses it.
    Taken,
    /// The system refused it, with its reason.
    Refused(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotkeyError::Taken => f.write_str("another application or the system already uses it"),
            HotkeyError::Refused(reason) => write!(f, "the system refused it: {reason}"),
        }
    }
}

/// Registers shortcuts with the system, so that pressing one is reported
/// even while another application has focus. Presses go to the
/// [`PressSender`] the adapter was made with.
///
/// The launcher calls it on the window's thread (macOS registers hotkeys
/// with the main run loop). Pane keeps one adapter for the whole process;
/// dropping a system adapter releases every shortcut it registered and
/// stops its thread (Windows, X11), so tests and a replaced adapter leave no
/// registration behind.
pub trait Hotkeys: Send + Sync + 'static {
    /// Why global hotkeys cannot be used here at all, such as on Wayland;
    /// `None` when they can.
    fn unavailable(&self) -> Option<String>;

    /// Registers `shortcut` system-wide. On Windows a registration the
    /// system refuses — another application has the shortcut, or Windows
    /// keeps it — is not an error: the adapter falls back to its own
    /// keyboard hook and answers `Ok` (ADR 0039), so only what no adapter
    /// can take is refused.
    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError>;

    /// Releases `shortcut`, registered earlier, so that other applications
    /// may use it and it is no longer reported.
    fn unregister(&self, shortcut: &Shortcut);

    // The rest is the seam the keyboard hook fallback adds (#252): until
    // an adapter answers them, each says the system's registration and
    // that no hook is in use.

    /// How `shortcut`, registered earlier, is dispatched: through the
    /// system's own registration, or through Pane's own keyboard hook,
    /// where the system refused the registration or cannot express the
    /// binding (Windows, #252). Asked once a registration succeeded, so
    /// the binding's row can say which — [`Route::note_on`].
    fn route(&self, _shortcut: &Shortcut) -> Route {
        Route::System
    }

    /// The state of Pane's own keyboard hook, where this system's adapter
    /// uses one; `None` where none is, which is everywhere but Windows'
    /// adapter with a binding the system refused. The Settings pages and
    /// Copy Diagnostics show it (#259).
    fn hook_health(&self) -> Option<HookHealth> {
        None
    }

    // The seam the binding kinds add (#260): an adapter without a hook of
    // its own — every system's but Windows' — answers that the kinds are
    // unavailable and offers no recording session.

    /// Why `shortcut` cannot be bound on this system, if it cannot: the
    /// binding kinds that need Pane's own keyboard hook — a lone tap or a
    /// double tap of a modifier, a side-specific modifier, and a key
    /// beyond letters, digits, F1 to F12 and Space, the numpad's among
    /// them — work only where the adapter has one, which is Windows
    /// (#260). `None` when the system can bind it; the default explains
    /// every such kind where the platform has no hook, answering `None`
    /// on Windows, whose adapter says nothing is outright by overriding
    /// this. The launcher checks this before registering, and the
    /// binding's row shows the reason where a record names a kind this
    /// system cannot take.
    fn kind_unavailable(&self, shortcut: &Shortcut) -> Option<String> {
        kinds_unavailable(shortcut, Platform::current())
    }

    /// Starts a recording session for a recorder that is about to listen
    /// (#260): while the session lasts, the adapter holds the keys back
    /// from the system — the Start menu the Windows key alone opens stays
    /// closed — and reports the bindings the user pressed, so the kinds
    /// no system registration can express are recorded as easily as a
    /// chord. `None` where this system's adapter has no hook to hold
    /// keys back with (macOS, X11): the recorder records through the
    /// window's own keys, as it does today, and offers none of the new
    /// kinds. Only Windows' adapter answers one.
    fn recording(&self) -> Option<RecordingSession> {
        None
    }

    /// The Open Pane hotkey a fresh data folder on `platform` starts
    /// with, as this adapter can register it (#268, ADR 0039): the
    /// Windows key alone on Windows — Pane replaces the Start menu as
    /// the place the user starts everything — where this adapter's own
    /// keyboard hook can recognize the tap, and
    /// [`Shortcut::open_pane_default`] where it cannot, as on the other
    /// systems. The platform is named, so every system's default is
    /// decided and tested on every system; whether the folder is fresh
    /// is the caller's to know (the host settings' record does), and an
    /// existing record keeps the hotkey it holds wherever this is
    /// asked.
    ///
    /// The answer carries why the Windows key alone was not taken, where
    /// the platform is Windows and the adapter cannot recognize the tap:
    /// today's default is registered instead, and the reason is the
    /// Settings page's to show. `None` where the Windows key was taken,
    /// and where it was never the default (the other systems).
    fn open_pane_fresh_default(&self, platform: Option<Platform>) -> FreshOpenPane {
        if platform != Some(Platform::Windows) {
            // The other systems keep their defaults; the Windows key
            // alone was never a candidate, so there is nothing to
            // explain.
            return FreshOpenPane {
                shortcut: Shortcut::open_pane_default_on(platform),
                why_not_windows_key: None,
            };
        }
        let windows_key = Shortcut::windows_key();
        match self.kind_unavailable(&windows_key) {
            // The adapter's own keyboard hook recognizes the tap, as
            // Windows' does: the fresh install opens Pane with the
            // Windows key alone.
            None => FreshOpenPane {
                shortcut: windows_key,
                why_not_windows_key: None,
            },
            // The adapter has no hook to recognize a tap with (the
            // tests' fakes model one): today's default is registered
            // instead, and the reason says why the Windows key alone
            // was not taken.
            Some(reason) => FreshOpenPane {
                shortcut: Shortcut::open_pane_default_on(platform),
                why_not_windows_key: Some(format!(
                    "The Windows key alone is not the default: {reason}"
                )),
            },
        }
    }
}

/// The Open Pane hotkey a fresh data folder starts with, as
/// [`Hotkeys::open_pane_fresh_default`] decides it (#268, ADR 0039):
/// the Windows key alone on Windows where the adapter's own keyboard
/// hook can recognize the tap, today's default elsewhere or where it
/// cannot — with why the Windows key alone was not taken, where the
/// platform is Windows and the adapter cannot, for the Settings page to
/// show beside the binding that stands in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshOpenPane {
    /// The hotkey itself: what a fresh data folder's Open Pane binding
    /// comes to hold, and what registers.
    pub shortcut: Shortcut,
    /// Why the Windows key alone was not taken, where the platform is
    /// Windows and the adapter cannot recognize the tap: today's default
    /// stands in, and this says why. `None` where the Windows key was
    /// taken, and where it was never the default (the other systems).
    pub why_not_windows_key: Option<String>,
}

/// A recording session with the hotkeys adapter (#260): while one
/// lasts, the adapter holds the keys back from the system and reports
/// the bindings the user presses — a chord with its sides, a lone tap
/// of a modifier, a double tap — as [`Hotkeys::recording`] starts one.
/// A recorder takes its reports and runs the same checks a keystroke
/// recorded through the window does; the kinds no registration can
/// express are what a session is for. The reports end when the session
/// does: the recorder stops listening (its [`RecordingStop`] half is
/// dropped), the window loses focus, or Pane quits.
///
/// [`split`] divides it into the half the recorder keeps — whose drop
/// ends the session — and the half a task consumes the reports through,
/// so the recorder can stop listening without waiting for a key.
///
/// [`split`]: RecordingSession::split
pub struct RecordingSession {
    reports: Option<Presses>,
    stop: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl RecordingSession {
    /// The next binding the user pressed, waiting for it; `None` once
    /// the session ended: the recorder stopped listening, the window
    /// lost focus, or Pane quit.
    pub async fn next(&mut self) -> Option<Shortcut> {
        self.reports.as_mut()?.next().await
    }

    /// A binding already pressed, without waiting.
    pub fn try_next(&mut self) -> Option<Shortcut> {
        self.reports.as_mut()?.try_next()
    }

    /// Divides the session: the reports, for the task that consumes
    /// them, and the stop, for the recorder that owns the listening —
    /// dropping it ends the session, so the adapter stops holding the
    /// keys back. The session itself then ends nothing when it is
    /// dropped: its halves hold both of its parts.
    pub fn split(mut self) -> (RecordingReports, RecordingStop) {
        let reports = self
            .reports
            .take()
            .expect("a session holds its reports until it is split");
        let stop = self.stop.take();
        (RecordingReports(reports), RecordingStop(stop))
    }

    /// A session whose reports come from `reports` and whose ending
    /// `stop` brings about: for an adapter's own session, and for the
    /// tests' fakes.
    pub fn of(reports: Presses, stop: impl FnOnce() + Send + 'static) -> RecordingSession {
        RecordingSession {
            reports: Some(reports),
            stop: Some(Box::new(stop)),
        }
    }
}

impl Drop for RecordingSession {
    /// Ends the session, as dropping its stop half does — an unsplit
    /// session is its own stop.
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// The reports of a recording session, as the task the recorder spawns
/// consumes them ([`RecordingSession::split`]).
pub struct RecordingReports(Presses);

impl RecordingReports {
    /// The next binding the user pressed, waiting for it; `None` once
    /// the session ended.
    pub async fn next(&mut self) -> Option<Shortcut> {
        self.0.next().await
    }

    /// A binding already pressed, without waiting.
    pub fn try_next(&mut self) -> Option<Shortcut> {
        self.0.try_next()
    }
}

/// What ends a recording session when dropped: the half of it the
/// recorder keeps ([`RecordingSession::split`]), so that stopping
/// listening stops the adapter holding the keys back at once, whatever
/// the task that consumes the reports is doing.
pub struct RecordingStop(Option<Box<dyn FnOnce() + Send + 'static>>);

impl Drop for RecordingStop {
    /// Ends the session, wherever this half came from.
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            stop();
        }
    }
}

/// How a registered shortcut is dispatched: through the system's own
/// registration, or through Pane's own low-level keyboard hook, where the
/// system refused the registration or cannot express the binding
/// (Windows, #252). Rows and the Shortcuts page say which, so a binding
/// that behaves differently — the hook's does nothing while an elevated
/// application is in front, since Windows does not deliver those keys to
/// the hook of a normal process — is understood.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    /// The system's own registration: `RegisterHotKey` on Windows,
    /// Carbon's hot keys on macOS, an `XGrabKey` grab on X11.
    #[default]
    System,
    /// Pane's own `WH_KEYBOARD_LL` keyboard hook, which works while Pane
    /// runs and takes the shortcut from whoever had it, Windows included.
    Hook,
}

impl Route {
    /// What a hotkey's row appends for a binding dispatched this way on
    /// `platform`: nothing for the system's registration; for the hook,
    /// the route and the elevated-application limit, with what the system
    /// does with `shortcut` where it keeps it for itself, which Pane
    /// takes first while it runs. The platform is named so the wording is
    /// tested on every system.
    pub fn note_on(&self, shortcut: &Shortcut, platform: Option<Platform>) -> Option<String> {
        if *self == Route::System {
            return None;
        }
        let mut note = "through Pane's keyboard hook, which does nothing while an \
                        elevated application is in front"
            .to_string();
        if let Some(kept) = shortcut.kept_by(platform) {
            note.push_str("; ");
            note.push_str(kept);
            note.push_str(", which Pane takes first while it runs");
        }
        Some(note)
    }
}

/// The state of Pane's own keyboard hook, where this system's adapter
/// uses one (Windows, #252): what the Settings pages and Copy Diagnostics
/// say of it. `None` where no hook is in use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookHealth {
    /// How many times Windows removed the hook and Pane installed it
    /// again.
    pub reinstalls: u32,
    /// Whether the hook's code and data pages are pinned in memory, so a
    /// trimmed working set cannot fault the callback past Windows' hook
    /// timeout.
    pub pinned: bool,
    /// Why Pane gave up keeping the hook installed, if it did: its
    /// hook-based bindings do nothing until Pane starts again.
    pub given_up: Option<String>,
}

impl HookHealth {
    /// The note the Settings window's Keyboard page holds under the
    /// hook's name and Copy Diagnostics after "Keyboard hook: " (#259):
    /// that the hook is installed and its pages pinned in memory, how
    /// many times Windows removed it and Pane installed it again, and —
    /// in place of all of that — why Pane gave up keeping it installed,
    /// if it did.
    pub fn note(&self) -> String {
        if let Some(why) = &self.given_up {
            return why.clone();
        }
        let mut note = format!(
            "Installed, its pages {} pinned in memory",
            if self.pinned { "are" } else { "are not" }
        );
        match self.reinstalls {
            0 => {}
            1 => note.push_str("; Windows removed it once and Pane installed it again"),
            count => note.push_str(&format!(
                "; Windows removed it {count} times and Pane installed it again"
            )),
        }
        note
    }
}

/// Where an adapter reports presses of registered shortcuts.
#[derive(Clone)]
pub struct PressSender(tokio::sync::mpsc::UnboundedSender<Shortcut>);

impl PressSender {
    /// Reports a press of `shortcut`. Once the [`Presses`] are gone, it is
    /// dropped.
    pub fn send(&self, shortcut: Shortcut) {
        let _ = self.0.send(shortcut);
    }
}

/// The presses an adapter reports, in order.
pub struct Presses(tokio::sync::mpsc::UnboundedReceiver<Shortcut>);

impl Presses {
    /// The next press, waiting for it; `None` once no adapter can send any.
    pub async fn next(&mut self) -> Option<Shortcut> {
        self.0.recv().await
    }

    /// A press already reported, without waiting.
    pub fn try_next(&mut self) -> Option<Shortcut> {
        self.0.try_recv().ok()
    }
}

/// A channel from an adapter to the window.
pub fn channel() -> (PressSender, Presses) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (PressSender(sender), Presses(receiver))
}

/// This system's adapter, reporting presses to `presses`. On macOS it must
/// be made on the main thread, whose run loop receives the presses.
pub fn native(presses: PressSender) -> Arc<dyn Hotkeys> {
    #[cfg(target_os = "linux")]
    {
        let wayland = env_set("WAYLAND_DISPLAY");
        if wayland {
            return Arc::new(Unavailable(
                "Not available on Linux with Wayland: Wayland does not let an application see \
                 keys pressed in other applications, and Pane does not use the desktop's global \
                 shortcuts portal yet. Assign a shortcut in the desktop's keyboard settings \
                 instead, or run Pane on X11."
                    .into(),
            ));
        }
        match std::env::var("DISPLAY") {
            Ok(display) if !display.is_empty() => match X11Hotkeys::connect(&display, presses) {
                Ok(hotkeys) => Arc::new(hotkeys),
                Err(problem) => Arc::new(Unavailable(format!(
                    "Not available: Pane could not reach the X11 display {display}: {problem}"
                ))),
            },
            _ => Arc::new(Unavailable(
                "Not available: Pane is not running on an X11 or Wayland display".into(),
            )),
        }
    }
    #[cfg(target_os = "windows")]
    {
        match WindowsHotkeys::start(presses) {
            Ok(hotkeys) => Arc::new(hotkeys),
            Err(problem) => Arc::new(Unavailable(format!("Not available: {problem}"))),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match MacHotkeys::start(presses) {
            Ok(hotkeys) => Arc::new(hotkeys),
            Err(problem) => Arc::new(Unavailable(format!("Not available: {problem}"))),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = presses;
        Arc::new(Unavailable(format!(
            "Not available on this system ({}): Pane has global hotkeys only on Windows, macOS \
             and Linux",
            std::env::consts::OS
        )))
    }
}

#[cfg(target_os = "linux")]
fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// An adapter for where global hotkeys cannot be used, saying why.
pub struct Unavailable(pub String);

impl Hotkeys for Unavailable {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn kind_unavailable(&self, shortcut: &Shortcut) -> Option<String> {
        // No hotkeys at all, so no keyboard hook either: the kinds only
        // the hook recognizes are explained, as the platforms without
        // one explain them — macOS standing in where the binary runs on
        // Windows, so a fresh data folder there keeps today's Open Pane
        // default rather than taking the Windows key this adapter could
        // never register. An adapter that starts and has the hook says
        // so itself, by overriding this.
        let modeled = match Platform::current() {
            Some(Platform::Windows) => Some(Platform::Macos),
            platform => platform,
        };
        kinds_unavailable(shortcut, modeled)
    }

    fn register(&self, _shortcut: &Shortcut) -> Result<(), HotkeyError> {
        Err(HotkeyError::Refused(self.0.clone()))
    }

    fn unregister(&self, _shortcut: &Shortcut) {}
}

/// The adapter of a launcher that was given none.
pub(crate) fn none() -> Arc<dyn Hotkeys> {
    Arc::new(Unavailable(
        "Not available: this Pane has no global hotkeys".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{HookHealth, INJECTED_TAG, Kind, Route, Shortcut, kinds_unavailable};
    use crate::platform::Platform;

    fn shortcut(text: &str) -> Shortcut {
        Shortcut::parse(text).expect("a valid shortcut")
    }

    /// The Windows rules are checked on every system: `refusal_on` and
    /// `note_on` take the platform, so what Windows does is pinned without
    /// a Windows machine.
    const WINDOWS: Option<Platform> = Some(Platform::Windows);

    #[test]
    fn windows_reserved_shortcuts_are_warnings_rather_than_refusals() {
        // What Windows keeps for itself is no longer refused: the hook
        // takes the chord, and the row warns what Windows does with it.
        for (id, does) in [
            ("alt+f4", "Windows closes the active window with it"),
            ("super+d", "Windows shows the desktop with it"),
            ("super+e", "Windows opens File Explorer with it"),
            ("super+r", "Windows opens Run with it"),
            ("alt+space", "Windows opens the window menu with it"),
        ] {
            let shortcut = shortcut(id);
            assert_eq!(shortcut.refusal_on(WINDOWS), None, "{id} is refused");
            let note = Route::Hook.note_on(&shortcut, WINDOWS).unwrap();
            assert!(note.contains("through Pane's keyboard hook"), "{note}");
            assert!(note.contains("elevated application"), "{note}");
            assert!(note.contains(does), "{note}");
            assert!(note.contains("Pane takes first while it runs"), "{note}");
        }
        // A shortcut Windows does not keep warns of nothing but the route.
        let note = Route::Hook
            .note_on(&shortcut("ctrl+alt+g"), WINDOWS)
            .unwrap();
        assert_eq!(
            note,
            "through Pane's keyboard hook, which does nothing while an elevated application \
             is in front"
        );
        // The system's registration says nothing.
        assert_eq!(
            Route::System.note_on(&shortcut("ctrl+alt+g"), WINDOWS),
            None
        );
    }

    #[test]
    fn what_no_program_can_intercept_is_still_refused_with_the_reason() {
        // Win+L: Windows locks the computer with it, and the lock screen
        // is below every hook.
        assert_eq!(
            shortcut("super+l").refusal_on(WINDOWS),
            Some(
                "Win+L is reserved: Windows locks the computer with it, and no program can \
                 intercept it"
                    .into()
            )
        );
        // Ctrl+Alt+Delete: the secure screen owns it, on every system, so
        // it is refused before the key set's own wording. Delete is one
        // of the named keys #260 adds, so this stays reached.
        assert_eq!(
            Shortcut::parse("ctrl+alt+delete").unwrap_err(),
            "Ctrl+Alt+Delete is reserved: no program can intercept it"
        );
        // Other systems keep their own refusals: only Windows' list moved
        // to warnings.
        assert!(
            shortcut("alt+f4")
                .refusal_on(Some(Platform::Linux))
                .is_some()
        );
        assert!(
            shortcut("super+l")
                .refusal_on(Some(Platform::Linux))
                .is_some()
        );
        assert!(
            shortcut("super+space")
                .refusal_on(Some(Platform::Macos))
                .is_some()
        );
        assert_eq!(shortcut("alt+f4").refusal_on(WINDOWS), None);
    }

    #[test]
    fn the_grammar_round_trips_the_binding_kinds() {
        // Each of the kinds' forms parses to the binding it names, and
        // its id is the same text again; the synonyms read the same
        // binding, written in the canonical form.
        for (text, canonical) in [
            ("ctrl+alt+g", "ctrl+alt+g"),
            ("tap:win", "tap:win"),
            ("tap:super", "tap:win"),
            ("tap:rctrl", "tap:rctrl"),
            ("tap:RCTRL", "tap:rctrl"),
            ("tap:lalt", "tap:lalt"),
            ("double:ctrl", "double:ctrl"),
            ("double:shift", "double:shift"),
            ("double:ralt", "double:ralt"),
            ("rctrl+alt+g", "rctrl+alt+g"),
            ("rcontrol+g", "rctrl+g"),
            ("lwin+space", "lwin+space"),
            ("lcommand+space", "lwin+space"),
            ("rsuper+d", "rwin+d"),
            ("ctrl+alt+numpad_enter", "ctrl+alt+numpad_enter"),
            ("ctrl+alt+numpad5", "ctrl+alt+numpad5"),
            ("ctrl+alt+.", "ctrl+alt+."),
            ("ctrl+alt+comma", "ctrl+alt+,"),
            ("ctrl+shift+<", "ctrl+shift+,"),
            ("ctrl+alt+pageup", "ctrl+alt+pageup"),
            ("ctrl+alt+f13", "ctrl+alt+f13"),
            ("ctrl+alt+f24", "ctrl+alt+f24"),
            ("ctrl+alt+-", "ctrl+alt+-"),
            ("ctrl+alt+=", "ctrl+alt+="),
        ] {
            let binding = shortcut(text);
            assert_eq!(binding.id(), canonical, "{text}");
            // Reading the id back gives the same binding.
            assert_eq!(shortcut(&binding.id()), binding, "{text}");
        }
        // A tap holds no key, and names exactly one modifier, with its
        // side where one is named.
        let tap = shortcut("tap:rctrl");
        assert_eq!(tap.kind(), Kind::Tap);
        assert_eq!(tap.key(), "");
        assert!(tap.control());
        assert!(!tap.alt());
        assert_eq!(tap.sides(), [Some(super::Side::Right), None, None, None]);
        // A chord with no side named takes either key.
        let chord = shortcut("ctrl+g");
        assert_eq!(chord.sides(), [Some(super::Side::Any), None, None, None]);
        // A part that is not a modifier, a key that is not one, and a
        // modifier named twice are explained; so is a tap of something
        // that is not a modifier.
        assert_eq!(
            Shortcut::parse("meta+g").unwrap_err(),
            "“meta+g”: meta is not a modifier key"
        );
        assert_eq!(
            Shortcut::parse("ctrl+alt+escape").unwrap_err(),
            "Pane cannot use Escape in a hotkey: end it with a letter, a digit, F1 to F24, \
             Space, Enter, Tab, an arrow, a punctuation key, a navigation key or a numpad key"
        );
        assert_eq!(
            Shortcut::parse("lctrl+rctrl+g").unwrap_err(),
            "“lctrl+rctrl+g”: control is named twice"
        );
        assert_eq!(
            Shortcut::parse("tap:g").unwrap_err(),
            "“tap:g”: g is not a modifier key"
        );
    }

    #[test]
    fn the_binding_kinds_display_as_windows_names_them() {
        // "Win", "Right Ctrl" and "Ctrl Ctrl" are Windows' names; Linux
        // says Super for the Windows key and macOS Command, Control and
        // Option, but the sides and the doubled names are the same.
        let (win, ctrl, alt) = if cfg!(target_os = "macos") {
            ("Command", "Control", "Option")
        } else if cfg!(target_os = "windows") {
            ("Win", "Ctrl", "Alt")
        } else {
            ("Super", "Ctrl", "Alt")
        };
        assert_eq!(shortcut("tap:win").to_string(), win);
        assert_eq!(shortcut("tap:rctrl").to_string(), format!("Right {ctrl}"));
        assert_eq!(
            shortcut("double:ctrl").to_string(),
            format!("{ctrl} {ctrl}")
        );
        assert_eq!(
            shortcut("double:ralt").to_string(),
            format!("Right {alt} Right {alt}")
        );
        assert_eq!(
            shortcut("ralt+space").to_string(),
            format!("Right {alt}+Space")
        );
        // The extended key set keeps people's names: Page Up, Num 5, the
        // numpad's own Enter, and a punctuation key by its character.
        assert_eq!(
            shortcut("ctrl+alt+pageup").to_string(),
            format!("{ctrl}+{alt}+Page Up")
        );
        assert_eq!(
            shortcut("ctrl+alt+numpad5").to_string(),
            format!("{ctrl}+{alt}+Num 5")
        );
        assert_eq!(
            shortcut("ctrl+alt+numpad_enter").to_string(),
            format!("{ctrl}+{alt}+Num Enter")
        );
        assert_eq!(
            shortcut("ctrl+alt+.").to_string(),
            format!("{ctrl}+{alt}+.")
        );
        assert_eq!(
            shortcut("ctrl+alt+minus").to_string(),
            format!("{ctrl}+{alt}+-")
        );
        // A chord a side names still shows its other modifiers, and a
        // modifier no side is named for names none.
        assert_eq!(
            shortcut("shift+rctrl+g").to_string(),
            format!("Right {ctrl}+Shift+G")
        );
    }

    #[test]
    fn a_lone_or_double_tap_takes_over_no_typing_and_no_chord_guard_refuses_it() {
        // The tap kinds are modifiers, so the chord guard — a chord needs
        // Ctrl, Alt or the Windows key so it does not take over typing —
        // does not apply to them, on any system.
        for text in ["tap:shift", "tap:win", "double:ctrl", "tap:ralt"] {
            assert_eq!(shortcut(text).refusal(), None, "{text} is refused");
        }
        // A chord still needs one of the guarding modifiers, even with
        // the extended key set's keys.
        let refusal = shortcut("shift+,").refusal().unwrap();
        assert!(refusal.contains("needs "), "{refusal}");
        // The editing guard stays: the primary modifier with a single
        // character is refused (Command on macOS, Ctrl elsewhere).
        let editing = if cfg!(target_os = "macos") {
            shortcut("super+,")
        } else {
            shortcut("ctrl+,")
        };
        let refusal = editing.refusal().unwrap();
        assert!(refusal.contains("used by applications"), "{refusal}");
    }

    #[test]
    fn the_hook_only_kinds_are_explained_where_there_is_no_hook() {
        // A record copied from a Windows machine: the kinds that need the
        // hook are explained on the other systems, with the system named
        // and "work only on Windows for now", through the wording the
        // spec's platform-availability mechanism shows.
        for (text, why) in [
            ("tap:win", "lone modifier taps work only on Windows for now"),
            (
                "double:ctrl",
                "double-tap hotkeys work only on Windows for now",
            ),
            (
                "rctrl+g",
                "side-specific modifiers work only on Windows for now",
            ),
            (
                "ctrl+alt+f13",
                "keys beyond letters, digits, F1 to F12 and Space work only on Windows for now",
            ),
            (
                "ctrl+alt+numpad5",
                "keys beyond letters, digits, F1 to F12 and Space work only on Windows for now",
            ),
        ] {
            let shortcut = shortcut(text);
            for platform in [Some(Platform::Macos), Some(Platform::Linux)] {
                let reason = kinds_unavailable(&shortcut, platform).unwrap();
                assert!(reason.starts_with("Not available on"), "{reason}");
                assert!(reason.contains(why), "{reason} for {text}");
            }
        }
        // Nothing of the sort on Windows: every kind works there (the
        // adapter says so itself, by overriding the trait's method).
        for text in [
            "tap:win",
            "double:ctrl",
            "rctrl+g",
            "ctrl+alt+numpad5",
            "ctrl+alt+f13",
            "ctrl+alt+g",
            "ctrl+alt+space",
        ] {
            assert_eq!(
                kinds_unavailable(&shortcut(text), WINDOWS),
                None,
                "{text} is unavailable on Windows"
            );
        }
        // A plain chord is available everywhere, and a system the enum
        // does not name is still named in the message.
        assert_eq!(kinds_unavailable(&shortcut("ctrl+alt+g"), None), None);
        let reason = kinds_unavailable(&shortcut("tap:win"), None).unwrap();
        assert!(reason.contains("this system"), "{reason}");
    }

    #[test]
    fn the_injected_tag_spells_pane() {
        // "PANE" as four bytes: the marker every key Pane injects carries
        // in the event's extra information, spelled for a tool that reads
        // the markers other tools leave on their injections.
        let spelled: [u8; 4] = (INJECTED_TAG as u32).to_be_bytes();
        assert_eq!(spelled, *b"PANE");
        // The hook health an adapter answers for starts neutral.
        let health = HookHealth::default();
        assert_eq!(health.reinstalls, 0);
        assert!(!health.pinned);
        assert_eq!(health.given_up, None);
    }

    #[test]
    fn the_hook_s_state_is_said_plainly() {
        // What the Keyboard page holds under the hook's name and Copy
        // Diagnostics after "Keyboard hook: " (#259): the same words on
        // every system, since the hook is Windows' but the words are
        // Pane's.
        let quiet = HookHealth {
            reinstalls: 0,
            pinned: true,
            given_up: None,
        };
        assert_eq!(quiet.note(), "Installed, its pages are pinned in memory");
        let busier = HookHealth {
            reinstalls: 2,
            pinned: false,
            given_up: None,
        };
        assert_eq!(
            busier.note(),
            "Installed, its pages are not pinned in memory; Windows removed it 2 times and \
             Pane installed it again"
        );
        let once = HookHealth {
            reinstalls: 1,
            pinned: true,
            given_up: None,
        };
        assert_eq!(
            once.note(),
            "Installed, its pages are pinned in memory; Windows removed it once and Pane \
             installed it again"
        );
        // Giving up replaces the state: the reason, which says what
        // happened and what it leaves behind.
        let why = "Windows removed Pane's keyboard hook 6 times within 30 seconds; Pane gave \
                   up reinstalling it, and the hotkeys it dispatches do nothing until Pane \
                   starts again";
        let given_up = HookHealth {
            reinstalls: 6,
            pinned: true,
            given_up: Some(why.into()),
        };
        assert_eq!(given_up.note(), why);
    }
}
