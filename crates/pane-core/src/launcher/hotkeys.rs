//! Global hotkeys the user assigns to installed commands.
//!
//! The chosen hotkeys are Pane's own records, not extension data: they are
//! kept in `hotkeys.json` beside `installed.json`, by the command's id (its
//! package identity's key and the command's id in the manifest), so a
//! reinstalled or updated package keeps them. A hotkey is registered with
//! the system exactly while its command is offered: its package is enabled
//! and the command is available on this system. So disabling a package
//! releases its hotkeys and enabling it registers them again, and a command
//! an update removes releases its hotkey (the choice stays recorded, and
//! comes back with the command). Uninstalling a package releases its
//! hotkeys at once and forgets them, whether or not its saved data is kept
//! ([`Launcher::forget_hotkeys_of`]). The record is kept as the other
//! per-command choices are (see `choices`).
//!
//! The records are assigned through two entry points that take the same
//! checks and write the same record: the hotkey screen in the extension list
//! ([`Launcher::record_hotkey`], which ends that screen's asking), and the
//! Settings window's Shortcuts page ([`Launcher::set_hotkey`], which leaves
//! the launcher's screens where they are). A change either entry point
//! accepts takes effect with the system at once and is then recorded, and a
//! write that fails puts back what was last recorded, with the registration
//! following it, in both.
//!
//! Beside them stands the Open Pane hotkey
//! ([`OpenPane`](struct@OpenPane)): the application-owned binding that
//! summons the launcher from any application. It is not a command's
//! hotkey — it belongs to no package, stays registered while every
//! extension is disabled and while the runtime has failed, and is
//! recorded in the host settings (`crate::host_settings`), which the
//! window applies through this same registration path at startup and
//! after every change.
//!
//! Registering can fail when another application uses the shortcut; that is
//! explained on the hotkey's row ("Not active: ...") and tried again with
//! each change to the installed packages and at the next start.
//!
//! A command's registration is on its package's generation's undo list
//! ("hotkey", see `generation`). The system's hotkeys are registered and
//! released on the window's thread (macOS's run loop), so the generation's
//! end marks the registration ended rather than releasing it from
//! whichever thread ended the generation; the next sync, which every path
//! that ends a generation and changes what is offered runs (disabling,
//! updating, reloading, uninstalling), releases it if its command is no
//! longer offered, or keeps it for the generation then current. A paused
//! package stays enabled, so its hotkeys stay registered and explain the
//! pause when pressed, as before.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::Path;
use std::time::Instant;

use serde_json::{Map, Value};

use super::choices::{Choices, Record};
use super::{
    Entry, Launcher, LauncherView, Opening, Row, Screen, State, Status, Unavailable, off_thread,
};
use crate::generation::EndMark;
use crate::hotkeys::Shortcut;
use crate::launch::LaunchSource;
use crate::launcher::CommandRegistration;
use crate::packages::{CommandId, CommandMode, InstalledPackage, PackageIdentity};

/// Each command's hotkey by command id, recorded in `hotkeys.json` as
/// `{ "version": 1, "hotkeys": { "<command id>": "ctrl+alt+g" } }`.
pub(super) type HotkeyChoices = BTreeMap<String, Shortcut>;

impl Choices for HotkeyChoices {
    const FILE: &'static str = "hotkeys.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "hotkeys";

    /// An entry that is not a shortcut is left out.
    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let hotkeys = fields
            .get("hotkeys")
            .and_then(Value::as_object)
            .ok_or("it has no `hotkeys`")?;
        Ok(hotkeys
            .iter()
            .filter_map(|(command, shortcut)| {
                Some((command.clone(), Shortcut::parse(shortcut.as_str()?).ok()?))
            })
            .collect())
    }

    fn write(&self) -> Map<String, Value> {
        let hotkeys = self
            .iter()
            .map(|(command, shortcut)| (command.clone(), Value::String(shortcut.id())))
            .collect();
        Map::from_iter([("hotkeys".to_string(), Value::Object(hotkeys))])
    }

    fn restore(&mut self, command: &str, other: &Self) {
        match other.get(command) {
            Some(shortcut) => self.insert(command.to_owned(), shortcut.clone()),
            None => self.remove(command),
        };
    }

    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = self.len();
        BTreeMap::retain(self, |command, _| keep(command));
        self.len() != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.bindings.record
    }
}

