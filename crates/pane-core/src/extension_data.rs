//! Extension data: the values an installed package's commands save through
//! Pane, string values by key, of four kinds, each through its own
//! `pane:extension` interface (`wit/data.wit`), and the package's clipboard
//! history, which Pane keeps for it (`wit/clipboard.wit`, see `clipboard`).
//!
//! | Kind | File | Clear cache | Uninstall | Readable by |
//! |---|---|---|---|---|
//! | Settings | `settings.json` | kept | the user's choice | default |
//! | Content | `content.json` | kept | the user's choice | default |
//! | Cache | `cache.json`, `web-images/` | removed | removed | default |
//! | Local credentials | `credentials.json` | kept | removed | the user only (Unix: 0600); Windows: each value encrypted (DPAPI) |
//! | Clipboard history | `clipboard-history.json`, `clipboard-images/` | kept | the user's choice | the user only (Unix: 0600); Windows: each item's text encrypted (DPAPI) |
//!
//! On Windows the values of local credentials are written protected
//! (#130, see `protection`): the file is version 2, in which each value
//! records how it is protected, while package identities and keys stay
//! readable, so clearing the cache, uninstalling, deleting retained data and
//! the counts work without decrypting anything. A version-1 file an earlier
//! Pane wrote is converted when Pane starts, in one atomic write; if that
//! write fails, the file is read as it is and converted at the next start.
//! A value that cannot be decrypted on this computer is explained to the
//! extension that reads it and counted as unreadable, never dropped or
//! replaced but by the extension's own `set`. The other kinds' files, and
//! local credentials on macOS and Linux, stay at version 1.
//!
//! Each kind has one file next to `installed.json`, holding every package's
//! values under the package identity's key, so they belong to the source
//! identity rather than the title or the managed copy. The web images a
//! package's icons name (#142) are cache too, downloaded by Pane into a
//! folder per package under `web-images/`. They are kept while
//! the package is disabled, updated or Pane is not running. The kind decides
//! what a management action removes, and removing is done here by Pane,
//! never by running the package. Deleting retained data (an uninstalled
//! package's kept data) removes every kind. An unreadable file is reported
//! to the guest and never overwritten.
//!
//! Files are written by a thread of their own, one write after another in
//! the order the changes were made, so the lock on the values is never held
//! while a file is written and synced (#18): the runtime thread only waits
//! for its write to finish, and a runtime thread Pane gave up on never holds
//! the lock the next one needs. A change is made in memory first; if its
//! write fails and nothing changed since, it is taken back.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak, mpsc};

use serde::{Deserialize, Serialize};

use crate::atomic::{Readers, write_atomically};
use crate::clipboard::history::{HistoryStore, PackageHistory};
use crate::generation::{End, Fence, Generation, Undo};
use crate::packages::{PackageIdentity, SavedData};
use crate::protection::{self, Stored, Value};

/// The version of every kind's file whose values are all written as they
/// are: each value a string.
const DATA_VERSION: u64 = 1;
/// The version of a local credentials file whose values are protected
/// (#130): each value records how it is (see [`Stored`]).
const PROTECTED_VERSION: u64 = 2;

/// The kinds that hold preference values: a password's are local
/// credentials, every other's settings.
const PREFERENCE_KINDS: [DataKind; 2] = [DataKind::Settings, DataKind::LocalCredentials];
/// The folder beside the files holding each package's cached web images.
const WEB_IMAGES_DIR: &str = "web-images";

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
    /// What the user copied while the package kept clipboard history (and,
    /// for Pane's own Clipboard History, the images' PNGs beside it), and
    /// whether it keeps it; written by Pane, never by the package's
    /// code directly (see `clipboard`).
    ClipboardHistory,
}

impl DataKind {
    pub const ALL: [DataKind; 5] = [
        DataKind::Settings,
        DataKind::Content,
        DataKind::Cache,
        DataKind::LocalCredentials,
        DataKind::ClipboardHistory,
    ];

    /// The kinds that are a package's saved data: what the user chooses to
    /// keep or delete when uninstalling it.
    pub const SAVED: [DataKind; 3] = [
        DataKind::Settings,
        DataKind::Content,
        DataKind::ClipboardHistory,
    ];

    /// All of a package's values of this kind, as people call them.
    fn all(self) -> &'static str {
        match self {
            DataKind::Settings => "its settings",
            DataKind::Content => "its content",
            DataKind::Cache => "its cache",
            DataKind::LocalCredentials => "its credentials",
            DataKind::ClipboardHistory => "its clipboard history",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            DataKind::Settings => "settings.json",
            DataKind::Content => "content.json",
            DataKind::Cache => "cache.json",
            DataKind::LocalCredentials => "credentials.json",
            DataKind::ClipboardHistory => "clipboard-history.json",
        }
    }

    /// Who may read this kind's file: local credentials are secrets, and
    /// copied text may be.
    fn readers(self) -> Readers {
        match self {
            DataKind::LocalCredentials | DataKind::ClipboardHistory => Readers::OwnerOnly,
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
            DataKind::ClipboardHistory => "the clipboard history",
        }
    }

    /// What one value, and several values, of this kind are called when
    /// counted.
    fn counted(self) -> (&'static str, &'static str) {
        match self {
            DataKind::Settings => ("setting", "settings"),
            DataKind::Content => ("content record", "content records"),
            DataKind::Cache => ("cache value", "cache values"),
            DataKind::LocalCredentials => ("credential", "credentials"),
            DataKind::ClipboardHistory => ("clipboard history item", "clipboard history items"),
        }
    }

    /// Whether this system writes the values of this kind protected
    /// (#130): local credentials on Windows.
    fn protects_values(self) -> bool {
        self == DataKind::LocalCredentials && protection::PROTECTS
    }

    /// `text` as a value of this kind is kept: protected where this system
    /// protects the kind, as it is otherwise.
    fn keep(self, text: String) -> Result<Value, String> {
        if self.protects_values() {
            Value::protect(text)
        } else {
            Ok(Value::plain(text))
        }
    }

    /// The versions of this kind's file this Pane reads.
    fn versions(self) -> &'static str {
        match self {
            DataKind::LocalCredentials => "1 and 2",
            _ => "1",
        }
    }

    /// What reading a value of this kind that cannot be read on this
    /// computer answers, given why (#130): "Pane cannot read this
    /// credential on this computer: Windows could not decrypt it (…). Sign
    /// in again."
    fn cannot_read(self, why: &str) -> String {
        match self {
            DataKind::LocalCredentials => {
                format!(
                    "{}. Sign in again.",
                    protection::cannot_read("credential", why)
                )
            }
            _ => protection::cannot_read(self.counted().0, why),
        }
    }

    /// What all of a package's values of this kind are called, with a verb.
    fn kept_unchanged(self) -> &'static str {
        match self {
            DataKind::Settings => "its settings are kept unchanged",
            DataKind::Content => "its content is kept unchanged",
            DataKind::Cache => "its cache is kept unchanged",
            DataKind::LocalCredentials => "its credentials are kept unchanged",
            DataKind::ClipboardHistory => "its clipboard history is kept unchanged",
        }
    }
}

/// Values by package identity key, then by key.
type Values = BTreeMap<String, BTreeMap<String, Value>>;

/// One kind's file in memory.
#[derive(Clone, Default)]
struct DataJson {
    /// Values by package identity key, then by the extension's own key.
    packages: Values,
    /// The values the user set for the preferences packages declare (see
    /// `preferences`), by package identity key, then by the preference's
    /// storage key: Pane's own, apart from the keys a package's code saves
    /// under, which it neither reads nor sets through its data interfaces.
    /// Only settings and local credentials hold any.
    preferences: Values,
}

