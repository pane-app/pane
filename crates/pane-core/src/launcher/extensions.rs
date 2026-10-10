//! The extensions' operations that Pane's Settings window runs (#168, ADR
//! 0043): from each installed package's switch, reload, update, cache and
//! uninstall to the hotkeys, choices, development and retained data, read
//! as typed operations ([`Launcher::extension_operations`]: what each is,
//! whose, and whether it is on) and run one at a time
//! ([`Launcher::run_extension_operation`]) without moving the launcher off
//! the screen the user had open. The confirmations and details screens an
//! operation opens are the launcher's own screens, which Settings draws and
//! answers; once answered, the launcher returns to root search. The
//! launcher window has no screen for the extensions: its "Manage
//! Extensions" command opens Settings at them. The list the operations
//! come from can still be shown as a screen of its own for the tests
//! ([`Launcher::manage_extensions`]).
//!
//! Also here: turning one command of a package on or off, checking a
//! package for an update, the folder its page shows, and which of the
//! installed extensions are Pane's own (#280, ADR 0045).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::pausing::{self, Pauses};
use super::{
    Entry, Launcher, LauncherView, Row, Screen, State, first_index, network, programs, retained,
    updates,
};
use super::{Pending, Status};
use crate::packages::{InstalledPackage, PackageIdentity, Store};

impl Launcher {
    /// The extension list, as Settings reads it: its rows, read without
    /// leaving the screen the launcher is on. The rows are the ones [`Launcher::manage_extensions`]
    /// shows once the list is entered; a second window over the launcher
    /// (Pane's Settings) lists the installed extensions through this before
    /// the user opens the flow itself, so it stays a reading of the
    /// launcher's own records rather than a copy of them.
    pub fn extension_list(&self) -> LauncherView {
        let state = self.lock();
        let (rows, _) = self.extension_rows(&state);
        LauncherView::new(
            Screen::Extensions {
                details: Vec::new(),
            },
            "Extensions",
        )
        .with_rows(rows)
    }

    /// Shows the installed packages, each enabled or disabled, where the
    /// list was entered as a screen ([`Launcher::manage_extensions`]).
    /// Elsewhere — an operation Settings ran — what returned to the list
    /// returns to where the user was instead: a confirmation or details
    /// screen the operation opened goes back to root search, and any other
    /// screen stays.
    pub(in crate::launcher) fn show_extensions(&self, state: &mut State) {
        if !state.list_entered {
            if opened_by_operation(&state.view.screen) {
                self.show_root(state, None);
            }
            return;
        }
        let (rows, entries) = self.extension_rows(state);
        self.leave_command(state);
        state.entries = entries;
        // No lines of explanation: each extension's page in Settings says
        // what it needs to, and an operation's confirmation says the rest.
        state.view = LauncherView::new(
            Screen::Extensions {
                details: Vec::new(),
            },
            "Extensions",
        )
        .with_rows(rows);
        self.show_kept_development_status(state);
    }

