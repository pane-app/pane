//! Root search's inline argument fields (#205): the fields a command's
//! row shows after the query in the search field while the row is
//! selected, which replace the argument form there. The form stays for
//! every way in that has no fields of its own: a global hotkey, a quick
//! slot, another command's launch (see `argument_form`).
//!
//! The fields show for the selected row only, when its command declares
//! arguments (`arguments`, #120: at most three, of text, password and
//! dropdown). The window draws them and owns their focus; the launcher
//! holds their values, so they survive the list being rebuilt — a late
//! answer's re-rank keeps them while the same row is selected again, a
//! dropdown's while it is still a choice. A launch from root search —
//! the selected row's own, or through its alias — carries the values,
//! and one that leaves a required argument blank marks the field, says
//! "Enter <placeholder>" in the status line and runs nothing, in place
//! of the form. The values are the search's own state, never recorded:
//! they go when the query does, and a password's value is recorded
//! nowhere whatever happens. A dropdown's last choice is remembered per
//! command as the form remembers it (the `arguments.json` record, see
//! `choices`) and offered again while it is still a choice.
//!
//! Typing a command's alias and a space (or Tab after the alias alone)
//! is the user's way into the fields: the window focuses the command's
//! first empty argument and routes what is typed next into the first
//! text or password one (see `aliases`). A command without arguments
//! that takes no query opens at once instead, as Raycast does; one that
//! accepts fallback text keeps the row that sends the text after the
//! alias on Enter, since that text is its input.

use super::{Entry, FormField, Launcher, Screen, State, Status, argument_form, owner};
use crate::arguments::{self, ArgumentKind, ManifestArgument};
use crate::runtime::{Choice, FieldKind};

/// The fields of root search's selected row, as the window draws them
/// inline after the query (see [`Launcher::argument_fields`]): the
/// command's title, which the query field's placeholder becomes while
/// they show, and one field per argument the command declares, in the
/// order it declares them, with the values typed and the required ones
/// left blank marked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgumentFields {
    /// The command's title: the query field's placeholder while the
    /// fields show, so the user knows what they are filling in.
    pub title: String,
    /// One field per argument, as the argument form's fields are: a text
    /// field, a concealed one for a password, a choice for a dropdown —
    /// the value typed (or the dropdown's remembered choice), and a
    /// required one left blank carrying its mark as the field's `error`.
    pub fields: Vec<FormField>,
}

/// The values typed into the argument fields of one command's row, as
/// the launcher holds them between the list's rebuilds (#205): which
/// command they belong to, the values by argument name, and the required
/// arguments the user left blank at least once. An empty `command`
/// holds nothing: no fields have been typed into.
#[derive(Clone, Debug, Default)]
pub(super) struct Typed {
    /// The command whose fields these are, by its id in Pane's records.
    command: String,
    /// The values typed, by argument name; blank is no value.
    values: Vec<(String, String)>,
    /// The required arguments left blank at least once, by name: each
    /// mark clears when its argument gets a value.
    missing: Vec<String>,
}

impl Typed {
    /// The values typed for `command`, if these are them.
    pub(super) fn of(&self, command: &str) -> Option<&Vec<(String, String)>> {
        (!self.command.is_empty() && self.command == command).then(|| &self.values)
    }

    /// The value typed for the argument `name` of `command`, if any.
    fn value(&self, command: &str, name: &str) -> Option<&str> {
        self.of(command)?
            .iter()
            .find(|(argument, value)| argument == name && !arguments::blank(value))
            .map(|(_, value)| value.as_str())
    }

    /// Whether the argument `name` of `command` carries the missing mark.
    fn marked(&self, command: &str, name: &str) -> bool {
        !self.command.is_empty()
            && self.command == command
            && self.missing.iter().any(|missing| missing == name)
    }

    /// Sets the `value` of the argument `name` of `command`, clearing its
    /// missing mark; the values become `command`'s, whatever row they
    /// were another command's before.
    fn set(&mut self, command: &str, name: &str, value: &str) {
        if self.command != command {
            self.command = command.to_owned();
            self.values.clear();
            self.missing.clear();
        }
        match self
            .values
            .iter_mut()
            .find(|(argument, _)| argument == name)
        {
            Some(found) => found.1 = value.to_owned(),
            None => self.values.push((name.to_owned(), value.to_owned())),
        }
        self.missing.retain(|missing| missing != name);
    }

    /// Removes the value of the argument `name` (typing made it blank);
    /// the values stay `command`'s.
    fn unset(&mut self, command: &str, name: &str) {
        if self.command != command {
            return;
        }
        self.values.retain(|(argument, _)| argument != name);
    }

    /// The command the values belong to and the values by argument name,
    /// as they were typed (#206): `None` while the search holds no values
    /// — an empty `command` holds none. What the search history records
    /// with a query.
    pub(super) fn recorded(&self) -> Option<(&str, &[(String, String)])> {
        (!self.command.is_empty()).then(|| (self.command.as_str(), self.values.as_slice()))
    }

    /// The values a recent query's entry carries, as the search's own
    /// state again (#206): the command's fields' values, as they were
    /// recorded — a password's empty — with no argument marked missing.
    pub(super) fn recalled(command: &str, values: &[(String, String)]) -> Typed {
        Typed {
            command: command.to_owned(),
            values: values.to_vec(),
            missing: Vec::new(),
        }
    }

    /// Marks the argument `name` of `command` missing: left blank once.
    pub(super) fn mark(&mut self, command: &str, name: &str) {
        if self.command != command {
            self.command = command.to_owned();
            self.values.clear();
            self.missing.clear();
        }
        if !self.missing.iter().any(|missing| missing == name) {
            self.missing.push(name.to_owned());
        }
    }
}

