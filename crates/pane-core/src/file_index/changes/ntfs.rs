//! Windows: catch-up from the NTFS change journal of each root's volume,
//! read without administrator rights from the cursor saved with the index
//! (#174's [`read_journal`]), and live changes from
//! `ReadDirectoryChangesW` on each root. A volume whose records cannot be
//! used (the journal was recreated or its records discarded, there are
//! more than a walk would cost, the volume keeps none, the system refused)
//! has its roots reconciled instead.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use super::{Caught, CaughtUpBy, ChangeSource, NotifyWatch, Sink, Watching};
use crate::file_index::journal::{JournalCursor, JournalRead, Names, read_journal, resolve};
use crate::file_index::scope::Scope;
use crate::file_index::store::FileIndex;
use crate::file_index::{catch_up_changes, missing_from};

/// Records past which reading the journal costs more than reconciling.
const MAX_RECORDS: usize = 1_000_000;

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
        for root in &scope.rules().roots {
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
        _cancel: &AtomicBool,
    ) -> Caught {
        let mut changes = Vec::new();
        let mut walk = Vec::new();
        let mut reconcile: Vec<PathBuf> = Vec::new();
        let mut kept: Vec<JournalCursor> = Vec::new();
        let mut note = None;
        let mut folders = index.folder_ids();
        for root in &scope.rules().roots {
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
                    // no names: entries are named by their ids.
                    let mut names = Names::for_volume_of(root);
                    let caught = resolve(&records, &mut folders, &mut |id| names.name(id));
                    let (found, folders_to_walk) = catch_up_changes(scope, &caught);
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
                    for same in &scope.rules().roots {
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
        if !reconcile.is_empty() && reconcile.len() == scope.rules().roots.len() {
            return Caught::Reconcile(
                note.unwrap_or_else(|| "no change journal could be read".into()),
            );
        }
        Caught::Changes {
            changes,
            walk,
            reconcile,
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
        Ok(Box::new(NotifyWatch::start(&scope.rules().roots, sink)?))
    }
}