/// One kind's file as written: in version 1 each value is a string, in
/// version 2 (local credentials, #130) each is a [`Stored`], recording how
/// it is protected.
///
/// ```json
/// { "version": 2, "packages": { "local:/…": { "token": { "dpapi": "AQAAANCMnd8B…" } } } }
/// ```
#[derive(Serialize, Deserialize)]
struct FileJson<V> {
    version: u64,
    packages: BTreeMap<String, BTreeMap<String, V>>,
    // A constructor rather than `default`, which would make serde require
    // `V: Default`, and `Stored` has none.
    #[serde(default = "BTreeMap::new", skip_serializing_if = "BTreeMap::is_empty")]
    preferences: BTreeMap<String, BTreeMap<String, V>>,
}

/// How many values an identity keeps of one kind, and how many of them
/// cannot be read on this computer (#130).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Count {
    kept: usize,
    unreadable: usize,
}

impl Count {
    /// `kept` values, each of which can be read.
    fn readable(kept: usize) -> Count {
        Count {
            kept,
            unreadable: 0,
        }
    }
}

impl DataJson {
    /// How many values `owner` keeps in this file, its preferences' among
    /// them.
    fn count(&self, owner: &str) -> usize {
        self.packages.get(owner).map_or(0, BTreeMap::len)
            + self.preferences.get(owner).map_or(0, BTreeMap::len)
    }

    /// How many values `owner` keeps in this file, and how many of them
    /// cannot be read on this computer: counting needs no decryption; only
    /// telling which cannot be read tries each protected value.
    fn tally(&self, owner: &str) -> Count {
        let unreadable = [&self.packages, &self.preferences]
            .into_iter()
            .filter_map(|values| values.get(owner))
            .flat_map(BTreeMap::values)
            .filter(|value| !value.is_plain() && value.text().is_err())
            .count();
        Count {
            kept: self.count(owner),
            unreadable,
        }
    }

    /// Every value of the file.
    fn values(&self) -> impl Iterator<Item = &Value> {
        self.packages
            .values()
            .chain(self.preferences.values())
            .flat_map(BTreeMap::values)
    }

    /// Every value of the file, to change.
    fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        self.packages
            .values_mut()
            .chain(self.preferences.values_mut())
            .flat_map(BTreeMap::values_mut)
    }

    /// Protects, where this system protects `kind` (local credentials on
    /// Windows, #130), the values still written as they are: whether any
    /// was, and why one could not be, if one could not.
    fn protect_plain_values(&mut self, kind: DataKind) -> (bool, Option<String>) {
        let mut protected = false;
        let mut failed = None;
        if kind.protects_values() {
            for value in self.values_mut() {
                match value.protect_in_place() {
                    Ok(done) => protected |= done,
                    Err(why) => failed = Some(why),
                }
            }
        }
        (protected, failed)
    }

    /// The file as written, at `version`, each value made by `each`.
    fn file<V>(&self, version: u64, each: impl Fn(&Value) -> V) -> FileJson<V> {
        let written = |values: &Values| -> BTreeMap<String, BTreeMap<String, V>> {
            values
                .iter()
                .map(|(owner, kept)| {
                    let kept = kept
                        .iter()
                        .map(|(key, value)| (key.clone(), each(value)))
                        .collect();
                    (owner.clone(), kept)
                })
                .collect()
        };
        FileJson {
            version,
            packages: written(&self.packages),
            preferences: written(&self.preferences),
        }
    }

    /// The file `file` read, each value made by `each`.
    fn from_file<V>(file: FileJson<V>, each: impl Fn(V) -> Value) -> DataJson {
        let kept_values = |values: BTreeMap<String, BTreeMap<String, V>>| -> Values {
            values
                .into_iter()
                .map(|(owner, kept)| {
                    let kept = kept
                        .into_iter()
                        .map(|(key, value)| (key, each(value)))
                        .collect();
                    (owner, kept)
                })
                .collect()
        };
        DataJson {
            packages: kept_values(file.packages),
            preferences: kept_values(file.preferences),
        }
    }

    /// The text of `kind`'s file holding these values: version 1 while
    /// every value is written as it is (a value protected elsewhere, as in
    /// a folder copied from Windows, is kept as it was), version 2
    /// otherwise. Where this system protects the kind, an empty file is
    /// version 2, and values that are all still as they are (Windows could
    /// encrypt none of them) are written as version 1, as an earlier Pane
    /// wrote them, never as plain values in a version-2 file.
    fn to_json(&self, kind: DataKind) -> serde_json::Result<String> {
        let all_plain = self.values().all(Value::is_plain);
        let none = self.values().next().is_none();
        if all_plain && !(kind.protects_values() && none) {
            let file = self.file(DATA_VERSION, |value| {
                value.plain_text().unwrap_or_default().to_owned()
            });
            serde_json::to_string_pretty(&file)
        } else {
            let file = self.file(PROTECTED_VERSION, |value| value.stored().clone());
            serde_json::to_string_pretty(&file)
        }
    }
}

/// One kind's file as Pane last read or wrote it.
struct KindFile {
    path: PathBuf,
    /// The saved values, or why they could not be read.
    file: Result<DataJson, String>,
    /// Counts the changes made to `file` in memory, to tell whether a
    /// failed write may be taken back.
    changes: u64,
    /// How many writes of this file are queued and not yet done.
    pending: usize,
}

impl KindFile {
    fn open(dir: &Path, kind: DataKind) -> KindFile {
        let path = dir.join(kind.file_name());
        let file = read(&path, kind);
        let mut opened = KindFile {
            path,
            file,
            changes: 0,
            pending: 0,
        };
        opened.protect_plain_values(kind);
        opened
    }

    /// Protects the values of a kind this system protects (local
    /// credentials on Windows, #130) that the file still holds as they are,
    /// as an earlier Pane wrote them (version 1), and writes the file at
    /// once, in one atomic write. If that write fails, the file stays as it
    /// was, its values read the same, and the next start tries again; a
    /// value Windows could not encrypt stays as it is until then. No value
    /// is lost either way.
    fn protect_plain_values(&mut self, kind: DataKind) {
        if !kind.protects_values() {
            return;
        }
        let Ok(file) = &mut self.file else {
            return;
        };
        let (protected, failed) = file.protect_plain_values(kind);
        if let Some(why) = failed {
            crate::diagnostic!(
                "Pane could not protect a value in {}: {why}. It is kept as it is until Pane \
                 next starts.",
                self.path.display()
            );
        }
        if !protected {
            return;
        }
        let written = file
            .to_json(kind)
            .map_err(io::Error::other)
            .and_then(|text| write_atomically(&self.path, text.as_bytes(), kind.readers()));
        if let Err(error) = written {
            crate::diagnostic!(
                "Pane could not protect the values in {}: {error}. It reads them as they are and \
                 tries again when it next starts.",
                self.path.display()
            );
        }
    }
}

/// One file write for the writer thread: the values of `kind` as they were
/// changed in memory (`change`), and how to take the change back
/// (`previous`) if the write fails.
struct Write {
    kind: DataKind,
    path: PathBuf,
    readers: Readers,
    updated: DataJson,
    previous: Result<DataJson, String>,
    change: u64,
    done: tokio::sync::oneshot::Sender<io::Result<()>>,
}

