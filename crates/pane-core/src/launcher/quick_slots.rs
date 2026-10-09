//! Quick slots: the ordered list of results the user pins to root search's
//! home, to reach each with one click (and the first five with Ctrl+1 to
//! Ctrl+5, which the window numbers). The list has no gaps and no length
//! limit: pinning adds to its end, unpinning takes an entry out and closes
//! the gap, and moving an entry swaps it with its neighbor.
//!
//! A slot holds an identity, never a row: a registered command by its id,
//! or an indexed result (an installed application) by its own id under
//! the command that supplies it ([`PinTarget`]). A row index, a title, a
//! computed answer or a granted file's handle is never held, so a pin
//! survives a rename and a new order of the results, and a result that
//! only exists for one query cannot be pinned at all.
//!
//! What a slot shows and does is resolved through the registry as it is
//! now, every time: the enabled commands root search lists and the
//! indexed results Pane keeps. A target that is disabled, paused, not
//! installed or not listed yet keeps its slot and says why it cannot run;
//! it never runs a stale target, and enabling or installing the same
//! identity again resolves it once more. Invoking a slot resolves it again
//! at that moment, and the call it makes belongs to the package's
//! generation current then, so a late answer from code replaced or
//! uninstalled since is discarded as any other is.
//!
//! The arrangement is Pane's own record — `quick-slots.json` beside the
//! host settings in Pane's data folder, never extension data — under the
//! house record rules: versioned, validated (a record this Pane cannot
//! read is reported and kept as it is on disk; nothing replaces it while
//! Pane runs), written atomically, one write at a time, each holding the
//! arrangement as it is when it begins. A change takes effect at once;
//! a write that fails puts back the arrangement the record last held and
//! says why. A fresh installation has no record and pins nothing. A
//! record of the first version — five positional slots, `null` for an
//! empty one — is read as the list of its pins in order; the next change
//! writes the current version.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use super::actions::{ResultAction, ResultActionItem, ResultActions};
use super::choices::split;
use super::indexed::Listing;
use super::presentation::{self, RowKind};
use super::{CommandRegistration, Entry, Launcher, RootResult, Screen, State, Status, off_thread};
use crate::applications::Applications;
use crate::atomic::{Readers, write_atomically};
use crate::packages::{InstalledPackage, paused_reason};
use crate::search::same_text;

/// The record's file name, in Pane's data folder beside `settings.json`.
const FILE: &str = "quick-slots.json";

/// The record's version: `{ "version": 2, "pins": [ … ] }`.
const VERSION: u64 = 2;

/// The first version, still read: `{ "version": 1, "slots": [ … ] }`, five
/// positional slots with `null` for an empty one.
const SLOTS_VERSION: u64 = 1;

/// What a quick slot holds: a stable identity, resolved each time through
/// the registry as it is then.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PinTarget {
    /// A registered command — an installed package's, or one this build
    /// offers — by its command id.
    Command(String),
    /// An indexed result, such as an installed application, by its own
    /// id among the results of the command (by id) that supplies it.
    Indexed { command: String, result: String },
}

impl PinTarget {
    /// The id of the root row this target is when root search lists it:
    /// a command's id, or `<command id>:<result id>` for an indexed
    /// result. The Actions panel names its target by it.
    pub fn key(&self) -> String {
        match self {
            PinTarget::Command(id) => id.clone(),
            PinTarget::Indexed { command, result } => format!("{command}:{result}"),
        }
    }
}

/// One quick slot as it stands now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickSlot {
    /// What the slot holds.
    pub target: PinTarget,
    /// The title to show: the target's own as root search lists it, else
    /// the best name Pane has for it (its command's title, or its id).
    pub title: String,
    /// What tells the target apart from another result of the same title
    /// its command lists (two applications of one name): its row's
    /// subtitle, which for an application is its distinction. Shown as
    /// the slot's tooltip and said with it; `None` when no other result
    /// shares its title.
    pub detail: Option<String>,
    /// What invoking it reaches, once resolved.
    pub kind: Option<RowKind>,
    /// Why it cannot run now — disabled, paused, not installed, not listed
    /// yet — if it cannot; it keeps its slot meanwhile.
    pub unavailable: Option<String>,
}

impl QuickSlot {
    /// Whether invoking the slot runs its target now.
    pub fn ready(&self) -> bool {
        self.unavailable.is_none()
    }
}

