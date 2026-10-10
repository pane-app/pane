//! Pane's own host settings: the preferences the host records for itself,
//! separate from every extension's data.
//!
//! The record is `settings.json` in Pane's data folder, beside the
//! `extensions` folder that holds packages and their own records — the
//! host's preferences are never written inside a package's folder, and a
//! package's data is never written inside the host's record. It follows
//! the house record-file rules, as `installed.json`, `aliases.json` and
//! `updates.json` do: versioned, so a record another Pane cannot read is
//! refused rather than misread; validated, so a field this Pane does not
//! understand fails the whole record instead of half-loading; written
//! atomically, so a crash or power loss leaves either the old record or
//! the new one, never a torn one; and defaulted, so a missing field (or a
//! missing record) means the default, not an error.
//!
//! Reading never repairs: a record that cannot be read or parsed is
//! reported as a problem and left exactly as it is on disk, so the source
//! data stays for diagnosis. Whether to refuse replacing it is the
//! caller's policy (the window keeps the house rule: it does not replace
//! an unreadable record); this module only reads and writes.
//!
//! Nothing here knows the renderer: the preferences are plain values, and
//! the window layer resolves them into a theme and a material, including
//! the platform's own normalization of glass to solid.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::atomic::{Readers, write_atomically};
use crate::hotkeys::Shortcut;
use crate::keyboard::Keyboard;

/// The file the settings are recorded in, in Pane's data folder.
const FILE: &str = "settings.json";

/// The record's version as this Pane writes it.
const VERSION: u64 = 2;

/// Whether this Pane reads a record of `version`: its own, and 1, the
/// record an older Pane wrote. The Open Pane field's grammar moved with
/// #260's binding kinds — a lone tap (`tap:win`), a double tap
/// (`double:ctrl`), a side-specific modifier (`rctrl+space`) — but the
/// grammar is [`Shortcut::parse`], which reads a chord's id from version
/// 1 as it is, so a record of either version reads.
fn reads(version: u64) -> bool {
    version == VERSION || version == 1
}

/// The theme the user chose for Pane's windows: follow the operating
/// system's appearance, or force one of the two palettes.
///
/// The default is the reference's dark palette, as the launcher has
/// shipped since the visual rework; the appearance page records whichever
/// the user picks, and this value is renderer-independent — the window
/// layer resolves it against the system appearance to a palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    /// Pane's dark palette, whatever the system's appearance is.
    #[default]
    Dark,
    /// Pane's light palette, whatever the system's appearance is.
    Light,
    /// The palette that matches the system's appearance, following its
    /// changes while Pane runs.
    System,
}

/// The window surface the user chose: the translucent glass where the
/// platform can frost the window, or the deterministic solid surface.
///
/// A request for glass is a request, not a result: the platform may not
/// provide compositor frost, and the window layer normalizes such a
/// request to the solid surface and says so in the appearance page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaterialPreference {
    /// The translucent panel over a frosted window, where the platform
    /// provides frost; normalized to solid where it does not.
    #[default]
    Glass,
    /// The opaque window and solid panel, the same on every platform.
    Solid,
}

/// The display the launcher window opens on, as the user chose it. The
/// choice is a preference, not a placement: where the launcher actually
/// opens is the display layout the platform reports and the resolution
/// that turns this choice into a display (see `crate::placement`), which
/// falls back to an available display when the chosen one is gone.
///
/// The default is the display the pointer is on, as the user decided
/// (2026-10-05); where the system does not say where the pointer is, the
/// resolution falls back to the primary display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpeningMonitor {
    /// The system's primary display, whatever else is connected.
    Primary,
    /// The display the pointer is on when the launcher opens, where the
    /// system tells Pane where the pointer is.
    #[default]
    Pointer,
    /// The display of the operating system's active window — the one the
    /// user is working in — where the system tells Pane which window is
    /// active.
    #[serde(rename = "active-window")]
    ActiveWindow,
}

/// What reopening the launcher shows, as the user chose it (Raycast's
/// "Pop to Root Search"). Dismissal — hiding the launcher, by the Open
/// Pane hotkey or by Escape at root search with an empty query — never
/// quits Pane, and what the next opening starts from is this choice.
///
/// The default restores a still-valid view, as the settings
/// specification proposes provisionally; a view that is no longer valid —
/// its command removed or its extension disabled — returns safely to
/// root search either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reopening {
    /// Show the view the launcher was left on, when it is still valid:
    /// never pop to root search.
    #[serde(rename = "restore-view")]
    #[default]
    RestoreView,
    /// Start from root search, with an empty query, whatever was left.
    #[serde(rename = "root-search")]
    RootSearch,
    /// Restore the view if the launcher was hidden for less than 90
    /// seconds, start from root search otherwise.
    #[serde(rename = "after-90-seconds")]
    After90Seconds,
    /// As [`Reopening::After90Seconds`], after 3 minutes.
    #[serde(rename = "after-3-minutes")]
    After3Minutes,
}

