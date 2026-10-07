//! Preferences in the launcher (#143): the setup gate in front of every
//! launch, the Setup screen it shows, "Needs setup" on the rows of
//! commands that cannot run yet, and what Settings › Extensions reads and
//! changes on an extension's card.
//!
//! **The setup gate.** Every launch goes through [`Launcher::setup_gate`]
//! at the top of `launch_opening` (see `launching`), before anything runs:
//! a command whose required preferences (its package's or its own) are
//! unset does not run. A launch by the user shows the Setup screen
//! instead, a form of only those fields, in declaration order, each with
//! its description, with the extension's title, "Set these up before
//! using <command>" and the package's `HELP.md` beside them. Submitting
//! saves the values, checks again and launches the command with its
//! original launch record; Back cancels and launches nothing. A background
//! launch does not run and shows nothing. Neither counts as a failure of
//! the package.
//!
//! The gate is one call on the launch path, so other steps before a launch
//! compose with it: the argument form (#144) goes after it in
//! `launch_opening`, and a launch submitted from the Setup screen goes
//! through `launch_opening` again, meeting the argument form there.
//!
//! **Other ways in.** Root search's computed and indexed results, a
//! command's schedule and its continuing service are not asked of a
//! command that needs setup ([`Launcher::needs_setup`]); its row in root
//! search says "Needs setup" ([`super::RowPresentation::needs_setup`]).
//!
//! **Storing.** Values are the package's extension data (see
//! `extension_data`): a password's a local credential, every other value
//! an extension setting. An update keeps the values whose names are still
//! declared, moving one whose type changed to where its new type is kept,
//! or dropping it if it no longer fits ([`Launcher::carry_preferences`]).

use std::collections::{BTreeMap, HashSet};
use std::future::Future;

use super::{
    FormField, FormPurpose, FormView, Launcher, LauncherView, OpenForm, Opening, Screen, State,
    Status, choices, owner,
};
use crate::extension_data::DataKind;
use crate::packages::{InstalledPackage, Manifest, PackageIdentity};
use crate::preferences::{self, Declared, Preference, PreferenceKind};
use crate::runtime::{Choice, FieldKind, PathKind};

/// What the Setup screen shows above and beside its fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupHeader {
    /// The extension's identity key: the window draws its icon from it.
    pub package: String,
    /// The extension's title.
    pub title: String,
    /// "Set these up before using <command>".
    pub sentence: String,
    /// The paragraphs of the package's `HELP.md`, as plain text; empty when
    /// it ships none.
    pub help: Vec<String>,
}

/// The launch the Setup screen holds back until it is submitted.
pub(super) struct SetupGate {
    identity: PackageIdentity,
    /// The launch, with its original record.
    opening: Opening,
}

/// One preference as an extension's card in Settings shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferenceField {
    /// Where its value is kept, which names it to
    /// [`Launcher::set_preference`].
    pub key: String,
    /// The command it belongs to, by its id in `pane.json`; `None` for one
    /// of the package's.
    pub command: Option<String>,
    pub preference: Preference,
    /// The value the user set, as stored; `None` while they set none.
    pub value: Option<String>,
    /// Whether it is required and unset: it has no value that still fits
    /// (see `preferences::normalised`) and no default. Drawn in the error
    /// state.
    pub missing: bool,
}

/// One command's own preferences on its extension's card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPreferences {
    /// The command's id in `pane.json`.
    pub command: String,
    pub title: String,
    pub fields: Vec<PreferenceField>,
}

/// An extension's preferences, as its card in Settings › Extensions shows
/// them: the package's, then each command's under that command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagePreferences {
    pub identity: PackageIdentity,
    pub title: String,
    pub fields: Vec<PreferenceField>,
    /// The commands that declare preferences of their own, in manifest
    /// order.
    pub commands: Vec<CommandPreferences>,
}

