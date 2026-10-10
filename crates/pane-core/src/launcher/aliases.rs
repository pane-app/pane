//! Aliases and fallbacks the user gives installed commands, to reach them
//! from root search in fewer steps.
//!
//! - An **alias** is one word the user gives a command in the extension list.
//!   Typing it in root search lists the command first, above everything
//!   else. For a command that takes a query (`"takesQuery": true`, or a
//!   first argument that is text with every other optional; see
//!   `arguments`), typing the alias, a space and more text lists a row that
//!   sends that text to the command when the user invokes it.
//! - A **fallback** is a command that takes a query, which the user chose to
//!   have offered for any text typed in root search: it is listed below every
//!   other result, and when nothing but fallbacks is listed the first is
//!   selected, so Enter sends it the query (ADR 0031).
//!
//! The text sent also fills the command's first text or password argument
//! when it has one without a value (see `argument_form`).
//!
//! Nothing runs while the user types: the text is sent to the command only
//! when its row is invoked, as its launch record's fallback text, trimmed
//! (see `launching`). A no-view command, such as the query samples' Echo,
//! runs with it and its answer is shown while root search stays as it was;
//! a view command opens its screen with it.
//!
//! Both are Pane's own records (see `choices`):
//! `aliases.json` beside `installed.json`, by command id, so copies of a
//! package from other sources, even with the same titles, are distinct, and
//! a reinstalled or updated package keeps them. A disabled package's
//! command offers neither (and they never enable it); an uninstalled one's
//! are forgotten. A recorded choice that cannot be used now (its package is
//! disabled, paused or cannot load, its command is unavailable here, gone or
//! no longer takes a query) is shown in the extension list as not active,
//! with why.

use std::collections::BTreeMap;
use std::future::Future;

use serde_json::{Map, Value};

use super::choices::{Choices, Record, provider_title, split};
use super::{
    CommandRegistration, Entry, FormField, FormPurpose, FormView, Launcher, LauncherView, OpenForm,
    Opening, Row, Screen, State, Status, Unavailable, off_thread,
};
use crate::launch::{LaunchRecord, LaunchSource};
use crate::packages::{PackageIdentity, paused_reason};
use crate::runtime::FieldKind;
use crate::search::same_text;

/// The longest alias, in characters.
const MAX_ALIAS_CHARS: usize = 32;

/// The alias form's only field.
const ALIAS_FIELD: &str = "alias";

/// The user's aliases and fallbacks, recorded in `aliases.json` as
/// `{ "version": 1, "aliases": { "<command id>": "ec" }, "fallbacks":
/// ["<command id>"] }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct AliasChoices {
    /// Each command's alias, as the user typed it, by command id.
    aliases: BTreeMap<String, String>,
    /// The fallback commands' ids, in the order they are offered.
    fallbacks: Vec<String>,
}

impl Choices for AliasChoices {
    const FILE: &'static str = "aliases.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "aliases";

    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let mut choices = AliasChoices::default();
        if let Some(aliases) = fields.get("aliases") {
            let aliases = aliases.as_object().ok_or("`aliases` is not an object")?;
            for (command, alias) in aliases {
                match alias.as_str() {
                    Some(alias) if !alias.trim().is_empty() => {
                        choices.aliases.insert(command.clone(), alias.to_owned());
                    }
                    _ => {}
                }
            }
        }
        if let Some(fallbacks) = fields.get("fallbacks") {
            let fallbacks = fallbacks.as_array().ok_or("`fallbacks` is not a list")?;
            for command in fallbacks.iter().filter_map(Value::as_str) {
                if !choices.is_fallback(command) {
                    choices.fallbacks.push(command.to_owned());
                }
            }
        }
        Ok(choices)
    }

    fn write(&self) -> Map<String, Value> {
        let aliases = self
            .aliases
            .iter()
            .map(|(command, alias)| (command.clone(), Value::String(alias.clone())))
            .collect();
        let fallbacks = self.fallbacks.iter().cloned().map(Value::String).collect();
        Map::from_iter([
            ("aliases".to_string(), Value::Object(aliases)),
            ("fallbacks".to_string(), Value::Array(fallbacks)),
        ])
    }

    fn restore(&mut self, command: &str, other: &Self) {
        match other.aliases.get(command) {
            Some(alias) => self.aliases.insert(command.to_owned(), alias.clone()),
            None => self.aliases.remove(command),
        };
        match (other.is_fallback(command), self.is_fallback(command)) {
            (true, false) => self.fallbacks.push(command.to_owned()),
            (false, true) => self.fallbacks.retain(|id| id != command),
            _ => {}
        }
    }

    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = (self.aliases.len(), self.fallbacks.len());
        self.aliases.retain(|command, _| keep(command));
        self.fallbacks.retain(|command| keep(command));
        (self.aliases.len(), self.fallbacks.len()) != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.aliases
    }
}

