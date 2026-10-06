//! Preferences: typed values a package's manifest declares, for the whole
//! extension or for one command, and the user sets in Pane (#143).
//!
//! `pane.json` declares them under `preferences`, at the package level and
//! on each command. Pane reads and checks the declarations at install (an
//! invalid one refuses the package with the reason), keeps the values as
//! the package's extension data (see `extension_data`: a password as a local
//! credential, every other value as an extension setting), and hands a
//! command its effective values through `pane:extension/preferences`: the
//! package's preferences, then the command's own, each the stored value
//! (normalised: see [`normalised`]) or else its declared default.
//!
//! A required preference with no effective value is *unset*. A command with
//! an unset required preference is not run: a launch by the user shows the
//! Setup screen first (see the launcher's `setup`), and every other way in
//! (a background launch, root search's computed and indexed results, a
//! schedule, a continuing service) does not run it and its row says "Needs
//! setup".
//!
//! Values are text: a checkbox's is `true` or `false`, a dropdown's the
//! chosen option's `value`, a file's, folder's or application's a path.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::packages::Manifest;
use crate::platform::Platform;

/// The file a package ships beside `pane.json` to help the user set it up:
/// the Setup screen shows it beside the fields.
pub const HELP_FILE: &str = "HELP.md";

/// What kind of value a preference holds, and so the control that edits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PreferenceKind {
    /// One line of text.
    Text,
    /// A secret line of text, hidden as it is typed and kept as a local
    /// credential.
    Password,
    /// On or off, with a label beside it.
    Checkbox,
    /// One of the declared options.
    Dropdown,
    /// A file on this computer, by its path.
    File,
    /// A folder on this computer, by its path.
    Folder,
    /// An application on this computer, by its path.
    Application,
}

impl PreferenceKind {
    /// Every kind, in the order the manifest's documentation names them.
    pub const ALL: [PreferenceKind; 7] = [
        PreferenceKind::Text,
        PreferenceKind::Password,
        PreferenceKind::Checkbox,
        PreferenceKind::Dropdown,
        PreferenceKind::File,
        PreferenceKind::Folder,
        PreferenceKind::Application,
    ];

    /// The kind's name in `pane.json`'s `type`: `text`, `password`, …
    pub fn id(self) -> &'static str {
        match self {
            PreferenceKind::Text => "text",
            PreferenceKind::Password => "password",
            PreferenceKind::Checkbox => "checkbox",
            PreferenceKind::Dropdown => "dropdown",
            PreferenceKind::File => "file",
            PreferenceKind::Folder => "folder",
            PreferenceKind::Application => "application",
        }
    }

    fn parse(id: &str) -> Option<PreferenceKind> {
        PreferenceKind::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// Whether its values are secrets: kept as local credentials, never
    /// shown as typed.
    pub fn is_secret(self) -> bool {
        self == PreferenceKind::Password
    }

    /// Whether `value` is a value of this kind at all: a checkbox's is
    /// `true` or `false`; any text is a value of the other kinds (whether a
    /// dropdown's is among its options, or a path exists, is
    /// [`normalised`]'s question).
    pub fn fits(self, value: &str) -> bool {
        match self {
            PreferenceKind::Checkbox => matches!(value, "true" | "false"),
            _ => true,
        }
    }
}

/// One option of a dropdown preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferenceOption {
    /// What the command receives when it is chosen.
    pub value: String,
    /// What the user sees; the value when the manifest gives no title.
    pub title: String,
}

/// A preference a manifest declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preference {
    /// Names the value the command receives; unique among the package's
    /// preferences and each command's together.
    pub name: String,
    pub kind: PreferenceKind,
    /// Names the field on screen.
    pub title: String,
    /// What the value is for, shown under the field.
    pub description: Option<String>,
    /// Shown in an empty text field.
    pub placeholder: Option<String>,
    /// Whether the command needs a value to run.
    pub required: bool,
    /// The value used while the user set none, on this system: the
    /// manifest gives one for every system or one per system. A required
    /// preference with a default is never unset.
    pub default: Option<String>,
    /// A checkbox's label, beside it.
    pub label: Option<String>,
    /// A dropdown's options, in order.
    pub options: Vec<PreferenceOption>,
}

