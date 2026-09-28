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
//! ([`Launcher::forget_hotkeys_of`]).
//!
//! Registering can fail when another application uses the shortcut; that is
//! explained on the hotkey's row ("Not active: ...") and tried again with
//! each change to the installed packages and at the next start.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::{Entry, Launcher, LauncherView, Row, Screen, State, Status, off_thread};
use crate::atomic::{Readers, write_atomically};
use crate::hotkeys::Shortcut;
use crate::launcher::CommandRegistration;
use crate::packages::{InstalledPackage, PackageIdentity};

const HOTKEYS_FILE: &str = "hotkeys.json";
const HOTKEYS_VERSION: u64 = 1;

#[derive(Serialize, Deserialize)]
struct HotkeysJson {
    version: u64,
    /// Each command's hotkey by command id, as `Shortcut::id` writes it.
    hotkeys: BTreeMap<String, String>,
}

/// The chosen hotkeys and which are registered with the system.
#[derive(Default)]
pub(super) struct Bindings {
    /// Where they are recorded; `None` for a launcher that installs no
    /// packages.
    file: Option<PathBuf>,
    /// The user's choices by command id.
    chosen: BTreeMap<String, Shortcut>,
    /// Why the record could not be read, if it could not; it is then never
    /// overwritten.
    unreadable: Option<String>,
    /// The hotkeys registered with the system, by command id.
    registered: HashMap<String, Shortcut>,
    /// Why a chosen hotkey that should be registered is not.
    problems: HashMap<String, String>,
    /// The choices as last recorded (or read). Held while the record is
    /// written, so writes happen one at a time; see
    /// [`Launcher::save_hotkeys`].
    recorded: Arc<Mutex<BTreeMap<String, Shortcut>>>,
}

impl Bindings {
    /// Reads the hotkeys recorded in `dir`. An entry that is not a
    /// shortcut is left out.
    pub(super) fn open(dir: &Path) -> Bindings {
        let file = dir.join(HOTKEYS_FILE);
        let mut bindings = Bindings {
            file: Some(file.clone()),
            ..Bindings::default()
        };
        match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<HotkeysJson>(&text) {
                Ok(json) if json.version == HOTKEYS_VERSION => {
                    bindings.chosen = json
                        .hotkeys
                        .into_iter()
                        .filter_map(|(command, shortcut)| {
                            Some((command, Shortcut::parse(&shortcut).ok()?))
                        })
                        .collect();
                }
                Ok(json) => {
                    bindings.unreadable = Some(format!(
                        "{} has version {}, which this Pane does not read",
                        file.display(),
                        json.version
                    ))
                }
                Err(error) => {
                    bindings.unreadable = Some(format!("{} is invalid: {error}", file.display()))
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                bindings.unreadable = Some(format!("{} cannot be read: {error}", file.display()))
            }
        }
        *bindings.recorded.lock().unwrap_or_else(|p| p.into_inner()) = bindings.chosen.clone();
        bindings
    }

    /// The command whose chosen hotkey is `shortcut`, other than `except`.
    fn opened_by(&self, shortcut: &Shortcut, except: &str) -> Option<&str> {
        self.chosen
            .iter()
            .find(|(command, chosen)| *chosen == shortcut && command.as_str() != except)
            .map(|(command, _)| command.as_str())
    }

    /// The record of the choices as they are now, to be written by
    /// [`save`]; `Err` if there is nowhere to write them.
    fn record(&self) -> Result<(PathBuf, String), String> {
        if let Some(problem) = &self.unreadable {
            return Err(format!("Pane does not replace it: {problem}"));
        }
        let file = self
            .file
            .clone()
            .ok_or_else(|| "this launcher does not keep hotkeys".to_string())?;
        let json = HotkeysJson {
            version: HOTKEYS_VERSION,
            hotkeys: self
                .chosen
                .iter()
                .map(|(command, shortcut)| (command.clone(), shortcut.id()))
                .collect(),
        };
        let text = serde_json::to_string_pretty(&json).map_err(|error| error.to_string())?;
        Ok((file, text))
    }
}

/// Writes a record made by [`Bindings::record`].
fn save(record: Result<(PathBuf, String), String>) -> Result<(), String> {
    let (file, text) = record?;
    write_atomically(&file, text.as_bytes(), Readers::Default).map_err(|error| error.to_string())
}