    /// The extension list's rows: each package's state, reload and cache
    /// rows, then the hotkey of each command of the enabled packages, then
    /// one row per identity with retained data, then the global
    /// automatic-update choice, last of all.
    fn extension_rows(&self, state: &State) -> (Vec<Row>, Vec<Entry>) {
        let developed = |identity: &PackageIdentity| self.is_developed(identity);
        let (mut rows, mut entries): (Vec<Row>, Vec<Entry>) =
            self.runtime_rows().into_iter().unzip();
        let (package_rows, package_entries) = extension_rows(
            &state.packages,
            &state.paused,
            developed,
            &state.update_controls.off,
        );
        rows.extend(package_rows);
        entries.extend(package_entries);
        // A package's remembered confirmations, after its other rows: the
        // card shows "Reset confirmations" among its buttons.
        for (row, entry) in self.reset_rows(state) {
            rows.push(row);
            entries.push(entry);
        }
        // Pane's own Clipboard History's Clear History (#166): the card
        // shows "Clear history" among its buttons.
        for (row, entry) in self.clipboard_rows(state) {
            rows.push(row);
            entries.push(entry);
        }
        for (row, entry) in self
            .network_rows(state)
            .into_iter()
            .chain(self.program_rows(state))
        {
            rows.push(row);
            entries.push(entry);
        }
        let development = self.development_rows(&state.packages);
        for (row, entry) in self
            .hotkey_rows(state)
            .into_iter()
            .chain(self.choice_rows(state))
            .chain(development)
        {
            rows.push(row);
            entries.push(entry);
        }
        if let Some(installation) = &self.installation {
            let (retained_rows, retained_entries) =
                retained::rows(&state.retained, &installation.data);
            rows.extend(retained_rows);
            entries.extend(retained_entries);
            // The update results of the latest pass that recorded, before
            // the global automatic-update choice, last of all: what the
            // last pass did is found beside what governs the next one.
            if !state.update_results.results.is_empty() {
                let (row, entry) = results_row();
                rows.push(row);
                entries.push(entry);
            }
            // The global automatic-update choice comes last, after every
            // package's rows: the packages are the list, and what governs
            // them all is found beneath them.
            let (row, entry) = updates::global_row(state.update_controls.automatic);
            rows.push(row);
            entries.push(entry);
        }
        (rows, entries)
    }

    /// Shows the extension list with the first row whose entry is `wanted`
    /// selected, or the first row if there is none.
    pub(in crate::launcher) fn show_extensions_at(
        &self,
        state: &mut State,
        wanted: impl Fn(&Entry) -> bool,
    ) {
        self.show_extensions(state);
        let row = state.entries.iter().position(wanted);
        if row.is_some() {
            state.view.selected = row;
        }
    }

    /// Updates the installed packages on screen after one changed, keeping
    /// the selection on the same row.
    pub(in crate::launcher) fn refresh_extensions(&self, state: &mut State) {
        let (rows, entries) = self.extension_rows(state);
        // The same row stays selected; if it is gone (a Retry row once the
        // package started), the row before it.
        let selected = state.view.selected.and_then(|index| {
            let id = &state.view.rows.get(index)?.id;
            rows.iter()
                .position(|row| row.id == *id)
                .or_else(|| Some(index.saturating_sub(1).min(rows.len().checked_sub(1)?)))
        });
        state.entries = entries;
        state.view.selected = selected.or_else(|| first_index(&rows));
        state.view.rows = rows;
    }
}

/// Whether `screen` is one an extension's operation opens over the screen
/// the user had: a confirmation, a details screen or a hotkey recorder.
fn opened_by_operation(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::Extensions { .. }
            | Screen::Confirm { .. }
            | Screen::PauseDetails { .. }
            | Screen::NetworkDetails { .. }
            | Screen::ProgramDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::ExtensionLog { .. }
            | Screen::UpdateResults { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::Hotkey { .. }
    )
}

/// The extension list's row for the update results of the latest pass
/// that recorded, before the global choice: what the last pass did to
/// every extension it considered.
fn results_row() -> (Row, Entry) {
    let row = Row {
        id: "update-results".into(),
        title: "Update Results".into(),
        subtitle: Some("What the last check updated, skipped or failed".into()),
        unavailable: None,
    };
    (row, Entry::UpdateResults)
}