/// A preference as `pane.json` writes it.
#[derive(Deserialize)]
struct PreferenceJson {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    placeholder: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: Option<serde_json::Value>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    options: Option<Vec<OptionJson>>,
}

#[derive(Deserialize)]
struct OptionJson {
    value: String,
    #[serde(default)]
    title: Option<String>,
}

/// Reads the `preferences` of `whose` ("the package", "command `x`") from
/// `pane.json`, checked: each needs a `name` (no `#`) and a `title`, a
/// known `type`, a default of its kind (one for every system, or one per
/// system), and a dropdown its `options`, its default among them. A name in
/// `taken` (the package's, for a command's) or repeated is refused.
pub(crate) fn parse(
    values: Vec<serde_json::Value>,
    whose: &str,
    taken: &[Preference],
) -> Result<Vec<Preference>, String> {
    let mut read: Vec<Preference> = Vec::new();
    for value in values {
        let json: PreferenceJson = serde_json::from_value(value)
            .map_err(|error| format!("a preference of {whose} cannot be read: {error}"))?;
        let name = json.name.trim().to_owned();
        if name.is_empty() {
            return Err(format!("every preference of {whose} needs a `name`"));
        }
        if name.contains('#') {
            return Err(format!(
                "preference `{name}` of {whose} contains `#`, which preference names cannot"
            ));
        }
        if read.iter().chain(taken).any(|seen| seen.name == name) {
            return Err(format!(
                "preference `{name}` of {whose} is declared twice; a preference's `name` is \
                 unique among the package's preferences and each command's together"
            ));
        }
        let Some(kind) = PreferenceKind::parse(&json.kind) else {
            let known: Vec<&str> = PreferenceKind::ALL.iter().map(|kind| kind.id()).collect();
            return Err(format!(
                "preference `{name}` of {whose} has the type \"{}\"; a preference's `type` is \
                 one of {}",
                json.kind.escape_debug(),
                known.join(", ")
            ));
        };
        if json.title.trim().is_empty() {
            return Err(format!("preference `{name}` of {whose} needs a `title`"));
        }
        let options: Vec<PreferenceOption> = json
            .options
            .unwrap_or_default()
            .into_iter()
            .map(|option| PreferenceOption {
                title: option.title.unwrap_or_else(|| option.value.clone()),
                value: option.value,
            })
            .collect();
        if kind == PreferenceKind::Dropdown && options.is_empty() {
            return Err(format!(
                "dropdown preference `{name}` of {whose} has no `options`; give each its \
                 `value` and `title`"
            ));
        }
        let mut seen: Vec<&str> = Vec::new();
        for option in &options {
            if seen.contains(&option.value.as_str()) {
                return Err(format!(
                    "the option \"{}\" of preference `{name}` of {whose} is repeated",
                    option.value.escape_debug()
                ));
            }
            seen.push(&option.value);
        }
        let defaults = match json.default {
            None | Some(serde_json::Value::Null) => Defaults::default(),
            Some(serde_json::Value::Object(systems)) => {
                let mut defaults = Defaults::default();
                for (system, value) in systems {
                    let Some(platform) = Platform::ALL.into_iter().find(|p| p.id() == system)
                    else {
                        return Err(format!(
                            "the default of preference `{name}` of {whose} names the system \
                             \"{}\"; name windows, macos or linux",
                            system.escape_debug()
                        ));
                    };
                    let value = default_text(kind, value).map_err(|why| {
                        format!("the default of preference `{name}` of {whose} {why}")
                    })?;
                    defaults.per_system.push((platform, value));
                }
                defaults
            }
            Some(value) => Defaults {
                every: Some(default_text(kind, value).map_err(|why| {
                    format!("the default of preference `{name}` of {whose} {why}")
                })?),
                per_system: Vec::new(),
            },
        };
        if kind == PreferenceKind::Dropdown {
            for default in defaults.all() {
                if !options.iter().any(|option| option.value == default) {
                    return Err(format!(
                        "the default \"{}\" of preference `{name}` of {whose} is not among its \
                         options",
                        default.escape_debug()
                    ));
                }
            }
        }
        read.push(Preference {
            name,
            kind,
            title: json.title,
            description: json.description.filter(|text| !text.trim().is_empty()),
            placeholder: json.placeholder.filter(|text| !text.trim().is_empty()),
            required: json.required,
            default: defaults.here(),
            label: json.label.filter(|text| !text.trim().is_empty()),
            options,
        });
    }
    Ok(read)
}