/// What a quick slot change did (see [`Launcher::change_quick_slots`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotChange {
    /// Nothing: the action does not apply to that target now (it is no
    /// longer selected, a move at the end, an unreadable record).
    Refused,
    /// Nothing: the target already holds the slot at this index.
    AlreadyPinned(usize),
    /// The slots changed: the target now holds the slot at this index
    /// (`None` once removed). The returned future records it.
    Changed(Option<usize>),
}

/// The pinned targets, in order.
type Arrangement = Vec<PinTarget>;

/// The quick slots in Pane and their record.
#[derive(Default)]
pub(super) struct Kept {
    /// The record's file; `None` for a launcher given no data folder,
    /// whose slots last until it stops.
    file: Option<PathBuf>,
    /// The arrangement as it stands in Pane.
    chosen: Arrangement,
    /// The arrangement the record last held: what a failed write puts
    /// back.
    saved: Arrangement,
    /// Why the record could not be read, if it could not; it is then
    /// never replaced.
    unreadable: Option<String>,
    /// Held while the record is written, so writes happen one at a time.
    writing: Arc<Mutex<()>>,
}

impl Kept {
    /// The slots recorded in `dir`: none without a record, and none, with
    /// the problem, for a record that cannot be read.
    fn open(dir: &Path) -> Kept {
        let file = dir.join(FILE);
        let mut kept = Kept {
            file: Some(file.clone()),
            ..Kept::default()
        };
        match read(&file) {
            Ok(arrangement) => {
                kept.chosen = arrangement.clone();
                kept.saved = arrangement;
            }
            Err(problem) => kept.unreadable = Some(problem),
        }
        kept
    }

    /// Why the record could not be read, if it could not.
    pub(super) fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    /// The slot holding `target`, if one does.
    fn slot_of(&self, target: &PinTarget) -> Option<usize> {
        self.chosen.iter().position(|slot| slot == target)
    }

    /// The slot holding the target with key `key`, if one does.
    fn slot_keyed(&self, key: &str) -> Option<usize> {
        self.chosen.iter().position(|slot| slot.key() == key)
    }
}

/// What the status line says while an unreadable record keeps the slots
/// from being changed.
pub(super) fn unreadable_report(problem: &str) -> String {
    format!("Pane could not read the quick slots, so it keeps their record as it is: {problem}")
}

/// The arrangement recorded in `file`: none without a record; `Err` with
/// the problem, phrased with the file's path, for one that cannot be read.
fn read(file: &Path) -> Result<Arrangement, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Arrangement::default());
        }
        Err(error) => return Err(format!("{} cannot be read: {error}", file.display())),
    };
    parse(&text).map_err(|problem| format!("{} {problem}", file.display()))
}

/// The arrangement a record's text holds, or what is wrong with it,
/// phrased to follow the file's path ("is invalid: …").
fn parse(text: &str) -> Result<Arrangement, String> {
    let fields: Map<String, Value> =
        serde_json::from_str(text).map_err(|error| format!("is invalid: {error}"))?;
    // The first version's positional slots may be empty (`null`); the
    // current version's list holds pins only.
    let (field, gaps) = match fields.get("version").and_then(Value::as_u64) {
        Some(VERSION) => ("pins", false),
        Some(SLOTS_VERSION) => ("slots", true),
        Some(version) => {
            return Err(format!(
                "has version {version}, which this Pane does not read"
            ));
        }
        None => return Err("is invalid: it has no version".into()),
    };
    let mut arrangement = Arrangement::new();
    let Some(entries) = fields.get(field) else {
        return Ok(arrangement);
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| format!("is invalid: its {field} are not a list"))?;
    for (index, entry) in entries.iter().enumerate() {
        let target = match entry {
            Value::Null if gaps => continue,
            Value::Object(entry) => target_of(entry)
                .map_err(|problem| format!("is invalid: pin {}: {problem}", index + 1))?,
            _ => return Err(format!("is invalid: pin {} is not a pin", index + 1)),
        };
        if arrangement.contains(&target) {
            return Err(format!(
                "is invalid: pin {} pins what another pin pins",
                index + 1
            ));
        }
        arrangement.push(target);
    }
    Ok(arrangement)
}

