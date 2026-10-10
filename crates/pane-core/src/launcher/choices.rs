//! Choices the user makes about installed commands that Pane records
//! itself, not as extension data: global hotkeys (`hotkeys.json`) and
//! aliases and fallbacks (`aliases.json`), beside `installed.json`.
//!
//! Each is kept by command id, `<package identity key>#<manifest command
//! id>` ([`CommandId`]), whose package's part is everything before the
//! last `#` ([`split`]). A record is read once, and
//! written atomically, one change at a time, each write holding the choices
//! as they are when it begins, so the last write holds the latest choices
//! whatever order changes finish in. A change that cannot be written goes
//! back in Pane to what was last recorded, unless its package was
//! uninstalled meanwhile: an uninstalled package's choices never come back.
//! An unreadable record is never overwritten.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use super::{Launcher, State};
use crate::atomic::{Readers, write_atomically};
use crate::packages::{CommandId, PackageIdentity};

/// The package identity key and manifest command id of the command id
/// `command` ([`CommandId::parse`]).
pub(crate) fn split(command: &str) -> (&str, &str) {
    let CommandId { package, command } = CommandId::parse(command);
    (package, command)
}

/// The title of the command `command` when it is a root provider of an
/// installed package (`"mode": "provider"`, #164), which has no alias,
/// fallback, hotkey or pin; `None` for any other command.
pub(super) fn provider_title(state: &State, command: &str) -> Option<String> {
    let (key, manifest_id) = split(command);
    let package = state
        .packages
        .iter()
        .find(|package| package.identity.key() == key)?;
    if !package.is_provider(manifest_id) {
        return None;
    }
    package
        .commands()
        .into_iter()
        .find(|registration| registration.id == command)
        .map(|registration| registration.title)
}

/// One kind of per-command choices and the file it is recorded in.
pub(super) trait Choices: Clone + Default + Send + 'static {
    /// The record's file name, beside `installed.json`.
    const FILE: &'static str;
    /// The record's version as this Pane writes it; a record of a version
    /// [`reads`] does not is not read.
    const VERSION: u64;
    /// What the choices are, for messages ("hotkeys").
    const WHAT: &'static str;

    /// Whether this Pane reads a record of `version`: the version it
    /// writes by default, with the earlier ones a choices' record kept
    /// the grammar of (hotkeys' version 1, #252) reading too. Overridden
    /// by the choices whose record moved.
    fn reads(version: u64) -> bool {
        version == Self::VERSION
    }

    /// The choices in a record's fields (besides `version`). Entries that
    /// cannot be used are left out.
    fn read(fields: &Map<String, Value>) -> Result<Self, String>;
    /// The record's fields for these choices (besides `version`).
    fn write(&self) -> Map<String, Value>;
    /// Replaces the choices of the command `command` with those in `other`.
    fn restore(&mut self, command: &str, other: &Self);
    /// Keeps only the choices of the commands for which `keep` holds;
    /// whether any went.
    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool;
    /// These choices in the launcher's state.
    fn of(state: &mut State) -> &mut Record<Self>;
}

/// The recorded choices of one kind, and where they are recorded.
#[derive(Default)]
pub(super) struct Record<C> {
    /// Where they are recorded; `None` for a launcher that installs no
    /// packages.
    file: Option<PathBuf>,
    /// The user's choices as they are in Pane.
    pub(super) chosen: C,
    /// Why the record could not be read, if it could not; it is then never
    /// overwritten.
    unreadable: Option<String>,
    /// The choices as last recorded (or read). Held while the record is
    /// written, so writes happen one at a time; see [`Launcher::save`].
    recorded: Arc<Mutex<C>>,
    /// The package identity keys whose choices were forgotten when they
    /// were uninstalled; a failed write does not bring them back.
    forgotten: HashSet<String>,
}