/// A preference's declared defaults: one for every system, or one per
/// system.
#[derive(Default)]
struct Defaults {
    every: Option<String>,
    per_system: Vec<(Platform, String)>,
}

impl Defaults {
    fn all(&self) -> impl Iterator<Item = &str> {
        self.every
            .iter()
            .map(String::as_str)
            .chain(self.per_system.iter().map(|(_, value)| value.as_str()))
    }

    /// The default on this system.
    fn here(self) -> Option<String> {
        if self.every.is_some() {
            return self.every;
        }
        let current = Platform::current()?;
        self.per_system
            .into_iter()
            .find(|(platform, _)| *platform == current)
            .map(|(_, value)| value)
    }
}

/// A default's text, for a preference of `kind`: a checkbox's is a boolean,
/// every other kind's a string.
fn default_text(kind: PreferenceKind, value: serde_json::Value) -> Result<String, String> {
    match (kind, value) {
        (PreferenceKind::Checkbox, serde_json::Value::Bool(on)) => Ok(on.to_string()),
        (PreferenceKind::Checkbox, _) => Err("is not `true` or `false`".into()),
        (_, serde_json::Value::String(text)) => Ok(text),
        (kind, _) => Err(format!("is not text, as a {} preference's is", kind.id())),
    }
}

/// Where a value is kept: a package-level preference's under its name, a
/// command's under `<command id>#<name>` (command ids and preference names
/// have no `#`), so two commands may each declare a preference of the same
/// name.
pub(crate) fn storage_key(command: Option<&str>, name: &str) -> String {
    match command {
        Some(command) => format!("{command}#{name}"),
        None => name.to_owned(),
    }
}

/// A preference that applies to a command: the package's (`command` is
/// `None`) or the command's own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Declared<'a> {
    pub command: Option<&'a str>,
    pub preference: &'a Preference,
}

impl Declared<'_> {
    pub fn key(&self) -> String {
        storage_key(self.command, &self.preference.name)
    }
}

/// The preferences that apply to the command with id `command` of
/// `manifest`, in declaration order: the package's, then the command's.
/// Only the package's for a command the manifest does not have.
pub(crate) fn declared<'a>(manifest: &'a Manifest, command: &str) -> Vec<Declared<'a>> {
    let package = manifest.preferences.iter().map(|preference| Declared {
        command: None,
        preference,
    });
    let own = manifest
        .commands
        .iter()
        .filter(|declared| declared.id == command)
        .flat_map(|declared| {
            declared.preferences.iter().map(|preference| Declared {
                command: Some(declared.id.as_str()),
                preference,
            })
        });
    package.chain(own).collect()
}

/// Every preference `manifest` declares, the package's then each
/// command's.
pub(crate) fn all_declared(manifest: &Manifest) -> Vec<Declared<'_>> {
    let package = manifest.preferences.iter().map(|preference| Declared {
        command: None,
        preference,
    });
    let commands = manifest.commands.iter().flat_map(|command| {
        command.preferences.iter().map(|preference| Declared {
            command: Some(command.id.as_str()),
            preference,
        })
    });
    package.chain(commands).collect()
}

/// Whether `manifest` declares any preference that applies to the command
/// `command`: the package's or its own.
pub(crate) fn applies(manifest: &Manifest, command: &str) -> bool {
    !declared(manifest, command).is_empty()
}

/// The stored `value` of `preference`, normalised as Raycast does: a
/// dropdown's value no longer among its options, a checkbox's that is not
/// `true` or `false`, a file or folder that no longer exists, or an empty
/// value, is no value.
pub(crate) fn normalised(preference: &Preference, value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.is_empty() || !preference.kind.fits(value) {
        return None;
    }
    let kept = match preference.kind {
        PreferenceKind::Dropdown => preference.options.iter().any(|o| o.value == value),
        PreferenceKind::File => Path::new(value).is_file(),
        PreferenceKind::Folder => Path::new(value).is_dir(),
        PreferenceKind::Text
        | PreferenceKind::Password
        | PreferenceKind::Checkbox
        | PreferenceKind::Application => true,
    };
    kept.then(|| value.to_owned())
}

