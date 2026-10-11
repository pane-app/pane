//! Dynamic root items: what extensions registered at run time (#158),
//! as root search lists them. The registry (`crate::registrations`) keeps
//! what the guests registered; this module turns the root item entries
//! into root search's rows, ranked like indexed results, and keeps the
//! looks their rows draw.
//!
//! A dynamic root item is under one of the package's commands, in the
//! item shape of docs/list-tree.md plus the `mode` that makes it a
//! **dynamic command**: without one, invoking its row runs its first
//! action's callback, delivered to the command's event entry point
//! (`handle-event`); with one, invoking it launches its command with a
//! launch record naming the item's id (its `context`), so one component
//! can offer a row per workspace, or whatever it registered. A quick
//! slot, an alias or a global hotkey holds it by its command and item
//! id, and while it is not registered says so, as one of a missing
//! indexed result does.
//!
//! A row appears when the item is registered and disappears when it is
//! dropped, its instance goes or its generation ends: the registry's
//! hooks refresh root search. A package that waits keeps its items
//! listed, saying what the package needs, as its commands' rows do.

use std::collections::HashMap;
use std::path::PathBuf;

use super::looks;
use super::quick_slots::PinTarget;
use super::{Entry, Opening, RootResult, Row, State, Unavailable};
use crate::launch::{LaunchRecord, LaunchSource};
use crate::packages::{CommandMatches, CommandWhen, InstalledPackage, paused_reason};
use crate::registrations::{self, RegistrationOf};
use crate::runtime::ItemLook;
use crate::search::Keys;

/// The dynamic root items' looks, by each row's id (`<command id>:<item
/// id>`), resolved in its package's managed copy: what their rows draw
/// beyond the title and subtitle. Kept in the launcher's state, as the
/// open command's items' looks are (see `looks`).
#[derive(Default)]
pub(super) struct Looks {
    by_row: HashMap<String, ItemLook>,
}

impl Looks {
    /// The look of the dynamic root item whose row id is `id`, if it is
    /// still registered.
    pub(super) fn of(&self, id: &str) -> Option<&ItemLook> {
        self.by_row.get(id)
    }

    /// Keeps the looks `listed` names, dropping those of rows root search
    /// lists no more.
    pub(super) fn keep(&mut self, listed: HashMap<String, ItemLook>) {
        self.by_row = listed;
    }
}

/// The row id of the item `item` under the command with id `command`:
/// `<command id>:<item id>`, as an indexed result's row id is built, so
/// the Actions panel, the quick slots, the aliases and the hotkeys name
/// it by one stable id.
pub(super) fn row_id(command: &str, item: &str) -> String {
    format!("{command}:{item}")
}

/// The launch record of a dynamic command invocation, naming the item's
/// id in its `context`, which the command reads to know which of its
/// dynamic items the user chose.
pub(super) fn launch_of(item: &str, source: LaunchSource) -> LaunchRecord {
    LaunchRecord {
        context: Some(format!("{{\"dynamicItem\":\"{item}\"}}")),
        ..LaunchRecord::by_user(source)
    }
}

/// Why a dynamic root item's row cannot run now, if it cannot: its
/// package is paused, unavailable on this system or waiting for what it
/// needs (the command its item is under included, when a use is narrowed
/// to it). `None` when the row runs.
fn unavailable(state: &State, package: &InstalledPackage, manifest: &str) -> Option<Unavailable> {
    let title = package.title();
    let paused = state
        .paused
        .is_paused(&package.identity)
        .then(|| Unavailable::Paused(paused_reason(&title)));
    paused
        .or_else(|| {
            state
                .waiting
                .reason_for(&package.identity, manifest)
                .map(|reason| Unavailable::Waiting(reason.row.clone()))
        })
        .or_else(|| {
            package
                .available_commands()
                .into_iter()
                .find(|(offered, _)| offered.manifest_id() == manifest)
                .and_then(|(_, unavailable)| unavailable)
                .map(Unavailable::OnThisSystem)
        })
}