/// The chosen hotkeys and which are registered with the system.
#[derive(Default)]
pub(super) struct Bindings {
    /// The user's choices by command id, and their record.
    record: Record<HotkeyChoices>,
    /// The hotkeys registered with the system, by command id.
    registered: HashMap<String, Registered>,
    /// Why a chosen hotkey that should be registered is not.
    problems: HashMap<String, String>,
}

impl Bindings {
    /// Reads the hotkeys recorded in `dir`.
    pub(super) fn open(dir: &Path) -> Bindings {
        Bindings {
            record: Record::open(dir),
            ..Bindings::default()
        }
    }

    fn chosen(&self) -> &HotkeyChoices {
        &self.record.chosen
    }

    /// The command whose chosen hotkey is `shortcut`, other than `except`.
    fn opened_by(&self, shortcut: &Shortcut, except: &str) -> Option<&str> {
        self.chosen()
            .iter()
            .find(|(command, chosen)| *chosen == shortcut && command.as_str() != except)
            .map(|(command, _)| command.as_str())
    }

    /// The hotkey registered with the system for `command`: the one that
    /// actually opens it now, which root search shows beside its row.
    pub(super) fn registered_of(&self, command: &str) -> Option<Shortcut> {
        self.registered
            .get(command)
            .map(|registered| registered.shortcut.clone())
    }

    /// The hotkey recorded for `command`, for the Shortcuts catalog.
    pub(super) fn hotkey_of(&self, command: &str) -> Option<Shortcut> {
        self.chosen().get(command).cloned()
    }

    /// Why the hotkey recorded for `command` could not be registered with
    /// the system, if it could not; for the Shortcuts catalog.
    pub(super) fn problem_of(&self, command: &str) -> Option<String> {
        self.problems.get(command).cloned()
    }

    /// Every command id with a hotkey recorded; for the Shortcuts
    /// catalog's rows of choices whose commands are gone.
    pub(super) fn recorded(&self) -> Vec<&str> {
        self.chosen().keys().map(String::as_str).collect()
    }
}

/// A command's hotkey registered with the system, and its place on its
/// package's generation's undo list (see the module docs).
struct Registered {
    shortcut: Shortcut,
    generation: EndMark,
}

/// The Open Pane hotkey: the application-owned binding that summons the
/// launcher from any application, independent of every extension. The
/// host settings hold the choice (see `crate::host_settings`); the window
/// hands each recorded choice to the launcher, which applies it through
/// the same registration path the command hotkeys take.
#[derive(Default)]
pub(super) struct OpenPane {
    /// What is registered with the system, if anything.
    registered: Option<Shortcut>,
    /// Why the recorded choice is not the one registered, if it is not —
    /// a registration the system refused, a choice it would refuse, or a
    /// collision with a command's hotkey.
    problem: Option<String>,
}

impl OpenPane {
    /// Whether the command hotkey `shortcut` takes these keys from the
    /// working Open Pane binding: a registration must not, and a command
    /// whose recorded hotkey names them is explained instead.
    fn taken_by(&self, shortcut: &Shortcut) -> bool {
        self.registered
            .as_ref()
            .is_some_and(|open| open == shortcut)
    }
}

/// The commands offered by enabled packages, each with why it is
/// unavailable on this system, if it is. A root provider is never offered:
/// no hotkey opens it, and one recorded before it became one stays
/// unregistered.
fn offered(packages: &[InstalledPackage]) -> Vec<(CommandRegistration, Option<String>)> {
    packages
        .iter()
        .filter(|package| package.enabled)
        .flat_map(InstalledPackage::launchable_commands)
        .collect()
}