/// A required preference that is unset, as the setup gate finds it: one
/// field of the Setup screen.
pub(super) struct UnsetPreference {
    /// The command it belongs to, by its id in `pane.json`; `None` for one
    /// of the package's.
    command: Option<String>,
    preference: Preference,
}

impl UnsetPreference {
    /// Where its value is kept, which is the id of its Setup screen field.
    fn key(&self) -> String {
        preferences::storage_key(self.command.as_deref(), &self.preference.name)
    }
}

/// Where "Configure Command…" and "Configure Extension…" take the user:
/// an installed command's extension card in Settings, at that command's
/// preferences or its package's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferencesTarget {
    pub identity: PackageIdentity,
    /// The command's id in `pane.json`.
    pub command: String,
}

/// What the Setup screen's submission saves, and the launch it releases.
pub(super) struct SetupSubmit {
    identity: PackageIdentity,
    opening: Opening,
    /// Each field's storage key and value.
    values: Vec<(String, String)>,
}

impl Launcher {
    /// The required, unset preferences of the command `command` of
    /// `package`, in declaration order, with the command each belongs to
    /// (`None` for the package's): what holds its launches back.
    pub(super) fn unset_preferences(
        &self,
        package: &InstalledPackage,
        command: &str,
    ) -> Vec<UnsetPreference> {
        let (Ok(manifest), Some(installation)) = (&package.manifest, &self.installation) else {
            return Vec::new();
        };
        let declared = preferences::declared(manifest, command);
        if !declared.iter().any(|declared| declared.preference.required) {
            return Vec::new();
        }
        let stored = installation.data.preference_values(&package.identity);
        preferences::unset(&declared, &stored)
            .into_iter()
            .map(|unset| UnsetPreference {
                command: unset.command.map(str::to_owned),
                preference: unset.preference.clone(),
            })
            .collect()
    }

    /// Whether the command `command` of `package` needs setup: a required
    /// preference of its package's or its own is unset, so it does not run
    /// until the user sets it.
    pub(super) fn needs_setup(&self, package: &InstalledPackage, command: &str) -> bool {
        !self.unset_preferences(package, command).is_empty()
    }

    /// Notes which installed commands need setup now, for their rows in
    /// root search ("Needs setup"). Called whenever root search's results
    /// are built again, and after a preference changes.
    pub(super) fn note_setup_needed(&self, state: &mut State) {
        let needed: HashSet<String> = state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .flat_map(|package| {
                package
                    .commands()
                    .into_iter()
                    .filter(move |command| self.needs_setup(package, command.manifest_id()))
                    .map(|command| command.id)
            })
            .collect();
        state.setup_needed = needed;
    }

    /// Whether the launch of `opening` needs the Setup screen first.
    pub(super) fn opening_needs_setup(&self, state: &State, opening: &Opening) -> bool {
        owner(&state.packages, &opening.component)
            .is_some_and(|package| self.needs_setup(package, &opening.command))
    }

    /// The setup gate (see the module docs): `Some(opening)` when it may
    /// run now; `None` when it may not, its required preferences being
    /// unset. A launch by the user then shows the Setup screen, if the
    /// screen it was launched from is still shown (`epoch`), and asks for
    /// Pane's window; a background launch does nothing. Neither is a
    /// failure.
    pub(super) fn setup_gate(&self, epoch: u64, opening: Opening) -> Option<Opening> {
        let mut state = self.lock();
        let Some(package) = owner(&state.packages, &opening.component) else {
            // A command built into Pane declares no preferences.
            return Some(opening);
        };
        let unset = self.unset_preferences(package, &opening.command);
        if unset.is_empty() {
            return Some(opening);
        }
        let identity = package.identity.clone();
        let title = package.title();
        let location = package.location.clone();
        let command_title = package
            .manifest
            .as_ref()
            .ok()
            .and_then(|manifest| manifest.commands.iter().find(|c| c.id == opening.command))
            .map_or_else(|| opening.command.clone(), |command| command.title.clone());
        if opening.launch.is_background() || state.screen_epoch != epoch {
            // Not run, and nothing shown: no window was shown for it, or
            // the user left the screen it was launched from. Its row says
            // "Needs setup".
            return None;
        }
        let header = SetupHeader {
            package: identity.key(),
            title: title.clone(),
            sentence: format!("Set these up before using {command_title}"),
            help: preferences::help(&location),
        };
        let fields = unset.iter().map(setup_field).collect();
        let form = FormView {
            fields,
            submit_label: "Save and continue".into(),
            setup: Some(header),
        };
        show_setup_form(
            &mut state,
            LauncherView::new(Screen::Form(form), title),
            SetupGate { identity, opening },
        );
        None
    }