/// One registered root item's row, with the look its row draws, or `None`
/// when the command its item is under is another component's now (its
/// code was replaced since): the item waits to be registered again.
fn row(
    state: &State,
    package: &InstalledPackage,
    registered: &RegistrationOf,
) -> Option<(RootResult, ItemLook)> {
    let registrations::Kind::RootItem { command, item } = &registered.kind else {
        return None;
    };
    let registration = package
        .commands()
        .into_iter()
        .find(|offered| offered.manifest_id() == command)?;
    if registration.component != registered.component {
        return None;
    }
    let unavailable = unavailable(state, package, command);
    // A row that cannot run says why, as a command's row does; one that
    // runs does what the item declares: without a mode, invoking it runs
    // its first action's callback; with one, it launches its command.
    let entry = match (&unavailable, item.mode, item.item.actions.first()) {
        (Some(why), _, _) => Entry::Unavailable(why.reason().to_owned()),
        (None, Some(mode), _) => Entry::Open(Opening {
            component: registered.component.clone(),
            command: command.clone(),
            search: registration.search,
            no_view: mode == registrations::DynamicMode::NoView,
            launch: launch_of(&item.item.id, LaunchSource::RootSearch),
            initial_search: None,
        }),
        (None, None, Some(action)) => match action.callback() {
            Some(callback) => Entry::DynamicAction(DynamicAction {
                component: registered.component.clone(),
                command: command.clone(),
                callback: callback.to_owned(),
            }),
            // The first action opens a submenu, which root search cannot
            // offer: the row lists, and activating it says so.
            None => Entry::NoActions,
        },
        (None, None, None) => Entry::NoActions,
    };
    let row = Row {
        id: row_id(&registration.id, &item.item.id),
        title: item.item.title.clone(),
        subtitle: item.item.subtitle.clone(),
        unavailable,
    };
    let keys = Keys::new(&row.title, row.subtitle.as_deref(), Some(&package.title()));
    Some((
        RootResult {
            keys,
            entry,
            row,
            target: None,
            pin: Some(PinTarget::Dynamic {
                command: registration.id.clone(),
                item: item.item.id.clone(),
            }),
            // A dynamic item has no entry of its own to say: it is listed
            // while it is registered, whatever is typed, and matched by
            // its title.
            when: CommandWhen::Always,
            matches: CommandMatches::Title,
        },
        item.item.look.clone(),
    ))
}

/// Every dynamic root item the packages registered, as root search rows
/// with the looks their rows draw, resolved in their packages' managed
/// copies.
pub(super) fn rows(state: &State) -> (Vec<RootResult>, HashMap<String, ItemLook>) {
    let Some(registrations) = state.registrations.as_ref() else {
        return (Vec::new(), HashMap::new());
    };
    let mut results = Vec::new();
    let mut looks = HashMap::new();
    for package in state.packages.iter().filter(|package| package.enabled) {
        for registered in registrations.all() {
            if registered.owner != package.identity.key() {
                continue;
            }
            if let Some((result, look)) = row(state, package, &registered) {
                looks.insert(
                    result.row.id.clone(),
                    looks::resolved(look, &package.location),
                );
                results.push(result);
            }
        }
    }
    (results, looks)
}

/// The root search row of the dynamic item `item` under the command with
/// id `command`, as the registry holds it now: what a quick slot, an
/// alias or a hotkey resolves to. `None` when none is registered: the
/// surfaces that hold it say so.
pub(super) fn pinned(state: &State, command: &str, item: &str) -> Option<RootResult> {
    pinned_by_id(state, &row_id(command, item))
}

/// The root search row of the dynamic item whose row id is `id`.
pub(super) fn pinned_by_id(state: &State, id: &str) -> Option<RootResult> {
    rows(state).0.into_iter().find(|result| result.row.id == id)
}

/// What one dynamic root item's activation runs: its first action's
/// callback, which the command's `handle-event` receives. In the
/// launcher's `Entry`, so activating the row runs it wherever the row is
/// listed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DynamicAction {
    /// The component whose instance registered the item: where the
    /// callback goes.
    pub(super) component: PathBuf,
    /// The manifest id of the command the item is under.
    pub(super) command: String,
    /// The action's callback id.
    pub(super) callback: String,
}