impl Launcher {
    /// Registers with the system exactly the chosen hotkeys whose commands
    /// are offered and available here, releasing the others; each that the
    /// system refuses is noted with why.
    pub(super) fn sync_hotkeys(&self, state: &mut State) {
        let wanted: Vec<(String, Shortcut)> = if self.hotkeys.unavailable().is_some() {
            Vec::new()
        } else {
            offered(&state.packages)
                .into_iter()
                .filter(|(_, unavailable)| unavailable.is_none())
                .filter_map(|(command, _)| {
                    let shortcut = state.bindings.chosen().get(&command.id)?.clone();
                    Some((command.id, shortcut))
                })
                .collect()
        };
        // Each wanted command's mark on its package's current generation.
        let mut marks: HashMap<String, EndMark> = wanted
            .iter()
            .map(|(command, _)| (command.clone(), self.hotkey_mark(state, command)))
            .collect();
        let bindings = &mut state.bindings;
        let open_pane = state.open_pane.registered.clone();
        let stale: Vec<String> = bindings
            .registered
            .iter()
            .filter(|(command, registered)| {
                !wanted
                    .iter()
                    .any(|(id, wanted)| id == *command && *wanted == registered.shortcut)
            })
            .map(|(command, _)| command.clone())
            .collect();
        for command in stale {
            if let Some(registered) = bindings.registered.remove(&command) {
                self.hotkeys.unregister(&registered.shortcut);
            }
        }
        bindings
            .problems
            .retain(|command, _| wanted.iter().any(|(id, _)| id == command));
        for (command, shortcut) in wanted {
            if let Some(registered) = bindings.registered.get_mut(&command) {
                // Still offered: its generation ended (a reload, an
                // update), and the registration is the current one's now.
                if registered.generation.ended()
                    && let Some(mark) = marks.remove(&command)
                {
                    registered.generation = mark;
                }
                continue;
            }
            if bindings
                .registered
                .values()
                .any(|done| done.shortcut == shortcut)
            {
                // Only in a record edited by hand: one shortcut, one command.
                bindings
                    .problems
                    .insert(command, "another command has the same hotkey".into());
                continue;
            }
            if open_pane.as_ref().is_some_and(|open| *open == shortcut) {
                // The application's own binding keeps working: a command
                // whose recorded hotkey names the same keys is explained,
                // never registered over it.
                bindings
                    .problems
                    .insert(command, "the Open Pane hotkey uses it".into());
                continue;
            }
            match self.hotkeys.register(&shortcut) {
                Ok(()) => {
                    bindings.problems.remove(&command);
                    let generation = marks
                        .remove(&command)
                        .unwrap_or_else(|| EndMark::on(None, "hotkey", || {}));
                    bindings.registered.insert(
                        command,
                        Registered {
                            shortcut,
                            generation,
                        },
                    );
                }
                Err(error) => {
                    bindings.problems.insert(command, error.to_string());
                }
            }
        }
    }

    /// The mark of the hotkey of the command `command` on its package's
    /// current generation's undo list.
    fn hotkey_mark(&self, state: &State, command: &str) -> EndMark {
        let package = CommandId::parse(command).package;
        let data = state
            .packages
            .iter()
            .find(|installed| installed.identity.key() == package)
            .and_then(|installed| {
                Some(
                    self.installation
                        .as_ref()?
                        .data
                        .owned_by(&installed.identity),
                )
            });
        EndMark::on(data.as_ref().map(|data| data.generation()), "hotkey", || {})
    }