    /// Begins submitting the Setup screen, if it is the form on screen:
    /// every field needs a value (each is a required preference), or it is
    /// marked and nothing is saved. What to save, for
    /// [`Launcher::finish_setup`].
    pub(super) fn begin_setup_submit(&self, state: &mut State) -> Option<SetupSubmit> {
        let (Screen::Form(form), Some(open)) = (&mut state.view.screen, &mut state.form) else {
            return None;
        };
        let FormPurpose::Setup(gate) = &open.purpose else {
            return None;
        };
        if open.submitting {
            return None;
        }
        let mut empty = false;
        for field in &mut form.fields {
            if field.value.trim().is_empty() {
                field.error = Some("Required".into());
                empty = true;
            }
        }
        if empty {
            state.view.status =
                Status::Error("Fill in every field to continue: each is required".into());
            return None;
        }
        open.submitting = true;
        let submit = SetupSubmit {
            identity: gate.identity.clone(),
            opening: gate.opening.clone(),
            values: form
                .fields
                .iter()
                .map(|field| (field.id.clone(), field.value.clone()))
                .collect(),
        };
        state.view.status = Status::Running;
        Some(submit)
    }

    /// Saves what the Setup screen submitted, checks again and, when the
    /// command no longer needs setup, leaves the screen for the one it was
    /// shown over and launches the command with its original launch record.
    /// A value saved that still does not fit (a file that does not exist)
    /// is marked on its field, and the screen stays. Nothing launches if the
    /// user left the screen (`epoch`) meanwhile.
    pub(super) async fn finish_setup(&self, epoch: u64, submit: SetupSubmit) {
        let SetupSubmit {
            identity,
            opening,
            values,
        } = submit;
        let stored = self.store_preferences(&identity, values);
        if stored.is_ok() {
            self.after_preferences_changed(&mut self.lock());
            self.preferences_changed();
        }
        let written = match stored {
            Ok(writes) => writes.written().await,
            Err(problem) => Err(problem),
        };
        let launch = {
            let mut state = self.lock();
            let state = &mut *state;
            if state.screen_epoch != epoch {
                return;
            }
            if let Some(open) = state.form.as_mut() {
                open.submitting = false;
            }
            if let Err(problem) = written {
                state.view.status = Status::Error(problem);
                return;
            }
            let Some(package) = state.package(&identity) else {
                state.view.status = Status::Error(format!(
                    "{} is no longer installed",
                    state.title_of(&identity)
                ));
                return;
            };
            let unset = self.unset_preferences(package, &opening.command);
            if !unset.is_empty() {
                let still: HashSet<String> = unset.iter().map(UnsetPreference::key).collect();
                if let Screen::Form(form) = &mut state.view.screen {
                    for field in &mut form.fields {
                        if still.contains(&field.id) {
                            field.error = Some(still_unset(field.kind_of(&unset)));
                        }
                    }
                }
                state.view.status =
                    Status::Error("Some values cannot be used; correct them to continue".into());
                return;
            }
            let open = state.form.take().expect("the Setup screen is a form");
            state.next_screen();
            state.view = open.return_to;
            state.view.status = Status::Idle;
            if opening.no_view {
                Launcher::begin_run(state);
            } else {
                state.view.status = Status::Running;
            }
            let data = self.data_in(state, &opening.component);
            (state.screen_epoch, opening, data)
        };
        let (epoch, opening, data) = launch;
        self.launch_opening(epoch, opening, data).await;
    }

