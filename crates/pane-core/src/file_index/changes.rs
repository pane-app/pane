//! How Pane learns what changed under the index scope (#126, "Catching up
//! and watching, per system"): one change source per system, behind the
//! [`ChangeSource`] trait, so that tests drive catch-up, live changes and
//! fallbacks deterministically (the indexer coordinator takes any source).
//!
//! | | Catch-up at start | Live changes |
//! | --- | --- | --- |
//! | Windows | the NTFS change journal, from the saved cursor, per volume ([`super::read_journal`]) | `ReadDirectoryChangesW` on each root (the `notify` crate) |
//! | macOS | FSEvents' history, replayed by the live stream from the saved event id, per volume | the same FSEvents stream |
//! | Linux | a reconciling walk ([`super::reconcile()`]) | inotify, one watch per folder, the folders past the limit reconciled every few minutes |
//!
//! Whatever the system, a catch-up that cannot use what the system
//! recorded (the journal was recreated or its records discarded, FSEvents'
//! history was purged or the volume changed) asks for a reconciling walk,
//! and an overflow of live changes asks for one of the folder it concerns.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::format::EntryKind;
use super::journal::JournalCursor;
use super::scope::Scope;
use super::store::{Change, FileIndex};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod ntfs;

/// How the index was last brought up to date at start, for the status and
/// for tests to assert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaughtUpBy {
    /// From the NTFS change journal (Windows).
    Journal,
    /// From FSEvents' history (macOS).
    EventHistory,
    /// By a reconciling walk: Linux's catch-up, and every system's when the
    /// system's records could not be used.
    ReconcilingWalk,
    /// By a full walk: the first index, or a rebuilt one.
    FullWalk,
}

impl CaughtUpBy {
    /// How it reads on the File search page ("from the change journal").
    pub fn describe(self) -> &'static str {
        match self {
            CaughtUpBy::Journal => "from the change journal",
            CaughtUpBy::EventHistory => "from the file system's event history",
            CaughtUpBy::ReconcilingWalk => "by re-reading the folders that changed",
            CaughtUpBy::FullWalk => "by indexing every folder",
        }
    }
}

/// What a catch-up at start found.
#[derive(Debug)]
pub enum Caught {
    /// What changed since the saved cursors.
    Changes {
        /// Applied first, in order.
        changes: Vec<Change>,
        /// Folders renamed or moved into the scope: walked whole.
        walk: Vec<PathBuf>,
        /// Folders (roots, when a volume's records are gone) to reconcile.
        reconcile: Vec<PathBuf>,
        /// Folders whose ignore rules may have changed in ways the changes
        /// do not name (an ignore file deleted, one read again), to
        /// re-check with everything under them (#186).
        recheck: Vec<PathBuf>,
        /// The cursors to keep once the changes are in the index.
        cursors: Vec<JournalCursor>,
        how: CaughtUpBy,
        /// Why some folders are reconciled rather than caught up.
        note: Option<String>,
    },
    /// Nothing the system recorded can be used: every root is reconciled.
    Reconcile(String),
}

/// A change a source reports while Pane runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changed {
    /// These entries were created, changed, renamed or deleted: each is
    /// looked at now.
    Paths(Vec<PathBuf>),
    /// Anything under this folder may have changed (an overflow of the
    /// system's buffer, FSEvents asking for a rescan): it is reconciled.
    Rescan(PathBuf),
    /// The history replayed at start has all been sent (macOS); the
    /// cursors as of then.
    HistoryDone(Vec<JournalCursor>),
    /// The source's cursors as of the changes sent before this, to keep
    /// with the index (macOS's event ids).
    Cursors(Vec<JournalCursor>),
    /// Folders the source cannot watch (Linux's watch limit): reconciled
    /// every few minutes, and counted on the File search page.
    Unwatched(Vec<PathBuf>),
}

/// Where a source sends what it reports: the indexer's coordinator.
#[derive(Clone)]
pub struct Sink {
    pub(crate) sender: Sender<super::indexer::Message>,
    /// Counts what was sent and not yet handled, for
    /// `Indexer::wait_until_settled`.
    pub(crate) counter: Option<Weak<dyn super::indexer::Counter>>,
}