    /// Forgets the hotkeys of the uninstalled package with `identity`
    /// (their registrations went when it left the installed packages).
    /// Returns what writes the record without them, to run off the window's
    /// thread; nothing to write if it had none.
    pub(super) fn forget_hotkeys_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.bindings.record.forget(identity) {
            return None;
        }
        self.sync_hotkeys(state);
        let launcher = self.clone();
        Some(move || launcher.save::<HotkeyChoices>(None))
    }

    /// What the hotkey `shortcut` launches now, if anything: the offered,
    /// available command it is registered for.
    pub(super) fn hotkey_opening(&self, state: &State, shortcut: &Shortcut) -> Option<Opening> {
        let command = state
            .bindings
            .registered
            .iter()
            .find(|(_, registered)| registered.shortcut == *shortcut)
            .map(|(command, _)| command.as_str())?;
        state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .find_map(|package| {
                let (offered, _) =
                    package
                        .launchable_commands()
                        .into_iter()
                        .find(|(offered, unavailable)| {
                            offered.id == command && unavailable.is_none()
                        })?;
                let no_view = package.mode_of(offered.manifest_id()) == CommandMode::NoView;
                Some(Opening::of(&offered, no_view, LaunchSource::Hotkey))
            })
    }

    /// Launches the command whose hotkey `shortcut` is, as the system
    /// reported it pressed; await the returned future to show the command
    /// or its answer. A view command opens, leaving whatever Pane shows (an
    /// open command, form or view closes). A no-view command runs without
    /// changing what Pane shows, and without Pane's window
    /// ([`Launcher::hotkey_shows_window`]). A shortcut that launches
    /// nothing now, such as one released meanwhile, changes nothing and
    /// returns `None`, so the window is not raised for it.
    pub fn press_hotkey(
        &self,
        shortcut: &Shortcut,
    ) -> Option<impl Future<Output = ()> + Send + 'static> {
        let mut state = self.lock();
        let opening = self.hotkey_opening(&state, shortcut)?;
        if opening.no_view {
            Launcher::begin_run(&mut state);
        } else {
            self.show_root(&mut state, Some(opening.component.clone()));
            state.view.status = Status::Running { since: Instant::now() };
        }
        // Its data as the package is now, so a disable or reload meanwhile
        // stops the opening.
        let data = self.data_in(&state, &opening.component);
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        Some(async move { launcher.launch_opening(epoch, opening, data).await })
    }

    /// The hotkey rows of the extension list: one per command of each
    /// enabled package, saying its hotkey and, since copies of a package can
    /// share titles, its package's source.
    pub(super) fn hotkey_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        let everywhere = self.hotkeys.unavailable();
        let commands = state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .flat_map(|package| {
                package
                    .launchable_commands()
                    .into_iter()
                    .map(|(command, unavailable)| (command, unavailable, &package.identity))
            });
        commands
            .map(|(command, unavailable, identity)| {
                let bindings = &state.bindings;
                let state = match bindings.chosen().get(&command.id) {
                    Some(shortcut) => match bindings.problems.get(&command.id) {
                        Some(problem) => format!("{shortcut} · Not active: {problem}"),
                        None => format!("{shortcut} · Opens it from any application"),
                    },
                    None => "None · Choose keys that open it from any application".into(),
                };
                let subtitle = format!("{state} · {identity}");
                let unavailable = unavailable
                    .or_else(|| everywhere.clone())
                    .map(Unavailable::OnThisSystem);
                let entry = match &unavailable {
                    Some(reason) => Entry::Unavailable(reason.reason().to_owned()),
                    None => Entry::AskHotkey(command.id.clone()),
                };
                let row = Row {
                    id: format!("hotkey:{}", command.id),
                    title: format!("Hotkey for {}", command.title),
                    subtitle: Some(subtitle),
                    unavailable,
                };
                (row, entry)
            })
            .collect()
    }

    /// Shows the hotkey screen of the command `command`: it asks for the
    /// keys, and offers to remove its hotkey if it has one.
    pub(super) fn show_hotkey(&self, state: &mut State, command: &str) {
        state.actions_return = None;
        let Some((registration, _)) = offered(&state.packages)
            .into_iter()
            .find(|(offered, _)| offered.id == command)
        else {
            return;
        };
        let title = registration.title;
        let example = Shortcut::parse("ctrl+alt+g").expect("a valid shortcut");
        let mut details = vec![format!(
            "Press the keys that should open {title} from any application, such as {example}."
        )];
        let bindings = &state.bindings;
        let current = bindings.chosen().get(command);
        details.push(match current {
            Some(shortcut) => format!("Its hotkey is {shortcut}."),
            None => "It has no hotkey yet.".into(),
        });
        if let Some(problem) = bindings.problems.get(command) {
            details.push(format!("Not active: {problem}."));
        }
        details.push("Esc goes back without changing it.".into());
        let (rows, entries) = match current {
            Some(_) => (
                vec![Row {
                    id: "remove-hotkey".into(),
                    title: "Remove hotkey".into(),
                    subtitle: Some(format!("{title} will open only from Pane")),
                    unavailable: None,
                }],
                vec![Entry::RemoveHotkey(command.to_owned())],
            ),
            None => (Vec::new(), Vec::new()),
        };
        state.next_screen();
        state.entries = entries;
        let screen = Screen::Hotkey {
            command: command.to_owned(),
            details,
        };
        state.view = LauncherView::new(screen, format!("Hotkey for {title}")).with_rows(rows);
    }

    /// Assigns `shortcut`, as the user pressed it on the hotkey screen, to
    /// that screen's command, registering it with the system at once and
    /// releasing the hotkey it replaces. Await the returned future to record
    /// it; if it cannot be recorded, the earlier hotkey is restored.
    ///
    /// A shortcut that needs Ctrl, Alt or Super, is reserved, opens another
    /// command or is used by another application is explained, and the
    /// screen stays for another try. Ignored on other screens.
    pub fn record_hotkey(&self, shortcut: Shortcut) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let change = self.assign_hotkey(&mut state, shortcut);
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(change) = change {
                launcher.finish_hotkey_change(change).await;
            }
        }
    }

    fn assign_hotkey(&self, state: &mut State, shortcut: Shortcut) -> Option<HotkeyChange> {
        let Screen::Hotkey { command, .. } = &state.view.screen else {
            return None;
        };
        let command = command.clone();
        let set = match self.set_hotkey_of(state, &command, Some(shortcut)) {
            Err(reason) => {
                state.view.status = Status::Error(reason);
                return None;
            }
            Ok(set) => set,
        };
        self.leave_hotkey(state, &command);
        if set.write {
            state.view.status = Status::Running { since: Instant::now() };
            Some(HotkeyChange {
                command: set.command,
                done: set.done,
                epoch: state.screen_epoch,
            })
        } else {
            // The keys are already the command's working binding: nothing
            // to register, release or record.
            state.view.status = Status::Result(set.done);
            None
        }
    }

    /// Sets `shortcut` — or `None`, clearing — as the hotkey of the command
    /// `command` without opening the launcher's hotkey screen: the Settings
    /// window's Shortcuts page records it inline, as [`Launcher::set_alias`]
    /// edits an alias. The hotkey screen's rules apply: `Err` carries the
    /// reason when the command is not offered and available here, the
    /// shortcut needs no modifier, is reserved, opens another command or
    /// Pane itself, this system has no global hotkeys, or the system refuses
    /// the registration — and nothing changes: the binding the command has
    /// keeps working and nothing is kept.
    ///
    /// The change takes effect at once — the hotkey is registered with the
    /// system before the one it replaces is released, and the launcher's
    /// rows are refreshed, so the extension list and the next catalog agree
    /// — and the returned future records it; a record that cannot be
    /// written goes back to what was last recorded, with the registration
    /// following it (see [`Launcher::save`]), and its outcome says which it
    /// was. The launcher's screens are left where they are, unlike the hotkey
    /// screen's flow: the page that asked shows the outcome itself.
    pub fn set_hotkey(
        &self,
        command: &str,
        shortcut: Option<Shortcut>,
    ) -> Result<impl Future<Output = HotkeyOutcome> + Send + 'static, String> {
        let mut state = self.lock();
        let set = self.set_hotkey_of(&mut state, command, shortcut)?;
        self.refresh(&mut state);
        drop(state);
        let launcher = self.clone();
        Ok(async move { launcher.finish_set_hotkey(set).await })
    }

    /// Applies `shortcut` — `None` clearing the hotkey — as the command
    /// `command`'s hotkey in Pane's records and with the system, taking the
    /// checks every recording takes. The new hotkey is registered before the
    /// one it replaces is released, so a refusal leaves the old one working;
    /// the choice is kept in Pane for the future that records it. `Err`
    /// names the reason, as the message to show, and changes nothing.
    fn set_hotkey_of(
        &self,
        state: &mut State,
        command: &str,
        shortcut: Option<Shortcut>,
    ) -> Result<HotkeySet, String> {
        let title = self.command_title(state, command);
        let Some(shortcut) = shortcut else {
            // Clearing: the registration goes and the choice is forgotten;
            // a command whose package is disabled keeps the release and the
            // forgetting just the same.
            if let Some(old) = state.bindings.registered.remove(command) {
                self.hotkeys.unregister(&old.shortcut);
            }
            state.bindings.problems.remove(command);
            let recorded = state.bindings.record.chosen.remove(command).is_some();
            return Ok(HotkeySet {
                command: command.to_owned(),
                done: format!("{title} has no hotkey now"),
                write: recorded,
            });
        };
        // A root provider is never launched, so no hotkey opens it (#164).
        if super::choices::provider_title(state, command).is_some() {
            return Err(format!(
                "A hotkey cannot be recorded for {title}: it only answers root search"
            ));
        }
        // Recording is offered for a command that is offered and available,
        // as the hotkey screen's rows and the Shortcuts catalog's decide. A
        // catalog the page has not redrawn can still ask after the packages
        // changed, so the rule is here too.
        match offered(&state.packages)
            .into_iter()
            .find(|(offered, _)| offered.id == command)
        {
            Some((_, None)) => {}
            Some((_, Some(why))) => {
                return Err(format!("A hotkey cannot be recorded for {title}: {why}"));
            }
            None => {
                return Err(format!(
                    "A hotkey cannot be recorded for {title}: its extension is not enabled here"
                ));
            }
        }
        if let Some(refusal) = shortcut.refusal() {
            return Err(format!("{refusal}."));
        }
        if let Some(other) = state.bindings.opened_by(&shortcut, command) {
            let other = self.command_title(state, other);
            return Err(format!(
                "{shortcut} already opens {other}: remove it there first, or press another \
                 shortcut."
            ));
        }
        if state.open_pane.taken_by(&shortcut) {
            return Err(format!(
                "{shortcut} opens Pane itself: choose another shortcut for {title}, or change \
                 Pane's hotkey in Settings."
            ));
        }
        let previous = state.bindings.chosen().get(command);
        if previous == Some(&shortcut) && state.bindings.registered.contains_key(command) {
            return Ok(HotkeySet {
                command: command.to_owned(),
                done: format!("{shortcut} already opens {title}"),
                write: false,
            });
        }
        if let Some(reason) = self.hotkeys.unavailable() {
            return Err(reason);
        }
        // The new one first, so a refusal leaves the old one working.
        if let Err(error) = self.hotkeys.register(&shortcut) {
            return Err(format!(
                "{shortcut} cannot be used: {error}. Press another shortcut."
            ));
        }
        let generation = self.hotkey_mark(state, command);
        let bindings = &mut state.bindings;
        if let Some(old) = bindings.registered.insert(
            command.to_owned(),
            Registered {
                shortcut: shortcut.clone(),
                generation,
            },
        ) {
            self.hotkeys.unregister(&old.shortcut);
        }
        bindings.problems.remove(command);
        bindings
            .record
            .chosen
            .insert(command.to_owned(), shortcut.clone());
        Ok(HotkeySet {
            command: command.to_owned(),
            done: format!("{shortcut} now opens {title}"),
            write: true,
        })
    }

    /// Records a hotkey the Settings page set (see [`Launcher::set_hotkey`]),
    /// restoring what was last recorded — and the registration that follows
    /// it — if it cannot be, as [`Launcher::finish_hotkey_change`] does for
    /// the hotkey screen's changes.
    async fn finish_set_hotkey(&self, set: HotkeySet) -> HotkeyOutcome {
        let HotkeySet {
            command,
            done,
            write,
        } = set;
        if !write {
            // The keys already being the working binding: nothing was
            // changed, so nothing is written or rolled back.
            return HotkeyOutcome::Saved(done);
        }
        let launcher = self.clone();
        let saved = off_thread(move || launcher.save::<HotkeyChoices>(Some(&command))).await;
        match saved {
            Ok(()) => HotkeyOutcome::Saved(done),
            Err(problem) => {
                // What was last recorded is back in Pane (see
                // `Launcher::save`); the registration follows it again, on
                // the window's thread, as registering must (macOS).
                let mut state = self.lock();
                self.sync_hotkeys(&mut state);
                self.refresh(&mut state);
                HotkeyOutcome::NotKept(problem)
            }
        }
    }

    /// Removes the hotkey of `command`, releasing it; the future records it.
    pub(super) fn remove_hotkey(&self, state: &mut State, command: &str) -> Option<HotkeyChange> {
        state.bindings.chosen().get(command)?;
        let set = self
            .set_hotkey_of(state, command, None)
            .expect("clearing a hotkey takes no check");
        self.leave_hotkey(state, command);
        state.view.status = Status::Running { since: Instant::now() };
        Some(HotkeyChange {
            command: set.command,
            done: set.done,
            epoch: state.screen_epoch,
        })
    }

    /// Records a hotkey change, restoring what was last recorded if it
    /// cannot (see [`Launcher::save`]).
    pub(super) async fn finish_hotkey_change(&self, change: HotkeyChange) {
        let HotkeyChange {
            command,
            done,
            epoch,
        } = change;
        let launcher = self.clone();
        let saved = off_thread(move || launcher.save::<HotkeyChoices>(Some(&command))).await;
        let mut state = self.lock();
        let status = match saved {
            Ok(()) => Status::Result(done),
            Err(problem) => {
                // On the window's thread, as registering must be (macOS).
                self.sync_hotkeys(&mut state);
                self.refresh(&mut state);
                Status::Error(format!("Could not keep the hotkey: {problem}"))
            }
        };
        if state.screen_epoch == epoch {
            state.view.status = status;
        }
    }

    /// Leaves the hotkey screen of `command`: for the search the Actions
    /// panel opened it from, or else the extension list at its hotkey row.
    pub(super) fn leave_hotkey(&self, state: &mut State, command: &str) {
        if !self.return_from_actions_flow(state) {
            self.show_extensions_at_hotkey(state, command);
        }
    }

    /// Shows the extension list with the hotkey row of `command` selected.
    pub(super) fn show_extensions_at_hotkey(&self, state: &mut State, command: &str) {
        self.show_extensions(state);
        let row = state
            .entries
            .iter()
            .position(|entry| matches!(entry, Entry::AskHotkey(asked) if asked == command));
        if row.is_some() {
            state.view.selected = row;
        }
    }

    /// Why the chosen Open Pane hotkey is not the one registered with the
    /// system, if it is not — a registration the system refused, a choice
    /// it would refuse, or a collision with a command's recorded hotkey.
    /// The General page shows it.
    pub fn open_pane_problem(&self) -> Option<String> {
        self.lock().open_pane.problem.clone()
    }

    /// Whether `shortcut` is the registered Open Pane binding, as the
    /// system reported it pressed: the window then summons, focuses or
    /// hides the launcher instead of opening a command.
    pub fn opens_pane(&self, shortcut: &Shortcut) -> bool {
        self.lock().open_pane.registered.as_ref() == Some(shortcut)
    }

    /// Makes `shortcut` the Open Pane hotkey, as the user recorded it on
    /// the General page: it is checked against the combinations the
    /// system keeps for itself and against the command hotkeys, then
    /// registered before the binding it replaces is released, so a
    /// refusal leaves the previous binding working. `Err` names the
    /// reason and changes nothing — the previous binding keeps working
    /// and no choice is kept; the page reports the reason.
    pub fn set_open_pane(&self, shortcut: Shortcut) -> Result<(), String> {
        let mut state = self.lock();
        self.assign_open_pane(&mut state, shortcut)
    }

    /// Makes the registered Open Pane binding match `chosen`, the host
    /// settings' recorded choice: the application at startup and the
    /// rollback after a choice that could not be saved. Unlike a change,
    /// a choice that cannot be applied is *kept* (the record names it)
    /// with the reason recorded as its problem, for the General page —
    /// the last working binding still works.
    pub fn sync_open_pane(&self, chosen: Shortcut) -> Result<(), String> {
        let mut state = self.lock();
        let applied = self.assign_open_pane(&mut state, chosen);
        if let Err(reason) = &applied {
            state.open_pane.problem = Some(reason.clone());
        }
        applied
    }

    /// Releases every hotkey registered with the system — the commands'
    /// and the Open Pane binding — leaving nothing of Pane's registered.
    /// This is the quit path: the tray's Quit item calls it before it
    /// ends Pane, and the window-close quit path reaches it through the
    /// quit hooks, so the registrations are removed by Pane itself
    /// rather than left for the system to reclaim with the process. The
    /// adapters also release their registrations when they are dropped,
    /// but a quit that ends the process may never run a destructor.
    pub fn release_hotkeys(&self) {
        let mut state = self.lock();
        for (_, registered) in state.bindings.registered.drain() {
            self.hotkeys.unregister(&registered.shortcut);
        }
        if let Some(open) = state.open_pane.registered.take() {
            self.hotkeys.unregister(&open);
        }
        // Nothing is registered, so nothing is explained as not.
        state.open_pane.problem = None;
    }

    /// Applies `shortcut` to the Open Pane binding's state and the system:
    /// validate, register the new one, then release the one it replaces.
    /// `Err` leaves the state exactly as it was.
    fn assign_open_pane(&self, state: &mut State, shortcut: Shortcut) -> Result<(), String> {
        if state.open_pane.registered.as_ref() == Some(&shortcut) {
            // Already the working binding: nothing to register or release.
            state.open_pane.problem = None;
            return Ok(());
        }
        if let Some(reason) = self.hotkeys.unavailable() {
            // Global hotkeys cannot be used here at all (Wayland, or a
            // launcher with no adapter): the reason carries the guidance.
            return Err(reason);
        }
        if let Some(refusal) = shortcut.refusal() {
            return Err(refusal);
        }
        if let Some(other) = state.bindings.opened_by(&shortcut, "") {
            let other = self.command_title(state, other);
            return Err(format!(
                "{shortcut} already opens {other}: remove it there first, or press another \
                 shortcut."
            ));
        }
        // The new one first, so a refusal leaves the old one working.
        if let Err(error) = self.hotkeys.register(&shortcut) {
            return Err(format!("{shortcut} cannot be used: {error}."));
        }
        let open_pane = &mut state.open_pane;
        if let Some(old) = open_pane.registered.replace(shortcut) {
            self.hotkeys.unregister(&old);
        }
        open_pane.problem = None;
        Ok(())
    }
}

/// A hotkey change that has taken effect and is being recorded.
pub(super) struct HotkeyChange {
    command: String,
    /// The outcome once recorded.
    done: String,
    epoch: u64,
}

/// A hotkey change that took effect in Pane and with the system, for the
/// caller to record and report.
pub(super) struct HotkeySet {
    command: String,
    /// What the change came to, as the message to show once recorded.
    done: String,
    /// Whether the record is to be written: the keys already being the
    /// command's working binding changed nothing.
    write: bool,
}

/// What a hotkey set directly, through [`Launcher::set_hotkey`], came to.
/// The hotkey has taken effect in Pane either way; this says whether the
/// record on disk kept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyOutcome {
    /// The hotkey was recorded. The message says what its keys now open
    /// ("Ctrl+Alt+G now opens Say hello"), or that the command has no
    /// hotkey now.
    Saved(String),
    /// The hotkey took effect but could not be recorded: what was last
    /// recorded is back in Pane, and the registration follows it. The
    /// message says why it could not be kept.
    NotKept(String),
}
