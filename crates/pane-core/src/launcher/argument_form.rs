//! The argument form: Pane asks for a command's arguments (see
//! `arguments`) before a launch the user started runs it without a value
//! for a required one (proposed: a host-rendered form, until root search
//! has inline argument fields, #122, and afterwards for every way in that
//! has none: a global hotkey, a quick slot, another command's launch).
//!
//! Every launch passes through [`Launcher::ask_for_arguments`] on its way
//! to running (`launching`): the command's arguments are filled from what
//! the launch was given (another command's values, the text sent through
//! the alias or as a fallback), and when a required one is still without a
//! value Pane shows the form instead of running the command:
//!
//! - It is Pane's own form screen (`Screen::Form`), titled with the
//!   command's title, with one field per argument in declaration order:
//!   a text field, a concealed field for a password, a choice for a
//!   dropdown, each showing its placeholder. The window focuses the first
//!   required field that is empty.
//! - Submitting it with a required field still empty marks that field and
//!   says so, which takes focus to it, and runs nothing. Submitting it
//!   filled launches the command once, with the values, from the screen it
//!   was asked from (root search, with the query kept).
//! - Back (Escape) launches nothing and returns to that screen.
//!
//! A background launch never asks: another command's is refused before it
//! starts when a required argument has no value (`launching`), and a
//! command with a required argument cannot have a schedule.
//!
//! Each dropdown's value is remembered per command, as Pane's own record
//! `arguments.json` beside `installed.json` (see `choices`), and chosen
//! next time; it survives a restart and is forgotten on uninstall. A
//! password's value is never recorded: it is not remembered, and Pane
//! writes it into no history, log or crash record; it lives only in the
//! form and the launch record.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::choices::{Choices, Record};
use super::{
    FormField, FormPurpose, FormView, Launcher, LauncherView, OpenForm, Opening, Screen, State,
    Status, off_thread, owner,
};
use crate::arguments::{self, ArgumentKind, ManifestArgument};
use crate::extension_data::PackageData;
use crate::launch::LaunchSource;
use crate::packages::{InstalledPackage, PackageIdentity, paused_reason};
use crate::runtime::{Choice, FieldKind};

/// What a required field left empty says, next to it and in the status
/// line after its label.
pub(super) const MISSING: &str = "Enter a value to run the command";

/// The dropdown values each command was last launched with, recorded in
/// `arguments.json` as `{ "version": 1, "dropdowns": { "<command id>": {
/// "<argument>": "<value>" } } }`. Only dropdowns: a typed value, a
/// password above all, is never recorded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ArgumentChoices {
    dropdowns: BTreeMap<String, BTreeMap<String, String>>,
}

impl Choices for ArgumentChoices {
    const FILE: &'static str = "arguments.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "remembered arguments";

    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let mut choices = ArgumentChoices::default();
        let Some(dropdowns) = fields.get("dropdowns") else {
            return Ok(choices);
        };
        let dropdowns = dropdowns
            .as_object()
            .ok_or("`dropdowns` is not an object")?;
        for (command, values) in dropdowns {
            let Some(values) = values.as_object() else {
                continue;
            };
            let values: BTreeMap<String, String> = values
                .iter()
                .filter_map(|(argument, value)| Some((argument.clone(), value.as_str()?.into())))
                .collect();
            if !values.is_empty() {
                choices.dropdowns.insert(command.clone(), values);
            }
        }
        Ok(choices)
    }

    fn write(&self) -> Map<String, Value> {
        let dropdowns = self
            .dropdowns
            .iter()
            .map(|(command, values)| {
                let values = values
                    .iter()
                    .map(|(argument, value)| (argument.clone(), Value::String(value.clone())))
                    .collect();
                (command.clone(), Value::Object(values))
            })
            .collect();
        Map::from_iter([("dropdowns".to_string(), Value::Object(dropdowns))])
    }

    fn restore(&mut self, command: &str, other: &Self) {
        match other.dropdowns.get(command) {
            Some(values) => self.dropdowns.insert(command.to_owned(), values.clone()),
            None => self.dropdowns.remove(command),
        };
    }

    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = self.dropdowns.len();
        self.dropdowns.retain(|command, _| keep(command));
        self.dropdowns.len() != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.remembered_arguments
    }
}

impl ArgumentChoices {
    /// The value of the dropdown `argument` the command `command` was last
    /// launched with.
    pub(super) fn remembered(&self, command: &str, argument: &str) -> Option<&str> {
        self.dropdowns
            .get(command)?
            .get(argument)
            .map(String::as_str)
    }