/// What the writer thread does next.
enum Job {
    Write(Write),
    /// Answered once every write queued before it is done.
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Starts the thread writing the files of `data`, one write after another,
/// which stops once `data` is gone.
fn start_writer(data: Weak<Mutex<DataFile>>) -> mpsc::Sender<Job> {
    let (jobs, queue) = mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name("pane-data-writer".into())
        .spawn(move || {
            for job in queue {
                let write = match job {
                    Job::Flush(done) => {
                        let _ = done.send(());
                        continue;
                    }
                    Job::Write(write) => write,
                };
                let written = write
                    .updated
                    .to_json(write.kind)
                    .map_err(io::Error::other)
                    .and_then(|text| write_atomically(&write.path, text.as_bytes(), write.readers));
                if let Some(data) = data.upgrade() {
                    let mut file = lock_file(&data);
                    let kind = file.of(write.kind);
                    kind.pending -= 1;
                    // Taken back, unless something changed since.
                    if written.is_err() && kind.changes == write.change {
                        kind.file = write.previous;
                    }
                }
                let _ = write.done.send(written);
            }
        });
    if let Err(error) = started {
        crate::diagnostic!("pane: could not start the thread writing extension data: {error}");
    }
    jobs
}

fn lock_file(data: &Mutex<DataFile>) -> std::sync::MutexGuard<'_, DataFile> {
    data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
    /// The thread writing the files.
    writer: Option<mpsc::Sender<Job>>,
    /// Told after a generation or a clipboard capture state changed, with
    /// the files unlocked (see `clipboard::Capture`, and the launcher's
    /// scheduled work). Each is called in turn; each watcher registers one,
    /// once.
    changed: Vec<Changed>,
}

/// What [`ExtensionData::set_changed`] calls.
pub(crate) type Changed = Arc<dyn Fn() + Send + Sync>;

impl DataFile {
    fn of(&mut self, kind: DataKind) -> &mut KindFile {
        match kind {
            DataKind::Settings => &mut self.settings,
            DataKind::Content => &mut self.content,
            DataKind::Cache => &mut self.cache,
            DataKind::LocalCredentials => &mut self.local_credentials,
            // Kept by `clipboard::history`, never through a kind's file.
            DataKind::ClipboardHistory => {
                unreachable!("clipboard history has a store of its own")
            }
        }
    }

    /// Changes the values of `kind` in memory to `updated` and queues their
    /// write, whose outcome `done` receives.
    fn stage(
        &mut self,
        kind: DataKind,
        updated: DataJson,
    ) -> tokio::sync::oneshot::Receiver<io::Result<()>> {
        let (done, outcome) = tokio::sync::oneshot::channel();
        let data = self.of(kind);
        data.changes += 1;
        data.pending += 1;
        let previous = std::mem::replace(&mut data.file, Ok(updated.clone()));
        let write = Write {
            kind,
            path: data.path.clone(),
            readers: kind.readers(),
            updated,
            previous,
            change: data.changes,
            done,
        };
        let unsent = match &self.writer {
            Some(writer) => writer
                .send(Job::Write(write))
                .err()
                .map(|mpsc::SendError(job)| job),
            None => Some(Job::Write(write)),
        };
        if let Some(Job::Write(write)) = unsent {
            // No writer: nothing is written, and the change is taken back.
            let data = self.of(kind);
            data.pending -= 1;
            data.file = write_previous(write);
        }
        outcome
    }
}

/// What a write that could not be queued takes back.
fn write_previous(write: Write) -> Result<DataJson, String> {
    let _ = write.done.send(Err(io::Error::other(
        "Pane's thread writing extension data has stopped",
    )));
    write.previous
}

/// Every installed package's extension data. Cloning shares the same
/// files.
#[derive(Clone)]
pub(crate) struct ExtensionData {
    files: Arc<Mutex<DataFile>>,
    /// The folder the files are in.
    dir: Arc<PathBuf>,
    /// The packages' clipboard history, typed and in a file of its own.
    clipboard: Arc<HistoryStore>,
}

impl ExtensionData {
    /// Opens the data kept in `dir`. Nothing is written until a command
    /// saves a value, but for the conversion of files an earlier Pane wrote
    /// unprotected, on a system that protects them (#130): local
    /// credentials here, and clipboard history (see `clipboard::history`).
    pub fn open(dir: &Path) -> ExtensionData {
        let files = Arc::new(Mutex::new(DataFile {
            settings: KindFile::open(dir, DataKind::Settings),
            content: KindFile::open(dir, DataKind::Content),
            cache: KindFile::open(dir, DataKind::Cache),
            local_credentials: KindFile::open(dir, DataKind::LocalCredentials),
            generations: HashMap::new(),
            writer: None,
            changed: Vec::new(),
        }));
        lock_file(&files).writer = Some(start_writer(Arc::downgrade(&files)));
        ExtensionData {
            files,
            dir: Arc::new(dir.to_path_buf()),
            clipboard: Arc::new(HistoryStore::open(dir)),
        }
    }

    /// The folder Pane caches the web images in that the icons of the
    /// package with identity key `owner` name (#142): its extension cache,
    /// removed with its cache values ([`ExtensionData::clear_cache`]).
    pub fn web_images(&self, owner: &str) -> PathBuf {
        self.dir
            .join(WEB_IMAGES_DIR)
            .join(crate::icons::web_image_stem(owner))
    }

    /// Waits until every write queued so far is done.
    fn flush(&self) {
        let (done, flushed) = tokio::sync::oneshot::channel();
        let writer = self.lock().writer.clone();
        if let Some(writer) = writer
            && writer.send(Job::Flush(done)).is_ok()
        {
            let _ = flushed.blocking_recv();
        }
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
            fence: None,
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
        let mut undo = None;
        if !enabled {
            undo = Some(end_as(current, End::Disabled));
        } else if current.ended().is_some() {
            *current = Generation::new();
        }
        drop(file);
        // The ended generation's undo list runs once the files are let go.
        drop(undo);
        self.changed();
    }

    /// Notes that Pane paused the package with `identity` after it failed:
    /// its generation ends, which stops its pending calls, and its code can
    /// no longer read or save values until it is resumed.
    pub fn pause(&self, identity: &PackageIdentity) {
        let undo = self
            .lock()
            .generations
            .entry(identity.key())
            .or_insert_with(Generation::new)
            .end(End::Paused);
        drop(undo);
        self.changed();
    }

    /// Runs the package with `identity` again in a new generation, if Pane
    /// had paused it; otherwise changes nothing.
    pub fn resume(&self, identity: &PackageIdentity) {
        let mut file = self.lock();
        if let Some(current) = file.generations.get_mut(&identity.key())
            && current.ended() == Some(End::Paused)
        {
            *current = Generation::new();
        }
        drop(file);
        self.changed();
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
        let mut undo = None;
        match current.ended() {
            None => {
                undo = Some(current.end(End::Replaced));
                *current = Generation::new();
            }
            // Paused code is replaced by code that has not failed.
            Some(End::Paused) => *current = Generation::new(),
            Some(_) => {}
        }
        drop(file);
        drop(undo);
        self.changed();
    }

    /// Notes that the package with `identity` is being uninstalled: its
    /// generation ends, which stops its pending calls, and its code can no
    /// longer read or save values. Installing it again starts a new one
    /// ([`ExtensionData::set_enabled`]).
    pub fn uninstall(&self, identity: &PackageIdentity) {
        let mut file = self.lock();
        let current = file
            .generations
            .entry(identity.key())
            .or_insert_with(Generation::new);
        let undo = end_as(current, End::Uninstalled);
        drop(file);
        drop(undo);
        self.changed();
    }

    /// Puts back the package with `identity` after its uninstall could not
    /// be recorded: a new generation, ended at once if it is disabled.
    pub fn reinstate(&self, identity: &PackageIdentity, enabled: bool) {
        let generation = Generation::new();
        if !enabled {
            // A new generation has nothing to undo yet.
            drop(generation.end(End::Disabled));
        }
        self.lock().generations.insert(identity.key(), generation);
        self.changed();
    }