impl AliasChoices {
    /// The alias recorded for `command`, as the user typed it; for the
    /// Shortcuts catalog.
    pub(super) fn alias_of(&self, command: &str) -> Option<String> {
        self.aliases.get(command).cloned()
    }

    /// Every command id with an alias or a fallback recorded; for the
    /// Shortcuts catalog's rows of choices whose commands are gone.
    pub(super) fn recorded(&self) -> Vec<&str> {
        self.aliases
            .keys()
            .map(String::as_str)
            .chain(self.fallbacks.iter().map(String::as_str))
            .collect()
    }

    /// Whether `command` is offered as a fallback. `pub(super)` for the
    /// launcher's selected-action label, which turns with the choice.
    pub(super) fn is_fallback(&self, command: &str) -> bool {
        self.fallbacks.iter().any(|id| id == command)
    }

    fn has_any(&self, command: &str) -> bool {
        self.aliases.contains_key(command) || self.is_fallback(command)
    }

    /// Another command whose alias is `alias`, compared caselessly (full
    /// Unicode case folding after NFC, so "STRASSE" is "straße").
    pub(super) fn shared_with(&self, command: &str, alias: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(other, chosen)| other.as_str() != command && same_text(chosen, alias))
            .map(|(other, _)| other.as_str())
    }

    /// The alias of `command` as root search matches it: none if another
    /// command has the same one (only in a record edited by hand).
    pub(super) fn active_alias(&self, command: &str) -> Option<&str> {
        let alias = self.aliases.get(command)?;
        self.shared_with(command, alias)
            .is_none()
            .then_some(alias.as_str())
    }
}

/// An installed command of an enabled package as root search offers it,
/// kept with its root result so aliases and fallbacks can reach it.
#[derive(Clone, Debug)]
pub(super) struct Target {
    pub(super) registration: CommandRegistration,
    pub(super) identity: PackageIdentity,
    /// Why it cannot run now: paused, or unavailable on this system.
    pub(super) unavailable: Option<Unavailable>,
    /// Whether it is a no-view command, which runs with the text sent
    /// rather than opening a screen.
    pub(super) no_view: bool,
}

/// How a row sends the query to its command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Via {
    /// The query's first word is the command's alias; the rest is sent.
    Alias,
    /// The command is a fallback; the whole query is sent.
    Fallback,
    /// The query is a typed web address or path the command was declared
    /// for (`"matches"`, #195): the parsed address or resolved path is
    /// sent. Only `typed_query` builds such a row.
    Typed,
}

/// A root search row's query, to send to a command that takes one.
#[derive(Clone)]
pub(super) struct Sending {
    /// The command, launched from its alias, as a fallback, or for the
    /// address or path typed, with the text as its launch record's
    /// fallback text.
    pub(super) opening: Opening,
    pub(super) via: Via,
    /// Why the command cannot run now; invoking the row shows it.
    pub(super) unavailable: Option<String>,
}

