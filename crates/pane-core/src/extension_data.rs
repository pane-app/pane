//! Extension data: the values an installed package's commands save through
//! Pane, string values by key, of four kinds, each through its own
//! `pane:extension` interface (`wit/data.wit`).
//!
//! | Kind | File | Clear cache | Uninstall | Readable by |
//! |---|---|---|---|---|
//! | Settings | `settings.json` | kept | the user's choice | default |
//! | Content | `content.json` | kept | the user's choice | default |
//! | Cache | `cache.json` | removed | removed | default |
//! | Local credentials | `credentials.json` | kept | removed | the user only (Unix: 0600) |
//!
//! Each kind has one file next to `installed.json`, holding every package's
//! values under the package identity's key, so they belong to the source
//! identity rather than the title or the managed copy. They are kept while
//! the package is disabled, updated or Pane is not running. The kind decides
//! what a management action removes, and removing is done here by Pane,
//! never by running the package. An unreadable file is reported to the guest
//! and never overwritten.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::atomic::{Readers, write_atomically};
use crate::generation::{End, Generation};
use crate::packages::{PackageIdentity, SavedData};

/// The version of every kind's file.
const DATA_VERSION: u64 = 1;

/// A kind of data a package keeps through Pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DataKind {
    /// Values the package's commands save as its settings.
    Settings,
    /// The package's own durable records, such as notes or history.
    Content,
    /// Disposable values the package can compute or download again.
    Cache,
    /// Secrets kept on this computer, such as a sign-in token.
    LocalCredentials,
}

impl DataKind {
    const ALL: [DataKind; 4] = [
        DataKind::Settings,
        DataKind::Content,
        DataKind::Cache,
        DataKind::LocalCredentials,
    ];

