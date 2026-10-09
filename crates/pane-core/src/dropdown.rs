//! A dropdown's options, as `pane.json` declares them for a dropdown
//! preference (#143) and a dropdown argument (#144): one list, read the
//! same way for both.
//!
//! An option is its value alone (`"celsius"`, shown as it is) or an object
//! with its `value` and an optional `title` (`{ "value": "c", "title":
//! "Celsius" }`; the title defaults to the value). A dropdown has at least
//! one option, each with a value that is not empty, none repeated.

use schemars::JsonSchema;
use serde::Deserialize;

/// One option of a dropdown: its `value` is what the command receives when
/// it is chosen, its `title` what the user sees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownOption {
    pub value: String,
    /// The value when the manifest gives no title.
    pub title: String,
}

/// An option as `pane.json` writes it.
#[derive(Deserialize, JsonSchema)]
#[schemars(inline)]
#[serde(untagged)]
pub(crate) enum OptionJson {
    /// The option's value, shown as it is.
    Value(String),
    Titled {
        /// What the command receives when the option is chosen.
        value: String,
        /// What the user sees; the value when the manifest gives no title.
        #[serde(default)]
        title: Option<String>,
    },
}

/// Reads the `options` of the dropdown `what` names ("the argument `tone`
/// of command `greet`", "preference `units` of the package"), checked: at
/// least one, each with a value, none repeated. `Err` is the reason, for
/// the install to refuse the package with.
pub(crate) fn parse(
    what: &str,
    options: Option<Vec<OptionJson>>,
) -> Result<Vec<DropdownOption>, String> {
    let options = options.unwrap_or_default();
    if options.is_empty() {
        return Err(format!(
            "{what} is a dropdown without `options`; list the choices it offers"
        ));
    }
    let mut parsed: Vec<DropdownOption> = Vec::new();
    for option in options {
        let option = match option {
            OptionJson::Value(value) => DropdownOption {
                title: value.clone(),
                value,
            },
            OptionJson::Titled { value, title } => DropdownOption {
                title: title
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| value.clone()),
                value,
            },
        };
        if option.value.is_empty() {
            return Err(format!("{what} has an option with an empty `value`"));
        }
        if parsed.iter().any(|seen| seen.value == option.value) {
            return Err(format!(
                "{what} offers the option \"{}\" twice",
                option.value.escape_debug()
            ));
        }
        parsed.push(option);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(options: serde_json::Value) -> Result<Vec<DropdownOption>, String> {
        let options: Vec<OptionJson> = serde_json::from_value(options).unwrap();
        parse("the argument `d`", Some(options))
    }

    #[test]
    fn an_option_is_a_bare_value_or_a_value_with_a_title() {
        let options = read(json!(["warm", {"value": "brief", "title": "Brief"}, {"value": "b"}]));
        assert_eq!(
            options.unwrap(),
            [
                DropdownOption {
                    value: "warm".into(),
                    title: "warm".into(),
                },
                DropdownOption {
                    value: "brief".into(),
                    title: "Brief".into(),
                },
                DropdownOption {
                    value: "b".into(),
                    title: "b".into(),
                },
            ]
        );
    }

    #[test]
    fn no_option_an_empty_value_or_a_repeated_one_is_refused() {
        assert_eq!(
            parse("the argument `d`", None).unwrap_err(),
            "the argument `d` is a dropdown without `options`; list the choices it offers"
        );
        assert_eq!(
            read(json!([""])).unwrap_err(),
            "the argument `d` has an option with an empty `value`"
        );
        assert_eq!(
            read(json!(["x", {"value": "x"}])).unwrap_err(),
            "the argument `d` offers the option \"x\" twice"
        );
    }
}