/// The target a slot's entry names: `{ "command": "<id>" }`, or
/// `{ "command": "<id>", "result": "<id>" }` for an indexed result.
fn target_of(entry: &Map<String, Value>) -> Result<PinTarget, String> {
    if let Some(unknown) = entry
        .keys()
        .find(|key| *key != "command" && *key != "result")
    {
        return Err(format!(
            "it has a field “{unknown}” this Pane does not know"
        ));
    }
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| !command.is_empty())
        .ok_or("it names no command")?;
    match entry.get("result") {
        None => Ok(PinTarget::Command(command.to_owned())),
        Some(Value::String(result)) if !result.is_empty() => Ok(PinTarget::Indexed {
            command: command.to_owned(),
            result: result.clone(),
        }),
        Some(_) => Err("its result is not an id".into()),
    }
}

/// The record's text for `arrangement`: every pin, in order.
fn text(arrangement: &Arrangement) -> String {
    let pins: Vec<Value> = arrangement
        .iter()
        .map(|pin| match pin {
            PinTarget::Command(id) => serde_json::json!({ "command": id }),
            PinTarget::Indexed { command, result } => {
                serde_json::json!({ "command": command, "result": result })
            }
        })
        .collect();
    let record = serde_json::json!({ "version": VERSION, "pins": pins });
    serde_json::to_string_pretty(&record).expect("a JSON value always serializes")
}

/// How a target resolves now.
struct Resolved {
    title: String,
    /// What tells it apart from a result of the same title
    /// ([`QuickSlot::detail`]).
    detail: Option<String>,
    kind: Option<RowKind>,
    /// What invoking it does, or why it cannot run.
    outcome: Result<Entry, String>,
}

/// `target` resolved through the registry as it is in `state`.
fn resolve(launcher: &Launcher, state: &State, target: &PinTarget) -> Resolved {
    match target {
        PinTarget::Command(id) => resolve_command(launcher, state, target, id),
        PinTarget::Indexed { command, .. } => resolve_indexed(state, target, command),
    }
}

/// The command with id `id` among `package`'s, whether it runs or not.
fn command_in(package: &InstalledPackage, id: &str) -> Option<CommandRegistration> {
    package
        .commands()
        .into_iter()
        .find(|command| command.id == id)
}

/// The installed package offering the command with id `command`, with
/// that command, whether it runs or not.
fn registered<'a>(
    state: &'a State,
    command: &str,
) -> Option<(&'a InstalledPackage, CommandRegistration)> {
    state
        .packages
        .iter()
        .find_map(|package| Some((package, command_in(package, command)?)))
}

/// The best name for a command Pane cannot find: its id in its manifest,
/// never a guessed title.
fn missing_title(command: &str) -> String {
    match split(command).1 {
        "" => command.to_owned(),
        manifest_id => manifest_id.to_owned(),
    }
}

fn resolve_command(launcher: &Launcher, state: &State, target: &PinTarget, id: &str) -> Resolved {
    // What root search lists: this build's commands and the enabled
    // packages' (a paused one's say why they do not run).
    if let Some(listed) = launcher
        .root_results(state)
        .into_iter()
        .find(|result| result.pin.as_ref() == Some(target))
    {
        let outcome = match listed.entry {
            Entry::Unavailable(reason) => Err(reason),
            // A quick slot of a waiting command says why it cannot run and
            // runs nothing (see `waiting`).
            Entry::Waiting { identity, .. } => Err(state.waiting.reason(&identity).map_or_else(
                || format!("{} no longer waits", state.title_of(&identity)),
                |reason| reason.row.clone(),
            )),
            entry => Ok(entry),
        };
        return Resolved {
            title: listed.row.title,
            detail: None,
            kind: Some(RowKind::Command),
            outcome,
        };
    }
    // Not listed: why not.
    let key = split(id).0;
    let installed = state
        .packages
        .iter()
        .find(|package| package.identity.key() == key);
    let registered = installed.and_then(|package| command_in(package, id));
    let title = registered
        .map(|command| command.title)
        .unwrap_or_else(|| missing_title(id));
    let reason = match installed {
        Some(package) if !package.enabled => format!("{} is disabled", package.title()),
        Some(package) if package.manifest.is_err() => format!("{} cannot load", package.title()),
        // Pinned before it became a root provider (#164): the next start
        // removes the pin.
        Some(package) if package.is_provider(split(id).1) => {
            format!("{title} only answers root search now")
        }
        Some(package) => format!("{} no longer offers this command", package.title()),
        None => "Its extension is not installed".to_owned(),
    };
    Resolved {
        title,
        detail: None,
        kind: Some(RowKind::Command),
        outcome: Err(reason),
    }
}