    /// Saves `values` (storage key, value) as preference values of the
    /// package with `identity`, each where its kind keeps it; an empty
    /// value removes it.
    fn store_preferences(
        &self,
        identity: &PackageIdentity,
        values: Vec<(String, String)>,
    ) -> Result<crate::extension_data::PreferenceWrites, String> {
        let installation = self
            .installation
            .as_ref()
            .ok_or("this launcher does not install packages")?;
        let kinds: BTreeMap<String, PreferenceKind> = {
            let state = self.lock();
            let package = state
                .package(identity)
                .ok_or_else(|| format!("{} is not installed", state.title_of(identity)))?;
            let manifest = package
                .manifest
                .as_ref()
                .map_err(|error| format!("{} cannot load: {error}", package.title()))?;
            preferences::all_declared(manifest)
                .iter()
                .map(|declared| (declared.key(), declared.preference.kind))
                .collect()
        };
        for (key, _) in &values {
            if !kinds.contains_key(key) {
                return Err(format!("no preference is kept as `{key}`"));
            }
        }
        installation.data.change_preferences(identity, |stored| {
            for (key, value) in values {
                let kind = kinds[&key];
                if value.is_empty() {
                    stored.remove(&key);
                } else {
                    stored.insert(key, (kept_as(kind), value));
                }
            }
        })
    }

    /// What follows a change of preference values while the launcher is
    /// locked: the rows that say "Needs setup" are worked out again, and
    /// root search on screen shows them.
    fn after_preferences_changed(&self, state: &mut State) {
        self.note_setup_needed(state);
        if matches!(state.view.screen, Screen::Root { .. }) {
            self.refresh_root(state);
        }
    }

    /// What follows a change of preference values once the launcher is
    /// unlocked: the scheduler and the services look again, as after any
    /// change of a package (a schedule or service held back for setup may
    /// run now), and the window redraws.
    fn preferences_changed(&self) {
        if let Some(installation) = &self.installation {
            installation.data.changed();
        }
        self.changed();
    }

