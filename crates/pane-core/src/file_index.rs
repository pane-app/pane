//! The file index (#126, ADR 0034): the host's index of the names of the
//! files and folders under the index scope, kept in Pane's cache folder.
//! The measured first slice (#174) built the walker, the index engine and
//! the unprivileged change-journal catch-up; #175 adds the change sources,
//! the reconciling walk, the coordinator that keeps the index current while
//! a package uses it, and the host interface `pane:extension/file-index`.
//! See docs/files.md.
//!
//! - `scope`: the roots and the rules ([`ScopeRules`], [`Scope`]).
//! - `walker`: the parallel walk at background priority ([`walk`]).
//! - `store`: the name index ([`FileIndex`]): memory table, write-ahead
//!   log, segments with `fst` term dictionaries, tombstones, compaction.
//! - `journal`: the NTFS change journal ([`read_journal`], [`resolve`]).
//! - `reconcile`: the reconciling walk ([`reconcile()`]).
//! - `changes`: each system's change source ([`ChangeSource`]).
//! - `indexer`: the coordinator, the ids it gives and its checks
//!   ([`Indexer`]).
//! - `category`: the kinds of file, by extension ([`Category`]).
//! - `space`: the free-space probe behind the low-disk valve
//!   ([`free_space`]).
//! - `power`: whether the computer is awake, so indexing pauses while it
//!   sleeps ([`Awake`]).
//! - `privacy`: macOS's privacy refusals, told from the system's answer.
//! - `volume`: what kind of volume holds a folder, so that network shares
//!   and removable drives are left out unless the user includes them
//!   ([`VolumeKind`]).
//! - `wording`: counts, sizes and spans for people ([`size_words`]).
//! - `host`: `pane:extension/file-index` for guests.

mod category;
mod changes;
mod format;
mod host;
mod indexer;
mod journal;
mod power;
mod priority;
mod privacy;
mod reconcile;
mod scope;
mod segment;
mod space;
mod store;
mod terms;
mod text;
mod volume;
mod wal;
mod walker;
mod wording;

use std::path::{Path, PathBuf};

pub use format::{EntryKind, FORMAT_VERSION, Meta};
pub use journal::{
    CatchUp, JournalCursor, JournalRead, JournalRecord, Names, REASON_BASIC_INFO_CHANGE,
    REASON_CLOSE, REASON_DATA_EXTEND, REASON_DATA_OVERWRITE, REASON_DATA_TRUNCATION,
    REASON_FILE_CREATE, REASON_FILE_DELETE, REASON_HARD_LINK_CHANGE, REASON_RENAME_NEW_NAME,
    REASON_RENAME_OLD_NAME, REASON_REPARSE_POINT_CHANGE, read_journal, resolve,
};
pub use priority::lower_current_thread;
pub use scope::{Admitted, Excluded, Scope, ScopeRules};
pub use store::{
    Bulk, Change, FileIndex, Hit, IndexError, IndexRecord, IndexStats, Opened, PreparedBatch, Query,
};
pub use walker::{HUNG_AFTER, MAX_ENTRIES, WalkOptions, WalkReport, walk, walk_folders};

pub use category::Category;
pub use changes::{
    Caught, CaughtUpBy, ChangeSource, Changed, FolderIds, Sink, SinkClosed, Watching,
    native as native_changes,
};
pub use indexer::{
    Checked, FIRST_WALK_DELAY, Found, INDEX_DIR, IndexState, IndexStatus, Indexer, IndexerConfig,
    KnownEntry, MAX_RESULTS, Problem, ProblemKind, RULES_FILE, SearchOptions, Sort, UserRules,
    Valves, describe,
};
pub use power::{Awake, SystemAwake};
pub use reconcile::{Reconciled, reconcile};
pub use space::{FREE_SPACE_FLOOR, FreeSpace, free_space};
pub use volume::{VolumeKind, VolumeKinds, volume_kind};
pub use wording::{count_words, size_words};

