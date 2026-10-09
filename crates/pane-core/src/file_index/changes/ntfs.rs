//! Windows: catch-up from the NTFS change journal of each root's volume,
//! read without administrator rights from the cursor saved with the index
//! (#174's [`read_journal`]), and live changes from
//! `ReadDirectoryChangesW` on each root. A volume whose records cannot be
//! used (the journal was recreated or its records discarded, there are
//! more than a walk would cost, the volume keeps none, the system refused)
//! has its roots reconciled instead.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use super::{Caught, CaughtUpBy, ChangeSource, FolderIds, NotifyWatch, Sink, Watching};
use crate::file_index::journal::{
    CatchUp, JournalCursor, JournalRead, Names, gone_unheld, read_journal, repository_of_info,
    resolve,
};
use crate::file_index::scope::Scope;
use crate::file_index::store::{Change, FileIndex};
use crate::file_index::{EntryKind, catch_up_changes, missing_from};

/// Records past which reading the journal costs more than reconciling.
const MAX_RECORDS: usize = 1_000_000;

/// The most folders the index does not hold that one catch-up looks up by
/// id, for a repository's `.git/info` (#186); past it, an `exclude` changed
/// while Pane was not running in a folder not looked up is not seen.
const MAX_FOLDER_LOOKUPS: usize = 4_096;

/// The folders whose ignore rules `caught`'s records may have changed
/// without naming the file that sets them, to re-check (#186): a folder an
/// entry the index did not hold went from ([`gone_unheld`]: a hidden
/// `.gitignore` or `.git` deleted), and a repository whose `.git/info` a
/// record was in (its `exclude`; `.git` is never indexed, so such records
/// resolve to nothing).
fn rules_changed(
    index: &FileIndex,
    scope: &Scope,
    caught: &CatchUp,
    names: &mut Names,
) -> Vec<PathBuf> {
    let mut folders = gone_unheld(
        caught,
        &mut |folder: &std::path::Path| {
            index
                .children(folder)
                .into_iter()
                .map(|(_, meta)| meta.file_id)
                .collect::<HashSet<u64>>()
        },
        &mut |id| names.name(id).is_some(),
    );
    for &id in caught.unresolved_folders.iter().take(MAX_FOLDER_LOOKUPS) {
        if let Some(path) = names.path(id)
            && let Some(repository) = repository_of_info(&path)
            && scope.root_of(repository).is_some()
        {
            folders.push(repository.to_path_buf());
        }
    }
    folders
}

/// The folders among `found` that the index did not hold as folders: made,
/// moved in or no longer hidden, so that what they hold is not indexed
/// either; walked whole.
fn newly_admitted(index: &FileIndex, found: &[Change]) -> Vec<PathBuf> {
    found
        .iter()
        .filter_map(|change| match change {
            Change::Put(entry)
                if entry.meta.kind == EntryKind::Folder
                    && index
                        .get(&entry.path)
                        .is_none_or(|meta| meta.kind != EntryKind::Folder) =>
            {
                Some(entry.path.clone())
            }
            _ => None,
        })
        .collect()
}

pub(crate) struct Journal;

/// Where the journal of `root`'s volume is now: its cursor, naming the
/// volume.
fn current(root: &std::path::Path) -> Option<JournalCursor> {
    match read_journal(root, None, 0) {
        JournalRead::Records { cursor, .. } => Some(cursor),
        _ => None,
    }
}

/// Why a journal read that found no records cannot be used.
fn why(read: &JournalRead) -> String {
    match read {
        JournalRead::Records { .. } => String::new(),
        JournalRead::Recreated => "the volume's change journal was recreated".into(),
        JournalRead::Discarded => {
            "the change journal no longer holds the changes since Pane last ran".into()
        }
        JournalRead::TooMany => "too much changed since Pane last ran".into(),
        JournalRead::NoJournal => "the volume keeps no change journal".into(),
        JournalRead::Refused(error) => {
            format!("Windows refused to read the change journal: {error}")
        }
    }
}

impl ChangeSource for Journal {
    fn cursors(&self, scope: &Scope) -> Vec<JournalCursor> {
        let mut cursors: Vec<JournalCursor> = Vec::new();
        for root in &scope.kept_roots() {
            if let Some(cursor) = current(root)
                && !cursors.iter().any(|known| known.volume == cursor.volume)
            {
                cursors.push(cursor);
            }
        }
        cursors
    }

    fn catch_up(
        &self,
        index: &FileIndex,
        scope: &Scope,
        cursors: &[JournalCursor],
        folders: &mut FolderIds<'_>,
        _cancel: &AtomicBool,
    ) -> Caught {
        let mut changes = Vec::new();
        let mut walk = Vec::new();
        let mut recheck = Vec::new();
        let mut reconcile: Vec<PathBuf> = Vec::new();
        let mut kept: Vec<JournalCursor> = Vec::new();
        let mut note = None;
        // A root the rules leave out (a network share or a removable drive)
        // is not caught up: it holds nothing in the index.
        let roots = scope.kept_roots();
        for root in &roots {
            // The volume the root is on now, and where Pane left its journal.
            let Some(now) = current(root) else {
                reconcile.push(root.clone());
                note.get_or_insert_with(|| format!("{} keeps no change journal", root.display()));
                continue;
            };
            if kept.iter().any(|cursor| cursor.volume == now.volume) {
                // Another root on the same volume read it already.
                continue;
            }
            let saved = cursors.iter().find(|cursor| cursor.volume == now.volume);
            let read = match saved {
                Some(saved) => read_journal(root, Some(saved), MAX_RECORDS),
                // A root added since: nothing of its volume was read yet.
                None => JournalRead::Discarded,
            };
            match read {
                JournalRead::Records { records, cursor } => {
                    // Read without administrator rights, the records carry
                    // no names: entries are named by their ids. The folder
                    // ids are read from the index here, the first time
                    // records need them, and once for every volume.
                    let mut names = Names::for_volume_of(root);
                    let caught = resolve(&records, folders.ids(), &mut |id| names.name(id));
                    let (found, folders_to_walk) = catch_up_changes(scope, &caught);
                    recheck.extend(rules_changed(index, scope, &caught, &mut names));
                    walk.extend(newly_admitted(index, &found));
                    changes.extend(missing_from(index, &caught.listed));
                    changes.extend(found);
                    walk.extend(folders_to_walk);
                    kept.push(cursor);
                }
                other => {
                    if saved.is_some() {
                        note.get_or_insert_with(|| why(&other));
                    }
                    // Every root on this volume is reconciled.
                    for same in &roots {
                        if current(same).is_some_and(|cursor| cursor.volume == now.volume) {
                            reconcile.push(same.clone());
                        }
                    }
                    kept.push(now);
                }
            }
        }
        reconcile.sort();
        reconcile.dedup();
        walk.sort();
        walk.dedup();
        if !reconcile.is_empty() && reconcile.len() == roots.len() {
            return Caught::Reconcile(
                note.unwrap_or_else(|| "no change journal could be read".into()),
            );
        }
        Caught::Changes {
            changes,
            walk,
            reconcile,
            recheck,
            cursors: kept,
            how: CaughtUpBy::Journal,
            note,
        }
    }

    fn watch(
        &self,
        scope: &Scope,
        _cursors: &[JournalCursor],
        _folders: Vec<PathBuf>,
        sink: Sink,
    ) -> Result<Box<dyn Watching>, String> {
        // A network share is reconciled now and then instead.
        Ok(Box::new(NotifyWatch::start(&scope.watched_roots(), sink)?))
    }
}