    /// All of a package's values of this kind, as people call them.
    fn all(self) -> &'static str {
        match self {
            DataKind::Settings => "its settings",
            DataKind::Content => "its content",
            DataKind::Cache => "its cache",
            DataKind::LocalCredentials => "its credentials",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            DataKind::Settings => "settings.json",
            DataKind::Content => "content.json",
            DataKind::Cache => "cache.json",
            DataKind::LocalCredentials => "credentials.json",
        }
    }

    /// Who may read this kind's file: local credentials are secrets.
    fn readers(self) -> Readers {
        match self {
            DataKind::LocalCredentials => Readers::OwnerOnly,
            DataKind::Settings | DataKind::Content | DataKind::Cache => Readers::Default,
        }
    }

    /// What one value of this kind is called.
    fn value(self) -> &'static str {
        match self {
            DataKind::Settings => "the setting",
            DataKind::Content => "the content",
            DataKind::Cache => "the cache value",
            DataKind::LocalCredentials => "the credential",
        }
    }

    /// What all of a package's values of this kind are called, with a verb.
    fn kept_unchanged(self) -> &'static str {
        match self {
            DataKind::Settings => "its settings are kept unchanged",
            DataKind::Content => "its content is kept unchanged",
            DataKind::Cache => "its cache is kept unchanged",
            DataKind::LocalCredentials => "its credentials are kept unchanged",
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct DataJson {
    version: u64,
    /// Values by package identity key, then by the extension's own key.
    packages: BTreeMap<String, BTreeMap<String, String>>,
}

/// One kind's file as Pane last read or wrote it.
struct KindFile {
    kind: DataKind,
    path: PathBuf,
    /// The saved values, or why they could not be read.
    file: Result<DataJson, String>,
}

impl KindFile {
    fn open(dir: &Path, kind: DataKind) -> KindFile {
        let path = dir.join(kind.file_name());
        let file = read(&path);
        KindFile { kind, path, file }
    }

    /// Replaces the file with `updated` (see [`write_atomically`] for what a
    /// crash or a second Pane process can do to it), readable only by whom
    /// its kind allows.
    fn write(&self, updated: &DataJson) -> io::Result<()> {
        let text = serde_json::to_string_pretty(updated).map_err(io::Error::other)?;
        write_atomically(&self.path, text.as_bytes(), self.kind.readers())
    }
}

/// Every kind's file, as Pane last read or wrote it, and each package's
/// current generation.
struct DataFile {
    settings: KindFile,
    content: KindFile,
    cache: KindFile,
    local_credentials: KindFile,
    /// The current generation of each package by identity key; a disabled
    /// package's has ended, so its commands may not run or save values. The
    /// launcher keeps it in step with its packages; the runtime reads it
    /// here, on its own thread.
    generations: HashMap<String, Generation>,
}

impl DataFile {
    fn of(&mut self, kind: DataKind) -> &mut KindFile {
        match kind {
            DataKind::Settings => &mut self.settings,
            DataKind::Content => &mut self.content,
            DataKind::Cache => &mut self.cache,
            DataKind::LocalCredentials => &mut self.local_credentials,
        }
    }
}

/// Every installed package's extension data. Cloning shares the same
/// files.
#[derive(Clone)]
pub(crate) struct ExtensionData(Arc<Mutex<DataFile>>);

impl ExtensionData {
    /// Opens the data kept in `dir`. Nothing is written until a command
    /// saves a value.
    pub fn open(dir: &Path) -> ExtensionData {
        ExtensionData(Arc::new(Mutex::new(DataFile {
            settings: KindFile::open(dir, DataKind::Settings),
            content: KindFile::open(dir, DataKind::Content),
            cache: KindFile::open(dir, DataKind::Cache),
            local_credentials: KindFile::open(dir, DataKind::LocalCredentials),
            generations: HashMap::new(),
        })))
    }

    /// The data of the package with `identity`, as its commands see it,
    /// in the package's current generation: a call made with it belongs to
    /// that generation.
    pub fn owned_by(&self, identity: &PackageIdentity) -> PackageData {
        let owner = identity.key();
        let generation = self
            .lock()
            .generations
            .entry(owner.clone())
            .or_insert_with(Generation::new)
            .clone();
        PackageData {
            data: self.clone(),
            owner,
            generation,
        }
    }

    /// Records whether the package with `identity` is enabled. Disabling it
    /// ends its generation, which stops its pending calls; enabling it again
    /// starts a new one.
    pub fn set_enabled(&self, identity: &PackageIdentity, enabled: bool) {
        let mut file = self.lock();
        let current = file
            .generations
            .entry(identity.key())
            .or_insert_with(Generation::new);
        if !enabled {
            current.end(End::Disabled);
        } else if current.ended().is_some() {
            *current = Generation::new();
        }
    }

    /// Notes that the code of the package with `identity` was replaced (a
    /// reload or an update): its generation ends, which stops its pending
    /// calls, and an enabled package's new code runs in a new one. A
    /// disabled package stays disabled.
    pub fn replace_code(&self, identity: &PackageIdentity) {
        let mut file = self.lock();
        let Some(current) = file.generations.get_mut(&identity.key()) else {
            return;
        };
        if current.ended().is_none() {
            current.end(End::Replaced);
            *current = Generation::new();
        }
    }

    /// Notes that the package with `identity` is being uninstalled: its
    /// generation ends, which stops its pending calls, and its code can no
    /// longer read or save values. Installing it again starts a new one
    /// ([`ExtensionData::set_enabled`]).
    pub fn uninstall(&self, identity: &PackageIdentity) {
        self.lock()
            .generations
            .entry(identity.key())
            .or_insert_with(Generation::new)
            .end(End::Uninstalled);
    }

    /// Puts back the package with `identity` after its uninstall could not
    /// be recorded: a new generation, ended at once if it is disabled.
    pub fn reinstate(&self, identity: &PackageIdentity, enabled: bool) {
        let generation = Generation::new();
        if !enabled {
            generation.end(End::Disabled);
        }
        self.lock().generations.insert(identity.key(), generation);
    }

    /// Removes every cache value of the package with `identity`, and nothing
    /// else: its other kinds of data and other packages' caches stay. Works
    /// whether or not the package is enabled or its code loads. The cache
    /// file is read again first, so values another Pane process saved since
    /// are kept, and a file the user repaired or deleted can be cleared
    /// without restarting Pane. On failure nothing is removed, and the reason
    /// says what the user can do.
    pub fn clear_cache(&self, identity: &PackageIdentity) -> Result<(), String> {
        self.remove(DataKind::Cache, identity)
            .map_err(|failure| match failure {
                Removal::Unreadable(reason) => format!(
                    "{reason}. Nothing was deleted. That file holds only extension caches: \
                     repair or delete it, then clear the cache again."
                ),
                Removal::Unwritable(path, error) => format!(
                    "Cannot write {}: {error}. Nothing was deleted; check that Pane can write \
                     that folder, then clear the cache again.",
                    path.display()
                ),
            })
    }

    /// Removes the data of an uninstalled package with `identity`, without
    /// running it: its cache and local credentials, and its settings and
    /// content too when `saved` is [`SavedData::Delete`]. Each kind is
    /// removed on its own, as [`ExtensionData::clear_cache`] removes the
    /// cache. Returns why each kind that could not be removed was not; its
    /// values remain where they were.
    pub fn remove_uninstalled(&self, identity: &PackageIdentity, saved: SavedData) -> Vec<String> {
        let mut kinds = vec![DataKind::Cache, DataKind::LocalCredentials];
        if saved == SavedData::Delete {
            kinds.extend([DataKind::Settings, DataKind::Content]);
        }
        kinds
            .into_iter()
            .filter_map(|kind| {
                let failure = self.remove(kind, identity).err()?;
                Some(match failure {
                    Removal::Unreadable(reason) => {
                        format!("could not delete {}: {reason}", kind.all())
                    }
                    Removal::Unwritable(path, error) => format!(
                        "could not delete {}: Cannot write {}: {error}",
                        kind.all(),
                        path.display()
                    ),
                })
            })
            .collect()
    }

    /// Whether any data may be kept for the package with `identity`: a kind
    /// holding some of its values, or whose file cannot be read.
    pub fn holds_any(&self, identity: &PackageIdentity) -> bool {
        DataKind::ALL
            .into_iter()
            .any(|kind| self.count(kind, identity) != Ok(0))
    }

    /// How many values of `kind` the package with `identity` keeps, as Pane
    /// last read or wrote them, or why they cannot be read.
    pub fn count(&self, kind: DataKind, identity: &PackageIdentity) -> Result<usize, String> {
        let mut store = self.lock();
        let file = store.of(kind).file.as_ref().map_err(Clone::clone)?;
        Ok(file.packages.get(&identity.key()).map_or(0, BTreeMap::len))
    }

    /// Removes every value of `kind` of the package with `identity`, and
    /// nothing else. The file is read again first, so values another Pane
    /// process saved since are kept, and a file the user repaired or deleted
    /// is used without restarting Pane. On failure nothing is removed.
    fn remove(&self, kind: DataKind, identity: &PackageIdentity) -> Result<(), Removal> {
        let mut store = self.lock();
        let data = store.of(kind);
        data.file = read(&data.path);
        let file = data
            .file
            .as_ref()
            .map_err(|reason| Removal::Unreadable(reason.clone()))?;
        if !file.packages.contains_key(&identity.key()) {
            return Ok(());
        }
        let mut updated = file.clone();
        updated.packages.remove(&identity.key());
        data.write(&updated)
            .map_err(|error| Removal::Unwritable(data.path.clone(), error))?;
        data.file = Ok(updated);
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DataFile> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Why a kind's values could not be removed; none were.
enum Removal {
    /// The file cannot be read, for this reason.
    Unreadable(String),
    /// The file at this path cannot be written.
    Unwritable(PathBuf, io::Error),
}

/// One package's extension data, handed to the runtime with each call into
/// the package's commands.
#[derive(Clone)]
pub(crate) struct PackageData {
    data: ExtensionData,
    owner: String,
    /// The generation the call using it belongs to.
    generation: Generation,
}

impl PackageData {
    /// The generation of the package's code this data was handed out in.
    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    /// Why this data's generation ended, if it has: its commands may no
    /// longer run or save values.
    pub fn stopped(&self) -> Option<End> {
        self.generation.ended()
    }

    /// Why code of this data's generation may no longer read or save
    /// values, if its generation has ended.
    fn refusal(&self) -> Option<&'static str> {
        match self.stopped()? {
            End::Disabled => Some("the extension is disabled"),
            End::Replaced => {
                Some("this code of the extension was replaced by a reload or an update")
            }
            End::Uninstalled => Some("the extension was uninstalled"),
        }
    }

    /// The value of `kind` saved under `key`, if any, unless this data's
    /// generation has ended: stopped code reads nothing more either.
    pub fn get(&self, kind: DataKind, key: &str) -> Result<Option<String>, String> {
        if let Some(refusal) = self.refusal() {
            return Err(refusal.into());
        }
        let mut store = self.data.lock();
        let file = store.of(kind).file.as_ref().map_err(Clone::clone)?;
        Ok(file
            .packages
            .get(&self.owner)
            .and_then(|values| values.get(key))
            .cloned())
    }

    /// Saves `value` of `kind` under `key`, unless this data's generation
    /// has ended: code that was disabled or replaced saves nothing more.
    pub fn set(&self, kind: DataKind, key: &str, value: &str) -> Result<(), String> {
        let mut store = self.data.lock();
        if let Some(refusal) = self.refusal() {
            return Err(format!("{refusal}; {}", kind.kept_unchanged()));
        }
        let data = store.of(kind);
        let file = data.file.as_ref().map_err(Clone::clone)?;
        let mut updated = file.clone();
        updated
            .packages
            .entry(self.owner.clone())
            .or_default()
            .insert(key.to_owned(), value.to_owned());
        data.write(&updated)
            .map_err(|error| format!("Could not save {}: {error}", kind.value()))?;
        data.file = Ok(updated);
        Ok(())
    }
}

/// Reads one kind's file; a missing file holds no values.
fn read(path: &Path) -> Result<DataJson, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<DataJson>(&text)
            .map_err(|error| error.to_string())
            .and_then(|file| {
                if file.version == DATA_VERSION {
                    Ok(file)
                } else {
                    Err(format!(
                        "it has version {}, this Pane reads {DATA_VERSION}",
                        file.version
                    ))
                }
            }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DataJson {
            version: DATA_VERSION,
            packages: BTreeMap::new(),
        }),
        Err(error) => Err(error.to_string()),
    }
    .map_err(|reason| format!("Cannot read {}: {reason}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Code whose generation ended reads and saves nothing more, even though a newer
    /// generation of the same package may.
    #[test]
    fn replaced_or_disabled_code_reads_and_saves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let old = data.owned_by(&identity);

        data.replace_code(&identity);
        let new = data.owned_by(&identity);

        assert_eq!(
            old.set(DataKind::Settings, "key", "old"),
            Err(
                "this code of the extension was replaced by a reload or an update; its \
                 settings are kept unchanged"
                    .into()
            )
        );
        assert_eq!(
            old.get(DataKind::Settings, "key"),
            Err("this code of the extension was replaced by a reload or an update".into())
        );
        assert_eq!(new.set(DataKind::Settings, "key", "new"), Ok(()));
        data.set_enabled(&identity, false);
        assert_eq!(
            new.set(DataKind::Content, "key", "late"),
            Err("the extension is disabled; its content is kept unchanged".into())
        );
        assert_eq!(
            new.get(DataKind::Settings, "key"),
            Err("the extension is disabled".into())
        );
        data.set_enabled(&identity, true);
        assert!(new.stopped().is_some());
        assert_eq!(data.owned_by(&identity).stopped(), None);
        assert_eq!(
            data.owned_by(&identity).get(DataKind::Settings, "key"),
            Ok(Some("new".into()))
        );
    }
}
