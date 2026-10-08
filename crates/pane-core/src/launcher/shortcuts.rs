//! The Shortcuts catalog: the installed commands with the aliases and
//! global hotkeys the user gave them, grouped by their packages, as the
//! Settings window's Shortcuts page lists them.
//!
//! The catalog is one read over the same records the launcher's own
//! management flow uses — `aliases.json` and `hotkeys.json`, by command id
//! — so the page and the extension pages cannot disagree, and nothing runs
//! to build it: it lists what the installed packages' manifests declare
//! and what Pane recorded, with each command's own explanation of why its
//! configuration is not active (its package is disabled or paused, the
//! command is unavailable on this system or gone from its package, the
//! system refused to register the hotkey, or another command has the same
//! alias). The build's own sample commands are not listed: no alias or
//! hotkey can be given to them, so the page would show nothing they can
//! do.
//!
//! Recording a hotkey is a later slice's work (#76): the catalog shows
//! each command's hotkey as it is recorded and whether it is active, and
//! offers no way to change it. Aliases are edited through
//! [`Launcher::set_alias`], which applies the alias form's rules without
//! opening the launcher's form screen; the Settings window calls it from
//! its own inline field.

use std::collections::HashSet;

use super::choices::split;
use super::{Launcher, State};
use crate::hotkeys::Shortcut;
use crate::packages::{InstalledPackage, PackageIdentity, paused_reason};

/// The installed commands with their aliases and hotkeys, grouped by
/// their packages: one snapshot of what the Settings window's Shortcuts
/// page lists. Rebuilt on demand — [`Launcher::shortcut_catalog`] — so
/// package changes appear in the next one that is built.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShortcutCatalog {
    /// Each installed package's commands, in the extension list's order,
    /// then the recorded choices whose commands no installed package has.
    pub groups: Vec<ShortcutGroup>,
    /// Why this system has no global hotkeys at all, if it has none; the
    /// page shows it beside the Hotkey column, since no hotkey here can
    /// be active.
    pub hotkeys_unavailable: Option<String>,
}

/// One package's commands as the Shortcuts page lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
    /// The package's identity — its source, which tells copies of a
    /// package apart. `None` for the one group of recorded choices whose
    /// package is not installed.
    pub identity: Option<PackageIdentity>,
    /// The group's title: the package's display title, or "Not installed"
    /// for the group of recorded choices whose package is not installed.
    pub title: String,
    /// Why the package's commands' configuration is not active, if it is
    /// not: it is disabled, or paused after an error.
    pub inactive: Option<String>,
    /// The package's commands in manifest order, then a row for each
    /// recorded choice whose command the package no longer has.
    pub commands: Vec<ShortcutCommand>,
}

/// One command as the Shortcuts page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutCommand {
    /// The command's id: its package identity's key and its id in the
    /// package's manifest.
    pub id: String,
    /// The command's title: the Name column.
    pub title: String,
    /// The command's subtitle, as root search shows it.
    pub subtitle: Option<String>,
    /// Whether the command takes a query: typing its alias and more text
    /// in root search sends the text to it.
    pub takes_query: bool,
    /// The alias the user gave it, as they typed it, if any.
    pub alias: Option<String>,
    /// Why the alias is not active, if it is not: the command's package is
    /// disabled or paused, it is unavailable on this system, it is gone
    /// from its package, or another command has the same alias (only in a
    /// record edited by hand).
    pub alias_inactive: Option<String>,
    /// Whether an alias can be given to the command here: a command that
    /// is gone keeps the alias recorded for it, but none can be given to
    /// it, only forgotten (its extension's page in Settings, or
    /// uninstalling).
    pub editable: bool,
    /// The global hotkey the user gave it, if any.
    pub hotkey: Option<Shortcut>,
    /// Why the hotkey is not active, if it is not: the command is not
    /// offered (its package is disabled, or the command is unavailable on
    /// this system), the system refused to register it, or this system has
    /// no global hotkeys at all. A command that cannot have a hotkey
    /// recorded here says so whether or not one is recorded, as its other
    /// configuration does.
    pub hotkey_inactive: Option<String>,
    /// Whether a hotkey can be recorded for the command here: a command
    /// whose package is disabled or that is unavailable on this system
    /// keeps the hotkey recorded, but none can be given to it until it is
    /// offered and available again; one that is gone from its package
    /// keeps the choice, which only uninstalling forgets.
    pub hotkey_editable: bool,
}