fn resolve_indexed(state: &State, target: &PinTarget, command: &str) -> Resolved {
    let Some((package, registration)) = registered(state, command) else {
        return Resolved {
            title: missing_title(command),
            detail: None,
            kind: None,
            outcome: Err("Its extension is not installed".into()),
        };
    };
    let unresolved = |reason: String| Resolved {
        title: registration.title.clone(),
        detail: None,
        kind: None,
        outcome: Err(reason),
    };
    if !package.enabled {
        return unresolved(format!("{} is disabled", package.title()));
    }
    if state.paused.is_paused(&package.identity) {
        return unresolved(paused_reason(&package.title()));
    }
    if !package
        .indexed_result_commands()
        .iter()
        .any(|indexing| indexing.id == command)
    {
        return unresolved(format!(
            "{} does not list results on this system",
            registration.title
        ));
    }
    if let Some(found) = state
        .indexes
        .results()
        .find(|result| result.pin.as_ref() == Some(target))
    {
        // Two results of one title (two applications of one name): the
        // slot says what its row's subtitle says to tell them apart.
        let theirs = |other: &&RootResult| matches!(&other.pin, Some(PinTarget::Indexed { command: pinned, .. }) if pinned == command);
        let shared =
            state.indexes.results().filter(theirs).any(|other| {
                other.pin != found.pin && same_text(&other.row.title, &found.row.title)
            });
        return Resolved {
            title: found.row.title.clone(),
            detail: found.row.subtitle.clone().filter(|_| shared),
            kind: presentation::kind(&found.entry),
            outcome: Ok(found.entry.clone()),
        };
    }
    unresolved(match state.indexes.listing(&registration.component) {
        Listing::NotAsked | Listing::Asking => {
            format!("Waiting for {} to list it", registration.title)
        }
        Listing::Failed(problem) => problem,
        Listing::Listed => format!("{} no longer lists it", registration.title),
    })
}

/// What a quick slot pinning one of `command`'s results by `result`, an
/// id the command no longer lists, holds now, when `result` names an
/// installed application by another id, such as the path that was its id
/// before applications had stable identities: the command's result for
/// that application (the one with its current id, else the one opening
/// it). `None` when `result` is listed or names no application.
fn carried(
    state: &State,
    command: &str,
    result: &str,
    applications: &dyn Applications,
) -> Option<PinTarget> {
    let theirs = |found: &&RootResult| matches!(&found.pin, Some(PinTarget::Indexed { command: pinned, .. }) if pinned == command);
    let listed = |id: &str| {
        state.indexes.results().filter(theirs).find(
            |found| matches!(&found.pin, Some(PinTarget::Indexed { result, .. }) if result == id),
        )
    };
    if listed(result).is_some() {
        return None;
    }
    let current = applications
        .current_id(result)
        .filter(|current| current != result)?;
    listed(&current)
        .or_else(|| {
            state.indexes.results().filter(theirs).find(
                |found| matches!(&found.entry, Entry::OpenApplication { id, .. } if *id == current),
            )
        })
        .and_then(|found| found.pin.clone())
}

/// Carries the quick slots pinning `command`'s results by an id it no
/// longer lists over to the result now listed for the same application
/// ([`carried`]), as the command's results arrive: a pin made before
/// applications had stable identities resolves, and its record is
/// rewritten. A pin carried to a result already pinned leaves its slot.
/// Returns whether anything changed, so the caller records it; nothing
/// changes while the record cannot be read.
pub(super) fn carry_over(
    state: &mut State,
    command: &str,
    applications: &dyn Applications,
) -> bool {
    if state.quick_slots.unreadable.is_some() {
        return false;
    }
    let chosen = std::mem::take(&mut state.quick_slots.chosen);
    let mut changed = false;
    let mut kept = Arrangement::with_capacity(chosen.len());
    for target in chosen {
        let carried_to = match &target {
            PinTarget::Indexed {
                command: pinned,
                result,
            } if pinned == command => carried(state, command, result, applications),
            _ => None,
        };
        let target = match carried_to {
            Some(carried_to) => {
                changed = true;
                carried_to
            }
            None => target,
        };
        if kept.contains(&target) {
            changed = true;
            continue;
        }
        kept.push(target);
    }
    state.quick_slots.chosen = kept;
    changed
}