/// The commands offered by enabled packages, each with why it is
/// unavailable on this system, if it is.
fn offered(packages: &[InstalledPackage]) -> Vec<(CommandRegistration, Option<String>)> {
    packages
        .iter()
        .filter(|package| package.enabled)
        .flat_map(InstalledPackage::available_commands)
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
                    let shortcut = state.bindings.chosen.get(&command.id)?.clone();
                    Some((command.id, shortcut))
                })
                .collect()
        };
        let bindings = &mut state.bindings;
        let stale: Vec<String> = bindings
            .registered
            .iter()
            .filter(|(command, shortcut)| {
                !wanted
                    .iter()
                    .any(|(id, wanted)| id == *command && wanted == *shortcut)
            })
            .map(|(command, _)| command.clone())
            .collect();
        for command in stale {
            if let Some(shortcut) = bindings.registered.remove(&command) {
                self.hotkeys.unregister(&shortcut);
            }
        }
        bindings
            .problems
            .retain(|command, _| wanted.iter().any(|(id, _)| id == command));
        for (command, shortcut) in wanted {
            if bindings.registered.contains_key(&command) {
                continue;
            }
            if bindings.registered.values().any(|done| *done == shortcut) {
                // Only in a record edited by hand: one shortcut, one command.
                bindings
                    .problems
                    .insert(command, "another command has the same hotkey".into());
                continue;
            }
            match self.hotkeys.register(&shortcut) {
                Ok(()) => {
                    bindings.problems.remove(&command);
                    bindings.registered.insert(command, shortcut);
                }
                Err(error) => {
                    bindings.problems.insert(command, error.to_string());
                }
            }
        }
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
        let prefix = format!("{}#", identity.key());
        let before = state.bindings.chosen.len();
        state
            .bindings
            .chosen
            .retain(|command, _| !command.starts_with(&prefix));
        if state.bindings.chosen.len() == before {
            return None;
        }
        self.sync_hotkeys(state);
        let launcher = self.clone();
        Some(move || launcher.save_hotkeys(None))
    }

    /// Writes the choices as they are when the write begins, blocking: run
    /// it off the window's thread. Writes happen one at a time and each
    /// writes the latest choices, so whatever order changes' writes run in,
    /// the last one leaves the latest choices on disk.
    ///
    /// If it cannot write them and `undo` names a command, that command's
    /// choice in Pane goes back to what was last recorded, so Pane and the
    /// record agree (its registration follows with the next
    /// [`Launcher::sync_hotkeys`]); a later change's write, done or still to
    /// come, then writes the choices with it undone.
    fn save_hotkeys(&self, undo: Option<&str>) -> Result<(), String> {
        let recorded = self.lock().bindings.recorded.clone();
        let mut recorded = recorded.lock().unwrap_or_else(|p| p.into_inner());
        let (record, chosen) = {
            let state = self.lock();
            (state.bindings.record(), state.bindings.chosen.clone())
        };
        let saved = save(record);
        match (&saved, undo) {
            (Ok(()), _) => *recorded = chosen,
            (Err(_), Some(command)) => {
                let mut state = self.lock();
                let chosen = &mut state.bindings.chosen;
                match recorded.get(command) {
                    Some(shortcut) => chosen.insert(command.to_owned(), shortcut.clone()),
                    None => chosen.remove(command),
                };
            }
            (Err(_), None) => {}
        }
        saved
    }

    /// Opens the command whose hotkey `shortcut` is, as the system reported
    /// it pressed, leaving whatever Pane shows (an open command, form or
    /// view closes); await the returned future to show the command. A
    /// shortcut that opens nothing now, such as one released meanwhile,
    /// changes nothing and returns `None`, so the window is not raised for
    /// it.
    pub fn press_hotkey(
        &self,
        shortcut: &Shortcut,
    ) -> Option<impl Future<Output = ()> + Send + 'static> {
        let mut state = self.lock();
        let command = state
            .bindings
            .registered
            .iter()
            .find(|(_, registered)| *registered == shortcut)
            .map(|(command, _)| command.clone())?;
        let component = offered(&state.packages)
            .into_iter()
            .find(|(offered, unavailable)| offered.id == command && unavailable.is_none())
            .map(|(offered, _)| offered.component)?;
        self.show_root(&mut state, Some(component.clone()));
        state.view.status = Status::Running;
        // Its data as the package is now, so a disable or reload meanwhile
        // stops the opening.
        let data = self.data_in(&state, &component);
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        Some(async move { launcher.open_command(epoch, component, data).await })
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
                    .available_commands()
                    .into_iter()
                    .map(|(command, unavailable)| (command, unavailable, &package.identity))
            });
        commands
            .map(|(command, unavailable, identity)| {
                let bindings = &state.bindings;
                let state = match bindings.chosen.get(&command.id) {
                    Some(shortcut) => match bindings.problems.get(&command.id) {
                        Some(problem) => format!("{shortcut} · Not active: {problem}"),
                        None => format!("{shortcut} · Opens it from any application"),
                    },
                    None => "None · Choose keys that open it from any application".into(),
                };
                let subtitle = format!("{state} · {identity}");
                let unavailable = unavailable.or_else(|| everywhere.clone());
                let entry = match &unavailable {
                    Some(reason) => Entry::Unavailable(reason.clone()),
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
        let current = bindings.chosen.get(command);
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
        state.screen_epoch += 1;
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
        let title = self.command_title(state, &command);
        if let Some(refusal) = shortcut.refusal() {
            state.view.status = Status::Error(format!("{refusal}."));
            return None;
        }
        if let Some(other) = state.bindings.opened_by(&shortcut, &command) {
            let other = self.command_title(state, other);
            state.view.status = Status::Error(format!(
                "{shortcut} already opens {other}: remove it there first, or press another \
                 shortcut."
            ));
            return None;
        }
        let previous = state.bindings.chosen.get(&command);
        if previous == Some(&shortcut) && state.bindings.registered.contains_key(&command) {
            self.show_extensions_at_hotkey(state, &command);
            state.view.status = Status::Result(format!("{shortcut} already opens {title}"));
            return None;
        }
        if let Some(reason) = self.hotkeys.unavailable() {
            state.view.status = Status::Error(reason);
            return None;
        }
        // The new one first, so a refusal leaves the old one working.
        if let Err(error) = self.hotkeys.register(&shortcut) {
            state.view.status = Status::Error(format!(
                "{shortcut} cannot be used: {error}. Press another shortcut."
            ));
            return None;
        }
        let bindings = &mut state.bindings;
        if let Some(old) = bindings
            .registered
            .insert(command.clone(), shortcut.clone())
        {
            self.hotkeys.unregister(&old);
        }
        bindings.problems.remove(&command);
        bindings.chosen.insert(command.clone(), shortcut.clone());
        self.show_extensions_at_hotkey(state, &command);
        state.view.status = Status::Running;
        Some(HotkeyChange {
            command,
            done: format!("{shortcut} now opens {title}"),
            epoch: state.screen_epoch,
        })
    }

    /// Removes the hotkey of `command`, releasing it; the future records it.
    pub(super) fn remove_hotkey(&self, state: &mut State, command: &str) -> Option<HotkeyChange> {
        let title = self.command_title(state, command);
        state.bindings.chosen.remove(command)?;
        self.sync_hotkeys(state);
        self.show_extensions_at_hotkey(state, command);
        state.view.status = Status::Running;
        Some(HotkeyChange {
            command: command.to_owned(),
            done: format!("{title} has no hotkey now"),
            epoch: state.screen_epoch,
        })
    }

    /// Records a hotkey change, restoring what was last recorded if it
    /// cannot (see [`Launcher::save_hotkeys`]).
    pub(super) async fn finish_hotkey_change(&self, change: HotkeyChange) {
        let HotkeyChange {
            command,
            done,
            epoch,
        } = change;
        let launcher = self.clone();
        let saved = off_thread(move || launcher.save_hotkeys(Some(&command))).await;
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

    /// The title of the offered command `command`, or its id.
    fn command_title(&self, state: &State, command: &str) -> String {
        offered(&state.packages)
            .into_iter()
            .find(|(offered, _)| offered.id == command)
            .map_or_else(|| command.to_owned(), |(offered, _)| offered.title)
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
}

/// A hotkey change that has taken effect and is being recorded.
pub(super) struct HotkeyChange {
    command: String,
    /// The outcome once recorded.
    done: String,
    epoch: u64,
}