    /// The preferences of the installed package with `identity`, as its
    /// card in Settings › Extensions shows them; `None` when it is not
    /// installed, cannot load, or declares none.
    pub fn preferences_of(&self, identity: &PackageIdentity) -> Option<PackagePreferences> {
        let state = self.lock();
        let package = state.package(identity)?;
        let manifest = package.manifest.as_ref().ok()?;
        let installation = self.installation.as_ref()?;
        let stored = installation.data.preference_values(identity);
        let field = |declared: Declared<'_>| {
            let key = declared.key();
            let value = stored.get(&key).cloned();
            PreferenceField {
                missing: declared.preference.required
                    && preferences::effective(declared.preference, value.as_deref()).is_none(),
                command: declared.command.map(str::to_owned),
                preference: declared.preference.clone(),
                value,
                key,
            }
        };
        let all = preferences::all_declared(manifest);
        let fields: Vec<PreferenceField> = all
            .iter()
            .filter(|declared| declared.command.is_none())
            .map(|declared| field(*declared))
            .collect();
        let commands: Vec<CommandPreferences> = manifest
            .commands
            .iter()
            .filter(|command| !command.preferences.is_empty())
            .map(|command| CommandPreferences {
                command: command.id.clone(),
                title: command.title.clone(),
                fields: all
                    .iter()
                    .filter(|declared| declared.command == Some(command.id.as_str()))
                    .map(|declared| field(*declared))
                    .collect(),
            })
            .collect();
        if fields.is_empty() && commands.is_empty() {
            return None;
        }
        Some(PackagePreferences {
            identity: identity.clone(),
            title: package.title(),
            fields,
            commands,
        })
    }

    /// Sets the value of the preference kept as `key` (see
    /// [`PreferenceField::key`]) of the installed package with `identity`:
    /// Settings › Extensions saves each change as it is made. `None` or an
    /// empty value removes it. Refused, with the reason, for a key the
    /// package does not declare, a checkbox's value other than `true` or
    /// `false`, or a dropdown's not among its options. The change applies
    /// at once (a command that needed setup may run now); the returned
    /// future says whether it was written.
    pub fn set_preference(
        &self,
        identity: &PackageIdentity,
        key: &str,
        value: Option<&str>,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let value = value.unwrap_or_default().to_owned();
        let checked = (|| {
            let state = self.lock();
            let package = state
                .package(identity)
                .ok_or_else(|| format!("{} is not installed", state.title_of(identity)))?;
            let manifest = package
                .manifest
                .as_ref()
                .map_err(|error| format!("{} cannot load: {error}", package.title()))?;
            let declared = preferences::all_declared(manifest)
                .into_iter()
                .find(|declared| declared.key() == key)
                .ok_or_else(|| format!("{} has no preference `{key}`", package.title()))?;
            refusal(declared.preference, &value).map_or(Ok(()), Err)
        })();
        let stored =
            checked.and_then(|()| self.store_preferences(identity, vec![(key.to_owned(), value)]));
        if stored.is_ok() {
            self.after_preferences_changed(&mut self.lock());
            self.preferences_changed();
        }
        async move { stored?.written().await }
    }

    /// The package identity and command id of the installed command whose
    /// root search row is `row`, when it or its package declares
    /// preferences: where "Configure Command…" and "Configure Extension…"
    /// take the user (its extension's card in Settings).
    pub fn preferences_target(&self, row: &str) -> Option<PreferencesTarget> {
        let state = self.lock();
        let (key, command) = choices::split(row);
        let package = state
            .packages
            .iter()
            .find(|package| package.identity.key() == key)?;
        let manifest = package.manifest.as_ref().ok()?;
        preferences::applies(manifest, command).then(|| PreferencesTarget {
            identity: package.identity.clone(),
            command: command.to_owned(),
        })
    }

    /// Carries the preference values of the package with `identity` over
    /// an update from `old` to `new`: kept while their names are still
    /// declared, moved to where a value of their new type is kept, and
    /// dropped once undeclared or of a type they no longer fit (see
    /// `preferences::after_update`). Written in the background.
    pub(super) fn carry_preferences(&self, identity: &PackageIdentity, new: &Manifest) {
        let Some(installation) = &self.installation else {
            return;
        };
        let carried = installation.data.change_preferences(identity, |stored| {
            stored.retain(
                |key, (kind, value)| match preferences::after_update(new, key, value) {
                    Some(fits) => {
                        *kind = kept_as(fits);
                        true
                    }
                    None => false,
                },
            );
        });
        if let Err(problem) = carried {
            eprintln!(
                "pane: could not carry the preferences of {identity} over its update: {problem}"
            );
        }
    }
}

/// The kind of data a preference of `kind` is kept as: a password as a
/// local credential, every other as a setting.
fn kept_as(kind: PreferenceKind) -> DataKind {
    if kind.is_secret() {
        DataKind::LocalCredentials
    } else {
        DataKind::Settings
    }
}

/// Why `value` cannot be the value of `preference`, if it cannot.
fn refusal(preference: &Preference, value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    match preference.kind {
        PreferenceKind::Checkbox if !preference.kind.fits(value) => Some(format!(
            "{} is a checkbox: its value is `true` or `false`",
            preference.title
        )),
        PreferenceKind::Dropdown if !preference.options.iter().any(|o| o.value == value) => {
            Some(format!(
                "\"{value}\" is not one of the options of {}",
                preference.title
            ))
        }
        _ => None,
    }
}