    /// Has `changed` called after each change of a package's generation or
    /// clipboard capture state, with the files unlocked. Call it once per
    /// watcher: every `changed` so far is called in turn.
    pub fn set_changed(&self, changed: Changed) {
        self.lock().changed.push(changed);
    }

    /// Calls each what [`ExtensionData::set_changed`] set.
    pub fn changed(&self) {
        let changed = self.lock().changed.clone();
        for changed in changed {
            changed();
        }
    }

    /// The identity keys whose code may run now: their generation has not
    /// ended.
    pub fn running_owners(&self) -> Vec<String> {
        self.lock()
            .generations
            .iter()
            .filter(|(_, generation)| generation.ended().is_none())
            .map(|(owner, _)| owner.clone())
            .collect()
    }

    /// Every package's clipboard history.
    pub fn clipboard_history(&self) -> &HistoryStore {
        &self.clipboard
    }

    /// Removes the clipboard history items that expired, now and from now
    /// on on a thread of Pane's own whenever they expire, whether their
    /// package runs, is disabled or was uninstalled with its data kept.
    pub fn keep_expiring_clipboard_history(&self) {
        self.clipboard.sweep();
        self.clipboard.keep_expiring();
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
    /// running it: its cache and local credentials, and its saved data
    /// (settings, content and clipboard history) too when `saved` is
    /// [`SavedData::Delete`]. Each kind is
    /// removed on its own, as [`ExtensionData::clear_cache`] removes the
    /// cache. Returns why each kind that could not be removed was not; its
    /// values remain where they were.
    pub fn remove_uninstalled(&self, identity: &PackageIdentity, saved: SavedData) -> Vec<String> {
        let mut kinds = vec![DataKind::Cache, DataKind::LocalCredentials];
        if saved == SavedData::Delete {
            kinds.extend(DataKind::SAVED);
        }
        self.remove_kinds(identity, &kinds)
    }

    /// Removes every kind of data Pane keeps for `identity`, a package that
    /// is not installed, without running it: its retained data. Each kind is
    /// removed on its own, as [`ExtensionData::remove_uninstalled`] does, and
    /// other identities' values stay. Returns why each kind that could not be
    /// removed was not; its values remain where they were.
    pub fn remove_retained(&self, identity: &PackageIdentity) -> Vec<String> {
        self.remove_kinds(identity, &DataKind::ALL)
    }

    /// Removes `kinds` of the data of `identity`, returning why each that
    /// could not be removed was not.
    fn remove_kinds(&self, identity: &PackageIdentity, kinds: &[DataKind]) -> Vec<String> {
        kinds
            .iter()
            .copied()
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

    /// The values the user set for the preferences of the package with
    /// `identity`, by storage key (see `preferences`): its settings' and its
    /// local credentials' together. A file that cannot be read holds none,
    /// and a value that cannot be read on this computer (#130) is as if it
    /// was never set, so the user is asked for it again. Pane's own record,
    /// readable whether or not the package runs.
    pub fn preference_values(&self, identity: &PackageIdentity) -> BTreeMap<String, String> {
        let owner = identity.key();
        let mut store = self.lock();
        let mut values = BTreeMap::new();
        for kind in PREFERENCE_KINDS {
            if let Ok(file) = &store.of(kind).file
                && let Some(kept) = file.preferences.get(&owner)
            {
                values.extend(
                    kept.iter().filter_map(|(key, value)| {
                        Some((key.clone(), value.text().ok()?.to_owned()))
                    }),
                );
            }
        }
        values
    }

    /// Changes the preference values of the package with `identity`:
    /// `change` gets each by storage key with the kind it is kept as (a
    /// password's [`DataKind::LocalCredentials`], every other
    /// [`DataKind::Settings`]) and may set, move or remove them. The changes
    /// are made at once and written in the background, whether or not the
    /// returned writes are awaited; [`PreferenceWrites::written`] says
    /// whether they were. Pane's own record: it changes whether or not the
    /// package runs, and the package's code never writes it. Nothing
    /// changes, and the reason is returned, if either file cannot be read
    /// or a new local credential cannot be protected (#130). A value that
    /// cannot be read on this computer is not handed to `change` and stays
    /// as it is, unless `change` sets one under its key.
    pub fn change_preferences(
        &self,
        identity: &PackageIdentity,
        change: impl FnOnce(&mut BTreeMap<String, (DataKind, String)>),
    ) -> Result<PreferenceWrites, String> {
        let owner = identity.key();
        let mut store = self.lock();
        let mut values = BTreeMap::new();
        let mut unreadable = Vec::new();
        for kind in PREFERENCE_KINDS {
            let file = store.of(kind).file.as_ref().map_err(Clone::clone)?;
            for (key, value) in file.preferences.get(&owner).into_iter().flatten() {
                match value.text() {
                    Ok(text) => {
                        values.insert(key.clone(), (kind, text.to_owned()));
                    }
                    Err(_) => unreadable.push((kind, key.clone(), value.clone())),
                }
            }
        }
        let before = values.clone();
        change(&mut values);
        let of = |values: &BTreeMap<String, (DataKind, String)>, kind: DataKind| {
            values
                .iter()
                .filter(|(_, (held, _))| *held == kind)
                .map(|(key, (_, value))| (key.clone(), value.clone()))
                .collect::<BTreeMap<String, String>>()
        };
        let mut changed = Vec::new();
        for kind in PREFERENCE_KINDS {
            let wanted = of(&values, kind);
            if wanted == of(&before, kind) {
                continue;
            }
            let mut updated = store.of(kind).file.as_ref().map_err(Clone::clone)?.clone();
            let earlier = updated.preferences.remove(&owner).unwrap_or_default();
            let mut kept: BTreeMap<String, Value> = unreadable
                .iter()
                .filter(|(held, key, _)| *held == kind && !values.contains_key(key))
                .map(|(_, key, value)| (key.clone(), value.clone()))
                .collect();
            for (key, text) in wanted {
                // A value set again unchanged is written as it was, not
                // protected again.
                let value = match earlier.get(&key) {
                    Some(value) if value.text() == Ok(text.as_str()) => value.clone(),
                    _ => kind
                        .keep(text)
                        .map_err(|why| format!("Could not save {}: {why}", kind.value()))?,
                };
                kept.insert(key, value);
            }
            if !kept.is_empty() {
                updated.preferences.insert(owner.clone(), kept);
            }
            changed.push((kind, updated));
        }
        let writes = changed
            .into_iter()
            .map(|(kind, updated)| (kind, store.stage(kind, updated)))
            .collect();
        Ok(PreferenceWrites(writes))
    }

    /// What Pane keeps of `kinds`, read from their files now, so that a file
    /// repaired or changed by another Pane since is counted as it is. Each
    /// file is read once, however many identities are then described.
    /// Counting decrypts nothing; only [`Kept::describe_unreadable`] tries
    /// the protected values of the one identity it describes (#130).
    pub fn kept_now(&self, kinds: &[DataKind]) -> Kept {
        Kept(
            kinds
                .iter()
                .map(|&kind| {
                    if kind == DataKind::ClipboardHistory {
                        let counts = self.clipboard.counts_now().map(KindKept::Counted);
                        return (kind, counts);
                    }
                    let path = self.lock().of(kind).path.clone();
                    (kind, read(&path, kind).map(KindKept::Read))
                })
                .collect(),
        )
    }

    /// How many values of `kind` the package with `identity` keeps, as Pane
    /// last read or wrote them, or why they cannot be read.
    pub fn count(&self, kind: DataKind, identity: &PackageIdentity) -> Result<usize, String> {
        if kind == DataKind::ClipboardHistory {
            // Its items, and its choices as one more.
            let history = self.clipboard.get(&identity.key())?;
            let choices = history.has_choices();
            return Ok(history.items.len() + usize::from(choices));
        }
        let mut store = self.lock();
        let file = store.of(kind).file.as_ref().map_err(Clone::clone)?;
        Ok(file.count(&identity.key()))
    }

    /// Removes every value of `kind` of the package with `identity`, and
    /// nothing else. The file is read again first, so values another Pane
    /// process saved since are kept, and a file the user repaired or deleted
    /// is used without restarting Pane. On failure nothing is removed.
    ///
    /// Called off the runtime thread: it waits for the writer. The file is
    /// read again once no write of it is queued, without the lock held (a
    /// runtime thread saving meanwhile never waits on the file system), and
    /// used only if nothing was saved meanwhile, so a value saved just
    /// before is not lost.
    ///
    /// The cache's web images (see [`ExtensionData::web_images`]) are
    /// removed first, with their folder.
    fn remove(&self, kind: DataKind, identity: &PackageIdentity) -> Result<(), Removal> {
        if kind == DataKind::ClipboardHistory {
            return self.clipboard.remove(&identity.key());
        }
        if kind == DataKind::Cache {
            let images = self.web_images(&identity.key());
            match fs::remove_dir_all(&images) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(Removal::Unwritable(images, error)),
            }
        }
        let (outcome, path) = loop {
            self.flush();
            let (path, before) = {
                let mut store = self.lock();
                let data = store.of(kind);
                if data.pending > 0 {
                    continue;
                }
                (data.path.clone(), data.changes)
            };
            let mut fresh = read(&path, kind);
            // Values the file still holds as they are (a conversion at
            // start that could not be written, #130) are protected before
            // it is written again, as at start.
            let failed = match &mut fresh {
                Ok(file) => file.protect_plain_values(kind).1,
                Err(_) => None,
            };
            if let Some(why) = failed {
                crate::diagnostic!(
                    "Pane could not protect a value in {}: {why}. It is kept as it is until Pane \
                     next starts.",
                    path.display()
                );
            }
            let mut store = self.lock();
            let data = store.of(kind);
            if data.pending > 0 || data.changes != before {
                continue;
            }
            data.file = fresh;
            data.changes += 1;
            let file = data
                .file
                .as_ref()
                .map_err(|reason| Removal::Unreadable(reason.clone()))?;
            if !file.packages.contains_key(&identity.key())
                && !file.preferences.contains_key(&identity.key())
            {
                return Ok(());
            }
            let mut updated = file.clone();
            updated.packages.remove(&identity.key());
            updated.preferences.remove(&identity.key());
            break (store.stage(kind, updated), path);
        };
        match outcome.blocking_recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(Removal::Unwritable(path, error)),
            Err(_) => Err(Removal::Unwritable(
                path,
                io::Error::other("Pane's thread writing extension data has stopped"),
            )),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DataFile> {
        lock_file(&self.files)
    }
}

/// The writes a change of preference values queued (see
/// [`ExtensionData::change_preferences`]): they happen whether or not they
/// are awaited.
pub(crate) struct PreferenceWrites(Vec<(DataKind, tokio::sync::oneshot::Receiver<io::Result<()>>)>);

impl PreferenceWrites {
    /// Waits until every write is done; why the first that failed did, if
    /// one did.
    pub async fn written(self) -> Result<(), String> {
        for (kind, written) in self.0 {
            let failed = |error: io::Error| format!("Could not save {}: {error}", kind.value());
            match written.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(failed(error)),
                Err(_) => {
                    return Err(failed(io::Error::other(
                        "Pane's thread writing extension data has stopped",
                    )));
                }
            }
        }
        Ok(())
    }
}

/// One kind's values as they were read at one moment, or why the kind's
/// file cannot be read.
type Counts = Result<KindKept, String>;

/// One kind's values as they were read.
enum KindKept {
    /// Clipboard history: how many items each identity key keeps (an
    /// identity that keeps only its choices counts 0).
    Counted(BTreeMap<String, usize>),
    /// Any other kind: its file.
    Read(DataJson),
}

impl KindKept {
    /// How many values the identity with key `owner` keeps, if it keeps
    /// any; with `unreadable`, also how many of them cannot be read on this
    /// computer, which tries each of its protected values (#130).
    fn count(&self, owner: &str, unreadable: bool) -> Option<Count> {
        match self {
            KindKept::Counted(counts) => counts.get(owner).copied().map(Count::readable),
            KindKept::Read(file) => {
                if !file.packages.contains_key(owner) && !file.preferences.contains_key(owner) {
                    None
                } else if unreadable {
                    Some(file.tally(owner))
                } else {
                    Some(Count::readable(file.count(owner)))
                }
            }
        }
    }
}

/// Some kinds' files as they were read at one moment, to say what Pane keeps
/// for a package: each kind's [`Counts`].
pub(crate) struct Kept(Vec<(DataKind, Counts)>);

impl Kept {
    /// How many values of each kind Pane keeps for `identity`, such as
    /// "1 setting and 1 content record", or `None` if it keeps none. A kind
    /// whose file is missing keeps none; one whose file cannot be read says
    /// so. Nothing is decrypted: a value that cannot be read on this
    /// computer is counted like any other, as confirmations count (#130).
    pub fn describe(&self, identity: &PackageIdentity) -> Option<String> {
        self.describe_counting(identity, false)
    }

