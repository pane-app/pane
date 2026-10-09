//! The JSON Schema of `pane.json`, generated from the manifest types Pane
//! itself reads (#224, ADR 0047): the `ManifestJson` layer in `packages`,
//! with the parts a raw `serde_json::Value` cannot describe — an icon, a
//! preference, an argument, a command's `mode`, a platform list, a helper's
//! targets — written here as types of their own, so the published schema
//! documents exactly what Pane accepts and cannot drift from it. `cargo
//! xtask schema` writes and checks the committed copy (see `xtask`); CI
//! regenerates and compares it, so editing the file by hand does not stick.
//!
//! These types are never used to parse anything: `packages` parses, and the
//! schema follows. Where a rule lives in code rather than in types (an
//! icon's shape, three arguments at most, the target names a helper's keys
//! may take), the types below repeat it and the tests pin the repetition to
//! the code that decides, so a change to either shows up as a failing test
//! or a changed schema.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde_json::Value;

use crate::packages::{MANIFEST_FILE, MANIFEST_VERSION, ManifestJson};
use crate::{MAX_ARGUMENTS, MAX_KEYWORDS};

/// The committed JSON Schema for `pane.json`, as text: every field a
/// `pane.json` may hold, each with its description, and the manifest
/// version pinned to the one this Pane reads. The schema is deterministic:
/// the same Pane source generates the same text, which is what CI compares.
///
/// Where the schema lives, and where it will be published: committed at
/// `guests/js/schema/pane.schema.json`, shipped inside the `@pane-app`
/// npm packages (their `files` lists name the folder), and — publishing
/// being a person's step, never CI's — eventually served at a stable URL
/// per manifest version, which a `pane.json` may then name in `$schema`
/// (Pane ignores that field like any unknown one).
pub fn manifest_schema() -> String {
    let mut schema = SchemaGenerator::default().into_root_schema_for::<ManifestJson>();
    let object = schema.ensure_object();
    object.insert("title".into(), "Pane package manifest".into());
    object.insert(
        "description".into(),
        format!(
            "A Pane extension package's `{MANIFEST_FILE}`: what the package is, the commands \
             it contributes to root search, the operations it publishes, the helpers it ships \
             and the preferences it declares. Pane ignores fields it does not know, so a \
             newer manifest may hold more than this schema describes."
        )
        .into(),
    );
    object.insert(
        "$comment".into(),
        "Generated from Pane's own manifest types (crates/pane-core) by `cargo xtask schema`; \
         regenerate with `cargo xtask schema --write` rather than editing by hand."
            .into(),
    );
    // `manifestVersion` is the one field the manifest layer checks before
    // deserializing (`packages` reads it first, so a newer format is
    // explained rather than misread), so no type carries it: the schema
    // states it directly, at the version this Pane reads.
    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(
            MANIFEST_VERSION_KEY.into(),
            json_schema!({
                "const": MANIFEST_VERSION,
                "description": "The pane.json format version: 1. A newer Pane is needed for \
                 a newer one."
            })
            .to_value(),
        );
    }
    if let Some(required) = object.get_mut("required").and_then(Value::as_array_mut) {
        let version = Value::String(MANIFEST_VERSION_KEY.into());
        if !required.contains(&version) {
            required.push(version);
        }
    }
    let mut text = serde_json::to_string_pretty(&schema)
        .expect("a generated schema is plain JSON, always writable");
    text.push('\n');
    text
}

/// The field `packages` checks before anything else: see
/// [`manifest_schema`].
const MANIFEST_VERSION_KEY: &str = "manifestVersion";

/// An icon, as `pane.json` may write it for a package or a command: a
/// built-in icon's name alone, or an object naming what it draws and how
/// it is drawn. Pane reads the same shape in the extension UI's trees, where
/// it may also name a URL, a file or an application — a manifest may not
/// (see `icons::parse_manifest_icon`); the schema describes what a manifest
/// accepts.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "Icon", rename_all = "camelCase")]
pub(crate) struct Icon {
    /// A built-in icon Pane has, by its reicon name in kebab case, such as
    /// `arrow-up-right`.
    pub(crate) builtin: Option<String>,
    /// Whether the built-in icon is drawn in its Filled weight instead of
    /// the Outline one.
    pub(crate) filled: Option<bool>,
    /// An image the package ships (PNG or SVG), by its path relative to the
    /// package folder: one file for both themes, which may also have
    /// `name@light.png` and `name@dark.png` variants beside it.
    pub(crate) path: Option<String>,
    /// The image drawn in the light theme, with `dark` naming the one drawn
    /// in the dark theme: a pair names both.
    pub(crate) light: Option<String>,
    /// The image drawn in the dark theme, with `light` naming the one drawn
    /// in the light theme.
    pub(crate) dark: Option<String>,
    /// The colour the icon is drawn in: a theme tone's name (`primary`,
    /// `secondary`, `accent`, `red`, `orange`, `yellow`, `green`, `blue`,
    /// `purple`, `magenta`) or a raw colour as `0xRRGGBBAA`, which Pane
    /// corrects for contrast against what it is drawn on; or one colour for
    /// each theme.
    pub(crate) tint: Option<Tint>,
    /// The shape the icon is clipped to: `circle` or `rounded-rectangle`.
    pub(crate) mask: Option<Mask>,
    /// What is drawn instead when this icon cannot be: another icon, in the
    /// same shape. Without one, a package without a working icon gets a
    /// first-letter tile.
    pub(crate) fallback: Option<Box<Icon>>,
    /// Shown on hover and read by assistive technology; without one the
    /// icon is decorative and assistive technology skips it.
    pub(crate) tooltip: Option<String>,
}