impl Launcher {
    /// The installed commands with their aliases and hotkeys, grouped by
    /// their packages, for the Settings window's Shortcuts page. See the
    /// module docs for what the catalog holds; alias editing goes through
    /// [`Launcher::set_alias`] and hotkey recording through
    /// [`Launcher::set_hotkey`].
    pub fn shortcut_catalog(&self) -> ShortcutCatalog {
        let state = self.lock();
        catalog(self, &state)
    }
}

/// The catalog over the launcher's state as it is now.
pub(super) fn catalog(launcher: &Launcher, state: &State) -> ShortcutCatalog {
    let everywhere = launcher.hotkeys.unavailable();
    // The command ids the installed packages have listed rows for, so the
    // recorded choices left over are found however they were left.
    let mut listed = HashSet::new();
    let mut groups = Vec::new();
    for package in &state.packages {
        let title = package.title();
        let disabled = (!package.enabled).then(|| format!("{title} is disabled"));
        let paused = state
            .paused
            .is_paused(&package.identity)
            .then(|| paused_reason(&title));
        // The alias row's package-level wording, as the extension list has
        // it: a disabled or paused package's choices are kept but not
        // active.
        let package_inactive = disabled.clone().or(paused);
        // The hotkey stays registered while the package is enabled, as its
        // row in the extension list shows it; only a disabled package
        // releases it.
        let hotkey_package_inactive = disabled;
        let mut commands = Vec::new();
        for entry in package.listed_commands() {
            let registration = entry.registration;
            // A root provider has no alias or hotkey, so the page does not
            // list it, nor what was recorded for it before it became one
            // (the next start forgets that).
            if entry.mode == crate::packages::CommandMode::Provider {
                listed.insert(registration.id);
                continue;
            }
            listed.insert(registration.id.clone());
            // A command the user turned off on its extension's page keeps
            // its alias and hotkey, not active until it is on again
            // (#168).
            let off = (!entry.enabled).then(|| format!("{} is turned off", registration.title));
            commands.push(command(
                state,
                package_inactive.as_deref().or(off.as_deref()),
                hotkey_package_inactive.as_deref().or(off.as_deref()),
                registration.id,
                registration.title,
                registration.subtitle,
                registration.takes_query,
                entry.unavailable.as_deref(),
            ));
        }
        // Choices whose command this package no longer lists: an update
        // dropped it, or its manifest cannot be read.
        commands.extend(missing(state, &mut listed, package));
        groups.push(ShortcutGroup {
            identity: Some(package.identity.clone()),
            title,
            inactive: package_inactive,
            commands,
        });
    }
    // Choices whose package is not installed at all: only a record edited
    // by hand leaves one, since uninstalling forgets a package's choices.
    let mut unlisted = Vec::new();
    for id in recorded(state) {
        if listed.contains(&id) {
            continue;
        }
        let (alias, hotkey) = choices_of(state, &id);
        if alias.is_none() && hotkey.is_none() {
            continue;
        }
        let command = split(&id).1.to_owned();
        let why = "its extension is not installed".to_owned();
        let hotkey_inactive = hotkey.is_some().then_some(why.clone());
        unlisted.push(ShortcutCommand {
            id,
            title: format!("`{command}`"),
            subtitle: None,
            takes_query: false,
            alias,
            alias_inactive: Some(why),
            editable: false,
            hotkey,
            hotkey_inactive,
            hotkey_editable: false,
        });
    }
    if !unlisted.is_empty() {
        groups.push(ShortcutGroup {
            identity: None,
            title: "Not installed".into(),
            inactive: None,
            commands: unlisted,
        });
    }
    ShortcutCatalog {
        groups,
        hotkeys_unavailable: everywhere,
    }
}