    /// [`Kept::describe`], and how many of `identity`'s values cannot be
    /// read on this computer (#130): "2 credentials, 1 unreadable". It
    /// tries `identity`'s protected values only, for the Manage extensions
    /// row that describes it.
    pub fn describe_unreadable(&self, identity: &PackageIdentity) -> Option<String> {
        self.describe_counting(identity, true)
    }

    fn describe_counting(&self, identity: &PackageIdentity, unreadable: bool) -> Option<String> {
        let key = identity.key();
        let parts: Vec<String> = self
            .0
            .iter()
            .filter_map(|(kind, counts)| {
                let (one, many) = kind.counted();
                let count = counts.as_ref().map(|kept| kept.count(&key, unreadable));
                if *kind == DataKind::ClipboardHistory
                    && matches!(count, Ok(Some(Count { kept: 0, .. })))
                {
                    // Only whether it keeps history, and the excluded programs.
                    return Some("clipboard history settings".into());
                }
                match count.map(Option::unwrap_or_default) {
                    Ok(Count { kept: 0, .. }) => None,
                    Ok(Count {
                        kept: 1,
                        unreadable: 0,
                    }) => Some(format!("1 {one}")),
                    Ok(Count {
                        kept,
                        unreadable: 0,
                    }) => Some(format!("{kept} {many}")),
                    Ok(Count { kept, unreadable }) => {
                        let called = if kept == 1 { one } else { many };
                        Some(format!("{kept} {called}, {unreadable} unreadable"))
                    }
                    Err(reason) => Some(format!("{many} that cannot be read now ({reason})")),
                }
            })
            .collect();
        match parts.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            [rest @ .., last] => Some(format!("{} and {last}", rest.join(", "))),
        }
    }
}

/// Why a kind's values could not be removed; none were.
pub(crate) enum Removal {
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
    /// The runtime thread running the code using it: once Pane gave up on
    /// that thread, the code is stopped as if its generation had ended.
    fence: Option<Fence>,
}

impl PackageData {
    /// The identity key of the package the data belongs to.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The generation of the package's code this data was handed out in.
    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    /// This data for code run by the runtime thread whose fence is `fence`
    /// (see [`Fence`]).
    pub fn fenced(mut self, fence: Fence) -> PackageData {
        self.fence = Some(fence);
        self
    }

    /// Why code using this data is stopped, if it is: its generation ended,
    /// or Pane gave up on the runtime thread running it. Its commands may no
    /// longer run or save values.
    pub fn stopped(&self) -> Option<End> {
        self.stopped_while(self.fence.as_ref().is_some_and(Fence::closed))
    }

