//! A command's arguments: up to three typed values it asks for before each
//! run (`"arguments"` in its `pane.json` entry).
//!
//! Each argument has a `name`, a `type` (`text`, `password` or
//! `dropdown`), an optional `placeholder`, `required` (false unless it
//! says) and, for a dropdown, its `options`. The install refuses a fourth
//! argument, a repeated name, an unknown type and a dropdown without
//! options, each with the reason.
//!
//! The values reach the command in its launch record by name, in the order
//! the command declares them; an argument without a value (an optional one
//! left empty, or a blank one) is absent. Text sent through the command's
//! alias or to it as a fallback fills its first text or password argument
//! unless that already has a value, and stays the launch record's fallback
//! text too (Raycast's rule). A command may be offered as a fallback when it
//! accepts fallback text (`"takesQuery": true`), or when its first argument
//! is text and every other argument is optional.
//!
//! A launch the user started that leaves a required argument without a
//! value asks for it first, in Pane's argument form (see
//! `launcher/argument_form`); a background launch that does is refused.

use serde::Deserialize;

use crate::dropdown::{self, DropdownOption, OptionJson};

/// The most arguments a command may declare.
pub const MAX_ARGUMENTS: usize = 3;

/// One of a command's arguments, as its `pane.json` entry declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestArgument {
    /// Its name: its value reaches the command by it.
    pub name: String,
    pub kind: ArgumentKind,
    /// What its field shows while it is empty; its name when the entry
    /// gives none.
    pub placeholder: Option<String>,
    /// Whether the command needs a value before it runs.
    pub required: bool,
}

/// What an argument holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgumentKind {
    /// Text the user types.
    Text,
    /// Text the user types, concealed while it is typed and recorded
    /// nowhere: not in any history, remembered value, log or crash record.
    Password,
    /// One of these options, the first chosen unless Pane remembers
    /// another (the last one chosen for this command).
    Dropdown(Vec<DropdownOption>),
}

impl ManifestArgument {
    /// What its field is called where it is shown: its placeholder, else
    /// its name.
    pub fn label(&self) -> &str {
        self.placeholder.as_deref().unwrap_or(&self.name)
    }

    /// Whether the user types its value: a text or password argument.
    pub fn is_typed(&self) -> bool {
        matches!(self.kind, ArgumentKind::Text | ArgumentKind::Password)
    }

    /// Whether `value` is one of a dropdown's options; any value suits a
    /// typed argument.
    pub(crate) fn accepts(&self, value: &str) -> bool {
        match &self.kind {
            ArgumentKind::Dropdown(options) => options.iter().any(|option| option.value == value),
            ArgumentKind::Text | ArgumentKind::Password => true,
        }
    }
}

/// Whether a command declaring `declared` may be offered as a fallback for
/// its arguments: its first argument is text and every other is optional.
/// (A command that takes a query may be one whatever its arguments.)
pub(crate) fn accept_fallback_text(declared: &[ManifestArgument]) -> bool {
    match declared.split_first() {
        Some((first, others)) => {
            first.kind == ArgumentKind::Text && others.iter().all(|other| !other.required)
        }
        None => false,
    }
}

/// Whether `value` counts as no value: empty, or only whitespace.
pub(crate) fn blank(value: &str) -> bool {
    value.trim().is_empty()
}

/// The values the arguments `declared` receive for a launch given the
/// values `given` by name and the fallback text `fallback_text`: each
/// given value that is not blank, then the fallback text in the first text
/// or password argument if that has none. In the order the command
/// declares them; a name it does not declare is left out (Pane checks a
/// guest's names before, with [`check_given`]).
pub(crate) fn fill(
    declared: &[ManifestArgument],
    given: &[(String, String)],
    fallback_text: Option<&str>,
) -> Vec<(String, String)> {
    let first_typed = declared.iter().position(ManifestArgument::is_typed);
    declared
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| {
            let given = given
                .iter()
                .find(|(name, value)| *name == argument.name && !blank(value))
                .map(|(_, value)| value.clone());
            let sent = || {
                fallback_text
                    .filter(|text| first_typed == Some(index) && !blank(text))
                    .map(str::to_owned)
            };
            given
                .or_else(sent)
                .map(|value| (argument.name.clone(), value))
        })
        .collect()
}

