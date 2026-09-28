//! Global hotkeys: a shortcut the user assigns to an installed command,
//! which opens that command in Pane's window even while another application
//! has focus.
//!
//! The launcher decides which shortcuts are bound and which should be
//! registered; the system is reached through one small trait, [`Hotkeys`],
//! with one adapter per system, chosen by [`native`]:
//!
//! - Windows: `RegisterHotKey` on a thread of Pane's own ([`windows`]);
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
//! asks the launcher to open the bound command.

use std::fmt;
use std::sync::Arc;

use crate::platform::Platform;

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
const FUNCTION_KEYS: u8 = 12;

/// A key combination: modifier keys held while one other key is pressed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    control: bool,
    alt: bool,
    shift: bool,
    /// The Windows key, Command on macOS, Super on Linux.
    super_key: bool,
    key: String,
}

impl Shortcut {
    /// The shortcut of the modifiers held and `key`, as the window reports a
    /// key press: `a` to `z`, `0` to `9`, `f1` to `f12` or `space`, in any
    /// case. Another key is explained.
    pub fn new(
        control: bool,
        alt: bool,
        shift: bool,
        super_key: bool,
        key: &str,
    ) -> Result<Shortcut, String> {
        let key = key.to_ascii_lowercase();
        let supported = match key.as_bytes() {
            [b'a'..=b'z' | b'0'..=b'9'] => true,
            _ => key == "space" || (1..=FUNCTION_KEYS).any(|number| key == format!("f{number}")),
        };
        if !supported {
            return Err(format!(
                "Pane cannot use {} in a hotkey: end it with a letter, a digit, F1 to F12 or Space",
                key_name(&key)
            ));
        }
        Ok(Shortcut {
            control,
            alt,
            shift,
            super_key,
            key,
        })
    }

    /// Reads a shortcut as [`Shortcut::id`] writes it, such as
    /// `ctrl+alt+p`.
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = parts.pop().filter(|key| !key.is_empty());
        let Some(key) = key else {
            return Err(format!("“{text}” names no key"));
        };
        let (mut control, mut alt, mut shift, mut super_key) = (false, false, false, false);
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => control = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                "super" | "win" | "cmd" | "command" => super_key = true,
                other => return Err(format!("“{text}”: {other} is not a modifier key")),
            }
        }
        Shortcut::new(control, alt, shift, super_key, key)
    }

    /// The same text on every system, for records: the modifiers in a fixed
    /// order, then the key, such as `ctrl+alt+p`.
    pub fn id(&self) -> String {
        let mut parts = Vec::new();
        for (held, name) in self.modifiers() {
            if held {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }

    pub fn control(&self) -> bool {
        self.control
    }

    pub fn alt(&self) -> bool {
        self.alt
    }

    pub fn shift(&self) -> bool {
        self.shift
    }

    /// Whether the Windows key (Windows), Command (macOS) or Super (Linux)
    /// is held.
    pub fn super_key(&self) -> bool {
        self.super_key
    }

    /// `a` to `z`, `0` to `9`, `f1` to `f12` or `space`.
    pub fn key(&self) -> &str {
        &self.key
    }

    fn modifiers(&self) -> [(bool, &'static str); 4] {
        [
            (self.control, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.super_key, "super"),
        ]
    }

    /// Why Pane does not bind this shortcut on this system, if it does not:
    /// it has no Ctrl, Alt or Super (Command) key, so it would take over
    /// typing, or the system or other applications use it.
    pub fn refusal(&self) -> Option<String> {
        if !(self.control || self.alt || self.super_key) {
            let modifiers = match Platform::current() {
                Some(Platform::Macos) => "Control, Option or Command",
                Some(Platform::Windows) => "Ctrl, Alt or the Windows key",
                _ => "Ctrl, Alt or Super",
            };
            return Some(format!(
                "{self} needs {modifiers}, so that it does not take over typing"
            ));
        }
        let reserved = reserved(Platform::current());
        reserved
            .iter()
            .find(|(id, _)| Shortcut::parse(id).is_ok_and(|reserved| reserved == *self))
            .map(|(_, why)| format!("{self} is reserved: {why}"))
            .or_else(|| self.editing_refusal())
    }

    /// Ctrl (Command on macOS) with only a letter or digit: every
    /// application uses those for its own commands, such as copying.
    fn editing_refusal(&self) -> Option<String> {
        let primary = if Platform::current() == Some(Platform::Macos) {
            self.super_key && !self.control
        } else {
            self.control && !self.super_key
        };
        let single = primary && !self.alt && !self.shift;
        let character = self.key.len() == 1;
        (single && character).then(|| {
            format!(
                "{self} is used by applications for their own commands, such as copying; add Alt \
                 or Shift"
            )
        })
    }
}

/// Shortcuts the system keeps for itself, each with why, on `platform`.
fn reserved(platform: Option<Platform>) -> &'static [(&'static str, &'static str)] {
    match platform {
        Some(Platform::Windows) => &[
            ("alt+f4", "Windows closes the active window with it"),
            ("super+l", "Windows locks the computer with it"),
            ("super+d", "Windows shows the desktop with it"),
            ("super+e", "Windows opens File Explorer with it"),
            ("super+r", "Windows opens Run with it"),
            ("alt+space", "Windows opens the window menu with it"),
        ],
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

/// People's name for `key`: `P`, `5`, `F5`, `Space`.
fn key_name(key: &str) -> String {
    match key {
        "space" => "Space".into(),
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

/// People's names on this system: "Ctrl+Alt+P" on Linux and Windows (with
/// "Super" or "Win"), "Control+Option+Command+P" on macOS.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: [&str; 4] = match Platform::current() {
            Some(Platform::Macos) => ["Control", "Option", "Shift", "Command"],
            Some(Platform::Windows) => ["Ctrl", "Alt", "Shift", "Win"],
            _ => ["Ctrl", "Alt", "Shift", "Super"],
        };
        for ((held, _), name) in self.modifiers().into_iter().zip(names) {
            if held {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&key_name(&self.key))
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

    /// Registers `shortcut` system-wide.
    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError>;

    /// Releases `shortcut`, registered earlier, so that other applications
    /// may use it and it is no longer reported.
    fn unregister(&self, shortcut: &Shortcut);
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