/// The installed commands root search offers.
fn targets(state: &State) -> impl Iterator<Item = &Target> {
    state
        .root
        .iter()
        .filter_map(|result| result.target.as_ref())
}

/// A root search row that sends `text` to `target` when invoked.
fn send_row(state: &State, target: &Target, text: &str, via: Via, how: &str) -> (Row, Entry) {
    let registration = &target.registration;
    // Copies of a package share titles: then the row names its source.
    let shared = targets(state)
        .filter(|other| other.registration.title == registration.title)
        .count()
        > 1;
    let source = if shared {
        format!(" · {}", target.identity)
    } else {
        String::new()
    };
    let kind = match via {
        Via::Alias => "alias",
        Via::Fallback => "fallback",
        // A typed query's rows are built in `typed_query`, never here.
        Via::Typed => "typed",
    };
    let row = Row {
        id: format!("{kind}:{}", registration.id),
        title: registration.title.clone(),
        subtitle: Some(format!("Send “{text}” · {how}{source}")),
        unavailable: target.unavailable.clone(),
    };
    let from = match via {
        Via::Alias => LaunchSource::Alias,
        // A typed query's rows are built in `typed_query`, never here.
        Via::Fallback | Via::Typed => LaunchSource::Fallback,
    };
    let mut opening = Opening::of(registration, target.no_view, from);
    opening.launch = LaunchRecord::sending(from, text);
    let entry = Entry::Send(Sending {
        opening,
        via,
        unavailable: target.unavailable.as_ref().map(|u| u.reason().to_owned()),
    });
    (row, entry)
}

/// The rows for `query` typed as an alias followed by text: for the command
/// that takes a query whose alias is the query's first word, a row that
/// sends it the rest.
pub(super) fn rows_sending_after_alias(state: &State, query: &str) -> Vec<(Row, Entry)> {
    let Some((word, text)) = query.trim().split_once(char::is_whitespace) else {
        return Vec::new();
    };
    let text = text.trim();
    targets(state)
        .filter(|target| target.registration.takes_query)
        .filter_map(|target| {
            let alias = state.aliases.chosen.active_alias(&target.registration.id)?;
            same_text(alias, word).then(|| {
                let how = format!("alias {alias}");
                send_row(state, target, text, Via::Alias, &how)
            })
        })
        .collect()
}

/// The fallback rows for `query`, in the order the user chose them: each
/// sends the whole query to its command. None for a blank query.
pub(super) fn fallback_rows(state: &State, query: &str) -> Vec<(Row, Entry)> {
    let text = query.trim();
    if text.is_empty() {
        return Vec::new();
    }
    state
        .aliases
        .chosen
        .fallbacks
        .iter()
        .filter_map(|id| targets(state).find(|target| target.registration.id == *id))
        .filter(|target| target.registration.takes_query)
        .map(|target| send_row(state, target, text, Via::Fallback, "fallback"))
        .collect()
}

/// The row root search selects by itself: the first one that is not a
/// fallback, whose command the user must choose — or, when nothing but
/// fallbacks is listed, the first of them, so Enter sends it the query
/// (ADR 0031).
pub(super) fn first_choice(entries: &[Entry]) -> Option<usize> {
    let chosen = entries.iter().position(|entry| {
        !matches!(
            entry,
            Entry::Send(Sending {
                via: Via::Fallback,
                ..
            })
        )
    });
    chosen.or((!entries.is_empty()).then_some(0))
}

/// Whether `query` is one word that some installed command's active alias
/// starts with, as [`Launcher::could_still_be_alias`] reads it: the query
/// folded as the alias is matched, against the alias's first as many
/// characters. A blank query, or one with a space in it, is never such a
/// word.
fn alias_prefix(state: &State, query: &str) -> bool {
    if query.is_empty() || query.chars().any(char::is_whitespace) {
        return false;
    }
    let length = query.chars().count();
    targets(state).any(|target| {
        state
            .aliases
            .chosen
            .active_alias(&target.registration.id)
            .is_some_and(|alias| {
                let word: String = alias.chars().take(length).collect();
                same_text(&word, query)
            })
    })
}