    /// Like [`PackageData::stopped`], with whether the fence is closed.
    fn stopped_while(&self, fenced: bool) -> Option<End> {
        self.generation
            .ended()
            .or_else(|| fenced.then_some(End::Abandoned))
    }

    /// Why code using this data may no longer read or save values, if it is
    /// stopped (see [`PackageData::stopped`]).
    fn refusal(&self, fenced: bool) -> Option<&'static str> {
        Some(refusal(self.stopped_while(fenced)?))
    }

    /// The value of `kind` saved under `key`, if any, unless this data's
    /// generation has ended: stopped code reads nothing more either. A
    /// value that cannot be read on this computer (#130) is an error saying
    /// why, which the extension can turn into "sign in again"; it stays
    /// until the extension sets another.
    pub fn get(&self, kind: DataKind, key: &str) -> Result<Option<String>, String> {
        if let Some(refusal) = self.refusal(self.fence.as_ref().is_some_and(Fence::closed)) {
            return Err(refusal.into());
        }
        let mut store = self.data.lock();
        let file = store.of(kind).file.as_ref().map_err(Clone::clone)?;
        let Some(value) = file
            .packages
            .get(&self.owner)
            .and_then(|values| values.get(key))
        else {
            return Ok(None);
        };
        value
            .text()
            .map(|text| Some(text.to_owned()))
            .map_err(|why| kind.cannot_read(why))
    }

    /// Saves `value` of `kind` under `key`, unless code using this data is
    /// stopped: code that was disabled or replaced, or whose runtime thread
    /// Pane gave up on, saves nothing more. The change is checked and made
    /// while the fence is held, so none lands after the thread was given up
    /// on; the file is written by the writer thread, which this awaits. A
    /// local credential is protected first, on Windows (#130), and replaces
    /// whatever was saved under `key`, one that could not be read too.
    pub async fn set(&self, kind: DataKind, key: &str, value: &str) -> Result<(), String> {
        let failed = |error: io::Error| format!("Could not save {}: {error}", kind.value());
        // Protected before the values are locked.
        let value = kind
            .keep(value.to_owned())
            .map_err(|why| format!("Could not save {}: {why}", kind.value()))?;
        let outcome = {
            let fence = self.fence.as_ref().map(Fence::hold);
            let fenced = fence.as_deref() == Some(&true);
            let mut store = self.data.lock();
            if let Some(refusal) = self.refusal(fenced) {
                return Err(format!("{refusal}; {}", kind.kept_unchanged()));
            }
            let file = store.of(kind).file.as_ref().map_err(Clone::clone)?;
            let mut updated = file.clone();
            updated
                .packages
                .entry(self.owner.clone())
                .or_default()
                .insert(key.to_owned(), value);
            store.stage(kind, updated)
        };
        match outcome.await {
            Ok(written) => written.map_err(failed),
            Err(_) => Err(failed(io::Error::other(
                "Pane's thread writing extension data has stopped",
            ))),
        }
    }

    /// Every package's clipboard history, to read, unless code using this
    /// data is stopped (see [`PackageData::stopped`]): stopped code reads
    /// nothing more. It changes this package's history through
    /// [`PackageData::update_clipboard_history`].
    pub fn clipboard_history(&self) -> Result<&HistoryStore, String> {
        match self.refusal(self.fence.as_ref().is_some_and(Fence::closed)) {
            Some(refusal) => Err(refusal.into()),
            None => Ok(self.data.clipboard_history()),
        }
    }

    /// Changes this package's clipboard history with `change`, written
    /// before this returns, unless code using this data is stopped; returns
    /// its answer and whether the capture state changed. As for
    /// [`PackageData::set`], the change is checked and made in memory while
    /// the fence is held, so none lands after the runtime thread was given
    /// up on; the file is written after, with the fence released.
    pub fn update_clipboard_history<R>(
        &self,
        change: impl FnOnce(&mut PackageHistory) -> Result<R, String>,
    ) -> Result<(R, bool), String> {
        let staged = {
            let fence = self.fence.as_ref().map(Fence::hold);
            let fenced = fence.as_deref() == Some(&true);
            if let Some(refusal) = self.refusal(fenced) {
                let kept = DataKind::ClipboardHistory.kept_unchanged();
                return Err(format!("{refusal}; {kept}"));
            }
            self.data.clipboard.stage(&self.owner, change)?
        };
        self.data.clipboard.write_staged(staged)
    }

    /// Says that this package's clipboard capture state changed, which
    /// starts or stops watching the clipboard (see
    /// [`ExtensionData::set_changed`]).
    pub fn changed(&self) {
        self.data.changed();
    }
}

/// Why stopped code may no longer read or save values.
fn refusal(end: End) -> &'static str {
    match end {
        End::Disabled => "the extension is disabled",
        End::Replaced => "this code of the extension was replaced by a reload or an update",
        End::Uninstalled => "the extension was uninstalled",
        End::Paused => "the extension is paused after an error",
        End::Abandoned => {
            "Pane's extension runtime stopped responding and was replaced while this code ran"
        }
    }
}

/// Ends `current` for `why`, returning its undo list for the caller to run
/// once it lets its locks go. A generation Pane paused is replaced by one
/// ended for `why`, so its calls say what the user did, not that it was
/// paused.
fn end_as(current: &mut Generation, why: End) -> Undo {
    if current.ended() == Some(End::Paused) {
        *current = Generation::new();
    }
    current.end(why)
}

/// Reads the file of `kind` at `path`, without decrypting anything; a
/// missing file holds no values.
fn read(path: &Path, kind: DataKind) -> Result<DataJson, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text, kind),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DataJson::default()),
        Err(error) => Err(error.to_string()),
    }
    .map_err(|reason| format!("Cannot read {}: {reason}", path.display()))
}