/// What one of the extensions' operations does (#168): Settings offers each
/// where it belongs, by this, never by a row's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    /// Turns the extension on or off (asking first about the extensions
    /// that require it).
    Enable,
    /// Turns its automatic updates on or off; with no owner, every
    /// extension's.
    AutomaticUpdates,
    /// Replaces its code with the current build of its source folder.
    Reload,
    /// Starts it again after Pane paused it.
    Retry,
    /// Shows why Pane paused it.
    WhyPaused,
    /// Shows what it did on the network this session.
    NetworkUse,
    /// Shows the system programs it ran this session.
    ProgramsRun,
    /// Forgets the answers remembered for its confirmations.
    ResetConfirmations,
    /// Clears the history of Pane's own Clipboard History (#166), asking
    /// first.
    ClearHistory,
    /// Clears its cache, asking first.
    ClearCache,
    /// Uninstalls it, asking first whether to keep its saved data.
    Uninstall,
    /// Builds and reloads it after each save in its source folder.
    Develop,
    /// Stops developing it.
    StopDeveloping,
    /// Shows why its last build failed.
    WhyNotBuilt,
    /// Shows its extension log while it is developed: "Logs for <title>".
    Logs,
    /// Records the keys of a command's hotkey (on the page, the Shortcuts
    /// columns do).
    Hotkey,
    /// Sets a command's alias (likewise).
    Alias,
    /// Offers a command below root search's results for any text typed.
    Fallback,
    /// Forgets the alias and fallback recorded for a command it no longer
    /// has.
    ForgetChoices,
    /// Shows why Pane's extension runtime stopped.
    RuntimeDetails,
    /// Starts Pane's extension runtime again.
    RestartRuntime,
    /// Deletes the data kept for an extension no longer installed.
    DeleteRetainedData,
    /// Shows the update results of the latest pass that recorded: every
    /// extension it considered, grouped as Updated, Skipped and Failed.
    UpdateResults,
}

impl OperationKind {
    /// What a menu calls it, without the extension's name.
    fn label(self) -> &'static str {
        match self {
            OperationKind::Enable => "Enabled",
            OperationKind::AutomaticUpdates => "Update Automatically",
            OperationKind::Reload => "Reload",
            OperationKind::Retry => "Retry",
            OperationKind::WhyPaused => "Why Paused",
            OperationKind::NetworkUse => "Network Use",
            OperationKind::ProgramsRun => "Programs Run",
            OperationKind::ResetConfirmations => "Reset Confirmations",
            OperationKind::ClearHistory => "Clear History",
            OperationKind::ClearCache => "Clear Cache",
            OperationKind::Uninstall => "Uninstall",
            OperationKind::Develop => "Develop",
            OperationKind::StopDeveloping => "Stop Developing",
            OperationKind::WhyNotBuilt => "Why the Build Failed",
            OperationKind::Logs => "Logs",
            OperationKind::Hotkey => "Hotkey",
            OperationKind::Alias => "Alias",
            OperationKind::Fallback => "Fallback",
            OperationKind::ForgetChoices => "Forget",
            OperationKind::RuntimeDetails => "Why the Runtime Stopped",
            OperationKind::RestartRuntime => "Restart the Runtime",
            OperationKind::DeleteRetainedData => "Delete Retained Data",
            OperationKind::UpdateResults => "Update Results",
        }
    }
}

/// One of the extensions' operations, as the launcher's records say it now
/// ([`Launcher::extension_operations`]); [`Launcher::run_extension_operation`]
/// runs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionOperation {
    pub kind: OperationKind,
    /// The extension it is about: an installed one, or, deleting retained
    /// data, the one no longer installed whose data is kept. `None` for
    /// the runtime's and for every extension's automatic updates.
    pub owner: Option<PackageIdentity>,
    /// The command it is about, by its full id: a hotkey, an alias, a
    /// fallback, choices to forget.
    pub command: Option<String>,
    /// What a menu calls it, without the extension's name: "Reload",
    /// "Clear Cache".
    pub label: String,
    /// What it is called in full: "Reload Hello", "Clear cache of Hello".
    pub title: String,
    /// Whether it is on now, for what turns something on or off (enabled,
    /// automatic updates, fallback).
    pub on: Option<bool>,
    /// Why it cannot be used now, if it cannot.
    pub unavailable: Option<String>,
    /// Its stable id, the same while it is offered: what the sidebar's
    /// search names it by, and what [`Launcher::run_extension_operation`]
    /// finds it by.
    pub id: String,
}