impl Reopening {
    /// How long the launcher may stay hidden before reopening starts from
    /// root search: zero for [`Reopening::RootSearch`], `None` for never.
    pub fn pops_after(self) -> Option<std::time::Duration> {
        match self {
            Reopening::RestoreView => None,
            Reopening::RootSearch => Some(std::time::Duration::ZERO),
            Reopening::After90Seconds => Some(std::time::Duration::from_secs(90)),
            Reopening::After3Minutes => Some(std::time::Duration::from_secs(180)),
        }
    }
}

/// How much of the launcher shows while root search's query is blank:
/// Raycast's window modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    /// The whole launcher: the search field, the pinned home, the results
    /// and the footer.
    #[default]
    Expanded,
    /// Only the search field until something is typed; the results and
    /// the footer appear with a query.
    Compact,
}

/// How root search's pinned home lays out its quick slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PinnedLayout {
    /// A row of tiles above the results.
    #[default]
    Horizontal,
    /// Result rows, one per pinned result, above the results.
    Vertical,
}

/// What the launcher's back key (Escape by default) does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EscapeBehavior {
    /// Leave the open screen, one level at a time, and hide the launcher
    /// from an empty root search.
    #[default]
    #[serde(rename = "back-or-hide")]
    BackOrHide,
    /// Hide the launcher from wherever it is; reopening follows
    /// [`Reopening`].
    #[serde(rename = "hide")]
    Hide,
}

/// Extra keys that move the selection, beside the Keyboard page's
/// previous and next result bindings. They hold Alt, as Raycast for
/// Windows' do, so none meets the Ctrl chords the launcher already has
/// (Ctrl+K opens the Actions panel); on macOS, where Option types
/// characters, they hold Control, as Raycast for Mac's do. Raycast's
/// left and right keys (B and F, H and L) move between its search
/// fields: Pane's take Left and Right between the query and the
/// argument fields, as Raycast's do between its query and the inline
/// argument fields beside it (#258).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavigationBindings {
    /// No extra keys.
    #[default]
    None,
    /// Alt+P and Alt+N (Control on macOS).
    Emacs,
    /// Alt+K and Alt+J (Control on macOS), Raycast's Vim Motions.
    Vim,
}

impl NavigationBindings {
    /// The extra bindings' ids, previous result's then next result's.
    pub fn bindings(self) -> Option<(&'static str, &'static str)> {
        let mac = cfg!(target_os = "macos");
        match self {
            NavigationBindings::None => None,
            NavigationBindings::Emacs if mac => Some(("ctrl-p", "ctrl-n")),
            NavigationBindings::Emacs => Some(("alt-p", "alt-n")),
            NavigationBindings::Vim if mac => Some(("ctrl-k", "ctrl-j")),
            NavigationBindings::Vim => Some(("alt-k", "alt-j")),
        }
    }

    /// The choice's Left and Right, the keys that move between the query
    /// and the argument fields: backward's then forward's. As the
    /// selection keys', Alt on Windows and Linux, Control on macOS.
    pub fn left_right(self) -> Option<(&'static str, &'static str)> {
        let mac = cfg!(target_os = "macos");
        match self {
            NavigationBindings::None => None,
            NavigationBindings::Emacs if mac => Some(("ctrl-b", "ctrl-f")),
            NavigationBindings::Emacs => Some(("alt-b", "alt-f")),
            NavigationBindings::Vim if mac => Some(("ctrl-h", "ctrl-l")),
            NavigationBindings::Vim => Some(("alt-h", "alt-l")),
        }
    }
}

/// The texture drawn into the launcher's background image (ADR 0028), as
/// Roboco's new-thread background offers them: the picture as it is, or
/// one of four treatments of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundEffect {
    /// The picture as it is.
    None,
    /// An ordered (Bayer) dither in 2px dots.
    Dither,
    /// The picture redrawn in small bitmap glyphs.
    Ascii,
    /// A halftone of round dots.
    Halftone,
    /// Every third row darkened, as a display's scanlines.
    #[default]
    Scanlines,
}