    /// Remembers `value` for the dropdown `argument` of `command`; whether
    /// it changed what was remembered.
    fn remember(&mut self, command: &str, argument: &str, value: &str) -> bool {
        let values = self.dropdowns.entry(command.to_owned()).or_default();
        values
            .insert(argument.to_owned(), value.to_owned())
            .as_deref()
            != Some(value)
    }
}

/// A launch waiting in the argument form for its arguments.
pub(super) struct Asking {
    opening: Opening,
    /// The command's id in Pane's records, `<identity key>#<manifest id>`.
    command: String,
    declared: Vec<ManifestArgument>,
}

/// A launch whose arguments the form gave, for [`Launcher::submit_form`]'s
/// future to start ([`Launcher::launch_submitted`]).
pub(super) struct Submitted {
    opening: Opening,
    epoch: u64,
    data: Option<PackageData>,
    /// The command whose remembered dropdown values changed, to record.
    remembered: Option<String>,
}

/// The arguments the command `opening` launches declares, its id in
/// Pane's records and its title; `None` for a command that declares none
/// (or one built into Pane).
pub(super) fn declared(
    state: &State,
    opening: &Opening,
) -> Option<(Vec<ManifestArgument>, String, String)> {
    let package = owner(&state.packages, &opening.component)?;
    let declared = package.arguments_of(&opening.command);
    if declared.is_empty() {
        return None;
    }
    Some((
        declared.to_vec(),
        package.identity.command_id(&opening.command),
        command_title(package, &opening.command),
    ))
}

/// The title of `package`'s command with manifest id `command`.
fn command_title(package: &InstalledPackage, command: &str) -> String {
    package
        .manifest
        .as_ref()
        .ok()
        .and_then(|manifest| manifest.commands.iter().find(|c| c.id == command))
        .map_or_else(|| command.to_owned(), |command| command.title.clone())
}

/// Whether launching `opening` now would show the argument form: a launch
/// the user started that leaves a required argument without a value. A
/// no-view command's global hotkey then shows Pane's window
/// ([`Launcher::hotkey_shows_window`]).
pub(super) fn asks_first(state: &State, opening: &Opening) -> bool {
    let Some((declared, ..)) = declared(state, opening) else {
        return false;
    };
    let launch = &opening.launch;
    let values = arguments::fill(
        &declared,
        &launch.arguments,
        launch.fallback_text.as_deref(),
    );
    !launch.is_background() && arguments::first_missing(&declared, &values).is_some()
}

impl Launcher {
    /// The argument form's step of a launch: fills `opening`'s arguments
    /// from what the launch was given and returns it, ready to launch,
    /// with the command whose dropdown values to remember; `None` when
    /// a required argument is still without a value — the form is shown
    /// then, except in root search, where the selected row's fields are
    /// on screen in the form's place: the blank required one is marked
    /// and named in the status line, and nothing runs (#205, see
    /// `argument_fields`). Nothing is shown, and nothing launched, once
    /// the screen of `epoch` was left, or for a background launch (a
    /// guest's is refused before it starts).
    pub(super) fn ask_for_arguments(
        &self,
        epoch: u64,
        mut opening: Opening,
    ) -> Option<(Opening, Option<String>)> {
        let mut state = self.lock();
        let Some((declared, command, title)) = declared(&state, &opening) else {
            return Some((opening, None));
        };
        // Root search's inline fields (#205) carry the values typed into
        // them when the launch is the selected row's own — from root
        // search, or through its alias. Every other way in (a hotkey, a
        // quick slot, another command) asks with what the launch was
        // given, as it always did.
        let inline = matches!(state.view.screen, Screen::Root { .. })
            && matches!(
                opening.launch.source,
                LaunchSource::RootSearch | LaunchSource::Alias
            );
        if inline && let Some(typed) = state.arguments.of(&command) {
            opening.launch.arguments = typed.clone();
        }
        let launch = &mut opening.launch;
        launch.arguments = arguments::fill(
            &declared,
            &launch.arguments,
            launch.fallback_text.as_deref(),
        );
        if arguments::first_missing(&declared, &launch.arguments).is_none() {
            // The inline fields' dropdown values are remembered as the
            // form remembers them on submitting; the form's own are,
            // when it is submitted.
            let remembered = if inline {
                remember_dropdowns(&mut state, &command, &declared, &launch.arguments)
            } else {
                None
            };
            return Some((opening, remembered));
        }
        if launch.is_background() || state.screen_epoch != epoch {
            return None;
        }
        if inline {
            // The fields replace the form here: the blank required one is
            // marked and the status line names it, and nothing runs —
            // the window takes focus to it (#205).
            if let Some(missing) = arguments::first_missing(&declared, &launch.arguments) {
                state.arguments.mark(&command, &missing.name);
                state.view.status = Status::Error(format!("Enter {}", missing.label()));
            }
            return None;
        }
        let asking = Asking {
            opening,
            command,
            declared,
        };
        self.show_argument_form(&mut state, asking, title);
        None
    }