impl Launcher {
    /// Every operation of the installed extensions, of the runtime and of
    /// the data kept for uninstalled extensions, in the extension list's
    /// order (each package's switch, reload and retry rows, automatic
    /// updates, cache and uninstall; then confirmations, Clipboard History,
    /// network and programs, hotkeys, choices and development; retained
    /// data; every extension's automatic updates last), read now.
    pub fn extension_operations(&self) -> Vec<ExtensionOperation> {
        let state = self.lock();
        let (rows, entries) = self.extension_rows(&state);
        rows.into_iter()
            .zip(entries)
            .filter_map(|(row, entry)| operation(&state, row, &entry))
            .collect()
    }

    /// Runs `operation` (one of [`Launcher::extension_operations`]) as
    /// Settings' pages do: the launcher stays on the screen the user had,
    /// and what it came to is that screen's status; a confirmation or
    /// details screen it asks for shows instead, Settings answering it.
    /// Await the returned future for the operation to finish. An operation
    /// no longer offered does nothing; one that cannot be used now says why
    /// in the status.
    pub fn run_extension_operation(
        &self,
        operation: &ExtensionOperation,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        state.sent_from = None;
        let (rows, entries) = self.extension_rows(&state);
        let found = rows
            .into_iter()
            .zip(entries)
            .find(|(row, _)| row.id == operation.id);
        let pending = match found {
            Some((row, _)) if row.unavailable.is_some() => {
                let why = row.unavailable.map(|why| why.reason().to_owned());
                state.view.status = Status::Error(why.unwrap_or_default());
                Pending::Nothing
            }
            Some((_, entry)) => self.activation(&mut state, entry),
            None => Pending::Nothing,
        };
        let work = self.pending_work(&state, pending);
        drop(state);
        work
    }
}