/// The colour an icon is drawn in, as `pane.json` writes it: one for both
/// themes, or one for each.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "Tint", untagged)]
pub(crate) enum Tint {
    /// One colour for both themes: a theme tone's name, or a raw colour as
    /// `0xRRGGBBAA`.
    One(String),
    /// One colour for the light theme and one for the dark.
    Pair {
        /// The colour for the light theme.
        light: String,
        /// The colour for the dark theme.
        dark: String,
    },
}

/// The shape an icon is clipped to.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "Mask", rename_all = "kebab-case")]
pub(crate) enum Mask {
    /// A circle.
    Circle,
    /// A rounded rectangle.
    RoundedRectangle,
}

/// An operating system a package, command, operation or dependency may
/// declare it supports. Kept to `pane_target::Platform` by
/// [`platforms_match`], below.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "Platform", rename_all = "lowercase")]
pub(crate) enum PlatformId {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    Macos,
    /// Linux.
    Linux,
}

/// One preference, as `pane.json` writes it for a package or a command: see
/// `preferences`, which parses it.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "Preference", rename_all = "camelCase")]
pub(crate) struct Preference {
    /// The preference's name, unique among the package's preferences and
    /// each command's together, without `#`: the command receives its value
    /// by it.
    pub(crate) name: String,
    /// What kind of value the preference holds, and so the control that
    /// edits it.
    #[serde(rename = "type")]
    pub(crate) kind: PreferenceType,
    /// The field's name on screen.
    pub(crate) title: String,
    /// What the value is for, shown under the field.
    pub(crate) description: Option<String>,
    /// Shown in an empty text field.
    pub(crate) placeholder: Option<String>,
    /// Whether the command needs a value before it runs; a declared
    /// `default` satisfies that.
    pub(crate) required: bool,
    /// The value used while the user set none: a checkbox's is `true` or
    /// `false`, a dropdown's must be among its `options`, any other's is
    /// text — or an object naming one default per system (`"windows"`,
    /// `"macos"`, `"linux"`).
    pub(crate) default: Option<Value>,
    /// A checkbox's label, beside it.
    pub(crate) label: Option<String>,
    /// A dropdown's choices, in order; only a dropdown has them.
    pub(crate) options: Option<Vec<crate::dropdown::OptionJson>>,
}

/// What kind of value a preference holds. Kept to `preferences::
/// PreferenceKind` by [`preference_types_match`], below.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "PreferenceType", rename_all = "lowercase")]
pub(crate) enum PreferenceType {
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
    /// Applications on this computer, by their file names, separated by
    /// commas.
    Applications,
}

/// One argument of a command, as `pane.json` writes it: see `arguments`,
/// which parses it.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "Argument", deny_unknown_fields)]
pub(crate) struct Argument {
    /// The argument's name, unique among the command's arguments: its value
    /// reaches the command by it, in its launch record.
    pub(crate) name: String,
    /// What the argument holds, and so the control that asks for it.
    #[serde(rename = "type")]
    pub(crate) kind: ArgumentType,
    /// What its field shows while it is empty; its name when the entry
    /// gives none.
    pub(crate) placeholder: Option<String>,
    /// Whether the command needs a value before it runs.
    pub(crate) required: bool,
    /// A dropdown's choices, in order; only a dropdown has them.
    pub(crate) options: Option<Vec<crate::dropdown::OptionJson>>,
}

/// What an argument holds. Kept to `arguments::ArgumentKind` by
/// [`argument_types_match`], below.
#[derive(JsonSchema)]
#[allow(dead_code)] // Read by the schema the derive generates, never by code.
#[serde(rename = "ArgumentType", rename_all = "lowercase")]
pub(crate) enum ArgumentType {
    /// Text the user types.
    Text,
    /// Text the user types, concealed while it is typed and recorded
    /// nowhere.
    Password,
    /// One of these options, the first chosen unless Pane remembers
    /// another.
    Dropdown,
}