/// The host settings as the user chose them: one theme preference, one
/// material preference, the Open Pane hotkey, the tray visibility, the
/// launch-at-login choice, the launcher's opening display and what
/// reopening shows, and the in-app navigation bindings, the whole of
/// what the Settings pages built so far offer. Later pages add fields
/// beside these, with the same rules: missing fields default, and
/// unknown values fail the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSettings {
    /// The theme the user chose for Pane's windows.
    pub theme: ThemePreference,
    /// The surface the user chose for Pane's windows.
    pub material: MaterialPreference,
    /// The global shortcut that opens Pane itself from any application —
    /// the application-owned binding the General page records. It is a
    /// host setting, not a command's hotkey: it belongs to no package,
    /// stays while extensions are disabled, and is applied through the
    /// same platform registration path the command hotkeys use.
    pub open_pane: Shortcut,
    /// Whether Pane shows its tray or menu-bar entry — the item whose
    /// menu opens the launcher, Settings and Quit, reachable while the
    /// launcher is hidden. A provisional default, as the Open Pane
    /// shortcut's is: the entry is shown, since it is the one place
    /// those actions live outside Pane's own windows; the General page
    /// can hide it, where the platform provides one.
    pub tray_visible: bool,
    /// Whether Pane shows the taskbar while the launcher window is open
    /// (#268, ADR 0039): for a user whose taskbar hides itself, the
    /// Windows adapter shows it while the launcher is open — so the
    /// Start button stays one click away once the Windows key opens Pane
    /// — and puts it back as the user had it when the launcher hides.
    /// Windows only; where the platform has no taskbar of the kind, the
    /// General page does not offer the choice. A provisional default,
    /// proposed by ADR 0039: off.
    pub show_taskbar: bool,
    /// Whether the user chose Pane to start at login. A preference, not a
    /// registration: whether Pane actually starts is the platform's own
    /// login integration, which the window layer reconciles with this
    /// choice (see `crate::autostart`) rather than trusting either side
    /// alone.
    pub launch_at_login: bool,
    /// The display the launcher window opens on, as the Launcher page
    /// records it. A preference: the placement is resolved against the
    /// display layout when the launcher opens (see `crate::placement`),
    /// with a fallback when the chosen display is gone.
    pub opening_monitor: OpeningMonitor,
    /// What reopening the launcher shows: the view it was left on, when
    /// still valid, or root search. A preference the Launcher page
    /// records; dismissal behavior itself follows the specification's
    /// Escape contract and is not a choice here.
    pub reopening: Reopening,
    /// The in-app navigation bindings the Keyboard page rebinds: one
    /// binding per action of the bounded set, from the same defaults when
    /// the record holds none. These keys belong to Pane's own windows, not
    /// to any focused field's text editing.
    pub keyboard: Keyboard,
    /// How much of the launcher shows while the query is blank.
    pub window_mode: WindowMode,
    /// Whether the compact window shows the pins as a row of icons under
    /// the search field (Raycast's "Show favorites in compact mode").
    pub compact_pinned: bool,
    /// How the pinned home lays out its quick slots.
    pub pinned_layout: PinnedLayout,
    /// What the launcher's back key does.
    pub escape: EscapeBehavior,
    /// Whether Escape closes the Settings window.
    pub escape_closes_settings: bool,
    /// Extra keys that move the selection.
    pub navigation: NavigationBindings,
    /// The launcher's background image (ADR 0028): the file name of Pane's
    /// own copy of the picture the user chose, in the data folder's
    /// `backgrounds` folder; `None` for the plain panel. Always a plain
    /// file name — a record naming a path anywhere else fails to read.
    pub background: Option<String>,
    /// The texture drawn into the background image.
    pub background_effect: BackgroundEffect,
}

impl Default for HostSettings {
    fn default() -> HostSettings {
        HostSettings {
            theme: ThemePreference::default(),
            material: MaterialPreference::default(),
            open_pane: Shortcut::open_pane_default(),
            tray_visible: true,
            show_taskbar: false,
            launch_at_login: false,
            opening_monitor: OpeningMonitor::default(),
            reopening: Reopening::default(),
            keyboard: Keyboard::default_for_this_system(),
            window_mode: WindowMode::default(),
            compact_pinned: false,
            pinned_layout: PinnedLayout::default(),
            escape: EscapeBehavior::default(),
            escape_closes_settings: true,
            navigation: NavigationBindings::default(),
            background: None,
            background_effect: BackgroundEffect::default(),
        }
    }
}

/// Whether `name` is a plain file name: no folder, no parent, nothing a
/// path could escape the `backgrounds` folder with.
fn plain_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

