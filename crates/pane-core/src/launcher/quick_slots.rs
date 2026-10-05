//! Quick slots: the five ordered places on root search's home where the
//! user pins results, to reach each with one click or Ctrl+1 to Ctrl+5.
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
//! says why. A fresh installation has no record and pins nothing.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use super::actions::{ResultAction, ResultActionItem, ResultActions};
use super::choices::split;
use super::indexed::Listing;
use super::presentation::{self, RowKind};
use super::{CommandRegistration, Entry, Launcher, Screen, State, Status, off_thread};
use crate::atomic::{Readers, write_atomically};
use crate::packages::{InstalledPackage, paused_reason};

/// How many quick slots there are.
pub const QUICK_SLOTS: usize = 5;

/// The record's file name, in Pane's data folder beside `settings.json`.
const FILE: &str = "quick-slots.json";

/// The record's version; a record of another version is not read.
const VERSION: u64 = 1;

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
    /// What the slot holds; `None` for an empty slot, which invokes
    /// nothing.
    pub target: Option<PinTarget>,
    /// The title to show: the target's own as root search lists it, else
    /// the best name Pane has for it (its command's title, or its id).
    /// Empty for an empty slot.
    pub title: String,
    /// What invoking it reaches, once resolved.
    pub kind: Option<RowKind>,
    /// Why it cannot run now — disabled, paused, not installed, not listed
    /// yet — if it cannot; it keeps its slot meanwhile.
    pub unavailable: Option<String>,
}

impl QuickSlot {
    fn empty() -> QuickSlot {
        QuickSlot {
            target: None,
            title: String::new(),
            kind: None,
            unavailable: None,
        }
    }

    /// Whether the slot holds nothing.
    pub fn is_empty(&self) -> bool {
        self.target.is_none()
    }

    /// Whether invoking the slot runs its target now.
    pub fn ready(&self) -> bool {
        self.target.is_some() && self.unavailable.is_none()
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
    /// Nothing yet: all five slots are taken, so the slot to replace must
    /// be chosen first ([`Launcher::pin_replacements`]).
    ChooseReplacement,
    /// The slots changed: the target now holds the slot at this index
    /// (`None` once removed). The returned future records it.
    Changed(Option<usize>),
}

/// The five slots, in order.
type Arrangement = [Option<PinTarget>; QUICK_SLOTS];

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
        self.chosen
            .iter()
            .position(|slot| slot.as_ref() == Some(target))
    }

    /// The slot holding the target with key `key`, if one does.
    fn slot_keyed(&self, key: &str) -> Option<usize> {
        self.chosen
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|target| target.key() == key))
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
    match fields.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(version) => {
            return Err(format!(
                "has version {version}, which this Pane does not read"
            ));
        }
        None => return Err("is invalid: it has no version".into()),
    }
    let mut arrangement = Arrangement::default();
    let Some(slots) = fields.get("slots") else {
        return Ok(arrangement);
    };
    let slots = slots
        .as_array()
        .ok_or("is invalid: its slots are not a list")?;
    if slots.len() > QUICK_SLOTS {
        return Err(format!(
            "is invalid: it has {} slots, and there are {QUICK_SLOTS}",
            slots.len()
        ));
    }
    for (index, slot) in slots.iter().enumerate() {
        let target = match slot {
            Value::Null => continue,
            Value::Object(entry) => target_of(entry)
                .map_err(|problem| format!("is invalid: slot {}: {problem}", index + 1))?,
            _ => {
                return Err(format!(
                    "is invalid: slot {} is neither empty nor a pin",
                    index + 1
                ));
            }
        };
        if arrangement.contains(&Some(target.clone())) {
            return Err(format!(
                "is invalid: slot {} pins what another slot pins",
                index + 1
            ));
        }
        arrangement[index] = Some(target);
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

/// The record's text for `arrangement`: every slot, in order, `null` for
/// an empty one.
fn text(arrangement: &Arrangement) -> String {
    let slots: Vec<Value> = arrangement
        .iter()
        .map(|slot| match slot {
            None => Value::Null,
            Some(PinTarget::Command(id)) => serde_json::json!({ "command": id }),
            Some(PinTarget::Indexed { command, result }) => {
                serde_json::json!({ "command": command, "result": result })
            }
        })
        .collect();
    let record = serde_json::json!({ "version": VERSION, "slots": slots });
    serde_json::to_string_pretty(&record).expect("a JSON value always serializes")
}

/// How a target resolves now.
struct Resolved {
    title: String,
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
            entry => Ok(entry),
        };
        return Resolved {
            title: listed.row.title,
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
        Some(package) => format!("{} no longer offers this command", package.title()),
        None => "Its extension is not installed".to_owned(),
    };
    Resolved {
        title,
        kind: Some(RowKind::Command),
        outcome: Err(reason),
    }
}