/// The value the command receives for `preference`: the normalised stored
/// value, else the declared default; `None` while it has neither.
pub(crate) fn effective(preference: &Preference, stored: Option<&str>) -> Option<String> {
    normalised(preference, stored).or_else(|| preference.default.clone())
}

/// The preferences of `declared` that are required and unset with the
/// `stored` values (by storage key), in declaration order.
pub(crate) fn unset<'a>(
    declared: &[Declared<'a>],
    stored: &BTreeMap<String, String>,
) -> Vec<Declared<'a>> {
    declared
        .iter()
        .filter(|declared| {
            declared.preference.required
                && effective(
                    declared.preference,
                    stored.get(&declared.key()).map(String::as_str),
                )
                .is_none()
        })
        .copied()
        .collect()
}

/// The effective values of `declared` with the `stored` values, as the
/// JSON object a command receives: each name to its value, a checkbox's as
/// a boolean (`false` while it has none), every other kind's as text, and a
/// name with no value left out. The package's come first and the command's
/// after, so a command's value is the one kept for a name both declare (the
/// manifest refuses that, so it does not happen).
pub(crate) fn values_json(declared: &[Declared<'_>], stored: &BTreeMap<String, String>) -> String {
    let mut values = serde_json::Map::new();
    for declared in declared {
        let preference = declared.preference;
        let value = effective(preference, stored.get(&declared.key()).map(String::as_str));
        let json = match (preference.kind, value) {
            (PreferenceKind::Checkbox, value) => {
                serde_json::Value::Bool(value.as_deref() == Some("true"))
            }
            (_, Some(value)) => serde_json::Value::String(value),
            (_, None) => continue,
        };
        values.insert(preference.name.clone(), json);
    }
    serde_json::Value::Object(values).to_string()
}

/// What happens to a stored value when its package is updated to
/// `manifest`: kept as it is (`Some` with the same kind), kept and moved to
/// where values of its new kind are kept (`Some` with another kind), or
/// dropped (`None`): its name is no longer declared, or its type changed so
/// that it no longer fits.
pub(crate) fn after_update(manifest: &Manifest, key: &str, value: &str) -> Option<PreferenceKind> {
    let (command, name) = match key.split_once('#') {
        Some((command, name)) => (Some(command), name),
        None => (None, key),
    };
    let preference = all_declared(manifest)
        .into_iter()
        .find(|declared| declared.command == command && declared.preference.name == name)?
        .preference;
    preference.kind.fits(value).then_some(preference.kind)
}

/// The paragraphs of the `HELP.md` the package in `folder` ships, as plain
/// text: blank lines separate them, and a line's leading Markdown heading
/// or list marks are left out. Empty when it ships none. Rendered as
/// Markdown once the extension UI has its Markdown component (#121).
pub(crate) fn help(folder: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(folder.join(HELP_FILE)) else {
        return Vec::new();
    };
    let mut paragraphs = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
            continue;
        }
        // A heading is a paragraph of its own.
        if line.starts_with('#') {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
            paragraphs.push(line.trim_start_matches('#').trim_start().to_owned());
            continue;
        }
        current.push(line.to_owned());
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one(value: serde_json::Value) -> Result<Preference, String> {
        parse(vec![value], "the package", &[]).map(|mut read| read.remove(0))
    }

    #[test]
    fn every_type_is_read() {
        for kind in PreferenceKind::ALL {
            let mut value = json!({"name": "p", "type": kind.id(), "title": "P"});
            if kind == PreferenceKind::Dropdown {
                value["options"] = json!([{"value": "a", "title": "A"}, {"value": "b"}]);
            }
            let read = one(value).unwrap();
            assert_eq!(read.kind, kind);
            if kind == PreferenceKind::Dropdown {
                assert_eq!(read.options[1].title, "b", "a title defaults to the value");
            }
        }
    }

    #[test]
    fn an_unknown_type_is_refused_with_the_reason() {
        let refused = one(json!({"name": "p", "type": "colour", "title": "P"})).unwrap_err();
        assert!(refused.contains("has the type \"colour\""), "{refused}");
        assert!(refused.contains("text, password, checkbox"), "{refused}");
    }

    #[test]
    fn a_duplicate_name_is_refused_also_across_the_package_and_a_command() {
        let twice = parse(
            vec![
                json!({"name": "p", "type": "text", "title": "P"}),
                json!({"name": "p", "type": "text", "title": "Q"}),
            ],
            "the package",
            &[],
        )
        .unwrap_err();
        assert!(twice.contains("declared twice"), "{twice}");
        let package = one(json!({"name": "p", "type": "text", "title": "P"})).unwrap();
        let across = parse(
            vec![json!({"name": "p", "type": "checkbox", "title": "P"})],
            "command `c`",
            &[package],
        )
        .unwrap_err();
        assert!(across.contains("declared twice"), "{across}");
    }

    #[test]
    fn a_dropdown_default_must_be_among_its_options_on_every_system() {
        let options = json!([{"value": "a"}, {"value": "b"}]);
        let refused = one(json!({"name": "p", "type": "dropdown", "title": "P",
            "options": options, "default": "c"}))
        .unwrap_err();
        assert!(refused.contains("is not among its options"), "{refused}");
        let per_system = one(json!({"name": "p", "type": "dropdown", "title": "P",
            "options": options, "default": {"windows": "a", "linux": "z"}}))
        .unwrap_err();
        assert!(per_system.contains("\"z\""), "{per_system}");
        let accepted = one(json!({"name": "p", "type": "dropdown", "title": "P",
            "options": options, "default": "b"}))
        .unwrap();
        assert_eq!(accepted.default.as_deref(), Some("b"));
    }

    #[test]
    fn a_default_per_system_is_this_systems() {
        let read = one(json!({"name": "p", "type": "text", "title": "P",
            "default": {"windows": "w", "macos": "m", "linux": "l"}}))
        .unwrap();
        let here = Platform::current().map(|platform| match platform {
            Platform::Windows => "w",
            Platform::Macos => "m",
            Platform::Linux => "l",
        });
        assert_eq!(read.default.as_deref(), here);
        let checkbox = one(json!({"name": "c", "type": "checkbox", "title": "C",
            "default": true}))
        .unwrap();
        assert_eq!(checkbox.default.as_deref(), Some("true"));
        let wrong = one(json!({"name": "c", "type": "checkbox", "title": "C",
            "default": "yes"}))
        .unwrap_err();
        assert!(wrong.contains("is not `true` or `false`"), "{wrong}");
    }

    #[test]
    fn stale_values_are_normalised_to_none() {
        let dropdown = one(json!({"name": "p", "type": "dropdown", "title": "P",
            "options": [{"value": "a"}]}))
        .unwrap();
        assert_eq!(normalised(&dropdown, Some("a")).as_deref(), Some("a"));
        assert_eq!(normalised(&dropdown, Some("gone")), None);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let folder = one(json!({"name": "f", "type": "folder", "title": "F"})).unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(normalised(&folder, Some(path)).as_deref(), Some(path));
        assert_eq!(normalised(&folder, Some(file.to_str().unwrap())), None);
        let files = one(json!({"name": "g", "type": "file", "title": "G"})).unwrap();
        assert!(normalised(&files, Some(file.to_str().unwrap())).is_some());
        assert_eq!(
            normalised(&files, Some(dir.path().join("gone").to_str().unwrap())),
            None
        );
    }

    #[test]
    fn help_is_read_as_plain_paragraphs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(help(dir.path()).is_empty());
        std::fs::write(
            dir.path().join(HELP_FILE),
            "# Getting a key\n\nOpen the site\nand copy it.\n\n\n## Folder\nPick one.\n",
        )
        .unwrap();
        assert_eq!(
            help(dir.path()),
            [
                "Getting a key",
                "Open the site and copy it.",
                "Folder",
                "Pick one."
            ]
        );
    }
}