/// Takes out the slots pinning a root provider (#164), which has no row
/// and cannot be pinned: one pinned before it became one. An indexed
/// result it supplies, such as an application, keeps its slot. Whether any
/// went; each is noted for the toast (see `providers`).
fn forget_provider_pins(state: &mut State) -> bool {
    let providers = super::providers::providers(state);
    let mut gone = Vec::new();
    state.quick_slots.chosen.retain(|target| match target {
        PinTarget::Command(id) => match super::providers::title_of(&providers, id) {
            Some(title) => {
                gone.push(title.to_owned());
                false
            }
            None => true,
        },
        PinTarget::Indexed { .. } => true,
    });
    for title in &gone {
        Launcher::note_provider_pin(state, title);
    }
    !gone.is_empty()
}

/// The slot holding `target` as it stands in `state`.
fn view(launcher: &Launcher, state: &State, target: &PinTarget) -> QuickSlot {
    let resolved = resolve(launcher, state, target);
    QuickSlot {
        target: target.clone(),
        title: resolved.title,
        detail: resolved.detail,
        kind: resolved.kind,
        unavailable: resolved.outcome.err(),
    }
}

/// The identity a quick slot would hold for root search's selected row,
/// if it is a result one can hold: a command's row, available or not, or
/// an indexed result's. `None` off root search, with nothing selected, and
/// for every other row (Pane's own, a computed answer, a file, an alias's
/// or a fallback's text).
pub(super) fn pin_of_selected(state: &State) -> Option<PinTarget> {
    if !matches!(state.view.screen, Screen::Root { .. }) {
        return None;
    }
    let index = state.view.selected?;
    let row = state.view.rows.get(index)?;
    if !matches!(
        state.entries.get(index),
        Some(
            Entry::Open(_)
                | Entry::Unavailable(_)
                | Entry::Waiting { .. }
                | Entry::OpenApplication { .. }
                | Entry::OpenTarget { .. }
        )
    ) {
        return None;
    }
    state
        .root
        .iter()
        .chain(state.indexes.results())
        .find(|result| result.row.id == row.id)
        .and_then(|result| result.pin.clone())
}

/// A quick slot entry of the Actions panel.
fn item(action: ResultAction, available: bool) -> ResultActionItem {
    ResultActionItem {
        action,
        label: action
            .quick_slot_label()
            .expect("a quick slot entry")
            .to_owned(),
        available,
    }
}

/// The Actions panel's quick slot entry for root search's selected row, a
/// result a slot can hold: pinning it, or unpinning it once it is pinned
/// (its slot's own panel also moves it, [`slot_items`]). It cannot run
/// while the record cannot be read.
pub(super) fn pin_item(state: &State) -> ResultActionItem {
    let pinned =
        pin_of_selected(state).is_some_and(|pin| state.quick_slots.slot_of(&pin).is_some());
    let action = if pinned {
        ResultAction::Unpin
    } else {
        ResultAction::Pin
    };
    item(action, state.quick_slots.unreadable.is_none())
}

/// A pinned slot's own entries, for the slot at `slot` of `count`:
/// removing it, and moving it up and down where there is a slot to move
/// to.
fn slot_items(slot: usize, count: usize, readable: bool) -> Vec<ResultActionItem> {
    vec![
        item(ResultAction::Unpin, readable),
        item(ResultAction::MovePinUp, readable && slot > 0),
        item(ResultAction::MovePinDown, readable && slot + 1 < count),
    ]
}

/// The title of root search's selected row.
fn selected_title(state: &State) -> String {
    state
        .view
        .selected
        .and_then(|index| state.view.rows.get(index))
        .map(|row| row.title.clone())
        .unwrap_or_default()
}