impl HostSettings {
    /// Reads the settings recorded in `dir`. No record at all means the
    /// defaults, as a record with missing fields does; a record that
    /// cannot be read, parsed or validated is `Err` with the problem,
    /// phrased with the file's path, and is left as it is. No record is
    /// also what makes a data folder fresh, the one case whose Open Pane
    /// hotkey starts with the fresh-install default rather than what a
    /// record names — that is the caller's to decide against the
    /// hotkeys adapter ([`crate::hotkeys::Hotkeys::open_pane_fresh_default`],
    /// #268), and [`HostSettings::recorded`] says which case it is.
    pub fn open(dir: &Path) -> Result<HostSettings, String> {
        let file = dir.join(FILE);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(HostSettings::default());
            }
            Err(error) => return Err(format!("{} cannot be read: {error}", file.display())),
        };
        let fields: Map<String, Value> = serde_json::from_str(&text)
            .map_err(|error| format!("{} is invalid: {error}", file.display()))?;
        match fields.get("version").and_then(Value::as_u64) {
            Some(version) if reads(version) => {}
            Some(version) => {
                return Err(format!(
                    "{} has version {version}, which this Pane does not read",
                    file.display()
                ));
            }
            None => {
                return Err(format!(
                    "{} is invalid: it has no version number",
                    file.display()
                ));
            }
        }
        let recorded: Recorded = serde_json::from_value(Value::Object(fields))
            .map_err(|error| format!("{} is invalid: {error}", file.display()))?;
        let open_pane = match recorded.open_pane {
            None => Shortcut::open_pane_default(),
            Some(text) => Shortcut::parse(&text).map_err(|problem| {
                format!(
                    "{} is invalid: its open pane hotkey is not one: {problem}",
                    file.display()
                )
            })?,
        };
        let keyboard = match recorded.keyboard {
            None => Keyboard::default_for_this_system(),
            Some(fields) => Keyboard::parse(&fields)
                .map_err(|problem| format!("{} is invalid: {problem}", file.display()))?,
        };
        if let Some(name) = &recorded.background
            && !plain_file_name(name)
        {
            return Err(format!(
                "{} is invalid: its background {name:?} is not a file name",
                file.display()
            ));
        }
        Ok(HostSettings {
            theme: recorded.theme,
            material: recorded.material,
            open_pane,
            tray_visible: recorded.tray_visible,
            show_taskbar: recorded.show_taskbar,
            launch_at_login: recorded.launch_at_login,
            opening_monitor: recorded.opening_monitor,
            reopening: recorded.reopening,
            keyboard,
            window_mode: recorded.window_mode,
            compact_pinned: recorded.compact_pinned,
            pinned_layout: recorded.pinned_layout,
            escape: recorded.escape_behavior,
            escape_closes_settings: recorded.escape_closes_settings,
            navigation: recorded.navigation_bindings,
            background: recorded.background,
            background_effect: recorded.background_effect,
        })
    }

    /// Whether `dir` holds a settings record at all (#268): a fresh data
    /// folder — one no Pane has saved settings into — holds none, and it
    /// is the one whose Open Pane hotkey starts with the fresh-install
    /// default rather than what a record names. A record that cannot be
    /// read still counts: it exists, and is never replaced (see
    /// [`HostSettings::open`]).
    pub fn recorded(dir: &Path) -> bool {
        dir.join(FILE).exists()
    }

    /// Writes these settings to the record in `dir`, atomically: the
    /// record is replaced whole or not at all, and a crash leaves the
    /// previous one. `Err` with the problem, phrased with the file's path,
    /// when the record cannot be written. An unreadable record is not
    /// treated specially here — the caller decides what it replaces.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let recorded = Recorded {
            version: VERSION,
            theme: self.theme,
            material: self.material,
            open_pane: Some(self.open_pane.id()),
            tray_visible: self.tray_visible,
            show_taskbar: self.show_taskbar,
            launch_at_login: self.launch_at_login,
            opening_monitor: self.opening_monitor,
            reopening: self.reopening,
            keyboard: Some(self.keyboard.recorded()),
            window_mode: self.window_mode,
            compact_pinned: self.compact_pinned,
            pinned_layout: self.pinned_layout,
            escape_behavior: self.escape,
            escape_closes_settings: self.escape_closes_settings,
            navigation_bindings: self.navigation,
            background: self.background.clone(),
            background_effect: self.background_effect,
        };
        let text = serde_json::to_string_pretty(&recorded).map_err(|error| error.to_string())?;
        let file = dir.join(FILE);
        write_atomically(&file, text.as_bytes(), Readers::Default)
            .map_err(|error| format!("{} cannot be written: {error}", file.display()))
    }
}