/// An installed command as the extension list lists its alias and fallback.
struct Configured<'a> {
    id: String,
    title: String,
    takes_query: bool,
    identity: &'a PackageIdentity,
    /// Why its alias and fallback are not active, if they are not: its
    /// package is disabled or paused, or it is unavailable on this system.
    inactive: Option<String>,
}

impl Launcher {
    /// The alias and fallback rows of the extension list: for each command
    /// of an enabled package, and of a disabled one with an alias or
    /// fallback, a row for its alias and, if it takes a query, one for
    /// whether it is a fallback; then one row for each recorded choice whose
    /// command cannot be listed (gone, or its package cannot load or is not
    /// installed). Each row names its package's source, since copies of a
    /// package can share titles.
    pub(super) fn choice_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        let chosen = &state.aliases.chosen;
        let mut configured = Vec::new();
        for package in &state.packages {
            let Ok(manifest) = &package.manifest else {
                continue;
            };
            let title = package.title();
            let package_inactive = if !package.enabled {
                Some(format!("{title} is disabled"))
            } else if state.paused.is_paused(&package.identity) {
                Some(paused_reason(&title))
            } else {
                None
            };
            let commands = package.listed_commands().into_iter();
            for (listed, command) in commands.zip(&manifest.commands) {
                // A root provider has no alias or fallback to set.
                if command.mode == crate::packages::CommandMode::Provider {
                    continue;
                }
                let registration = listed.registration;
                // A command turned off on its extension's page (#168) keeps
                // its choices, not active, as a disabled package's are.
                let off =
                    (!listed.enabled).then(|| format!("{} is turned off", registration.title));
                if (package.enabled && listed.enabled) || chosen.has_any(&registration.id) {
                    configured.push(Configured {
                        id: registration.id,
                        title: registration.title,
                        takes_query: command.accepts_fallback_text(),
                        identity: &package.identity,
                        inactive: package_inactive.clone().or(off).or(listed.unavailable),
                    });
                }
            }
        }
        let mut rows = Vec::new();
        for command in &configured {
            let not_active = |why: Option<String>| {
                command
                    .inactive
                    .clone()
                    .or(why)
                    .map_or_else(String::new, |why| format!(" · Not active: {why}"))
            };
            let alias = match chosen.aliases.get(&command.id) {
                Some(alias) => {
                    let shared = chosen
                        .shared_with(&command.id, alias)
                        .map(|_| "another command has the same alias".to_string());
                    format!("“{alias}”{}", not_active(shared))
                }
                None => "None · A word that finds it in root search".into(),
            };
            rows.push((
                Row {
                    id: format!("alias-setting:{}", command.id),
                    title: format!("Alias for {}", command.title),
                    subtitle: Some(format!("{alias} · {}", command.identity)),
                    unavailable: None,
                },
                Entry::AskAlias(command.id.clone()),
            ));
            let fallback = chosen.is_fallback(&command.id);
            if !(command.takes_query || fallback) {
                continue;
            }
            let state = match (fallback, command.takes_query) {
                (true, true) => format!(
                    "On · Offered below the results for any text typed{}",
                    not_active(None)
                ),
                (true, false) => format!(
                    "On{}",
                    not_active(Some(format!("{} no longer takes a query", command.title)))
                ),
                (false, _) => "Off · Offer it below the results for any text typed".into(),
            };
            rows.push((
                Row {
                    id: format!("fallback-setting:{}", command.id),
                    title: format!("Fallback: {}", command.title),
                    subtitle: Some(format!("{state} · {}", command.identity)),
                    unavailable: None,
                },
                Entry::ToggleFallback(command.id.clone()),
            ));
        }
        // Choices whose command cannot be listed: dropped by an update, its
        // package cannot load, or recorded for a package not installed.
        let mut unlisted: Vec<&String> = chosen
            .aliases
            .keys()
            .chain(&chosen.fallbacks)
            .filter(|id| !configured.iter().any(|command| command.id == **id))
            .collect();
        unlisted.sort();
        unlisted.dedup();
        for id in unlisted {
            let what = match (chosen.aliases.get(id), chosen.is_fallback(id)) {
                (Some(alias), true) => format!("Alias “{alias}” and fallback"),
                (Some(alias), false) => format!("Alias “{alias}”"),
                (None, _) => "Fallback".into(),
            };
            let (key, command) = split(id);
            let owner = state.packages.iter().find(|p| p.identity.key() == key);
            let (title, why) = match owner {
                Some(owner) => match &owner.manifest {
                    Err(error) => (
                        format!("{what} of `{command}`"),
                        format!("{} cannot load: {error}", owner.title()),
                    ),
                    // An update made it a root provider, which has no
                    // alias or fallback; the next start forgets them.
                    Ok(_) if owner.is_provider(command) => (
                        format!("{what} of `{command}`"),
                        format!(
                            "`{command}` of {} only answers root search now",
                            owner.title()
                        ),
                    ),
                    Ok(_) => (
                        format!("{what} of a missing command"),
                        format!("{} has no command `{command}` now", owner.title()),
                    ),
                },
                None => (
                    format!("{what} of a missing command"),
                    "its extension is not installed".into(),
                ),
            };
            rows.push((
                Row {
                    id: format!("unlisted-setting:{id}"),
                    title,
                    subtitle: Some(format!("Not active: {why}; Enter forgets it · {id}")),
                    unavailable: None,
                },
                Entry::ForgetChoices(id.clone()),
            ));
        }
        rows
    }

    /// Shows the form that sets the alias of the command `command`, with
    /// its current alias filled in (Pane's own form; an extension's form
    /// starts empty).
    pub(super) fn show_alias_form(&self, state: &mut State, command: &str) {
        state.actions_return = None;
        let title = self.command_title(state, command);
        let current = state
            .aliases
            .chosen
            .aliases
            .get(command)
            .cloned()
            .unwrap_or_default();
        let form = FormView {
            fields: vec![FormField {
                id: ALIAS_FIELD.into(),
                label: format!("Alias: one word that finds {title} in root search; empty for none"),
                kind: FieldKind::Text {
                    placeholder: Some("such as ec".into()),
                },
                value: current,
                error: None,
                description: None,
                required: false,
            }],
            submit_label: "Save alias".into(),
            setup: None,
        };
        let view = LauncherView::new(Screen::Form(form), format!("Alias for {title}"));
        let return_to = std::mem::replace(&mut state.view, view);
        state.form = Some(OpenForm {
            purpose: FormPurpose::Alias(command.to_owned()),
            return_to,
            submitting: false,
        });
        state.next_screen();
    }

    /// Applies the alias submitted for `command` in its form: refused, with
    /// the reason next to the field, if it is not one word, is too long or
    /// is another command's; else it takes effect at once and the extension
    /// list is shown, and the returned change records it.
    pub(super) fn submit_alias(&self, state: &mut State, command: &str) -> Option<ChoiceChange> {
        let Screen::Form(form) = &state.view.screen else {
            return None;
        };
        let alias = form.fields.first()?.value.trim().to_owned();
        if let Some(refusal) =
            alias_refusal(self, state, command, &alias).filter(|_| !alias.is_empty())
        {
            let Screen::Form(form) = &mut state.view.screen else {
                unreachable!("the alias form is open");
            };
            form.fields[0].error = Some(refusal.clone());
            state.view.status = Status::Error(format!("Alias: {refusal}"));
            return None;
        }
        let done = apply_alias(self, state, command, &alias);
        state.form = None;
        let at = |entry: &Entry| matches!(entry, Entry::AskAlias(id) if id == command);
        Some(self.choices_changed(state, at, command, done))
    }

    /// Sets `alias` as the alias of the command `command` without opening
    /// the launcher's alias form: the Settings window's Shortcuts page
    /// edits it inline. The alias form's rules apply: `Err` carries the
    /// reason when the alias is not one word, is over the limit, or is
    /// another command's; an empty or blank alias clears it.
    ///
    /// The alias takes effect at once — the rows on screen are refreshed,
    /// so the next query root search runs finds it — and the returned
    /// future records it; a record that cannot be written goes back to
    /// what was last recorded (see [`Launcher::save`]), and its outcome
    /// says which it was. The launcher's screens are left where they are,
    /// unlike the form's submission: the page that asked shows the
    /// outcome itself.
    pub fn set_alias(
        &self,
        command: &str,
        alias: &str,
    ) -> Result<impl Future<Output = AliasOutcome> + Send + 'static, String> {
        let alias = alias.trim();
        let mut state = self.lock();
        if let Some(refusal) =
            alias_refusal(self, &state, command, alias).filter(|_| !alias.is_empty())
        {
            return Err(refusal);
        }
        let done = apply_alias(self, &mut state, command, alias);
        self.refresh(&mut state);
        let command = command.to_owned();
        let saving = self.clone();
        let after = self.clone();
        Ok(async move {
            let saved = off_thread(move || saving.save::<AliasChoices>(Some(&command))).await;
            match saved {
                Ok(()) => AliasOutcome::Saved(done),
                Err(problem) => {
                    // What was last recorded is back in Pane (see
                    // `Launcher::save`); the rows on screen follow it again.
                    let mut state = after.lock();
                    after.refresh(&mut state);
                    AliasOutcome::NotKept(problem)
                }
            }
        })
    }

    /// Makes the command `command` a fallback if it is not one, else no
    /// longer one.
    pub(super) fn toggle_fallback(&self, state: &mut State, command: &str) -> ChoiceChange {
        let title = self.command_title(state, command);
        let fallbacks = &mut state.aliases.chosen.fallbacks;
        let done = if fallbacks.iter().any(|id| id == command) {
            fallbacks.retain(|id| id != command);
            format!("{title} is no longer a fallback")
        } else {
            fallbacks.push(command.to_owned());
            format!("{title} is now offered for any text typed in root search")
        };
        let at = |entry: &Entry| matches!(entry, Entry::ToggleFallback(id) if id == command);
        self.choices_changed(state, at, command, done)
    }

    /// Forgets the alias and fallback of `command`, whose command cannot be
    /// listed.
    pub(super) fn forget_choices(&self, state: &mut State, command: &str) -> ChoiceChange {
        let chosen = &mut state.aliases.chosen;
        chosen.aliases.remove(command);
        chosen.fallbacks.retain(|id| id != command);
        let done = "Forgot the alias and fallback".into();
        let at = |entry: &Entry| matches!(entry, Entry::ForgetChoices(_));
        self.choices_changed(state, at, command, done)
    }

    /// Shows the extension list at the first row `at` accepts after the
    /// choices of `command` changed — or the search the Actions panel
    /// opened the alias form from — with the change to record.
    fn choices_changed(
        &self,
        state: &mut State,
        at: impl Fn(&Entry) -> bool,
        command: &str,
        done: String,
    ) -> ChoiceChange {
        if !self.return_from_actions_flow(state) {
            self.show_extensions_at(state, at);
        }
        state.view.status = Status::Running;
        ChoiceChange {
            command: command.to_owned(),
            done,
            epoch: state.screen_epoch,
        }
    }

    /// Records an alias or fallback change, restoring what was last
    /// recorded if it cannot (see [`Launcher::save`]).
    pub(super) async fn finish_choice_change(&self, change: ChoiceChange) {
        let ChoiceChange {
            command,
            done,
            epoch,
        } = change;
        let launcher = self.clone();
        let saved = off_thread(move || launcher.save::<AliasChoices>(Some(&command))).await;
        let mut state = self.lock();
        let status = match saved {
            Ok(()) => Status::Result(done),
            Err(problem) => {
                self.refresh(&mut state);
                Status::Error(format!("Could not keep the change: {problem}"))
            }
        };
        if state.screen_epoch == epoch {
            state.view.status = status;
        }
    }

    /// Forgets the aliases and fallbacks of the uninstalled package with
    /// `identity`. Returns what writes the record without them, to run off
    /// the window's thread; nothing to write if it had none.
    pub(super) fn forget_aliases_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.aliases.forget(identity) {
            return None;
        }
        let launcher = self.clone();
        Some(move || launcher.save::<AliasChoices>(None))
    }

    /// Whether the query typed in root search could still turn into a
    /// command's alias (#203): it is one word that some installed command's
    /// active alias starts with, compared caselessly as the alias itself
    /// is matched. While it could — and the current query's list is not
    /// yet published — the window holds a space typed next; what an alias
    /// and a space then do is #205's to decide, and until then the space
    /// lands in the field as text. `false` off root search, and for a
    /// query that is blank or already holds a space: such a query can no
    /// longer become an alias's word.
    pub fn could_still_be_alias(&self) -> bool {
        let state = self.lock();
        let Screen::Root { query } = &state.view.screen else {
            return false;
        };
        alias_prefix(&state, query)
    }
}