/// The values of `kind`'s file whose text is `text`: version 1 for every
/// kind, version 2 for local credentials too (#130); any other version is
/// refused, so a file a newer Pane wrote is never overwritten.
fn parse(text: &str, kind: DataKind) -> Result<DataJson, String> {
    #[derive(Deserialize)]
    struct Versioned {
        version: u64,
    }
    let Versioned { version } =
        serde_json::from_str::<Versioned>(text).map_err(|error| error.to_string())?;
    match version {
        DATA_VERSION => serde_json::from_str::<FileJson<String>>(text)
            .map(|file| DataJson::from_file(file, Value::plain)),
        PROTECTED_VERSION if kind == DataKind::LocalCredentials => {
            serde_json::from_str::<FileJson<Stored>>(text)
                .map(|file| DataJson::from_file(file, Value::from_stored))
        }
        other => {
            return Err(format!(
                "it has version {other}, this Pane reads {}",
                kind.versions()
            ));
        }
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    /// Clipboard history is kept only for packages whose code may run, and
    /// each change of a generation says so.
    #[test]
    fn generation_changes_are_told_and_decide_who_runs() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let changes = Arc::new(AtomicUsize::new(0));
        let counted = changes.clone();
        data.set_changed(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        let told = || changes.load(Ordering::SeqCst);
        data.set_enabled(&identity, true);
        assert_eq!(data.running_owners(), [identity.key()]);
        for stop in [
            ExtensionData::pause as fn(&ExtensionData, &PackageIdentity),
            ExtensionData::uninstall,
        ] {
            let before = told();
            stop(&data, &identity);
            assert!(data.running_owners().is_empty());
            data.reinstate(&identity, true);
            assert_eq!(data.running_owners(), [identity.key()]);
            assert_eq!(told(), before + 2);
        }
        data.set_enabled(&identity, false);
        assert!(data.running_owners().is_empty());
        // Stopped code can no longer reach the history.
        let stopped = data.owned_by(&identity);
        assert!(stopped.clipboard_history().is_err());
        data.set_enabled(&identity, true);
        data.replace_code(&identity);
        assert_eq!(data.running_owners(), [identity.key()]);
        assert!(data.owned_by(&identity).clipboard_history().is_ok());
    }

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
            block_on(old.set(DataKind::Settings, "key", "old")),
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
        assert_eq!(block_on(new.set(DataKind::Settings, "key", "new")), Ok(()));
        data.set_enabled(&identity, false);
        assert_eq!(
            block_on(new.set(DataKind::Content, "key", "late")),
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

    /// Waits until `done`, failing the test if it takes more than a
    /// generous minute.
    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !done() {
            assert!(
                std::time::Instant::now() < deadline,
                "{what} did not happen"
            );
            std::thread::yield_now();
        }
    }

    /// Code of a runtime thread Pane gave up on saves nothing once the
    /// thread's fence is closed, while its generation goes on: a save
    /// already under way when the fence closes lands before the close
    /// returns, never after, and every save after it is refused.
    #[test]
    fn a_save_never_lands_after_the_fence_closed() {
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let fence = Fence::default();
        let fenced = data.owned_by(&identity).fenced(fence.clone());
        let saving = std::thread::spawn(move || {
            let mut outcomes = Vec::new();
            for n in 0.. {
                let saved = block_on(fenced.set(DataKind::Settings, "n", &n.to_string()));
                let refused = saved.is_err();
                outcomes.push(saved);
                if refused {
                    return outcomes;
                }
            }
            unreachable!()
        });
        let current = data.owned_by(&identity);
        until("a save landed", || {
            current.get(DataKind::Settings, "n") != Ok(None)
        });

        fence.close();
        let at_close = current.get(DataKind::Settings, "n").unwrap();

        let outcomes = saving.join().unwrap();
        let (last, landed) = outcomes.split_last().unwrap();
        assert!(landed.iter().all(Result::is_ok));
        assert_eq!(
            last,
            &Err(
                "Pane's extension runtime stopped responding and was replaced while this code \
                 ran; its settings are kept unchanged"
                    .into()
            )
        );
        data.flush();
        assert_eq!(current.get(DataKind::Settings, "n").unwrap(), at_close);
        let settings = DataKind::Settings;
        let file = read(&dir.path().join(settings.file_name()), settings).unwrap();
        assert_eq!(
            file.packages[&identity.key()]
                .get("n")
                .map(|value| value.text().unwrap()),
            at_close.as_deref(),
            "the file holds the last save before the close"
        );
        // The package's generation goes on: a fresh thread's code saves.
        assert_eq!(current.stopped(), None);
        assert_eq!(
            block_on(current.set(DataKind::Settings, "n", "fresh")),
            Ok(())
        );
    }

    /// Clipboard history (#35) is behind the same fence as the other kinds:
    /// once it closes, code of the thread Pane gave up on neither reads nor
    /// changes it, and a change under way when it closes lands before the
    /// close returns, never after, while the package's generation goes on.
    #[test]
    fn a_clipboard_change_never_lands_after_the_fence_closed() {
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let fence = Fence::default();
        let fenced = data.owned_by(&identity).fenced(fence.clone());
        // Copied now, so that none has expired (#36).
        let now = data.clipboard_history().now();
        let adding = std::thread::spawn(move || {
            let mut outcomes = Vec::new();
            for n in 0_u64.. {
                let added = fenced.update_clipboard_history(|history| {
                    history.add(&n.to_string(), None, now);
                    Ok(())
                });
                let refused = added.is_err();
                outcomes.push(added.map(|_| ()));
                if refused {
                    let read = fenced.clipboard_history().err();
                    return (outcomes, read);
                }
            }
            unreachable!()
        });
        let items = || data.clipboard_history().get(&identity.key()).unwrap().items;
        until("a change landed", || !items().is_empty());

        fence.close();
        let at_close = items();

        let (outcomes, read) = adding.join().unwrap();
        let (last, landed) = outcomes.split_last().unwrap();
        assert!(landed.iter().all(Result::is_ok));
        let abandoned =
            "Pane's extension runtime stopped responding and was replaced while this code ran";
        assert_eq!(
            last,
            &Err(format!(
                "{abandoned}; its clipboard history is kept unchanged"
            ))
        );
        assert_eq!(read.as_deref(), Some(abandoned), "reads are fenced too");
        assert_eq!(items(), at_close);
        let on_disk = HistoryStore::open(dir.path())
            .get(&identity.key())
            .unwrap()
            .items;
        assert_eq!(
            on_disk, at_close,
            "the file holds the last change before the close"
        );
        // The package's generation goes on: a fresh thread's code changes it.
        let current = data.owned_by(&identity);
        assert_eq!(current.stopped(), None);
        let cleared = current.update_clipboard_history(|history| Ok(history.clear()));
        assert_eq!(cleared, Ok((at_close.len(), false)));
    }

    /// Preference values are Pane's: a password's a local credential, the
    /// others settings, apart from the keys the package's code saves under;
    /// uninstalling removes the credentials and, unless the saved data is
    /// kept, the settings too.
    #[test]
    fn preference_values_are_kept_by_kind_and_removed_as_their_kind_is() {
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let code = data.owned_by(&identity);
        block_on(code.set(DataKind::Settings, "apiKey", "the code's own")).unwrap();
        let writes = data
            .change_preferences(&identity, |values| {
                values.insert(
                    "apiKey".into(),
                    (DataKind::LocalCredentials, "secret".into()),
                );
                values.insert("units".into(), (DataKind::Settings, "metric".into()));
            })
            .unwrap();
        block_on(writes.written()).unwrap();
        let values = data.preference_values(&identity);
        assert_eq!(values.get("apiKey").map(String::as_str), Some("secret"));
        assert_eq!(values.get("units").map(String::as_str), Some("metric"));
        assert_eq!(
            code.get(DataKind::Settings, "apiKey"),
            Ok(Some("the code's own".into())),
            "apart from the code's own keys"
        );
        assert_eq!(data.count(DataKind::LocalCredentials, &identity), Ok(1));
        assert_eq!(data.count(DataKind::Settings, &identity), Ok(2));
        // Written to the files, where another Pane reads them.
        let reopened = ExtensionData::open(dir.path());
        assert_eq!(reopened.preference_values(&identity), values);

        assert!(
            data.remove_uninstalled(&identity, SavedData::Keep)
                .is_empty()
        );
        let kept = data.preference_values(&identity);
        assert_eq!(
            kept.get("apiKey"),
            None,
            "credentials are removed either way"
        );
        assert_eq!(kept.get("units").map(String::as_str), Some("metric"));
        assert!(
            data.remove_uninstalled(&identity, SavedData::Delete)
                .is_empty()
        );
        assert!(data.preference_values(&identity).is_empty());
    }

    /// Disabling, reloading or updating (a replacement of the code),
    /// uninstalling and pausing a package each end its generation, which
    /// runs what the generation registered to undo, newest first, once,
    /// with the files let go (a teardown may use them).
    #[test]
    fn every_end_of_a_generation_runs_its_undo_list() {
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        type Ending = fn(&ExtensionData, &PackageIdentity);
        let ends: [(&str, Ending); 4] = [
            ("disable", |data, identity| {
                data.set_enabled(identity, false)
            }),
            ("replace", ExtensionData::replace_code),
            ("uninstall", ExtensionData::uninstall),
            ("pause", ExtensionData::pause),
        ];
        for (name, end) in ends {
            data.reinstate(&identity, true);
            let generation = data.owned_by(&identity).generation().clone();
            let ran = Arc::new(Mutex::new(Vec::new()));
            let note = |what: &'static str| {
                let (ran, files) = (ran.clone(), data.clone());
                move || {
                    // The files are not locked while it runs.
                    files.running_owners();
                    ran.lock().unwrap().push(what);
                    Ok(())
                }
            };
            let _first = generation.on_end("first", note("first"));
            let _second = generation.on_end("second", note("second"));

            end(&data, &identity);

            assert_eq!(*ran.lock().unwrap(), ["second", "first"], "{name}");
            assert!(generation.undo_list().is_empty(), "{name}");
            end(&data, &identity);
            assert_eq!(ran.lock().unwrap().len(), 2, "{name}: undone once");
        }
    }

    /// The local credentials file in `dir`, as text.
    fn credentials_file(dir: &Path) -> String {
        fs::read_to_string(dir.join(DataKind::LocalCredentials.file_name())).unwrap()
    }

    /// #130: a credential reads back as it was saved, also after a restart;
    /// on Windows the file holds it encrypted (version 2), elsewhere as it
    /// is (version 1, as before).
    #[test]
    fn a_credential_reads_back_and_is_written_as_this_system_protects_it() {
        let dir = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(dir.path());
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let code = data.owned_by(&identity);
        let credentials = DataKind::LocalCredentials;
        block_on(code.set(credentials, "token", "sample-token")).unwrap();
        block_on(code.set(DataKind::Settings, "style", "formal")).unwrap();
        assert_eq!(
            code.get(credentials, "token"),
            Ok(Some("sample-token".into()))
        );

        let file = credentials_file(dir.path());
        let json: serde_json::Value = serde_json::from_str(&file).unwrap();
        if protection::PROTECTS {
            assert!(!file.contains("sample-token"), "{file}");
            assert_eq!(json["version"], 2);
            let stored = &json["packages"][&identity.key()]["token"];
            assert!(stored["dpapi"].is_string(), "{stored}");
        } else {
            assert_eq!(json["version"], 1);
            assert_eq!(json["packages"][&identity.key()]["token"], "sample-token");
        }
        // The other kinds stay at version 1, as they are.
        let settings = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(settings["version"], 1);
        assert_eq!(settings["packages"][&identity.key()]["style"], "formal");

        let restarted = ExtensionData::open(dir.path());
        assert_eq!(
            restarted.owned_by(&identity).get(credentials, "token"),
            Ok(Some("sample-token".into()))
        );
        assert_eq!(restarted.count(credentials, &identity), Ok(1));
    }

    /// #130: on Windows a credentials file an earlier Pane wrote (version
    /// 1) is protected when Pane starts, with every value kept.
    #[cfg(windows)]
    #[test]
    fn a_version_1_credentials_file_is_protected_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let earlier = serde_json::json!({
            "version": 1,
            "packages": { identity.key(): { "token": "sample-token" } },
            "preferences": { identity.key(): { "apiKey": "the key" } },
        });
        fs::write(dir.path().join("credentials.json"), earlier.to_string()).unwrap();

        let data = ExtensionData::open(dir.path());

        let file = credentials_file(dir.path());
        assert!(
            !file.contains("sample-token") && !file.contains("the key"),
            "{file}"
        );
        let json: serde_json::Value = serde_json::from_str(&file).unwrap();
        assert_eq!(json["version"], 2);
        let code = data.owned_by(&identity);
        assert_eq!(
            code.get(DataKind::LocalCredentials, "token"),
            Ok(Some("sample-token".into()))
        );
        assert_eq!(
            data.preference_values(&identity)
                .get("apiKey")
                .map(String::as_str),
            Some("the key")
        );
    }

    /// #130: a value that cannot be decrypted on this computer (damaged, or
    /// protected by Windows elsewhere) is explained to the extension, while
    /// its other values read; it is counted as unreadable, kept as it was
    /// by every other write, and replaced only by a `set` of its key. A
    /// password preference that cannot be read is as if unset.
    #[test]
    fn a_value_that_cannot_be_read_is_explained_and_kept_until_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let damaged = serde_json::json!({ "dpapi": "AAAA" });
        let file = serde_json::json!({
            "version": 2,
            "packages": { identity.key(): {
                "token": damaged,
                "other": { "plain": "still read" },
            } },
            "preferences": { identity.key(): { "apiKey": damaged } },
        });
        let path = dir.path().join("credentials.json");
        fs::write(&path, file.to_string()).unwrap();
        let data = ExtensionData::open(dir.path());
        let code = data.owned_by(&identity);
        let credentials = DataKind::LocalCredentials;

        let error = code.get(credentials, "token").unwrap_err();
        assert!(
            error.starts_with("Pane cannot read this credential on this computer: "),
            "{error}"
        );
        assert!(error.ends_with(". Sign in again."), "{error}");
        if protection::PROTECTS {
            assert!(
                error.contains(": Windows could not decrypt it ("),
                "{error}"
            );
        }
        assert_eq!(
            code.get(credentials, "other"),
            Ok(Some("still read".into()))
        );
        // Counted without reading them, and said to be unreadable where
        // Manage extensions describes the identity.
        assert_eq!(data.count(credentials, &identity), Ok(3));
        let kept = data.kept_now(&[credentials]);
        assert_eq!(kept.describe(&identity).as_deref(), Some("3 credentials"));
        assert_eq!(
            kept.describe_unreadable(&identity).as_deref(),
            Some("3 credentials, 2 unreadable")
        );
        // A password that cannot be read is asked for again.
        assert_eq!(data.preference_values(&identity).get("apiKey"), None);

        // Another write keeps it as it was.
        block_on(code.set(credentials, "other", "changed")).unwrap();
        let writes = data
            .change_preferences(&identity, |values| {
                values.insert("units".into(), (DataKind::Settings, "metric".into()));
            })
            .unwrap();
        block_on(writes.written()).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["packages"][&identity.key()]["token"], damaged);
        assert_eq!(written["preferences"][&identity.key()]["apiKey"], damaged);
        assert!(code.get(credentials, "token").is_err());

        // Setting it replaces it.
        block_on(code.set(credentials, "token", "signed in again")).unwrap();
        assert_eq!(
            code.get(credentials, "token"),
            Ok(Some("signed in again".into()))
        );
        let writes = data
            .change_preferences(&identity, |values| {
                values.insert("apiKey".into(), (credentials, "new key".into()));
            })
            .unwrap();
        block_on(writes.written()).unwrap();
        assert_eq!(
            data.preference_values(&identity)
                .get("apiKey")
                .map(String::as_str),
            Some("new key")
        );
        let reopened = ExtensionData::open(dir.path());
        assert_eq!(
            reopened
                .kept_now(&[credentials])
                .describe_unreadable(&identity)
                .as_deref(),
            Some("3 credentials")
        );
    }

    /// A credentials file of a version this Pane does not know (a newer
    /// Pane's) is refused, and never overwritten.
    #[test]
    fn a_credentials_file_of_another_version_is_refused_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let identity = PackageIdentity::local(dir.path()).unwrap();
        let path = dir.path().join("credentials.json");
        let newer = r#"{ "version": 3, "packages": {} }"#;
        fs::write(&path, newer).unwrap();
        let data = ExtensionData::open(dir.path());
        let code = data.owned_by(&identity);
        let credentials = DataKind::LocalCredentials;

        let error = code.get(credentials, "token").unwrap_err();
        assert!(
            error.ends_with("it has version 3, this Pane reads 1 and 2"),
            "{error}"
        );
        assert!(block_on(code.set(credentials, "token", "new")).is_err());
        assert!(
            !data
                .remove_uninstalled(&identity, SavedData::Keep)
                .is_empty()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), newer);
    }
}