/// The first of the arguments `declared` that is required and has no value
/// among `values`.
pub(crate) fn first_missing<'a>(
    declared: &'a [ManifestArgument],
    values: &[(String, String)],
) -> Option<&'a ManifestArgument> {
    declared.iter().find(|argument| {
        argument.required
            && !values
                .iter()
                .any(|(name, value)| *name == argument.name && !blank(value))
    })
}

/// Checks the values another command passes with `launch` for the
/// arguments `declared`: each names a declared argument once, and a
/// dropdown's is one of its options. The reason names arguments and
/// dropdown values, never a typed value, which may be a password.
pub(crate) fn check_given(
    declared: &[ManifestArgument],
    given: &[(String, String)],
) -> Result<(), String> {
    for (index, (name, value)) in given.iter().enumerate() {
        let Some(argument) = declared.iter().find(|argument| argument.name == *name) else {
            return Err(match declared.len() {
                0 => format!("it declares no arguments, so it has no argument `{name}`"),
                _ => format!(
                    "it has no argument `{name}`; its arguments are {}",
                    declared
                        .iter()
                        .map(|argument| format!("`{}`", argument.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        };
        if given[..index].iter().any(|(earlier, _)| earlier == name) {
            return Err(format!("the argument `{name}` is passed twice"));
        }
        if !blank(value) && !argument.accepts(value) {
            let ArgumentKind::Dropdown(options) = &argument.kind else {
                unreachable!("a typed argument accepts any value");
            };
            return Err(format!(
                "\"{}\" is not an option of the argument `{name}`; its options are {}",
                value.escape_debug(),
                options
                    .iter()
                    .map(|option| format!("\"{}\"", option.value.escape_debug()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(())
}

/// An argument as `pane.json` writes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArgumentJson {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    placeholder: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    options: Option<Vec<OptionJson>>,
}

/// The `arguments` of the command with id `command`, as its `pane.json`
/// entry writes them, checked: at most [`MAX_ARGUMENTS`], each named, once,
/// with a known type, and a dropdown with its options. `Err` is the reason,
/// for the install to refuse the package with.
pub(crate) fn parse(
    command: &str,
    arguments: Option<serde_json::Value>,
) -> Result<Vec<ManifestArgument>, String> {
    let Some(arguments) = arguments else {
        return Ok(Vec::new());
    };
    let arguments: Vec<serde_json::Value> = serde_json::from_value(arguments)
        .map_err(|_| format!("the `arguments` of command `{command}` are not a list"))?;
    if arguments.len() > MAX_ARGUMENTS {
        return Err(format!(
            "command `{command}` declares {} arguments; a command has at most {MAX_ARGUMENTS}",
            arguments.len()
        ));
    }
    let mut parsed: Vec<ManifestArgument> = Vec::new();
    for argument in arguments {
        let argument: ArgumentJson = serde_json::from_value(argument)
            .map_err(|error| format!("an argument of command `{command}`: {error}"))?;
        let name = argument.name;
        if name.trim().is_empty() {
            return Err(format!(
                "an argument of command `{command}` has an empty `name`; every argument needs one"
            ));
        }
        if parsed.iter().any(|seen| seen.name == name) {
            return Err(format!(
                "command `{command}` declares the argument `{name}` twice; argument names are \
                 unique"
            ));
        }
        let kind = match (argument.kind.as_str(), argument.options) {
            ("text", None) => ArgumentKind::Text,
            ("password", None) => ArgumentKind::Password,
            ("text" | "password", Some(_)) => {
                return Err(format!(
                    "the argument `{name}` of command `{command}` has `options`, but only a \
                     dropdown has them"
                ));
            }
            ("dropdown", options) => ArgumentKind::Dropdown(dropdown::parse(
                &format!("the argument `{name}` of command `{command}`"),
                options,
            )?),
            (other, _) => {
                return Err(format!(
                    "the argument `{name}` of command `{command}` has the type \"{}\"; an \
                     argument's `type` is \"text\", \"password\" or \"dropdown\"",
                    other.escape_debug()
                ));
            }
        };
        parsed.push(ManifestArgument {
            name,
            kind,
            placeholder: argument
                .placeholder
                .filter(|placeholder| !placeholder.trim().is_empty()),
            required: argument.required,
        });
    }
    Ok(parsed)
}

/// Checks that a command with a schedule has no required argument: Pane
/// runs it in the background, where nobody can be asked for one.
pub(crate) fn check_scheduled(command: &str, declared: &[ManifestArgument]) -> Result<(), String> {
    match declared.iter().find(|argument| argument.required) {
        Some(argument) => Err(format!(
            "command `{command}` has a schedule, but its argument `{}` is required, which a \
             scheduled run cannot ask for; make it optional or remove the schedule",
            argument.name
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(arguments: serde_json::Value) -> Result<Vec<ManifestArgument>, String> {
        parse("greet", Some(arguments))
    }

    fn text(name: &str, required: bool) -> ManifestArgument {
        ManifestArgument {
            name: name.into(),
            kind: ArgumentKind::Text,
            placeholder: None,
            required,
        }
    }

    fn password(name: &str) -> ManifestArgument {
        ManifestArgument {
            kind: ArgumentKind::Password,
            ..text(name, false)
        }
    }

    fn dropdown(name: &str, values: &[&str]) -> ManifestArgument {
        ManifestArgument {
            kind: ArgumentKind::Dropdown(
                values
                    .iter()
                    .map(|value| DropdownOption {
                        value: (*value).into(),
                        title: (*value).into(),
                    })
                    .collect(),
            ),
            ..text(name, false)
        }
    }

    fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn three_arguments_of_each_type_are_read() {
        let arguments = parsed(json!([
            { "name": "name", "type": "text", "placeholder": "Name", "required": true },
            { "name": "secret", "type": "password" },
            { "name": "tone", "type": "dropdown",
              "options": ["warm", { "value": "brief", "title": "Brief" }] },
        ]))
        .unwrap();
        assert_eq!(
            arguments,
            [
                ManifestArgument {
                    placeholder: Some("Name".into()),
                    ..text("name", true)
                },
                password("secret"),
                ManifestArgument {
                    kind: ArgumentKind::Dropdown(vec![
                        DropdownOption {
                            value: "warm".into(),
                            title: "warm".into()
                        },
                        DropdownOption {
                            value: "brief".into(),
                            title: "Brief".into()
                        },
                    ]),
                    ..text("tone", false)
                },
            ]
        );
        assert_eq!(arguments[0].label(), "Name");
        assert_eq!(arguments[1].label(), "secret");
        assert_eq!(parse("greet", None), Ok(Vec::new()));
    }

    #[test]
    fn mistakes_are_refused_with_the_reason() {
        let one = json!({ "name": "a", "type": "text" });
        let refused = |arguments| parsed(arguments).unwrap_err();
        assert_eq!(
            refused(
                json!([one, { "name": "b", "type": "text" }, { "name": "c", "type": "text" },
                           { "name": "d", "type": "text" }])
            ),
            "command `greet` declares 4 arguments; a command has at most 3"
        );
        assert_eq!(
            refused(json!([one, one])),
            "command `greet` declares the argument `a` twice; argument names are unique"
        );
        assert_eq!(
            refused(json!([{ "name": "a", "type": "number" }])),
            "the argument `a` of command `greet` has the type \"number\"; an argument's `type` \
             is \"text\", \"password\" or \"dropdown\""
        );
        let without = "the argument `a` of command `greet` is a dropdown without `options`; list \
                       the choices it offers";
        assert_eq!(
            refused(json!([{ "name": "a", "type": "dropdown" }])),
            without
        );
        assert_eq!(
            refused(json!([{ "name": "a", "type": "dropdown", "options": [] }])),
            without
        );
        assert_eq!(
            refused(json!([{ "name": "a", "type": "dropdown", "options": ["x", "x"] }])),
            "the argument `a` of command `greet` offers the option \"x\" twice"
        );
        assert_eq!(
            refused(json!([{ "name": "a", "type": "text", "options": ["x"] }])),
            "the argument `a` of command `greet` has `options`, but only a dropdown has them"
        );
        assert_eq!(
            refused(json!([{ "name": " ", "type": "text" }])),
            "an argument of command `greet` has an empty `name`; every argument needs one"
        );
        assert!(refused(json!({ "name": "a" })).contains("are not a list"));
        assert!(
            refused(json!([{ "name": "a", "type": "text", "requried": true }]))
                .contains("requried")
        );
    }

    #[test]
    fn values_follow_the_declaration_and_blank_ones_are_absent() {
        let declared = [
            text("name", true),
            password("secret"),
            dropdown("tone", &["warm"]),
        ];
        assert_eq!(
            fill(
                &declared,
                &pairs(&[("tone", "warm"), ("secret", "  "), ("name", "Ada")]),
                None
            ),
            pairs(&[("name", "Ada"), ("tone", "warm")])
        );
        assert_eq!(first_missing(&declared, &[]), Some(&declared[0]));
        assert_eq!(
            first_missing(&declared, &pairs(&[("name", " ")])),
            Some(&declared[0])
        );
        assert_eq!(first_missing(&declared, &pairs(&[("name", "Ada")])), None);
    }

    #[test]
    fn fallback_text_fills_the_first_typed_argument_unless_it_has_a_value() {
        let declared = [
            dropdown("tone", &["warm"]),
            password("secret"),
            text("name", false),
        ];
        assert_eq!(
            fill(&declared, &[], Some("hello")),
            pairs(&[("secret", "hello")])
        );
        assert_eq!(
            fill(&declared, &pairs(&[("secret", "kept")]), Some("hello")),
            pairs(&[("secret", "kept")])
        );
        assert!(fill(&[dropdown("tone", &["warm"])], &[], Some("hello")).is_empty());
        assert!(fill(&declared, &[], Some("  ")).is_empty());
    }

    #[test]
    fn a_fallback_needs_a_first_text_argument_and_no_other_required() {
        assert!(accept_fallback_text(&[text("name", true)]));
        assert!(accept_fallback_text(&[
            text("name", true),
            password("secret"),
            dropdown("tone", &["warm"])
        ]));
        assert!(!accept_fallback_text(&[]));
        assert!(!accept_fallback_text(&[password("secret")]));
        assert!(!accept_fallback_text(&[
            dropdown("tone", &["warm"]),
            text("name", false)
        ]));
        assert!(!accept_fallback_text(&[
            text("name", false),
            text("other", true)
        ]));
    }

    #[test]
    fn values_another_command_passes_are_checked_without_echoing_typed_ones() {
        let declared = [
            text("name", true),
            password("secret"),
            dropdown("tone", &["warm", "brief"]),
        ];
        assert_eq!(
            check_given(&declared, &pairs(&[("name", "Ada"), ("tone", "brief")])),
            Ok(())
        );
        assert_eq!(
            check_given(&declared, &pairs(&[("colour", "red")])),
            Err("it has no argument `colour`; its arguments are `name`, `secret`, `tone`".into())
        );
        assert_eq!(
            check_given(&[], &pairs(&[("colour", "red")])),
            Err("it declares no arguments, so it has no argument `colour`".into())
        );
        assert_eq!(
            check_given(
                &declared,
                &pairs(&[("secret", "hunter2"), ("secret", "again")])
            ),
            Err("the argument `secret` is passed twice".into())
        );
        assert_eq!(
            check_given(&declared, &pairs(&[("tone", "loud")])),
            Err(
                "\"loud\" is not an option of the argument `tone`; its options are \"warm\", \
                 \"brief\""
                    .into()
            )
        );
    }

    #[test]
    fn a_scheduled_command_has_no_required_argument() {
        assert_eq!(check_scheduled("tick", &[text("name", false)]), Ok(()));
        assert_eq!(
            check_scheduled("tick", &[text("name", true)]),
            Err(
                "command `tick` has a schedule, but its argument `name` is required, which a \
                 scheduled run cannot ask for; make it optional or remove the schedule"
                    .into()
            )
        );
    }
}