/// Applies `action` to the target named `target` in `state`: see
/// [`Launcher::change_quick_slots`]. Returns what it did and, for a change,
/// what the status says once it is recorded.
fn change(
    launcher: &Launcher,
    state: &mut State,
    target: &str,
    action: ResultAction,
) -> (SlotChange, String) {
    let refused = (SlotChange::Refused, String::new());
    let quick_slot_action = !matches!(
        action,
        ResultAction::Invoke
            | ResultAction::Hotkey
            | ResultAction::Alias
            | ResultAction::ConfigureCommand
            | ResultAction::ConfigureExtension
            | ResultAction::DismissNotice
    );
    if !quick_slot_action || !matches!(state.view.screen, Screen::Root { .. }) {
        return refused;
    }
    if let Some(problem) = state.quick_slots.unreadable.clone() {
        state.view.status = Status::Error(unreadable_report(&problem));
        return refused;
    }
    match action {
        ResultAction::Pin => {
            let Some(pin) = pin_of_selected(state).filter(|pin| pin.key() == target) else {
                return refused;
            };
            let title = selected_title(state);
            if let Some(slot) = state.quick_slots.slot_of(&pin) {
                return (
                    SlotChange::AlreadyPinned(slot),
                    format!("{title} is already pinned"),
                );
            }
            state.quick_slots.chosen.push(pin);
            (
                SlotChange::Changed(Some(state.quick_slots.chosen.len() - 1)),
                format!("Pinned {title}"),
            )
        }
        ResultAction::Unpin | ResultAction::MovePinUp | ResultAction::MovePinDown => {
            let Some(slot) = state.quick_slots.slot_keyed(target) else {
                return refused;
            };
            let pinned = state.quick_slots.chosen[slot].clone();
            let title = view(launcher, state, &pinned).title;
            let count = state.quick_slots.chosen.len();
            let moved_to = match action {
                ResultAction::MovePinUp if slot > 0 => slot - 1,
                ResultAction::MovePinDown if slot + 1 < count => slot + 1,
                ResultAction::Unpin => {
                    state.quick_slots.chosen.remove(slot);
                    return (SlotChange::Changed(None), format!("Unpinned {title}"));
                }
                _ => return refused,
            };
            state.quick_slots.chosen.swap(slot, moved_to);
            (
                SlotChange::Changed(Some(moved_to)),
                format!("Moved {title} to place {}", moved_to + 1),
            )
        }
        ResultAction::Invoke
        | ResultAction::Hotkey
        | ResultAction::Alias
        | ResultAction::ConfigureCommand
        | ResultAction::ConfigureExtension
        | ResultAction::DismissNotice => refused,
    }
}

impl Launcher {
    /// This launcher keeping its quick slots in `dir`, Pane's data folder
    /// (beside the host settings' `settings.json`): the arrangement
    /// recorded there is read now. A record that cannot be read leaves
    /// every slot empty, is reported on root search's status line, and is
    /// never replaced. Without this, the slots last until the launcher
    /// stops. A slot pinning a command that has become a root provider is
    /// taken out and the record written, with a toast saying so (#164).
    pub fn with_quick_slots(self, dir: &Path) -> Self {
        let forgot = {
            let mut state = self.lock();
            state.quick_slots = Kept::open(dir);
            let problem = state.quick_slots.unreadable.clone();
            if let Some(problem) = problem
                && matches!(state.view.screen, Screen::Root { .. })
                && state.view.status == Status::Idle
            {
                state.view.status = Status::Error(unreadable_report(&problem));
            }
            forget_provider_pins(&mut state)
        };
        if forgot {
            // Written now, once: the next start reads the slots without them.
            if let Err(problem) = self.write_quick_slots() {
                crate::diagnostic!("Pane could not forget the pin of a root provider: {problem}");
            }
            let mut state = self.lock();
            self.show_provider_toast(&mut state);
        }
        self
    }

    /// The quick slots, in order, each resolved through the registry as
    /// it is now (see the module docs).
    pub fn quick_slots(&self) -> Vec<QuickSlot> {
        let state = self.lock();
        state
            .quick_slots
            .chosen
            .iter()
            .map(|target| view(self, &state, target))
            .collect()
    }

    /// Why the quick slots' record could not be read, if it could not:
    /// the slots are empty then, and no change is kept.
    pub fn quick_slots_problem(&self) -> Option<String> {
        self.lock().quick_slots.unreadable.clone()
    }

