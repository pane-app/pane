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
//! | Local credentials | `credentials.json` | kept | removed | the user only (Unix: 0600) |
//! | Clipboard history | `clipboard-history.json` | kept | the user's choice | the user only (Unix: 0600) |
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

/// The version of every kind's file.
const DATA_VERSION: u64 = 1;

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
    /// The text the user copied while the package kept clipboard history,
    /// and whether it keeps it; written by Pane, never by the package's
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

#[derive(Clone, Default, Serialize, Deserialize)]
struct DataJson {
    version: u64,
    /// Values by package identity key, then by the extension's own key.
    packages: BTreeMap<String, BTreeMap<String, String>>,
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
        let file = read(&path);
        KindFile {
            path,
            file,
            changes: 0,
            pending: 0,
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
                let written = serde_json::to_string_pretty(&write.updated)
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
        eprintln!("pane: could not start the thread writing extension data: {error}");
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
    /// saves a value.
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

    /// What Pane keeps of `kinds`, read from their files now, so that a file
    /// repaired or changed by another Pane since is counted as it is. Each
    /// file is read once, however many identities are then described.
    pub fn kept_now(&self, kinds: &[DataKind]) -> Kept {
        Kept(
            kinds
                .iter()
                .map(|&kind| {
                    if kind == DataKind::ClipboardHistory {
                        return (kind, self.clipboard.counts_now());
                    }
                    let path = self.lock().of(kind).path.clone();
                    let counts = read(&path).map(|file| {
                        file.packages
                            .into_iter()
                            .map(|(owner, values)| (owner, values.len()))
                            .collect()
                    });
                    (kind, counts)
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
        Ok(file.packages.get(&identity.key()).map_or(0, BTreeMap::len))
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
            let fresh = read(&path);
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
            if !file.packages.contains_key(&identity.key()) {
                return Ok(());
            }
            let mut updated = file.clone();
            updated.packages.remove(&identity.key());
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

/// A kind's count of values by identity key (for clipboard history, its
/// items; an identity that keeps only its choices counts 0), or why the
/// kind's file cannot be read.
type Counts = Result<BTreeMap<String, usize>, String>;

/// Some kinds' files as they were read at one moment, to say what Pane keeps
/// for a package: each kind's [`Counts`].
pub(crate) struct Kept(Vec<(DataKind, Counts)>);

impl Kept {
    /// How many values of each kind Pane keeps for `identity`, such as
    /// "1 setting and 1 content record", or `None` if it keeps none. A kind
    /// whose file is missing keeps none; one whose file cannot be read says
    /// so.
    pub fn describe(&self, identity: &PackageIdentity) -> Option<String> {
        let key = identity.key();
        let parts: Vec<String> = self
            .0
            .iter()
            .filter_map(|(kind, counts)| {
                let (one, many) = kind.counted();
                let count = counts.as_ref().map(|counts| counts.get(&key).copied());
                if *kind == DataKind::ClipboardHistory && count == Ok(Some(0)) {
                    // Only whether it keeps history, and the excluded programs.
                    return Some("clipboard history settings".into());
                }
                match count.map(Option::unwrap_or_default) {
                    Ok(0) => None,
                    Ok(1) => Some(format!("1 {one}")),
                    Ok(count) => Some(format!("{count} {many}")),
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
    /// generation has ended: stopped code reads nothing more either.
    pub fn get(&self, kind: DataKind, key: &str) -> Result<Option<String>, String> {
        if let Some(refusal) = self.refusal(self.fence.as_ref().is_some_and(Fence::closed)) {
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

    /// Saves `value` of `kind` under `key`, unless code using this data is
    /// stopped: code that was disabled or replaced, or whose runtime thread
    /// Pane gave up on, saves nothing more. The change is checked and made
    /// while the fence is held, so none lands after the thread was given up
    /// on; the file is written by the writer thread, which this awaits.
    pub async fn set(&self, kind: DataKind, key: &str, value: &str) -> Result<(), String> {
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
                .insert(key.to_owned(), value.to_owned());
            store.stage(kind, updated)
        };
        let failed = |error: io::Error| format!("Could not save {}: {error}", kind.value());
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
        let file = read(&dir.path().join(DataKind::Settings.file_name())).unwrap();
        assert_eq!(
            file.packages[&identity.key()].get("n"),
            at_close.as_ref(),
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
}