    /// Shows the argument form for `asking`, titled `title`, over the
    /// screen it was asked from: root search or a command's list (any
    /// other screen gives way to root search first, as a view command's
    /// hotkey does). Values the launch was given fill their fields; a
    /// dropdown chooses the value given, else the one remembered for the
    /// command, else its first option. A launch another command asked for
    /// also asks for Pane's window, which may be hidden.
    fn show_argument_form(&self, state: &mut State, asking: Asking, title: String) {
        if !matches!(
            state.view.screen,
            Screen::Root { .. } | Screen::Command | Screen::CommandSearch { .. }
        ) {
            self.show_root(state, None);
        }
        state.actions_return = None;
        state.sent_from = None;
        let launch = &asking.opening.launch;
        let remembered = &state.remembered_arguments.chosen;
        let fields = asking
            .declared
            .iter()
            .map(|argument| {
                let given = launch.argument(&argument.name);
                let placeholder = Some(argument.label().to_owned());
                let (kind, value) = match &argument.kind {
                    ArgumentKind::Text => (FieldKind::Text { placeholder }, given),
                    ArgumentKind::Password => (FieldKind::Password { placeholder }, given),
                    ArgumentKind::Dropdown(options) => {
                        let choices = options
                            .iter()
                            .map(|option| Choice {
                                id: option.value.clone(),
                                label: option.title.clone(),
                            })
                            .collect();
                        let value = given
                            .or_else(|| {
                                remembered
                                    .remembered(&asking.command, &argument.name)
                                    .filter(|value| argument.accepts(value))
                            })
                            .or_else(|| options.first().map(|option| option.value.as_str()));
                        (FieldKind::Choice(choices), value)
                    }
                };
                FormField {
                    id: argument.name.clone(),
                    label: argument.label().to_owned(),
                    kind,
                    value: value.unwrap_or_default().to_owned(),
                    error: None,
                    description: None,
                    required: argument.required,
                }
            })
            .collect();
        let submit_label = if asking.opening.no_view {
            "Run command"
        } else {
            "Open command"
        };
        let form = FormView {
            fields,
            submit_label: submit_label.into(),
            setup: None,
        };
        if asking.opening.launch.source == LaunchSource::Command {
            state.window_wanted = true;
        }
        let view = LauncherView::new(Screen::Form(form), title);
        let mut return_to = std::mem::replace(&mut state.view, view);
        return_to.status = Status::Idle;
        state.form = Some(OpenForm {
            purpose: FormPurpose::Arguments(Box::new(asking)),
            return_to,
            submitting: false,
        });
        state.next_screen();
    }

    /// Submits the open argument form: with a required field empty, marks
    /// it and says so, and launches nothing; else returns to the screen
    /// the form was asked from and returns the launch with the form's
    /// values (the blank ones left out), for [`Launcher::launch_submitted`].
    /// Remembers each dropdown's value for the command. Refused, with the
    /// reason, once the command's package is gone, disabled or paused.
    pub(super) fn submit_arguments(&self, state: &mut State) -> Option<Submitted> {
        let Some(OpenForm {
            purpose: FormPurpose::Arguments(asking),
            ..
        }) = &state.form
        else {
            return None;
        };
        let problem = match owner(&state.packages, &asking.opening.component) {
            None => Some("Its extension is no longer installed".to_owned()),
            Some(package) if !package.enabled => Some(format!("{} is disabled", package.title())),
            Some(package) if state.paused.is_paused(&package.identity) => {
                Some(paused_reason(&package.title()))
            }
            Some(_) => None,
        };
        if let Some(problem) = problem {
            state.view.status = Status::Error(problem);
            return None;
        }
        let Screen::Form(form) = &mut state.view.screen else {
            return None;
        };
        for field in &mut form.fields {
            field.error = None;
        }
        let missing = form
            .fields
            .iter_mut()
            .find(|field| field.required && arguments::blank(&field.value))
            .map(|field| {
                field.error = Some(MISSING.into());
                format!("{}: {MISSING}", field.label)
            });
        if let Some(missing) = missing {
            state.view.status = Status::Error(missing);
            return None;
        }
        let values: Vec<(String, String)> = form
            .fields
            .iter()
            .filter(|field| !arguments::blank(&field.value))
            .map(|field| (field.id.clone(), field.value.clone()))
            .collect();
        let open = state.form.take().expect("the argument form is open");
        let FormPurpose::Arguments(asking) = open.purpose else {
            unreachable!("matched above");
        };
        let Asking {
            mut opening,
            command,
            declared,
        } = *asking;
        let mut remembered = false;
        for argument in &declared {
            let value = values
                .iter()
                .find(|(name, _)| *name == argument.name)
                .map(|(_, value)| value);
            if let (ArgumentKind::Dropdown(_), Some(value)) = (&argument.kind, value) {
                remembered |=
                    state
                        .remembered_arguments
                        .chosen
                        .remember(&command, &argument.name, value);
            }
        }
        opening.launch.arguments = values;
        state.next_screen();
        state.view = open.return_to;
        if opening.no_view {
            Launcher::begin_run(state);
        } else {
            state.view.status = Status::Running;
        }
        let data = self.data_in(state, &opening.component);
        Some(Submitted {
            opening,
            epoch: state.screen_epoch,
            data,
            remembered: remembered.then_some(command),
        })
    }