/// The settings as the record holds them. Every field is written every
/// time; missing fields read as the defaults. The record's fields are
/// named as the house records name theirs, in camelCase.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recorded {
    version: u64,
    #[serde(default)]
    theme: ThemePreference,
    #[serde(default)]
    material: MaterialPreference,
    /// The Open Pane hotkey as its id, such as `ctrl+alt+space`, or one of
    /// the binding kinds #260 adds written as their ids (`tap:win`,
    /// `double:ctrl`, `rctrl+space`); missing means this system's
    /// provisional default. A value that is not a shortcut fails the whole
    /// record. The field keeps the name it was first recorded with, so
    /// records an earlier Pane wrote still read, while the record's other
    /// fields follow the house camelCase names.
    #[serde(default, rename = "open_pane")]
    open_pane: Option<String>,
    /// Whether the tray or menu-bar entry is shown; missing means shown,
    /// the provisional default. A value that is not a boolean fails the
    /// whole record, as unknown values do.
    #[serde(default = "shown_by_default")]
    tray_visible: bool,
    /// Whether Pane shows the taskbar while the launcher is open (#268);
    /// missing means not, the provisional default.
    #[serde(default)]
    show_taskbar: bool,
    /// Whether the user chose Pane to start at login; missing means not.
    #[serde(default)]
    launch_at_login: bool,
    /// The display the launcher opens on, as one of the three words the
    /// preference names; missing means the primary display, the
    /// provisional default.
    #[serde(default)]
    opening_monitor: OpeningMonitor,
    /// What reopening the launcher shows; missing means restoring a
    /// still-valid view, the provisional default.
    #[serde(default)]
    reopening: Reopening,
    /// The in-app navigation bindings, each action's id mapped to its
    /// binding's id, as [`Keyboard::recorded`] writes them; missing means
    /// this system's provisional defaults. An unknown action, a binding
    /// that is not one or two actions on one binding fails the whole
    /// record.
    #[serde(default)]
    keyboard: Option<BTreeMap<String, String>>,
    /// The window mode; missing means expanded.
    #[serde(default)]
    window_mode: WindowMode,
    /// Whether the compact window shows the pins; missing means it does
    /// not.
    #[serde(default)]
    compact_pinned: bool,
    /// The pinned home's layout; missing means horizontal.
    #[serde(default)]
    pinned_layout: PinnedLayout,
    /// The back key's behavior; missing means back, then hide.
    #[serde(default)]
    escape_behavior: EscapeBehavior,
    /// Whether Escape closes Settings; missing means it does.
    #[serde(default = "shown_by_default")]
    escape_closes_settings: bool,
    /// The extra selection keys; missing means none.
    #[serde(default)]
    navigation_bindings: NavigationBindings,
    /// The background image's file name in the `backgrounds` folder;
    /// missing means none.
    #[serde(default)]
    background: Option<String>,
    /// The background image's texture; missing means scanlines.
    #[serde(default)]
    background_effect: BackgroundEffect,
}