/// An entry to index: a path and what the index keeps of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub meta: Meta,
}

impl Entry {
    /// The entry at `path` as it is now, not following a link.
    pub fn read(path: &Path) -> std::io::Result<Entry> {
        Ok(Entry {
            path: path.to_path_buf(),
            meta: walker::meta_at(path)?,
        })
    }
}

/// The changes a catch-up asks for, in order (removals first), and the
/// folders to walk again with [`walk_folders`]: each path it touched is
/// looked at now, indexed if it exists and `scope` admits it (told with
/// what the scope keeps of the folders, [`Scope::admits_kept`]), removed
/// otherwise. A touched ignore file, repository or cache tag is among the
/// changes, so that the coordinator re-checks its folder (#186).
pub fn catch_up_changes(scope: &Scope, catch_up: &CatchUp) -> (Vec<Change>, Vec<PathBuf>) {
    let mut changes = Vec::new();
    for folder in &catch_up.removed_folders {
        changes.push(Change::RemoveUnder(folder.clone()));
    }
    for path in &catch_up.removed {
        if !catch_up.removed_folders.contains(path) {
            changes.push(Change::Remove(path.clone()));
        }
    }
    for path in &catch_up.touched {
        match Entry::read(path) {
            Ok(entry) if scope.admits_kept(path, entry.meta.kind == EntryKind::Folder) => {
                changes.push(Change::Put(entry));
            }
            _ => changes.push(Change::Remove(path.clone())),
        }
    }
    (changes, catch_up.walk.clone())
}

/// The changes that take out what the index holds directly in each of
/// `folders` and the disk no longer does: how a catch-up learns of entries
/// gone that its records do not name ([`CatchUp::listed`]). A folder that
/// cannot be listed is left as it is.
pub fn missing_from(index: &FileIndex, folders: &[PathBuf]) -> Vec<Change> {
    let mut changes = Vec::new();
    for folder in folders {
        let Ok(listing) = std::fs::read_dir(folder) else {
            continue;
        };
        let there: std::collections::HashSet<std::ffi::OsString> = listing
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        for (path, meta) in index.children(folder) {
            if path.file_name().is_some_and(|name| !there.contains(name)) {
                changes.push(match meta.kind {
                    EntryKind::Folder => Change::RemoveUnder(path),
                    EntryKind::File | EntryKind::Link => Change::Remove(path),
                });
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_catch_up_indexes_what_exists_and_is_admitted_and_removes_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::write(home.join("Documents").join("plan.txt"), b"x").unwrap();
        fs::write(home.join("Documents").join(".hidden.txt"), b"x").unwrap();
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let catch_up = CatchUp {
            removed: vec![home.join("old.txt"), home.join("Gone")],
            removed_folders: vec![home.join("Gone")],
            touched: vec![
                home.join("Documents").join("plan.txt"),
                home.join("Documents").join(".hidden.txt"),
                home.join("Documents").join("vanished.txt"),
            ],
            walk: vec![home.join("Documents")],
            listed: Vec::new(),
            unresolved: 0,
            gone: Vec::new(),
            unresolved_folders: Vec::new(),
        };
        let (changes, walk) = catch_up_changes(&scope, &catch_up);
        assert_eq!(walk, [home.join("Documents")]);
        assert_eq!(changes[0], Change::RemoveUnder(home.join("Gone")));
        assert_eq!(changes[1], Change::Remove(home.join("old.txt")));
        assert!(
            matches!(&changes[2], Change::Put(entry) if entry.path == home.join("Documents").join("plan.txt"))
        );
        assert_eq!(
            changes[3],
            Change::Remove(home.join("Documents").join(".hidden.txt"))
        );
        assert_eq!(
            changes[4],
            Change::Remove(home.join("Documents").join("vanished.txt"))
        );
        assert_eq!(changes.len(), 5);
    }
}