/// The coordinator has stopped: nothing more is wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SinkClosed;

impl Sink {
    pub(crate) fn new(
        sender: Sender<super::indexer::Message>,
        counter: Option<Weak<dyn super::indexer::Counter>>,
    ) -> Sink {
        Sink { sender, counter }
    }

    /// Reports `changed` to the coordinator.
    pub fn send(&self, changed: Changed) -> Result<(), SinkClosed> {
        let counter = self.counter.as_ref().and_then(Weak::upgrade);
        if let Some(counter) = &counter {
            counter.sent();
        }
        match self.sender.send(super::indexer::Message::Changed(changed)) {
            Ok(()) => Ok(()),
            Err(_) => {
                if let Some(counter) = &counter {
                    counter.unsent();
                }
                Err(SinkClosed)
            }
        }
    }
}

/// A running watch: dropping it stops it, at once.
pub trait Watching: Send {
    /// Watches these folders too, newly indexed (Linux's per-folder
    /// watches; the other systems watch each root whole).
    fn add_folders(&mut self, folders: &[PathBuf]) {
        let _ = folders;
    }
}

/// Each indexed folder's file id and path, as [`FileIndex::folder_ids`]
/// reads them, for one opening of the index (#187): read from the index
/// only when the catch-up or the watch setup first asks, and shared by
/// both. Reading it visits every entry of every segment, so a start reads
/// it once at most, never once for each; and it is let go once watching
/// has started, so the idle index keeps nothing of it.
pub struct FolderIds<'a> {
    index: &'a FileIndex,
    ids: Option<HashMap<u64, PathBuf>>,
}

impl<'a> FolderIds<'a> {
    pub fn new(index: &'a FileIndex) -> FolderIds<'a> {
        FolderIds { index, ids: None }
    }

    /// The table, read from the index the first time it is asked for. The
    /// NTFS catch-up changes it as the journal's records go (folders
    /// created, renamed, deleted), so that later records resolve.
    pub fn ids(&mut self) -> &mut HashMap<u64, PathBuf> {
        let index = self.index;
        self.ids.get_or_insert_with(|| index.folder_ids())
    }

    /// Brings the table, if it was read, up to `changes`, as they were
    /// applied to the index after it was read: each path changed or
    /// removed leaves it (a folder removed with everything under it), and
    /// each folder put with a file id comes in, in their order.
    pub fn apply(&mut self, changes: &[Change]) {
        let Some(ids) = &mut self.ids else {
            return;
        };
        if changes.is_empty() {
            return;
        }
        // Each path's id, so that a path is found without reading them all.
        let mut by_path: BTreeMap<PathBuf, u64> =
            ids.iter().map(|(&id, path)| (path.clone(), id)).collect();
        ids.retain(|id, path| by_path.get(path.as_path()) == Some(id));
        for change in changes {
            match change {
                Change::Put(entry) => {
                    forget(ids, &mut by_path, &entry.path);
                    let meta = entry.meta;
                    if meta.kind == EntryKind::Folder && meta.file_id != 0 {
                        if let Some(moved) = ids.insert(meta.file_id, entry.path.clone()) {
                            by_path.remove(&moved);
                        }
                        by_path.insert(entry.path.clone(), meta.file_id);
                    }
                }
                Change::Remove(path) => forget(ids, &mut by_path, path),
                Change::RemoveUnder(folder) => {
                    // Below a folder, paths sort right after it.
                    let under: Vec<PathBuf> = by_path
                        .range(folder.clone()..)
                        .take_while(|(path, _)| path.starts_with(folder))
                        .map(|(path, _)| path.clone())
                        .collect();
                    for path in &under {
                        forget(ids, &mut by_path, path);
                    }
                }
            }
        }
    }

    /// The folders in the table, but those `leave_out` says, shallowest
    /// first: what a source that watches each folder is given.
    pub fn folders(&mut self, leave_out: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = self
            .ids()
            .values()
            .filter(|folder| !leave_out(folder.as_path()))
            .cloned()
            .collect();
        folders.sort_by_key(|folder| folder.components().count());
        folders
    }
}

/// Takes `path` out of the folder-id table and its index by path.
fn forget(ids: &mut HashMap<u64, PathBuf>, by_path: &mut BTreeMap<PathBuf, u64>, path: &Path) {
    if let Some(id) = by_path.remove(path) {
        ids.remove(&id);
    }
}

/// One system's way of learning what changed (see the module docs).
pub trait ChangeSource: Send + Sync + 'static {
    /// The cursors to keep with the index once a full walk is done, taken
    /// before it starts, so that changes made during it are caught up
    /// later.
    fn cursors(&self, scope: &Scope) -> Vec<JournalCursor>;