impl<C: Choices> Record<C> {
    /// Reads the choices recorded in `dir`.
    pub(super) fn open(dir: &Path) -> Record<C> {
        let file = dir.join(C::FILE);
        let mut record = Record {
            file: Some(file.clone()),
            ..Record::default()
        };
        let read = match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<Map<String, Value>>(&text) {
                Ok(fields) => match fields.get("version").and_then(Value::as_u64) {
                    Some(version) if C::reads(version) => C::read(&fields)
                        .map(Some)
                        .map_err(|error| format!("{} is invalid: {error}", file.display())),
                    Some(version) => Err(format!(
                        "{} has version {version}, which this Pane does not read",
                        file.display()
                    )),
                    None => Err(format!("{} is invalid: it has no version", file.display())),
                },
                Err(error) => Err(format!("{} is invalid: {error}", file.display())),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{} cannot be read: {error}", file.display())),
        };
        match read {
            Ok(chosen) => record.chosen = chosen.unwrap_or_default(),
            Err(problem) => record.unreadable = Some(problem),
        }
        *record.recorded.lock().unwrap_or_else(|p| p.into_inner()) = record.chosen.clone();
        record
    }

    /// Why the record could not be read, if it could not: it is then
    /// never overwritten.
    pub(super) fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    /// Whether the choices are kept anywhere at all; a launcher that
    /// installs no packages keeps none.
    pub(super) fn kept(&self) -> bool {
        self.file.is_some()
    }

    /// The record's text as the choices are now, and where it goes; `Err`
    /// if there is nowhere to write it.
    fn text(&self) -> Result<(PathBuf, String), String> {
        if let Some(problem) = &self.unreadable {
            return Err(format!("Pane does not replace it: {problem}"));
        }
        let file = self
            .file
            .clone()
            .ok_or_else(|| format!("this launcher does not keep {}", C::WHAT))?;
        let mut fields = Map::new();
        fields.insert("version".into(), C::VERSION.into());
        fields.extend(C::write(&self.chosen));
        let text =
            serde_json::to_string_pretty(&Value::Object(fields)).map_err(|e| e.to_string())?;
        Ok((file, text))
    }

    /// Forgets the choices of the commands of the package with `identity`,
    /// which was uninstalled: exactly its commands, not those of a package
    /// whose identity only starts the same. Whether any went.
    pub(super) fn forget(&mut self, identity: &PackageIdentity) -> bool {
        let key = identity.key();
        self.forgotten.insert(key.clone());
        self.chosen.retain(&|command| split(command).0 != key)
    }

    /// Puts the choices back to what the record last held: a change whose
    /// write failed, with no one entry to undo it by (resetting all that
    /// root search learned does), never loses what was recorded — the
    /// next write holds what the record holds, not what failed to.
    pub(super) fn revert(&mut self) {
        let recorded = self
            .recorded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        self.chosen = recorded;
    }
}

impl Launcher {
    /// Writes the choices of kind `C` as they are when the write begins,
    /// blocking: run it off the window's thread. If it cannot write them and
    /// `undo` names a command, that command's choices in Pane go back to
    /// what was last recorded, unless its package was uninstalled since.
    pub(super) fn save<C: Choices>(&self, undo: Option<&str>) -> Result<(), String> {
        let recorded = C::of(&mut self.lock()).recorded.clone();
        let mut recorded = recorded.lock().unwrap_or_else(|p| p.into_inner());
        let (text, chosen) = {
            let mut state = self.lock();
            let record = C::of(&mut state);
            (record.text(), record.chosen.clone())
        };
        let saved = text.and_then(|(file, text)| {
            write_atomically(&file, text.as_bytes(), Readers::Default).map_err(|e| e.to_string())
        });
        match (&saved, undo) {
            (Ok(()), _) => *recorded = chosen,
            (Err(_), Some(command)) => {
                let mut state = self.lock();
                let key = split(command).0;
                let installed = state.packages.iter().any(|p| p.identity.key() == key);
                let record = C::of(&mut state);
                if installed || !record.forgotten.contains(key) {
                    record.chosen.restore(command, &recorded);
                }
            }
            (Err(_), None) => {}
        }
        saved
    }

    /// The title of the installed command `command`, whatever its
    /// package's state, or its id if no installed package has it.
    pub(super) fn command_title(&self, state: &State, command: &str) -> String {
        state
            .packages
            .iter()
            .flat_map(|package| package.commands())
            .find(|registration| registration.id == command)
            .map_or_else(|| command.to_owned(), |registration| registration.title)
    }
}