/// The operation the extension list's `row`, activating `entry`, is; `None`
/// for a row that is none (an unavailable hotkey's explanation).
fn operation(state: &State, row: Row, entry: &Entry) -> Option<ExtensionOperation> {
    let package_of_command = |command: &str| {
        let (key, _) = super::choices::split(command);
        state
            .packages
            .iter()
            .find(|package| package.identity.key() == key)
            .map(|package| package.identity.clone())
    };
    let (kind, owner, command, on) = match entry {
        Entry::Toggle(identity) => (
            OperationKind::Enable,
            Some(identity.clone()),
            None,
            Some(
                state
                    .package(identity)
                    .is_some_and(|package| package.enabled),
            ),
        ),
        Entry::ToggleUpdates(Some(identity)) => (
            OperationKind::AutomaticUpdates,
            Some(identity.clone()),
            None,
            Some(!state.update_controls.off.contains(&identity.key())),
        ),
        Entry::ToggleUpdates(None) => (
            OperationKind::AutomaticUpdates,
            None,
            None,
            Some(state.update_controls.automatic),
        ),
        Entry::Reload(identity) => (OperationKind::Reload, Some(identity.clone()), None, None),
        Entry::Retry(identity) => (OperationKind::Retry, Some(identity.clone()), None, None),
        Entry::PauseDetails(identity) => {
            (OperationKind::WhyPaused, Some(identity.clone()), None, None)
        }
        Entry::NetworkDetails(identity) => (
            OperationKind::NetworkUse,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::ProgramDetails(identity) => (
            OperationKind::ProgramsRun,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::ResetConfirmations(identity) => (
            OperationKind::ResetConfirmations,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::AskClearClipboardHistory(identity) => (
            OperationKind::ClearHistory,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::AskClearCache(identity) => (
            OperationKind::ClearCache,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::AskUninstall(identity) => {
            (OperationKind::Uninstall, Some(identity.clone()), None, None)
        }
        Entry::Develop(identity) => (OperationKind::Develop, Some(identity.clone()), None, None),
        Entry::StopDeveloping(identity) => (
            OperationKind::StopDeveloping,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::BuildDetails(identity) => (
            OperationKind::WhyNotBuilt,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::ExtensionLog(identity) => (OperationKind::Logs, Some(identity.clone()), None, None),
        Entry::AskHotkey(command) => (
            OperationKind::Hotkey,
            package_of_command(command),
            Some(command.clone()),
            None,
        ),
        Entry::AskAlias(command) => (
            OperationKind::Alias,
            package_of_command(command),
            Some(command.clone()),
            None,
        ),
        Entry::ToggleFallback(command) => (
            OperationKind::Fallback,
            package_of_command(command),
            Some(command.clone()),
            Some(state.aliases.chosen.is_fallback(command)),
        ),
        Entry::ForgetChoices(command) => (
            OperationKind::ForgetChoices,
            package_of_command(command),
            Some(command.clone()),
            None,
        ),
        Entry::RuntimeDetails => (OperationKind::RuntimeDetails, None, None, None),
        Entry::RestartRuntime => (OperationKind::RestartRuntime, None, None, None),
        Entry::AskDeleteRetained(identity) => (
            OperationKind::DeleteRetainedData,
            Some(identity.clone()),
            None,
            None,
        ),
        Entry::UpdateResults => (OperationKind::UpdateResults, None, None, None),
        _ => return None,
    };
    Some(ExtensionOperation {
        kind,
        owner,
        command,
        label: kind.label().to_owned(),
        on,
        unavailable: row.unavailable.as_ref().map(|why| why.reason().to_owned()),
        title: row.title,
        id: row.id,
    })
}

/// One row per installed package, saying whether it is enabled or paused
/// and which source it is, so copies with the same title can be told apart;
/// then the rows that reload each enabled package, each followed, if Pane
/// paused it, by a row that retries it and one that shows why it is paused;
/// then one row per package to clear its cache, and one to uninstall it, in
/// the same order.
fn extension_rows(
    packages: &[InstalledPackage],
    paused: &Pauses,
    developed: impl Fn(&PackageIdentity) -> bool,
    off: &std::collections::HashSet<String>,
) -> (Vec<Row>, Vec<Entry>) {
    let failure = |package: &InstalledPackage| paused.of(&package.identity).cloned();
    let toggles = packages.iter().map(|package| {
        let state = match (package.enabled, failure(package).map(|pause| pause.after)) {
            (false, _) => "Disabled",
            (true, None) => "Enabled",
            (true, Some(cause)) => cause.state(),
        };
        let developing = if developed(&package.identity) {
            " · Developing"
        } else {
            ""
        };
        let row = Row {
            id: package.identity.key(),
            title: package.title(),
            subtitle: Some(format!(
                "{state}{developing}{network}{programs} · {}",
                package.identity,
                network = if package.uses_network {
                    format!(" · {}", network::USES_THE_NETWORK)
                } else {
                    String::new()
                },
                programs = if package.uses_programs {
                    format!(" · {}", programs::RUNS_SYSTEM_PROGRAMS)
                } else {
                    String::new()
                }
            )),
            unavailable: None,
        };
        (row, Entry::Toggle(package.identity.clone()))
    });
    let reloads = packages
        .iter()
        .filter(|package| package.enabled)
        .flat_map(|package| {
            let title = package.title();
            let source = match package.identity.local_folder() {
                Some(folder) => folder.display().to_string(),
                None => package.identity.to_string(),
            };
            let reload = Row {
                id: format!("reload:{}", package.identity.key()),
                title: format!("Reload {title}"),
                subtitle: Some(format!(
                    "Replace its code with the current build in {source}"
                )),
                unavailable: None,
            };
            let paused = failure(package).into_iter().flat_map(move |pause| {
                let retry = Row {
                    id: format!("retry:{}", package.identity.key()),
                    title: pause.after.retry_title(&title),
                    subtitle: Some(format!(
                        "Paused: {}; start it again",
                        pause.after.failure("it")
                    )),
                    unavailable: None,
                };
                let details = Row {
                    id: format!("paused:{}", package.identity.key()),
                    title: pausing::details_title(&title),
                    subtitle: Some("The error and its diagnostics".into()),
                    unavailable: None,
                };
                [
                    (retry, Entry::Retry(package.identity.clone())),
                    (details, Entry::PauseDetails(package.identity.clone())),
                ]
            });
            // A package from npm or Git has no source folder to reload from;
            // to replace its code, install it again (Update).
            let local = package.identity.local_folder().is_some();
            std::iter::once((reload, Entry::Reload(package.identity.clone())))
                .filter(move |_| local)
                .chain(paused)
        });
    // Which packages the user turned updates off for, for their rows.
    let automatic = updates::package_rows(packages, off);
    let clear_cache = packages.iter().map(|package| {
        let row = Row {
            id: format!("clear-cache:{}", package.identity.key()),
            title: format!("Clear cache of {}", package.title()),
            subtitle: Some(format!(
                "Keeps its settings, content and credentials · {}",
                package.identity
            )),
            unavailable: None,
        };
        (row, Entry::AskClearCache(package.identity.clone()))
    });
    let uninstall = packages.iter().map(|package| {
        let row = Row {
            id: format!("uninstall:{}", package.identity.key()),
            title: format!("Uninstall {}", package.title()),
            subtitle: Some(format!(
                "Remove it and choose whether to keep its saved data · {}",
                package.identity
            )),
            unavailable: None,
        };
        (row, Entry::AskUninstall(package.identity.clone()))
    });
    toggles
        .chain(reloads)
        .chain(automatic)
        .chain(clear_cache)
        .chain(uninstall)
        .unzip()
}

impl Launcher {
    /// Turns the installed command with id `command` on or off (#168), as
    /// its switch on its package's page in Settings does: a command turned
    /// off is not offered — it leaves root search, and its results, its
    /// alias, hotkey, schedule and service stop — while the rest of its
    /// package works; turned on again, all of that comes back, with the
    /// alias and hotkey kept meanwhile. The choice applies at once and is
    /// recorded with the package's own record (`installed.json`), so it
    /// holds after a restart, an update and a reload. Await the returned
    /// future to record it: what was done, in a sentence, or why it could
    /// not be done — a choice that cannot be recorded is undone.
    pub fn set_command_enabled(
        &self,
        command: &str,
        enabled: bool,
    ) -> impl Future<Output = Result<String, String>> + Send + 'static {
        let begun = self.begin_command_switch(command, enabled);
        let launcher = self.clone();
        async move {
            let (identity, manifest_id, title, store) = match begun? {
                Switch::Done(done) => return Ok(done),
                Switch::Record {
                    identity,
                    manifest_id,
                    title,
                    store,
                } => (identity, manifest_id, title, store),
            };
            let recorded = {
                let (identity, manifest_id) = (identity.clone(), manifest_id.clone());
                super::off_thread(move || {
                    let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                    store.set_command_enabled(&identity, &manifest_id, enabled)
                })
                .await
            };
            match recorded {
                Ok(()) => Ok(if enabled {
                    format!("Turned on {title}")
                } else {
                    format!("Turned off {title}")
                }),
                Err(error) => {
                    let mut state = launcher.lock();
                    launcher.apply_command_enabled(&mut state, &identity, &manifest_id, !enabled);
                    Err(format!("Could not keep the choice: {error}"))
                }
            }
        }
    }

    /// Applies the switch [`Launcher::set_command_enabled`] asked for in
    /// Pane now, saying what is left to record.
    fn begin_command_switch(&self, command: &str, enabled: bool) -> Result<Switch, String> {
        let Some(installation) = &self.installation else {
            return Err("This launcher does not install extensions".into());
        };
        let mut state = self.lock();
        let (key, manifest_id) = super::choices::split(command);
        let Some(package) = state
            .packages
            .iter()
            .find(|package| package.identity.key() == key)
        else {
            return Err(format!(
                "No installed extension has the command `{command}`"
            ));
        };
        let Some(listed) = package
            .listed_commands()
            .into_iter()
            .find(|listed| listed.registration.id == command)
        else {
            return Err(format!(
                "{} has no command `{manifest_id}` now",
                package.title()
            ));
        };
        let identity = package.identity.clone();
        let title = listed.registration.title.clone();
        if listed.enabled == enabled {
            return Ok(Switch::Done(if enabled {
                format!("{title} is on")
            } else {
                format!("{title} is off")
            }));
        }
        let manifest_id = manifest_id.to_owned();
        self.apply_command_enabled(&mut state, &identity, &manifest_id, enabled);
        Ok(Switch::Record {
            identity,
            manifest_id,
            title,
            store: installation.store.clone(),
        })
    }

    /// Turns the command `command` (its manifest id) of the package with
    /// `identity` on or off in this launcher, without recording it: whether
    /// root search, its providers and its hotkey offer it.
    fn apply_command_enabled(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
        command: &str,
        enabled: bool,
    ) {
        let Some(package) = state
            .packages
            .iter_mut()
            .find(|package| package.identity == *identity)
        else {
            return;
        };
        package.set_command_enabled(command, enabled);
        // Its hotkey is released while it is off, and registered again
        // once it is on.
        self.sync_hotkeys(state);
        // Results kept for root search are asked again without it, or
        // with it.
        Launcher::forget_indexes(state);
        self.refresh(state);
        self.changed();
    }

    /// Checks the installed package with `identity` for an update, as its
    /// page's "Check for Update" does: a package from npm or Git is shown
    /// again from its source — the preview an install from npm or Git
    /// shows, which offers its Update when the source has a newer version
    /// or commit (a pinned npm version stays pinned, and a Git package
    /// keeps the branch, tag or commit it is installed from). `None` for a
    /// package with no source to check: a folder's (reload it instead) and
    /// a default extension's (Pane updates those itself).
    pub fn check_for_update(
        &self,
        identity: &PackageIdentity,
    ) -> Option<Pin<Box<dyn Future<Output = ()> + Send + 'static>>> {
        enum Source {
            Npm(String),
            Git(String),
        }
        let source = {
            let state = self.lock();
            let package = state.package(identity)?;
            match (package.identity.npm_name(), package.git.as_ref()) {
                (Some(name), _) => Source::Npm(name.to_owned()),
                (None, Some(git)) => Source::Git(git.url.clone()),
                (None, None) => return None,
            }
        };
        Some(match source {
            Source::Npm(name) => Box::pin(self.preview_npm(&name)),
            Source::Git(url) => Box::pin(self.preview_git(&url)),
        })
    }

    /// The folder an installed package's page offers to show: a folder
    /// package's source folder, which its author edits, else Pane's managed
    /// copy of it.
    pub fn source_folder(&self, identity: &PackageIdentity) -> Option<PathBuf> {
        let state = self.lock();
        let package = state.package(identity)?;
        Some(
            package
                .identity
                .local_folder()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| package.location.clone()),
        )
    }
}

impl Launcher {
    /// Shows the folder [`Launcher::source_folder`] names for the installed
    /// package with `identity` in the system's file manager, as its page's
    /// "Show Source Folder" does, through the launcher's opener (off the
    /// calling thread: a handler may take a moment to start). What was done,
    /// or why it could not be.
    pub fn show_source_folder(
        &self,
        identity: &PackageIdentity,
    ) -> impl Future<Output = Result<String, String>> + Send + 'static {
        let folder = self.source_folder(identity);
        let links = self.links.clone();
        async move {
            let Some(folder) = folder else {
                return Err("That extension is not installed".into());
            };
            let shown = folder.clone();
            super::off_thread(move || links.open_file(&folder))
                .await
                .map(|()| format!("Showed {}", shown.display()))
                .map_err(|why| format!("Could not show {}: {why}", shown.display()))
        }
    }
}

/// What needs saying about an installed extension beside its name, in
/// Settings' sidebar and on its page (#168).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionMark {
    /// Pane paused it after an error, until the user retries it: why.
    Paused(String),
    /// Its installed copy cannot be read: why.
    Broken(String),
    /// It is being updated, reloaded or installed: what is happening.
    Updating(String),
}

impl ExtensionMark {
    /// The mark in a word, as the sidebar shows it.
    pub fn word(&self) -> &'static str {
        match self {
            ExtensionMark::Paused(_) => "Paused",
            ExtensionMark::Broken(_) => "Broken",
            ExtensionMark::Updating(_) => "Updating",
        }
    }

    /// What it says in full, as the extension's page shows it.
    pub fn reason(&self) -> &str {
        match self {
            ExtensionMark::Paused(why)
            | ExtensionMark::Broken(why)
            | ExtensionMark::Updating(why) => why,
        }
    }
}

impl Launcher {
    /// What needs saying about the installed extension with `identity`, if
    /// anything (#168): it is being updated or reloaded, its copy cannot be
    /// read, or Pane paused it after an error, in that order.
    pub fn extension_mark(&self, identity: &PackageIdentity) -> Option<ExtensionMark> {
        let state = self.lock();
        let package = state.package(identity)?;
        let title = package.title();
        if let Some(changing) = state.changing.get(identity)
            && matches!(
                changing,
                super::Changing::Updating
                    | super::Changing::BackgroundUpdating
                    | super::Changing::Reloading
                    | super::Changing::Installing
            )
        {
            return Some(ExtensionMark::Updating(format!(
                "{title} {}",
                changing.doing()
            )));
        }
        if let Err(error) = &package.manifest {
            return Some(ExtensionMark::Broken(format!(
                "{title} cannot load: {error}"
            )));
        }
        state
            .paused
            .is_paused(identity)
            .then(|| ExtensionMark::Paused(crate::packages::paused_reason(&title)))
    }

    /// Whether the installed extension with `identity` is one of Pane's
    /// own, an official extension (ADR 0045): a default extension, or a
    /// package whose recorded Git source is a repository in the pane-app
    /// organization on GitHub — installed by Pane at first setup or by
    /// the user by hand alike. An extension from any other source, another
    /// Git host, npm or a folder, is not official, and Settings says
    /// nothing of it. The mark is read here rather than derived in the
    /// window, so the rule lives in one place beside the rest the window
    /// reads about an extension.
    pub fn extension_is_official(&self, identity: &PackageIdentity) -> bool {
        identity.default_id().is_some()
            || identity
                .git_repository()
                .is_some_and(in_pane_app_organization)
    }
}

/// Whether the repository a Git package's identity records, spelled
/// `host[:port]/path` ([`crate::git::Repository::name`]), is one of Pane's
/// own in the pane-app organization on GitHub (ADR 0045): the host
/// `github.com` and a path under `pane-app/`. Pane's own spelling of the
/// identity is what is checked, so the path is lowercase however the
/// user wrote the address (github.com serves a repository at any case of
/// its path); a host with a port, or the same path on another host, is
/// not the pane-app organization.
fn in_pane_app_organization(repository: &str) -> bool {
    repository.starts_with("github.com/pane-app/")
}

/// What [`Launcher::begin_command_switch`] left to do.
enum Switch {
    /// Nothing: the command was already as asked.
    Done(String),
    /// Record the switch, applied in Pane already.
    Record {
        identity: PackageIdentity,
        manifest_id: String,
        title: String,
        store: Arc<Mutex<Store>>,
    },
}