    /// What changed under the roots since `cursors`, saved with the index
    /// when Pane last ran. Called on the coordinator's thread at
    /// background priority; may read the system's records but should not
    /// walk folders itself, except where that is the catch-up (Linux).
    /// `folders` is the folder-id table the watch setup shares (#187): a
    /// source that resolves records by folder ids (NTFS) reads it there,
    /// never from the index itself.
    fn catch_up(
        &self,
        index: &FileIndex,
        scope: &Scope,
        cursors: &[JournalCursor],
        folders: &mut FolderIds<'_>,
        cancel: &AtomicBool,
    ) -> Caught;

    /// Whether [`ChangeSource::watch`] watches each indexed folder, and so
    /// must be given them (Linux's inotify). The others watch each root
    /// whole and are given none, so that the folders are not read for
    /// them (#187).
    fn watches_folders(&self) -> bool {
        false
    }

    /// Starts reporting live changes under the roots to `sink`, from
    /// `cursors` where the system replays history (macOS); `folders` are
    /// the folders indexed now, shallowest first, when the source
    /// [watches each](ChangeSource::watches_folders), and none otherwise.
    fn watch(
        &self,
        scope: &Scope,
        cursors: &[JournalCursor],
        folders: Vec<PathBuf>,
        sink: Sink,
    ) -> Result<Box<dyn Watching>, String>;
}

/// This system's change source.
pub fn native() -> std::sync::Arc<dyn ChangeSource> {
    #[cfg(target_os = "linux")]
    {
        std::sync::Arc::new(linux::Inotify)
    }
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos::FsEvents)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        std::sync::Arc::new(ntfs::Journal)
    }
}

/// Live changes through the `notify` crate's recommended watcher (on
/// Windows `ReadDirectoryChangesW`), each root watched whole. A lost
/// event or an error asks for the root to be reconciled.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) struct NotifyWatch {
    _watcher: notify::RecommendedWatcher,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Watching for NotifyWatch {}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl NotifyWatch {
    pub(crate) fn start(roots: &[PathBuf], sink: Sink) -> Result<NotifyWatch, String> {
        use notify::{RecursiveMode, Watcher};
        let all = roots.to_vec();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let _ = match event {
                    Ok(event) if event.need_rescan() => {
                        let root = event
                            .paths
                            .first()
                            .and_then(|path| all.iter().find(|root| path.starts_with(root)))
                            .cloned();
                        match root {
                            Some(root) => sink.send(Changed::Rescan(root)),
                            None => all
                                .iter()
                                .try_for_each(|root| sink.send(Changed::Rescan(root.clone()))),
                        }
                    }
                    Ok(event) if event.paths.is_empty() => Ok(()),
                    Ok(event) => sink.send(Changed::Paths(event.paths)),
                    // An error of the watcher (its buffer overflowed, a root
                    // went away): everything is reconciled.
                    Err(_) => all
                        .iter()
                        .try_for_each(|root| sink.send(Changed::Rescan(root.clone()))),
                };
            })
            .map_err(|error| format!("Pane could not watch for changes: {error}"))?;
        for root in roots {
            // A root that is missing (an unplugged drive) is not watched; it
            // is reconciled when Pane next starts.
            if root.is_dir() {
                watcher
                    .watch(root, RecursiveMode::Recursive)
                    .map_err(|error| {
                        format!(
                            "Pane could not watch {} for changes: {error}",
                            root.display()
                        )
                    })?;
            }
        }
        Ok(NotifyWatch { _watcher: watcher })
    }
}