    /// The command whose arguments the open argument form asks for, by its
    /// id in Pane's records (`<package identity key>#<manifest id>`); `None`
    /// on any other screen. The window draws the command's icon beside the
    /// form's title with it.
    pub fn arguments_asked_for(&self) -> Option<String> {
        let state = self.lock();
        match (&state.view.screen, &state.form) {
            (
                Screen::Form(_),
                Some(OpenForm {
                    purpose: FormPurpose::Arguments(asking),
                    ..
                }),
            ) => Some(asking.command.clone()),
            _ => None,
        }
    }

    /// Records the dropdown values remembered for `command` (the
    /// `arguments.json` record), off the calling thread as every record
    /// write is: root search's inline fields launch with them remembered
    /// (#205), as the form's submission does.
    pub(super) async fn record_remembered(&self, command: &str) {
        let launcher = self.clone();
        let command = command.to_owned();
        let saved = off_thread(move || launcher.save::<ArgumentChoices>(Some(&command))).await;
        if let Err(problem) = saved {
            crate::diagnostic!(
                "Pane could not remember a command's dropdown choices: {problem}"
            );
        }
    }

    /// Records the dropdown values the form remembered, then launches the
    /// command with the form's values: the setup gate, then the form, are
    /// behind it.
    pub(super) async fn launch_submitted(&self, submitted: Submitted) {
        let Submitted {
            opening,
            epoch,
            data,
            remembered,
        } = submitted;
        if let Some(command) = remembered {
            self.record_remembered(&command).await;
        }
        self.launch_ready(epoch, opening, data).await
    }

    /// Forgets the remembered dropdown values of the uninstalled package
    /// with `identity`. Returns what writes the record without them, to run
    /// off the window's thread; nothing to write if it had none.
    pub(super) fn forget_arguments_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.remembered_arguments.forget(identity) {
            return None;
        }
        let launcher = self.clone();
        Some(move || launcher.save::<ArgumentChoices>(None))
    }
}

/// Remembers the dropdown values of `command`'s launch for the next one
/// (the `arguments.json` record), as submitting the form does; the
/// command to record, if a dropdown's value changed what was remembered.
fn remember_dropdowns(
    state: &mut State,
    command: &str,
    declared: &[ManifestArgument],
    values: &[(String, String)],
) -> Option<String> {
    let mut remembered = false;
    for argument in declared {
        let value = values
            .iter()
            .find(|(name, _)| *name == argument.name)
            .map(|(_, value)| value);
        if let (ArgumentKind::Dropdown(_), Some(value)) = (&argument.kind, value) {
            remembered |=
                state
                    .remembered_arguments
                    .chosen
                    .remember(command, &argument.name, value);
        }
    }
    remembered.then(|| command.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_dropdowns_are_read_back_as_written() {
        let mut choices = ArgumentChoices::default();
        assert!(choices.remember("local:a#greet", "tone", "warm"));
        assert!(!choices.remember("local:a#greet", "tone", "warm"));
        assert!(choices.remember("local:a#greet", "tone", "brief"));
        assert_eq!(choices.remembered("local:a#greet", "tone"), Some("brief"));
        assert_eq!(choices.remembered("local:a#greet", "other"), None);
        let read = ArgumentChoices::read(&choices.write()).unwrap();
        assert_eq!(read, choices);

        let mut other = ArgumentChoices::default();
        other.restore("local:a#greet", &choices);
        assert_eq!(other, choices);
        assert!(other.retain(&|command| command != "local:a#greet"));
        assert_eq!(other, ArgumentChoices::default());
    }
}