/// One command the package lists, with its recorded alias and hotkey and
/// why each is not active, if it is not.
#[allow(clippy::too_many_arguments)]
fn command(
    state: &State,
    package_inactive: Option<&str>,
    hotkey_package_inactive: Option<&str>,
    id: String,
    title: String,
    subtitle: Option<String>,
    takes_query: bool,
    unavailable: Option<&str>,
) -> ShortcutCommand {
    let alias = state.aliases.chosen.alias_of(&id);
    // The alias row's wording: the package's own state first, then the
    // command's unavailability here, then a record two commands share.
    let alias_inactive = package_inactive
        .map(str::to_owned)
        .or_else(|| unavailable.map(str::to_owned))
        .or_else(|| {
            let shared = alias
                .as_deref()
                .and_then(|alias| state.aliases.chosen.shared_with(&id, alias))
                .is_some();
            shared.then(|| "another command has the same alias".to_owned())
        });
    let hotkey = state.bindings.hotkey_of(&id);
    // The hotkey cell's wording: a command that cannot have a hotkey
    // recorded here — its package is disabled, or it is unavailable on
    // this system — says so whether or not one is recorded, as the alias
    // cell does; a command that can says why its recorded hotkey is not
    // active, when it is not (a registration the system refused, or one
    // another command has).
    let hotkey_inactive = unavailable
        .map(str::to_owned)
        .or_else(|| hotkey_package_inactive.map(str::to_owned))
        .or_else(|| hotkey.as_ref().and_then(|_| state.bindings.problem_of(&id)));
    ShortcutCommand {
        id,
        title,
        subtitle,
        takes_query,
        alias,
        alias_inactive,
        editable: true,
        hotkey,
        hotkey_inactive,
        // A command whose package is disabled, or that is unavailable on
        // this system, is not offered: its hotkey is kept, not changed
        // here. A paused package's commands are offered still.
        hotkey_editable: unavailable.is_none() && hotkey_package_inactive.is_none(),
    }
}

/// The rows for the recorded choices of `package` whose commands it no
/// longer lists: its manifest cannot be read, or an update dropped the
/// command. Recorded in `listed`, so the not-installed group does not take
/// them too.
fn missing(
    state: &State,
    listed: &mut HashSet<String>,
    package: &InstalledPackage,
) -> Vec<ShortcutCommand> {
    let key = package.identity.key();
    let title = package.title();
    let mut rows = Vec::new();
    for id in recorded(state) {
        if listed.contains(&id) || split(&id).0 != key {
            continue;
        }
        let (alias, hotkey) = choices_of(state, &id);
        if alias.is_none() && hotkey.is_none() {
            continue;
        }
        let command = split(&id).1;
        let why = match &package.manifest {
            Err(error) => format!("{title} cannot load: {error}"),
            Ok(_) => format!("{title} has no command `{command}` now"),
        };
        let hotkey_inactive = hotkey.is_some().then_some(why.clone());
        rows.push(ShortcutCommand {
            id: id.clone(),
            title: format!("`{command}`"),
            subtitle: None,
            takes_query: false,
            alias,
            alias_inactive: Some(why),
            editable: false,
            hotkey,
            hotkey_inactive,
            hotkey_editable: false,
        });
        listed.insert(id);
    }
    rows
}

/// Every command id with an alias, a fallback or a hotkey recorded, in a
/// stable order.
fn recorded(state: &State) -> Vec<String> {
    let mut ids: Vec<String> = state
        .aliases
        .chosen
        .recorded()
        .into_iter()
        .chain(state.bindings.recorded())
        .map(str::to_owned)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The alias and hotkey recorded for `command`.
fn choices_of(state: &State, command: &str) -> (Option<String>, Option<Shortcut>) {
    (
        state.aliases.chosen.alias_of(command),
        state.bindings.hotkey_of(command),
    )
}