/// The record's default for the tray visibility (and Escape closing
/// Settings): on.
fn shown_by_default() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{FILE, HostSettings, MaterialPreference, Shortcut, ThemePreference};
    use crate::keyboard::{Keyboard, KeyboardAction};

    /// Reads what `text` records in a fresh folder.
    fn reading(text: &str) -> Result<HostSettings, String> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), text).unwrap();
        HostSettings::open(dir.path())
    }

    #[test]
    fn no_record_means_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            HostSettings::open(dir.path()).unwrap(),
            HostSettings {
                theme: ThemePreference::Dark,
                material: MaterialPreference::Glass,
                open_pane: Shortcut::open_pane_default(),
                ..HostSettings::default()
            }
        );
    }

    #[test]
    fn missing_fields_mean_the_defaults_and_unknown_fields_are_ignored() {
        assert_eq!(
            reading(r#"{ "version": 1 }"#).unwrap(),
            HostSettings::default()
        );
        assert_eq!(
            reading(r#"{ "version": 1, "later": "a field a newer Pane writes" }"#).unwrap(),
            HostSettings::default()
        );
    }

    #[test]
    fn a_record_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut keyboard = Keyboard::default_for_this_system();
        keyboard
            .checked_set(
                KeyboardAction::Back,
                crate::keyboard::Binding::parse("ctrl-b").unwrap(),
            )
            .unwrap();
        let settings = HostSettings {
            theme: ThemePreference::System,
            material: MaterialPreference::Solid,
            open_pane: Shortcut::parse("ctrl+alt+b").unwrap(),
            tray_visible: false,
            show_taskbar: true,
            launch_at_login: true,
            opening_monitor: super::OpeningMonitor::Pointer,
            reopening: super::Reopening::After90Seconds,
            keyboard,
            window_mode: super::WindowMode::Compact,
            compact_pinned: true,
            pinned_layout: super::PinnedLayout::Vertical,
            escape: super::EscapeBehavior::Hide,
            escape_closes_settings: false,
            navigation: super::NavigationBindings::Emacs,
            background: Some("background-1.jpg".into()),
            background_effect: super::BackgroundEffect::Halftone,
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(HostSettings::open(dir.path()).unwrap(), settings);
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        for field in [
            "\"background\": \"background-1.jpg\"",
            "\"backgroundEffect\": \"halftone\"",
            "\"reopening\": \"after-90-seconds\"",
            "\"windowMode\": \"compact\"",
            "\"compactPinned\": true",
            "\"pinnedLayout\": \"vertical\"",
            "\"escapeBehavior\": \"hide\"",
            "\"escapeClosesSettings\": false",
            "\"navigationBindings\": \"emacs\"",
            "\"showTaskbar\": true",
        ] {
            assert!(text.contains(field), "the record is {text}");
        }
    }

    #[test]
    fn the_saved_navigation_bindings_keep_their_names_and_hold_alt() {
        use super::NavigationBindings;

        // A record written while the keys held Ctrl still names the same
        // choices; they now take Alt's keys (Control's on macOS).
        for (saved, navigation) in [
            ("\"none\"", NavigationBindings::None),
            ("\"emacs\"", NavigationBindings::Emacs),
            ("\"vim\"", NavigationBindings::Vim),
        ] {
            assert_eq!(
                serde_json::from_str::<NavigationBindings>(saved).unwrap(),
                navigation
            );
            assert_eq!(serde_json::to_string(&navigation).unwrap(), saved);
        }
        let modifier = if cfg!(target_os = "macos") {
            "ctrl"
        } else {
            "alt"
        };
        assert_eq!(NavigationBindings::None.bindings(), None);
        assert_eq!(NavigationBindings::None.left_right(), None);
        assert_eq!(
            NavigationBindings::Emacs.bindings(),
            Some((
                format!("{modifier}-p").as_str(),
                format!("{modifier}-n").as_str()
            ))
        );
        assert_eq!(
            NavigationBindings::Vim.bindings(),
            Some((
                format!("{modifier}-k").as_str(),
                format!("{modifier}-j").as_str()
            ))
        );
        // Left and Right: B and F under the Emacs choice, H and L under
        // the Vim one (#258).
        assert_eq!(
            NavigationBindings::Emacs.left_right(),
            Some((
                format!("{modifier}-b").as_str(),
                format!("{modifier}-f").as_str()
            ))
        );
        assert_eq!(
            NavigationBindings::Vim.left_right(),
            Some((
                format!("{modifier}-h").as_str(),
                format!("{modifier}-l").as_str()
            ))
        );
    }

    #[test]
    fn the_launcher_choices_are_written_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let settings = HostSettings {
            opening_monitor: super::OpeningMonitor::ActiveWindow,
            reopening: super::Reopening::RootSearch,
            ..HostSettings::default()
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(HostSettings::open(dir.path()).unwrap(), settings);
        // The fields are named as the house records name theirs, and the
        // values as the preferences name theirs, so the choices survive a
        // Pane that knows them by name alone.
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(
            text.contains("\"openingMonitor\": \"active-window\""),
            "the record is {text}"
        );
        assert!(
            text.contains("\"reopening\": \"root-search\""),
            "the record is {text}"
        );
        // A record without the fields is one an older Pane wrote: the
        // defaults, not an error.
        assert_eq!(
            reading(r#"{ "version": 1 }"#).unwrap().opening_monitor,
            super::OpeningMonitor::Pointer
        );
        assert_eq!(
            reading(r#"{ "version": 1 }"#).unwrap().reopening,
            super::Reopening::RestoreView
        );
    }

    #[test]
    fn the_keyboard_field_defaults_and_an_invalid_one_fails_the_record() {
        // Missing: this system's provisional defaults, and a partial set
        // keeps the rest.
        assert_eq!(
            reading(r#"{ "version": 1, "keys": "missing" }"#).unwrap(),
            HostSettings::default()
        );
        let settings = reading(r#"{ "version": 1, "keyboard": { "back": "ctrl-b" } }"#).unwrap();
        assert_eq!(
            settings.keyboard.binding(KeyboardAction::Back).id(),
            "ctrl-b"
        );
        assert_eq!(
            settings.keyboard.binding(KeyboardAction::NextResult).id(),
            "down"
        );
        // Two actions on one binding fails the whole record, as a binding
        // that is not one and an action that is not one do.
        for text in [
            r#"{ "version": 1, "keyboard": { "back": "up" } }"#,
            r#"{ "version": 1, "keyboard": { "back": "not one" } }"#,
            r#"{ "version": 1, "keyboard": { "launch": "ctrl-l" } }"#,
            r#"{ "version": 1, "keyboard": { "back": "b" } }"#,
        ] {
            let problem = reading(text);
            assert!(problem.is_err(), "{text} half-loads");
        }
    }

    #[test]
    fn showing_the_taskbar_while_the_launcher_is_open_defaults_to_off_and_is_written_and_read() {
        // Missing: off, so nothing shows a taskbar the user did not ask
        // for (#268).
        assert!(!HostSettings::default().show_taskbar);
        assert!(!reading(r#"{ "version": 1 }"#).unwrap().show_taskbar);
        // Recorded as the record's camelCase field, and read back.
        assert_eq!(
            reading(r#"{ "version": 1, "showTaskbar": true }"#).unwrap(),
            HostSettings {
                show_taskbar: true,
                ..HostSettings::default()
            }
        );
        // A value that is not a boolean fails the whole record.
        let problem = reading(r#"{ "version": 1, "showTaskbar": "yes" }"#);
        assert!(problem.is_err(), "{problem:?}");
    }

    #[test]
    fn a_folder_with_no_record_is_fresh_and_one_with_any_record_is_not() {
        // The fresh data folder is the one with no settings record at
        // all: its Open Pane hotkey starts with the fresh-install
        // default (#268), while a record — readable or not, naming the
        // hotkey or defaulting it — keeps the hotkey it has.
        let dir = tempfile::tempdir().unwrap();
        assert!(!HostSettings::recorded(dir.path()));
        HostSettings::default().save(dir.path()).unwrap();
        assert!(HostSettings::recorded(dir.path()));
        std::fs::write(dir.path().join(FILE), "{ not a record").unwrap();
        assert!(
            HostSettings::recorded(dir.path()),
            "an unreadable record is still a record, never replaced"
        );
    }

    #[test]
    fn the_tray_visibility_defaults_to_shown_and_a_non_boolean_fails_the_record() {
        // Missing: shown, the provisional default.
        assert_eq!(
            reading(r#"{ "version": 1, "tray": "missing" }"#).unwrap(),
            HostSettings::default()
        );
        // Recorded as the record's camelCase field, and read back.
        assert_eq!(
            reading(r#"{ "version": 1, "trayVisible": false }"#).unwrap(),
            HostSettings {
                tray_visible: false,
                ..HostSettings::default()
            }
        );
        // A value that is not a boolean fails the whole record.
        let problem = reading(r#"{ "version": 1, "trayVisible": "no" }"#);
        assert!(problem.is_err(), "{problem:?}");
    }

    #[test]
    fn the_open_pane_field_defaults_and_a_value_that_is_not_a_shortcut_fails_the_record() {
        // Missing: this system's provisional default.
        assert_eq!(
            reading(r#"{ "version": 1, "open pane": "missing" }"#).unwrap(),
            HostSettings::default()
        );
        // Recorded as the shortcut's id, on any system.
        assert_eq!(
            reading(r#"{ "version": 1, "open_pane": "alt+space" }"#).unwrap(),
            HostSettings {
                open_pane: Shortcut::parse("alt+space").unwrap(),
                ..HostSettings::default()
            }
        );
        // The binding kinds #260 add are the field's grammar too, recorded
        // as their ids and read back.
        for text in ["tap:win", "double:ctrl", "rctrl+space", "tap:ralt"] {
            assert_eq!(
                reading(&format!(r#"{{ "version": 2, "open_pane": "{text}" }}"#)).unwrap(),
                HostSettings {
                    open_pane: Shortcut::parse(text).unwrap(),
                    ..HostSettings::default()
                },
                "{text} does not round trip"
            );
        }
        // A value that is not a shortcut fails the whole record.
        let problem = reading(r#"{ "version": 1, "open_pane": "not a shortcut" }"#);
        assert!(problem.is_err(), "{problem:?}");
        assert!(
            problem
                .unwrap_err()
                .contains("its open pane hotkey is not one"),
            "the field is named"
        );
        let problem = reading(r#"{ "version": 2, "open_pane": "tap:escape" }"#);
        assert!(problem.is_err(), "{problem:?}");
    }

    #[test]
    fn the_launch_at_login_choice_is_written_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let settings = HostSettings {
            launch_at_login: true,
            ..HostSettings::default()
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(HostSettings::open(dir.path()).unwrap(), settings);
        // The field is named as the house records name their fields, so
        // the choice survives a Pane that knows it by name alone.
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(
            text.contains("\"launchAtLogin\": true"),
            "the record is {text}"
        );
        // A record without the field is one an older Pane wrote: the
        // choice defaults to off, not an error.
        assert!(!reading(r#"{ "version": 1 }"#).unwrap().launch_at_login);
    }

    #[test]
    fn showing_the_pins_in_compact_mode_defaults_to_off_and_is_written_and_read() {
        // Missing: off, so the compact window is the search field alone.
        assert!(!HostSettings::default().compact_pinned);
        assert!(!reading(r#"{ "version": 1 }"#).unwrap().compact_pinned);
        // Recorded as the record's camelCase field, and read back.
        assert_eq!(
            reading(r#"{ "version": 1, "compactPinned": true }"#).unwrap(),
            HostSettings {
                compact_pinned: true,
                ..HostSettings::default()
            }
        );
        // A value that is not a boolean fails the whole record.
        let problem = reading(r#"{ "version": 1, "compactPinned": "yes" }"#);
        assert!(problem.is_err(), "{problem:?}");
    }

    #[test]
    fn the_background_defaults_to_none_with_scanlines_and_names_only_a_file() {
        let settings = reading(r#"{ "version": 1 }"#).unwrap();
        assert_eq!(settings.background, None);
        assert_eq!(
            settings.background_effect,
            super::BackgroundEffect::Scanlines
        );
        // A record naming a path rather than a file in the backgrounds
        // folder fails to read, so nothing outside it is ever drawn.
        for text in [
            r#"{ "version": 1, "background": "../settings.json" }"#,
            r#"{ "version": 1, "background": "C:\\Windows\\a.jpg" }"#,
            r#"{ "version": 1, "background": "/etc/a.jpg" }"#,
            r#"{ "version": 1, "background": "" }"#,
            r#"{ "version": 1, "backgroundEffect": "sepia" }"#,
        ] {
            assert!(reading(text).is_err(), "{text} reads");
        }
    }

    #[test]
    fn an_unparseable_record_is_a_problem() {
        let problem = reading("{ not a record");
        assert!(problem.is_err(), "{problem:?}");
        assert!(problem.unwrap_err().contains("is invalid"));
    }

    #[test]
    fn another_versions_record_is_not_read() {
        // Version 1 still reads (the Open Pane field's first grammar, a
        // chord's id); version 3, a later Pane's, does not.
        assert_eq!(
            reading(r#"{ "version": 1, "theme": "light" }"#).unwrap(),
            HostSettings {
                theme: ThemePreference::Light,
                ..HostSettings::default()
            }
        );
        let problem = reading(r#"{ "version": 3, "theme": "light" }"#);
        assert!(problem.is_err(), "{problem:?}");
        assert!(
            problem
                .unwrap_err()
                .contains("which this Pane does not read"),
            "the version is named"
        );
    }

    #[test]
    fn a_record_without_a_version_is_not_read() {
        let problem = reading(r#"{ "theme": "light" }"#);
        assert!(problem.is_err(), "{problem:?}");
        assert!(problem.unwrap_err().contains("no version number"));
    }

    #[test]
    fn unknown_values_fail_the_whole_record() {
        for text in [
            r#"{ "version": 1, "theme": "sepia" }"#,
            r#"{ "version": 1, "material": "frost" }"#,
            r#"{ "version": 1, "theme": 3 }"#,
            r#"{ "version": 1, "launchAtLogin": "yes" }"#,
            r#"{ "version": 1, "openingMonitor": "nearest" }"#,
            r#"{ "version": 1, "reopening": "blank" }"#,
        ] {
            assert!(reading(text).is_err(), "{text} half-loads");
        }
    }

    #[test]
    fn a_failed_write_leaves_the_previous_record_whole() {
        let dir = tempfile::tempdir().unwrap();
        HostSettings::default().save(dir.path()).unwrap();
        // A folder where the record belongs: the atomic replacement fails,
        // and the record it would have replaced is still the old one.
        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        std::fs::create_dir(dir.path().join(FILE)).unwrap();
        let failed = HostSettings {
            theme: ThemePreference::Light,
            material: MaterialPreference::Solid,
            open_pane: Shortcut::parse("ctrl+alt+b").unwrap(),
            tray_visible: false,
            launch_at_login: true,
            opening_monitor: super::OpeningMonitor::Pointer,
            reopening: super::Reopening::RootSearch,
            keyboard: Keyboard::default_for_this_system(),
            ..HostSettings::default()
        }
        .save(dir.path());
        assert!(failed.is_err(), "{failed:?}");
        assert!(dir.path().join(FILE).is_dir());
        // The temporary files of the failed write are cleaned up.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [FILE]);
    }
}