impl Launcher {
    /// The argument fields of root search's selected row, as the window
    /// draws them inline after the query (#205): the command's title and
    /// one field per argument it declares, with the values typed and the
    /// required ones left blank marked. `None` while none show: not on
    /// root search, or the row's command declares no arguments, or the
    /// row is not a command's own (a row that sends the query never
    /// shows fields).
    pub fn argument_fields(&self) -> Option<ArgumentFields> {
        fields_of(&self.lock())
    }

    /// Sets the value typed into the argument field `name` of the
    /// selected row's command (#205): the value the command is invoked
    /// with, clearing the field's missing mark. A name the command does
    /// not declare, or a dropdown value that is not one of its options,
    /// is left out.
    pub fn set_argument_value(&self, name: &str, value: &str) {
        let mut state = self.lock();
        let Some((declared, command, _)) = selected_declared(&state) else {
            return;
        };
        let Some(argument) = declared.iter().find(|argument| argument.name == name) else {
            return;
        };
        if arguments::blank(value) || !argument.accepts(value) {
            state.arguments.unset(&command, name);
            return;
        }
        state.arguments.set(&command, name, value);
    }

    /// Notes that the argument field `name` was left (#205): a required
    /// one left blank is marked missing from then on — never before the
    /// user has left it — until it gets a value. Nothing happens to an
    /// optional field, or one with a value.
    pub fn argument_left(&self, name: &str) {
        let mut state = self.lock();
        let Some((declared, command, _)) = selected_declared(&state) else {
            return;
        };
        let Some(argument) = declared.iter().find(|argument| argument.name == name) else {
            return;
        };
        if argument.required
            && state
                .arguments
                .value(&command, name)
                .is_none_or(|value| arguments::blank(value))
        {
            state.arguments.mark(&command, name);
        }
    }

    /// Enter pressed inside the argument field `name` (#205): a required
    /// one left blank is marked and the status line says "Enter
    /// <placeholder>"; whether it was, so the key runs nothing else. A
    /// field with a value, or an optional one, leaves the launch to go
    /// on.
    pub fn argument_entered(&self, name: &str) -> bool {
        let mut state = self.lock();
        let Some((declared, command, _)) = selected_declared(&state) else {
            return false;
        };
        let Some(argument) = declared.iter().find(|argument| argument.name == name) else {
            return false;
        };
        if !argument.required
            || state
                .arguments
                .value(&command, name)
                .is_some_and(|value| !arguments::blank(value))
        {
            return false;
        }
        state.arguments.mark(&command, name);
        state.view.status = Status::Error(format!("Enter {}", argument.label()));
        true
    }
}

/// The arguments the selected row of root search's list asks for: their
/// declarations, the command's id in Pane's records and its title.
/// `None` while the row's command declares none (or is not an installed
/// command's own row).
fn selected_declared(state: &State) -> Option<(Vec<ManifestArgument>, String, String)> {
    if !matches!(state.view.screen, Screen::Root { .. }) {
        return None;
    }
    let entry = state
        .view
        .selected
        .and_then(|index| state.entries.get(index))?;
    let Entry::Open(opening) = entry else {
        return None;
    };
    argument_form::declared(state, opening)
}

/// The fields of root search's selected row, as the window draws them
/// (see [`Launcher::argument_fields`]).
pub(super) fn fields_of(state: &State) -> Option<ArgumentFields> {
    let Some((declared, command, title)) = selected_declared(state) else {
        return None;
    };
    let typed = &state.arguments;
    let remembered = &state.remembered_arguments.chosen;
    let fields = declared
        .iter()
        .map(|argument| {
            let value = typed.value(&command, &argument.name).unwrap_or("");
            // A dropdown with no value typed chooses the one remembered
            // for the command, while it is still a choice; the leading
            // empty choice the inline dropdown lists is the window's.
            let value = match (&argument.kind, arguments::blank(value)) {
                (ArgumentKind::Dropdown(_), true) => remembered
                    .remembered(&command, &argument.name)
                    .filter(|value| argument.accepts(value))
                    .unwrap_or(""),
                _ => value,
            };
            let error = (argument.required
                && arguments::blank(value)
                && typed.marked(&command, &argument.name))
            .then(|| argument_form::MISSING.to_owned());
            FormField {
                id: argument.name.clone(),
                label: argument.label().to_owned(),
                kind: field_kind(argument),
                value: value.to_owned(),
                error,
                description: None,
                required: argument.required,
            }
        })
        .collect();
    Some(ArgumentFields { title, fields })
}

/// The field the window draws for `argument`: a text field, a concealed
/// one for a password, or the dropdown's options as a choice — the
/// empty choice the inline dropdown lists before them is the window's
/// to add.
pub(super) fn field_kind(argument: &ManifestArgument) -> FieldKind {
    match &argument.kind {
        ArgumentKind::Text => FieldKind::Text {
            placeholder: Some(argument.label().to_owned()),
        },
        ArgumentKind::Password => FieldKind::Password {
            placeholder: Some(argument.label().to_owned()),
        },
        ArgumentKind::Dropdown(options) => FieldKind::Choice(
            options
                .iter()
                .map(|option| Choice {
                    id: option.value.clone(),
                    label: option.title.clone(),
                })
                .collect(),
        ),
    }
}

/// The arguments the installed command of `component` with manifest id
/// `command` declares, for the rows that send the query and the alias
/// flow to ask about (see `aliases`); empty for a command built into
/// Pane.
pub(super) fn declared_of(
    state: &State,
    component: &std::path::Path,
    command: &str,
) -> Vec<ManifestArgument> {
    owner(&state.packages, component)
        .map(|package| package.arguments_of(command).to_vec())
        .unwrap_or_default()
}