/// A command's `arguments`, as the schema describes them: a list of at most
/// [`MAX_ARGUMENTS`] (what `arguments::parse` refuses beyond), or `null`,
/// which reads as the field's absence.
pub(crate) fn arguments(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = generator.subschema_for::<Vec<Argument>>();
    let object = schema.ensure_object();
    object.insert("maxItems".into(), Value::from(MAX_ARGUMENTS as u64));
    nullable(schema)
}

/// A package's `keywords`, as the schema describes them: a list of at most
/// [`MAX_KEYWORDS`] words (what `packages` refuses beyond), the empty ones
/// ignored.
pub(crate) fn keywords(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "items": {
            "type": "string",
            "description": "One word or short phrase a person would search for to find the \
             package."
        },
        "maxItems": MAX_KEYWORDS
    })
}

/// A helper's `targets`, as the schema describes them: the helper's file
/// for each system it is built for, keyed by target, with at least one
/// (what `packages` and `pane_target::Target` accept as a target).
pub(crate) fn targets(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "patternProperties": {
            "^(windows|macos|linux)-(x86_64|aarch64)$": {
                "type": "string",
                "description": "The helper's file for that target, a relative path inside the \
                 package folder."
            }
        },
        "additionalProperties": false,
        "minProperties": 1
    })
}

/// `schema`, also allowing `null`: a manifest field holding `null` reads as
/// its absence.
fn nullable(schema: Schema) -> Schema {
    json_schema!({
        "anyOf": [ schema, { "type": "null" } ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed schema is the generated one: the file this test
    /// compares is what `cargo xtask schema --write` writes, so a schema
    /// and the types that generate it cannot drift apart. Regenerate with
    /// `cargo xtask schema --write`.
    #[test]
    fn the_committed_schema_is_the_generated_one() {
        // The file is committed with LF endings and checked out with
        // whatever this system uses; the generated schema is one text.
        let committed =
            include_str!("../../../guests/js/schema/pane.schema.json").replace("\r\n", "\n");
        assert_eq!(
            committed,
            manifest_schema(),
            "the committed schema differs from the one Pane's manifest types generate; \
             regenerate it with `cargo xtask schema --write`"
        );
    }

    /// The schema's platform list is the one Pane reads.
    #[test]
    fn platforms_match() {
        use crate::platform::Platform;
        let values = enum_values::<PlatformId>();
        let expected: Vec<String> = Platform::ALL
            .iter()
            .map(|platform| platform.id().into())
            .collect();
        assert_eq!(values, expected);
    }

    /// The schema's preference types are the ones Pane reads.
    #[test]
    fn preference_types_match() {
        use crate::preferences::PreferenceKind;
        let values = enum_values::<PreferenceType>();
        let expected: Vec<String> = PreferenceKind::ALL
            .iter()
            .map(|kind| kind.id().into())
            .collect();
        assert_eq!(values, expected);
    }

    /// The schema's argument types are the ones Pane reads.
    #[test]
    fn argument_types_match() {
        let values = enum_values::<ArgumentType>();
        assert_eq!(values, ["text", "password", "dropdown"]);
    }

    /// The schema's helper target pattern accepts exactly the targets Pane
    /// reads, and no others.
    #[test]
    fn helper_target_keys_match() {
        use pane_target::{Arch, Platform, Target};
        let mut generator = SchemaGenerator::default();
        let schema = targets(&mut generator);
        let pattern = schema
            .get("patternProperties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.keys().next())
            .expect("helper targets are keyed by a pattern");
        let matches = target_pattern(pattern.as_str());
        for platform in Platform::ALL {
            for arch in Arch::ALL {
                let id = Target { os: platform, arch }.id();
                assert!(
                    matches(&id),
                    "the schema's helper target pattern does not accept {id}"
                );
            }
        }
        for refused in ["windows-x86", "beos-x86_64", "linux", ""] {
            assert!(
                !matches(refused),
                "the schema's helper target pattern accepts {refused:?}"
            );
        }
    }

    /// The string values the enum type `T` lists, as the schema writes them.
    fn enum_values<T: JsonSchema>() -> Vec<String> {
        let schema = SchemaGenerator::default().into_root_schema_for::<T>();
        schema
            .get("enum")
            .and_then(Value::as_array)
            .expect("an enum lists its values")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    }

    /// A small matcher for the fixed target pattern above, which needs no
    /// regular-expression engine: the pattern is two groups of alternatives
    /// joined by `-`.
    fn target_pattern(pattern: &str) -> impl Fn(&str) -> bool + '_ {
        let alternatives = |group: &str| {
            group
                .trim_start_matches('(')
                .trim_end_matches(')')
                .split('|')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let parts = pattern
            .trim_start_matches('^')
            .trim_end_matches('$')
            .split('-')
            .map(alternatives)
            .collect::<Vec<_>>();
        move |text: &str| {
            let words: Vec<&str> = text.split('-').collect();
            words.len() == parts.len()
                && words
                    .iter()
                    .zip(parts.iter())
                    .all(|(word, alternatives)| alternatives.iter().any(|a| a == word))
        }
    }
}