    /// Asks the enabled commands whose indexed results the quick slots pin
    /// for those results, if they never answered — what a cold visit of
    /// root search's home needs, since the indexed results are otherwise
    /// asked for only once a query is typed. The query stays as it is and
    /// nothing is searched; await the returned future to list them, which
    /// resolves the slots holding them.
    pub fn resolve_quick_slots(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut guard = self.lock();
        let state = &mut *guard;
        let pinned: Vec<String> = state
            .quick_slots
            .chosen
            .iter()
            .filter_map(|target| match target {
                PinTarget::Indexed { command, .. } => Some(command.clone()),
                PinTarget::Command(_) => None,
            })
            .collect();
        let commands: Vec<_> = state
            .packages
            .iter()
            .filter(|package| state.runs(package))
            .flat_map(|package| {
                let data = self
                    .installation
                    .as_ref()
                    .map(|installation| installation.data.owned_by(&package.identity));
                package
                    .indexed_result_commands()
                    .into_iter()
                    .map(move |command| (command, data.clone()))
            })
            .filter(|(command, _)| {
                pinned.contains(&command.id) && !state.indexes.answered(&command.component)
            })
            .collect();
        let asking = state.indexes.begin_asking(commands);
        drop(guard);
        let launcher = self.clone();
        async move { launcher.show_indexed_results(asking).await }
    }