/// An alias or fallback change that has taken effect and is being recorded.
pub(super) struct ChoiceChange {
    command: String,
    /// The outcome once recorded.
    done: String,
    epoch: u64,
}

/// What an alias set directly, through [`Launcher::set_alias`], came to.
/// The alias has taken effect in Pane either way; this says whether the
/// record on disk kept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AliasOutcome {
    /// The alias was recorded. The message says what typing it now finds
    /// (“Typing “ec” now finds Echo”), or that the command has no
    /// alias now.
    Saved(String),
    /// The alias took effect but could not be recorded: what was last
    /// recorded is back, and root search follows it again. The message
    /// says why it could not be kept.
    NotKept(String),
}

/// Why `alias` cannot be the alias of the command `command`, or `None` if
/// it can: a space in it, over the limit, or another command's (compared
/// caselessly). An empty alias is never refused — it clears the command's
/// alias — so the callers decide what an empty one means. This is the one
/// rule the alias form and the Settings window's inline field both apply.
fn alias_refusal(launcher: &Launcher, state: &State, command: &str, alias: &str) -> Option<String> {
    if let Some(title) = provider_title(state, command) {
        Some(format!(
            "{title} only answers root search, so it has no alias to set"
        ))
    } else if alias.chars().any(char::is_whitespace) {
        Some("An alias is one word, without spaces".to_string())
    } else if alias.chars().count() > MAX_ALIAS_CHARS {
        Some(format!("An alias has at most {MAX_ALIAS_CHARS} characters"))
    } else {
        state
            .aliases
            .chosen
            .shared_with(command, alias)
            .map(|other| {
                let other = launcher.command_title(state, other);
                format!(
                    "“{alias}” is already the alias of {other}: change it there first, or \
                     choose another"
                )
            })
    }
}

/// Applies `alias` to the command `command` in Pane's records — an empty
/// one clears it — returning the message that says what changed. The
/// caller records the change: the alias form shows the extension list,
/// [`Launcher::set_alias`] leaves the screens where they are, and both
/// await the record's write.
fn apply_alias(launcher: &Launcher, state: &mut State, command: &str, alias: &str) -> String {
    let title = launcher.command_title(state, command);
    let aliases = &mut state.aliases.chosen.aliases;
    if alias.is_empty() {
        aliases.remove(command);
        format!("{title} has no alias now")
    } else {
        let done = format!("Typing “{alias}” now finds {title}");
        aliases.insert(command.to_owned(), alias.to_owned());
        done
    }
}