fn resolve_indexed(state: &State, target: &PinTarget, command: &str) -> Resolved {
    let Some((package, registration)) = registered(state, command) else {
        return Resolved {
            title: missing_title(command),
            kind: None,
            outcome: Err("Its extension is not installed".into()),
        };
    };
    let unresolved = |reason: String| Resolved {
        title: registration.title.clone(),
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
        return Resolved {
            title: found.row.title.clone(),
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

/// The slot at `index` as it stands in `state`.
fn view(launcher: &Launcher, state: &State, index: usize) -> QuickSlot {
    let Some(Some(target)) = state.quick_slots.chosen.get(index) else {
        return QuickSlot::empty();
    };
    let resolved = resolve(launcher, state, target);
    QuickSlot {
        target: Some(target.clone()),
        title: resolved.title,
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
        Some(Entry::Open(_) | Entry::Unavailable(_) | Entry::OpenApplication { .. })
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
/// result a slot can hold: pinning it — a no-op naming its slot once it is
/// pinned, whose own panel removes and moves it ([`slot_items`]). It
/// cannot run while the record cannot be read.
pub(super) fn pin_item(state: &State) -> ResultActionItem {
    item(ResultAction::Pin, state.quick_slots.unreadable.is_none())
}

/// A pinned slot's own entries: removing it, and moving it left and right
/// where there is a slot to move to.
fn slot_items(slot: usize, readable: bool) -> Vec<ResultActionItem> {
    vec![
        item(ResultAction::Unpin, readable),
        item(ResultAction::MoveSlotLeft, readable && slot > 0),
        item(
            ResultAction::MoveSlotRight,
            readable && slot + 1 < QUICK_SLOTS,
        ),
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
        ResultAction::Invoke | ResultAction::Hotkey | ResultAction::Alias
    );
    if !quick_slot_action || !matches!(state.view.screen, Screen::Root { .. }) {
        return refused;
    }
    if let Some(problem) = state.quick_slots.unreadable.clone() {
        state.view.status = Status::Error(unreadable_report(&problem));
        return refused;
    }
    match action {
        ResultAction::Pin | ResultAction::ReplaceSlot(_) => {
            let Some(pin) = pin_of_selected(state).filter(|pin| pin.key() == target) else {
                return refused;
            };
            let title = selected_title(state);
            if let Some(slot) = state.quick_slots.slot_of(&pin) {
                return (
                    SlotChange::AlreadyPinned(slot),
                    format!("{title} is already in Quick Slot {}", slot + 1),
                );
            }
            let slot = match action {
                ResultAction::ReplaceSlot(slot) if slot < QUICK_SLOTS => slot,
                ResultAction::ReplaceSlot(_) => return refused,
                _ => match state.quick_slots.chosen.iter().position(Option::is_none) {
                    Some(slot) => slot,
                    None => return (SlotChange::ChooseReplacement, String::new()),
                },
            };
            state.quick_slots.chosen[slot] = Some(pin);
            (
                SlotChange::Changed(Some(slot)),
                format!("Pinned {title} to Quick Slot {}", slot + 1),
            )
        }
        ResultAction::Unpin | ResultAction::MoveSlotLeft | ResultAction::MoveSlotRight => {
            let Some(slot) = state.quick_slots.slot_keyed(target) else {
                return refused;
            };
            let title = view(launcher, state, slot).title;
            let moved_to = match action {
                ResultAction::MoveSlotLeft if slot > 0 => slot - 1,
                ResultAction::MoveSlotRight if slot + 1 < QUICK_SLOTS => slot + 1,
                ResultAction::Unpin => {
                    state.quick_slots.chosen[slot] = None;
                    return (
                        SlotChange::Changed(None),
                        format!("Removed {title} from Quick Slot {}", slot + 1),
                    );
                }
                _ => return refused,
            };
            state.quick_slots.chosen.swap(slot, moved_to);
            (
                SlotChange::Changed(Some(moved_to)),
                format!("Moved {title} to Quick Slot {}", moved_to + 1),
            )
        }
        ResultAction::Invoke | ResultAction::Hotkey | ResultAction::Alias => refused,
    }
}

impl Launcher {
    /// This launcher keeping its quick slots in `dir`, Pane's data folder
    /// (beside the host settings' `settings.json`): the arrangement
    /// recorded there is read now. A record that cannot be read leaves
    /// every slot empty, is reported on root search's status line, and is
    /// never replaced. Without this, the slots last until the launcher
    /// stops.
    pub fn with_quick_slots(self, dir: &Path) -> Self {
        {
            let mut state = self.lock();
            state.quick_slots = Kept::open(dir);
            let problem = state.quick_slots.unreadable.clone();
            if let Some(problem) = problem
                && matches!(state.view.screen, Screen::Root { .. })
                && state.view.status == Status::Idle
            {
                state.view.status = Status::Error(unreadable_report(&problem));
            }
        }
        self
    }

    /// The five quick slots, in order, each resolved through the registry
    /// as it is now (see the module docs).
    pub fn quick_slots(&self) -> Vec<QuickSlot> {
        let state = self.lock();
        (0..QUICK_SLOTS)
            .map(|index| view(self, &state, index))
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
            .flatten()
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

    /// Invokes the quick slot at `index` (0 to 4) from root search: its
    /// target is resolved again now and, when it can run, opened as its
    /// row would be — the command opens, the application is opened — in
    /// the package's generation current now. A target that cannot run
    /// says why on the status line and nothing runs; an empty slot,
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
        let target = state.quick_slots.chosen.get(index).cloned().flatten();
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
        // Only a command or an indexed result is ever pinned.
        let entry = match entry {
            Some(entry @ (Entry::Open(_) | Entry::OpenApplication { .. })) => Some(entry),
            _ => None,
        };
        if entry.is_some() {
            // The status line is about this action from now on.
            state.sent_from = None;
            state.view.status = Status::Running;
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
                Some(Entry::Open(opening)) => launcher.open_command(epoch, opening, data).await,
                Some(Entry::OpenApplication { id, name }) => {
                    launcher.open_application(epoch, id, name).await
                }
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
    /// a disabled or missing one can always be removed — and moving it
    /// left and right where there is a slot to move to. `None` off root
    /// search, or when no slot holds it.
    pub fn quick_slot_actions(&self, target: &str) -> Option<ResultActions> {
        let state = self.lock();
        if !matches!(state.view.screen, Screen::Root { .. }) {
            return None;
        }
        let slot = state.quick_slots.slot_keyed(target)?;
        let shown = view(self, &state, slot);
        let readable = state.quick_slots.unreadable.is_none();
        // Named as the footer names the same row's primary action.
        let primary = match shown.kind {
            Some(RowKind::Application) => "Open application",
            _ => "Open command",
        };
        let mut items = vec![ResultActionItem {
            action: ResultAction::Invoke,
            label: primary.to_owned(),
            available: shown.ready(),
        }];
        items.extend(slot_items(slot, readable));
        Some(ResultActions {
            target: target.to_owned(),
            title: shown.title,
            items,
        })
    }

    /// The explicit choice a full set of slots asks for when root search's
    /// selected row, `target`, is pinned: one entry per slot, naming what
    /// it holds now, each replacing it ([`ResultAction::ReplaceSlot`]).
    /// `None` unless `target` is still the selected row, can be pinned, is
    /// not pinned yet and every slot is taken.
    pub fn pin_replacements(&self, target: &str) -> Option<ResultActions> {
        let state = self.lock();
        let pin = pin_of_selected(&state).filter(|pin| pin.key() == target)?;
        let kept = &state.quick_slots;
        if kept.unreadable.is_some()
            || kept.slot_of(&pin).is_some()
            || kept.chosen.iter().any(Option::is_none)
        {
            return None;
        }
        let items = (0..QUICK_SLOTS)
            .map(|slot| ResultActionItem {
                action: ResultAction::ReplaceSlot(slot),
                label: format!(
                    "Replace Slot {}: {}",
                    slot + 1,
                    view(self, &state, slot).title
                ),
                available: true,
            })
            .collect();
        Some(ResultActions {
            target: target.to_owned(),
            title: selected_title(&state),
            items,
        })
    }

    /// Whether `action` can run on the quick slot holding `target` now, or
    /// — for [`ResultAction::ReplaceSlot`] — on the selected row `target`
    /// waiting for a slot to replace.
    pub fn quick_slot_action_ready(&self, target: &str, action: ResultAction) -> bool {
        let listed = |actions: Option<ResultActions>| {
            actions.is_some_and(|actions| {
                actions
                    .items
                    .iter()
                    .any(|item| item.action == action && item.available)
            })
        };
        listed(self.quick_slot_actions(target)) || listed(self.pin_replacements(target))
    }

    /// Changes the quick slots as `action` asks, for `target`: root
    /// search's selected row (by its id) for [`ResultAction::Pin`] and
    /// [`ResultAction::ReplaceSlot`], or the slot holding it for
    /// [`ResultAction::Unpin`], [`ResultAction::MoveSlotLeft`] and
    /// [`ResultAction::MoveSlotRight`].
    ///
    /// Pinning fills the first empty slot; with none empty it changes
    /// nothing and asks for the slot to replace instead. Pinning what a
    /// slot already holds changes nothing and names that slot. A change
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
            SlotChange::Refused | SlotChange::ChooseReplacement => None,
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