    /// Invokes the quick slot at `index` from root search: its target is
    /// resolved again now and, when it can run, opened as its row would
    /// be — the command opens, the application is opened — in the
    /// package's generation current now. A target that cannot run says why
    /// on the status line and nothing runs; an index past the list,
    /// another screen than root search, or an action still running (the
    /// slot's own opening, invoked again) does nothing. Await the returned
    /// future to apply the reply.
    pub fn activate_quick_slot(&self, index: usize) -> impl Future<Output = ()> + Send + 'static {
        let mut guard = self.lock();
        let state = &mut *guard;
        // Only root search's slots, and never while an action already runs:
        // a second press or click during an opening invokes nothing.
        let ready = matches!(state.view.screen, Screen::Root { .. })
            && state.view.status != Status::Running;
        let target = state.quick_slots.chosen.get(index).cloned();
        let entry = match target.filter(|_| ready) {
            Some(target) => match resolve(self, state, &target).outcome {
                Ok(entry) => Some(entry),
                Err(reason) => {
                    state.view.status = Status::Error(reason);
                    None
                }
            },
            None => None,
        };
        // Only a command or an indexed result is ever pinned. A command is
        // launched from its quick slot.
        let entry = match entry {
            Some(Entry::Open(mut opening)) => {
                opening.launch.source = crate::launch::LaunchSource::QuickSlot;
                Some(Entry::Open(opening))
            }
            Some(entry @ (Entry::OpenApplication { .. } | Entry::OpenTarget { .. })) => Some(entry),
            _ => None,
        };
        match &entry {
            // Root search stays while a no-view command runs.
            Some(Entry::Open(opening)) if opening.no_view => Launcher::begin_run(state),
            Some(_) => {
                // The status line is about this action from now on.
                state.sent_from = None;
                state.view.status = Status::Running;
            }
            None => {}
        }
        let data = match &entry {
            // A call into the package belongs to its generation as of now.
            Some(Entry::Open(opening)) => self.data_in(state, &opening.component),
            _ => None,
        };
        let epoch = state.screen_epoch;
        drop(guard);
        let launcher = self.clone();
        async move {
            match entry {
                Some(Entry::Open(opening)) => launcher.launch_opening(epoch, opening, data).await,
                Some(Entry::OpenApplication { id, name }) => {
                    launcher.open_application(epoch, id, name).await
                }
                Some(Entry::OpenTarget {
                    target,
                    application,
                    name,
                }) => launcher.open_target(epoch, target, application, name).await,
                _ => {}
            }
        }
    }

    /// The slot holding the target with key `target` ([`PinTarget::key`]),
    /// if one does.
    pub fn quick_slot_of(&self, target: &str) -> Option<usize> {
        self.lock().quick_slots.slot_keyed(target)
    }

    /// The Actions panel's entries for the quick slot holding `target`, as
    /// a slot's own panel lists them: invoking it (unavailable while its
    /// target cannot run), removing it — whatever its target's state, so
    /// a disabled or missing one can always be removed — and moving it up
    /// and down where there is a slot to move to. `None` off root search,
    /// or when no slot holds it.
    pub fn quick_slot_actions(&self, target: &str) -> Option<ResultActions> {
        let state = self.lock();
        if !matches!(state.view.screen, Screen::Root { .. }) {
            return None;
        }
        let slot = state.quick_slots.slot_keyed(target)?;
        let shown = view(self, &state, &state.quick_slots.chosen[slot]);
        let readable = state.quick_slots.unreadable.is_none();
        // Named as the footer names the same row's primary action.
        let primary = match shown.kind {
            Some(RowKind::Application) => "Open application",
            Some(RowKind::Link) => "Open link",
            _ => "Open command",
        };
        let mut items = vec![ResultActionItem {
            action: ResultAction::Invoke,
            label: primary.to_owned(),
            available: shown.ready(),
        }];
        items.extend(slot_items(slot, state.quick_slots.chosen.len(), readable));
        Some(ResultActions {
            target: target.to_owned(),
            title: shown.title,
            items,
        })
    }

    /// Whether `action` can run on the quick slot holding `target` now.
    pub fn quick_slot_action_ready(&self, target: &str, action: ResultAction) -> bool {
        self.quick_slot_actions(target).is_some_and(|actions| {
            actions
                .items
                .iter()
                .any(|item| item.action == action && item.available)
        })
    }

    /// Changes the quick slots as `action` asks, for `target`: root
    /// search's selected row (by its id) for [`ResultAction::Pin`], or the
    /// slot holding it for [`ResultAction::Unpin`],
    /// [`ResultAction::MovePinUp`] and [`ResultAction::MovePinDown`].
    ///
    /// Pinning adds a slot at the end; pinning what a slot already holds
    /// changes nothing and names that slot. A change
    /// takes effect at once and the returned future records it — off the
    /// window's thread, one write at a time — saying on the status line
    /// what changed, or, when the record cannot be written, why, with the
    /// arrangement it last held put back. Nothing changes while the
    /// record cannot be read, which the status line says.
    pub fn change_quick_slots(
        &self,
        target: &str,
        action: ResultAction,
    ) -> (SlotChange, impl Future<Output = ()> + Send + 'static) {
        let mut guard = self.lock();
        let state = &mut *guard;
        let (changed, said) = change(self, state, target, action);
        let save = match &changed {
            SlotChange::Changed(_) => {
                state.view.status = Status::Running;
                Some((state.screen_epoch, said))
            }
            SlotChange::AlreadyPinned(_) => {
                state.view.status = Status::Result(said);
                None
            }
            SlotChange::Refused => None,
        };
        drop(guard);
        let launcher = self.clone();
        let recording = async move {
            let Some((epoch, done)) = save else {
                return;
            };
            let writer = launcher.clone();
            let written = off_thread(move || writer.write_quick_slots()).await;
            let mut state = launcher.lock();
            match written {
                Ok(()) if state.screen_epoch == epoch => state.view.status = Status::Result(done),
                Ok(()) => {}
                // A failure is said wherever the launcher is now: the
                // arrangement on screen went back to what the record holds.
                Err(problem) => {
                    state.view.status =
                        Status::Error(format!("Could not keep the quick slots: {problem}"))
                }
            }
        };
        (changed, recording)
    }

    /// Records the quick slots after [`carry_over`] changed them, off the
    /// calling thread. A failed write puts back what the record holds,
    /// whose pins are carried over again when the results next arrive, so
    /// nothing is said about it.
    pub(super) async fn record_carried_over(&self) {
        let writer = self.clone();
        let _ = off_thread(move || writer.write_quick_slots()).await;
    }

    /// Writes the arrangement as it is when the write begins, blocking:
    /// run it off the window's thread. On failure the arrangement in Pane
    /// goes back to what the record last held — unless it changed again
    /// meanwhile, which that change's own write records — and `Err` says
    /// why. A launcher with no data folder writes nothing and says so,
    /// keeping its arrangement until it stops.
    fn write_quick_slots(&self) -> Result<(), String> {
        let writing = self.lock().quick_slots.writing.clone();
        let _one_at_a_time = writing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (file, snapshot, unreadable) = {
            let state = self.lock();
            let kept = &state.quick_slots;
            (
                kept.file.clone(),
                kept.chosen.clone(),
                kept.unreadable.clone(),
            )
        };
        if let Some(problem) = unreadable {
            return Err(format!("Pane does not replace it: {problem}"));
        }
        let Some(file) = file else {
            return Err(
                "Pane has no data folder to keep them in, so they last only until Pane quits"
                    .into(),
            );
        };
        let written = write_atomically(&file, text(&snapshot).as_bytes(), Readers::Default)
            .map_err(|error| format!("{} cannot be written: {error}", file.display()));
        let mut state = self.lock();
        let kept = &mut state.quick_slots;
        match &written {
            Ok(()) => kept.saved = snapshot,
            Err(_) if kept.chosen == snapshot => kept.chosen = kept.saved.clone(),
            Err(_) => {}
        }
        written
    }
}