/// The Setup screen's field for the unset preference `unset`: a choice for
/// a checkbox (off first) and a dropdown, a path field with the system's
/// picker for a file, folder or application (as its extension's card in
/// Settings has), and a text field for text and a password, a password's
/// hidden as it is typed.
fn setup_field(unset: &UnsetPreference) -> FormField {
    let preference = &unset.preference;
    let choice = |id: &str, label: &str| Choice {
        id: id.to_owned(),
        label: label.to_owned(),
    };
    let kind = match preference.kind {
        PreferenceKind::Checkbox => FieldKind::Choice(vec![
            choice("false", "Off"),
            choice("true", preference.label.as_deref().unwrap_or("On")),
        ]),
        PreferenceKind::Dropdown => FieldKind::Choice(
            preference
                .options
                .iter()
                .map(|option| choice(&option.value, &option.title))
                .collect(),
        ),
        // A password's text is hidden as it is typed: the argument form's
        // password field (#144).
        PreferenceKind::Password => FieldKind::Password {
            placeholder: preference.placeholder.clone(),
        },
        PreferenceKind::Text => FieldKind::Text {
            placeholder: preference.placeholder.clone(),
        },
        PreferenceKind::File => path_field(preference, PathKind::File, "The path of a file"),
        PreferenceKind::Folder => path_field(preference, PathKind::Folder, "The path of a folder"),
        PreferenceKind::Application => path_field(
            preference,
            PathKind::Application,
            "The path of an application",
        ),
    };
    let value = match &kind {
        FieldKind::Choice(choices) => choices
            .first()
            .map(|choice| choice.id.clone())
            .unwrap_or_default(),
        FieldKind::Text { .. } | FieldKind::Password { .. } | FieldKind::Path { .. } => {
            String::new()
        }
    };
    FormField {
        id: unset.key(),
        label: preference.title.clone(),
        kind,
        value,
        error: None,
        description: preference.description.clone(),
        required: true,
    }
}

/// A path field choosing a `pick` for `preference`, its placeholder
/// `placeholder` unless the manifest gives one.
fn path_field(preference: &Preference, pick: PathKind, placeholder: &str) -> FieldKind {
    FieldKind::Path {
        placeholder: Some(
            preference
                .placeholder
                .clone()
                .unwrap_or_else(|| placeholder.to_owned()),
        ),
        pick,
    }
}

/// Why a value saved from the Setup screen still leaves its preference
/// unset, for a preference of `kind`.
fn still_unset(kind: Option<PreferenceKind>) -> String {
    match kind {
        Some(PreferenceKind::File) => "No file has this path".into(),
        Some(PreferenceKind::Folder) => "No folder has this path".into(),
        Some(PreferenceKind::Dropdown) => "Not one of the options".into(),
        _ => "Required".into(),
    }
}

impl FormField {
    /// The kind of the preference among `unset` this Setup screen field
    /// edits.
    fn kind_of(&self, unset: &[UnsetPreference]) -> Option<PreferenceKind> {
        unset
            .iter()
            .find(|unset| unset.key() == self.id)
            .map(|unset| unset.preference.kind)
    }
}

/// Shows `view`, the Setup screen, over what is on screen: Back returns
/// there and launches nothing. If a form is already open (another Setup
/// screen, say), Back returns to what that form was shown over.
fn show_setup_form(state: &mut State, view: LauncherView, gate: SetupGate) {
    let return_to = match state.form.take() {
        Some(open) => {
            state.view = view;
            open.return_to
        }
        None => std::mem::replace(&mut state.view, view),
    };
    state.form = Some(OpenForm {
        purpose: FormPurpose::Setup(Box::new(gate)),
        return_to,
        submitting: false,
    });
    state.next_screen();
    state.sent_from = None;
    // A launch from a hidden window (a no-view command's hotkey) needs it
    // now.
    state.window_wanted = true;
}
