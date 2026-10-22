//! The reconciling walk (#126, "Catching up and watching, per system"):
//! the index's own folders are compared with the disk, and only a folder
//! whose modified time changed since it was indexed is read again (a
//! folder's time changes when an entry is added, removed or renamed in it).
//! It is Linux's catch-up at start, the catch-up anywhere a system's change
//! records are gone or refused, what a watcher's overflow or a "rescan this
//! folder" asks for, and Linux's fallback for the folders it cannot watch.
//!
//! A folder new to the index is walked whole, with the same rules as a
//! first walk; a folder gone takes everything under it out. A root that is
//! missing (an unplugged drive the user added) keeps its entries: they are
//! hidden from searches while it is away, and found again when it is back.
//!
//! A folder read again that holds an ignore file or a repository is named
//! for a re-check ([`Reconciled::recheck`]): the rules below it may have
//! changed with it, which the folders below it do not show by their times.
//! A re-check ([`recheck`], #186) is the same comparison reading every
//! folder whatever its time, a few at a time, so that the coordinator
//! handles live changes between them.
//!
//! Limits: a file changed in place does not change its folder's time, so
//! its size and time are refreshed by live changes, not by this walk; and
//! a change within the same second as the folder was indexed is not seen.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::format::{EntryKind, Meta};
use super::scope::{Admitted, CACHE_TAG, Candidate, Scope};
use super::store::{Change, FileIndex};
use super::walker::{self, WalkOptions};
use super::{Entry, walk_folders};

/// What a reconciling walk found to change.
#[derive(Debug, Default)]
pub struct Reconciled {
    /// The changes, in the order to apply them.
    pub changes: Vec<Change>,
    /// Folders read again from the disk because their time changed.
    pub folders_read: u64,
    /// Folders compared with the index.
    pub folders_compared: u64,
    /// Folders that did not answer within [`WalkOptions::hung_after`]:
    /// skipped for this walk, their entries kept as they were (#176). Every
    /// one is named, however many.
    pub hung_folders: Vec<PathBuf>,
    /// Folders read again because their time changed that hold an ignore
    /// file or a repository (`.gitignore`, `.ignore`, `.git`), as they do
    /// after one was added or replaced: to re-check whole, since the rules
    /// below them may have changed with it (#186). None from [`recheck`].
    pub recheck: Vec<PathBuf>,
}

/// Compares each of `folders` (folders under the roots of `scope`, or the
/// roots themselves) and everything the index holds under it with the
/// disk, and answers the changes that bring the index up to date. Stops
/// early, answering what it found so far, once `cancel` is set.
pub fn reconcile(
    index: &FileIndex,
    scope: &Scope,
    folders: &[PathBuf],
    options: &WalkOptions,
    cancel: &AtomicBool,
) -> Reconciled {
    let mut pending: Vec<PathBuf> = folders.to_vec();
    compare(
        index,
        scope,
        &mut pending,
        options,
        cancel,
        false,
        usize::MAX,
    )
}

/// Re-checks the folders `pending`, whose ignore rules may have changed
/// (#186), as [`reconcile`] compares them but reading every folder under
/// them again whatever its time, under the rules as they are now (no ignore
/// file read before is used): what the rules leave out now goes, what they
/// admit now comes in. Compares at most `budget` folders and leaves those
/// still to compare in `pending`, so that the coordinator handles the
/// changes reported meanwhile before it goes on.
pub(crate) fn recheck(
    index: &FileIndex,
    scope: &Scope,
    pending: &mut Vec<PathBuf>,
    options: &WalkOptions,
    cancel: &AtomicBool,
    budget: usize,
) -> Reconciled {
    compare(index, scope, pending, options, cancel, true, budget)
}

/// [`reconcile`] and [`recheck`]: compares the folders in `pending`, at
/// most `budget` of them, reading a folder again only when its time
/// changed unless `read_all`.
fn compare(
    index: &FileIndex,
    scope: &Scope,
    pending: &mut Vec<PathBuf>,
    options: &WalkOptions,
    cancel: &AtomicBool,
    read_all: bool,
    budget: usize,
) -> Reconciled {
    let mut known = Admitted::default();
    let mut reconciled = Reconciled::default();
    // The helper that lists folders, so that one that hangs holds up only
    // itself.
    let mut lister = None;
    // Folders new to the index, walked whole at the end.
    let mut new_folders: Vec<PathBuf> = Vec::new();
    let mut compared = 0;
    while compared < budget && !cancel.load(Ordering::Relaxed) {
        let Some(folder) = pending.pop() else {
            break;
        };
        compared += 1;
        reconciled.folders_compared += 1;
        let is_root = scope.rules().roots.contains(&folder);
        let indexed = index.get(&folder);
        if is_root
            && scope
                .root_of(&folder)
                .is_some_and(|root| scope.leaves_out_root(root))
        {
            // On a network share or a removable drive the rules leave out
            // (not even looked at): nothing of it stays in the index.
            if indexed.is_some() {
                reconciled.changes.push(Change::RemoveUnder(folder.clone()));
            }
            continue;
        }
        let on_disk = walker::meta_at(&folder);
        let on_disk = match on_disk {
            Ok(meta) => meta,
            Err(_) if is_root => continue,
            Err(_) => {
                if indexed.is_some() {
                    reconciled.changes.push(Change::RemoveUnder(folder.clone()));
                }
                continue;
            }
        };
        if on_disk.kind != EntryKind::Folder {
            // Replaced by a file or a link.
            if indexed.is_some_and(|meta| meta.kind == EntryKind::Folder) {
                reconciled.changes.push(Change::RemoveUnder(folder.clone()));
            }
            if scope.admits(&folder, false, &mut known) {
                reconciled.changes.push(Change::Put(Entry {
                    path: folder.clone(),
                    meta: on_disk,
                }));
            }
            continue;
        }
        if !is_root && !scope.admits(&folder, true, &mut known) {
            if indexed.is_some() {
                reconciled.changes.push(Change::RemoveUnder(folder.clone()));
            }
            continue;
        }
        let Some(indexed) = indexed.filter(|meta| meta.kind == EntryKind::Folder) else {
            new_folders.push(folder);
            continue;
        };
        let children = index.children(&folder);
        if !read_all && indexed.modified == on_disk.modified && on_disk.modified != 0 {
            // Unchanged: only its folders are compared in turn.
            pending.extend(
                children
                    .into_iter()
                    .filter(|(_, meta)| meta.kind == EntryKind::Folder)
                    .map(|(path, _)| path),
            );
            continue;
        }
        reconciled.folders_read += 1;
        let listing = match walker::list_within(&folder, on_disk.volume, options, &mut lister) {
            Ok(listing) => Some(listing),
            Err(walker::Unlisted::Hung) => {
                // Skipped for this walk: what the index holds of it stays.
                reconciled.hung_folders.push(folder.clone());
                continue;
            }
            Err(walker::Unlisted::Unreadable | walker::Unlisted::Refused) => None,
        };
        if !read_all && scope.rules().use_ignore_files && listing.as_deref().is_some_and(sets_rules)
        {
            reconciled.recheck.push(folder.clone());
        }
        read_again(
            scope,
            &folder,
            on_disk,
            listing,
            children,
            &mut known,
            &mut reconciled.changes,
            pending,
            &mut new_folders,
        );
    }
    if !new_folders.is_empty() && !cancel.load(Ordering::Relaxed) {
        let walked = Mutex::new(Vec::new());
        walk_folders(scope, &new_folders, options, cancel, &|entries: Vec<
            Entry,
        >| {
            walked
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend(entries);
        });
        reconciled.changes.extend(
            walked
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .into_iter()
                .map(Change::Put),
        );
    }
    reconciled
}

/// Whether `listing` holds what sets ignore rules for the folder and those
/// under it: a `.gitignore`, an `.ignore` or a repository (`.git`).
fn sets_rules(listing: &[walker::Listed]) -> bool {
    listing.iter().any(|entry| {
        entry.name == *".gitignore" || entry.name == *".ignore" || entry.name == *".git"
    })
}

/// Compares `listing`, what the disk holds in `folder` whose time changed
/// (`None` when it could not be read), with `children`, what the index
/// holds directly in it: entries gone or no longer admitted are taken out,
/// new or changed ones put in, new folders walked whole and the folders it
/// still holds compared in turn.
#[allow(clippy::too_many_arguments)]
fn read_again(
    scope: &Scope,
    folder: &Path,
    on_disk: Meta,
    listing: Option<Vec<walker::Listed>>,
    children: Vec<(PathBuf, Meta)>,
    known: &mut Admitted,
    changes: &mut Vec<Change>,
    pending: &mut Vec<PathBuf>,
    new_folders: &mut Vec<PathBuf>,
) {
    let gone = |path: &Path, meta: &Meta| match meta.kind {
        EntryKind::Folder => Change::RemoveUnder(path.to_path_buf()),
        EntryKind::File | EntryKind::Link => Change::Remove(path.to_path_buf()),
    };
    let listing = match listing {
        Some(listing) => listing,
        None => {
            // Unreadable now: indexed, its contents not, as a walk does.
            changes.push(Change::Put(Entry {
                path: folder.to_path_buf(),
                meta: on_disk,
            }));
            return;
        }
    };
    let Some(root) = scope.root_of(folder) else {
        return;
    };
    let context = scope.context_at(root, folder, known);
    let tagged = listing.iter().any(|entry| entry.name == *CACHE_TAG);
    let Some(context) = context.filter(|_| !tagged) else {
        // A cache now, or no longer admitted: out with everything in it.
        changes.push(Change::RemoveUnder(folder.to_path_buf()));
        return;
    };
    let mut indexed: std::collections::HashMap<PathBuf, Meta> = children.into_iter().collect();
    for listed in listing {
        let path = folder.join(&listed.name);
        let is_dir = listed.kind == EntryKind::Folder;
        let candidate = Candidate {
            path: &path,
            name: &listed.name,
            is_dir,
            hidden_attribute: listed.hidden_attribute,
        };
        let before = indexed.remove(&path);
        if scope.excludes(&context, &candidate).is_some()
            || (is_dir && scope.leaves_out_mount(&path, listed.volume, on_disk.volume))
        {
            if let Some(before) = before {
                changes.push(gone(&path, &before));
            }
            continue;
        }
        let meta = Meta {
            kind: listed.kind,
            size: listed.size,
            modified: listed.modified,
            file_id: listed.file_id,
            volume: listed.volume,
        };
        match before {
            Some(before) if before.kind != meta.kind => {
                changes.push(gone(&path, &before));
                if is_dir {
                    new_folders.push(path);
                } else {
                    changes.push(Change::Put(Entry { path, meta }));
                }
            }
            Some(_) if is_dir => pending.push(path),
            Some(before) if before == meta => {}
            Some(_) => changes.push(Change::Put(Entry { path, meta })),
            None if is_dir => new_folders.push(path),
            None => changes.push(Change::Put(Entry { path, meta })),
        }
    }
    // What the index holds that the folder no longer does.
    for (path, meta) in indexed {
        changes.push(gone(&path, &meta));
    }
    // The folder's own entry, with its new time, so it is not read again.
    changes.push(Change::Put(Entry {
        path: folder.to_path_buf(),
        meta: on_disk,
    }));
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::file_index::{Query, ScopeRules, walk};

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        index: FileIndex,
        scope: Scope,
    }

    fn names(index: &FileIndex, text: &str) -> Vec<String> {
        let mut names: Vec<String> = index
            .search(&Query {
                text,
                limit: 50,
                offset: 0,
                kind: None,
            })
            .into_iter()
            .map(|hit| hit.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn indexed() -> Fixture {
        indexed_with(&[
            "Documents/plan.txt",
            "Documents/old.txt",
            "Music/song.mp3",
            "Gone/deep/lost.txt",
        ])
    }

    /// A home folder holding `files`, indexed by a walk under the default
    /// rules.
    fn indexed_with(files: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        for file in files {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        }
        let (index, _) =
            FileIndex::open(&dir.path().join("index"), std::slice::from_ref(&home)).unwrap();
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let entries = Mutex::new(Vec::new());
        walk(
            &scope,
            &WalkOptions {
                background: false,
                ..WalkOptions::default()
            },
            &AtomicBool::new(false),
            &|batch| entries.lock().unwrap().extend(batch),
        );
        let changes: Vec<Change> = entries
            .into_inner()
            .unwrap()
            .into_iter()
            .map(Change::Put)
            .collect();
        index.apply(&changes).unwrap();
        Fixture {
            _dir: dir,
            home,
            index,
            scope,
        }
    }

    /// Sets the modified time of `folder` a minute later than it was, so
    /// that the walk sees it changed whatever the clock's resolution.
    fn touch(folder: &Path) {
        let time =
            fs::metadata(folder).unwrap().modified().unwrap() + std::time::Duration::from_secs(60);
        let mut options = fs::OpenOptions::new();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_WRITE_ATTRIBUTES, and FILE_FLAG_BACKUP_SEMANTICS to open
            // a folder.
            options.access_mode(0x100).custom_flags(0x0200_0000);
        }
        #[cfg(not(windows))]
        options.read(true);
        options
            .open(folder)
            .and_then(|file| file.set_modified(time))
            .expect("the folder's time is set");
    }

    #[test]
    fn only_changed_folders_are_read_and_the_index_matches_the_disk_after() {
        let fixture = indexed();
        let home = &fixture.home;
        // While Pane was not running: a file created, one deleted, a
        // folder created with a file, a folder deleted.
        fs::write(home.join("Documents/new plan.txt"), b"x").unwrap();
        fs::remove_file(home.join("Documents/old.txt")).unwrap();
        fs::create_dir_all(home.join("Projects/pane")).unwrap();
        fs::write(home.join("Projects/pane/notes.md"), b"x").unwrap();
        fs::remove_dir_all(home.join("Gone")).unwrap();
        touch(&home.join("Documents"));
        touch(home);

        let reconciled = reconcile(
            &fixture.index,
            &fixture.scope,
            std::slice::from_ref(home),
            &WalkOptions {
                background: false,
                ..WalkOptions::default()
            },
            &AtomicBool::new(false),
        );
        // The home folder and Documents changed; Music did not.
        assert_eq!(reconciled.folders_read, 2);
        fixture.index.apply(&reconciled.changes).unwrap();
        assert_eq!(names(&fixture.index, "plan"), ["new plan.txt", "plan.txt"]);
        assert!(names(&fixture.index, "old").is_empty());
        assert_eq!(names(&fixture.index, "notes"), ["notes.md"]);
        assert!(names(&fixture.index, "lost").is_empty());
        assert_eq!(names(&fixture.index, "song"), ["song.mp3"]);

        // Nothing changed since: nothing is read, nothing changes.
        let again = reconcile(
            &fixture.index,
            &fixture.scope,
            std::slice::from_ref(home),
            &WalkOptions::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(again.folders_read, 0);
        assert!(again.changes.is_empty(), "{:?}", again.changes);
    }

    #[test]
    fn a_missing_root_keeps_its_entries() {
        let fixture = indexed();
        let away = fixture.home.with_file_name("away");
        fs::rename(&fixture.home, &away).unwrap();
        let reconciled = reconcile(
            &fixture.index,
            &fixture.scope,
            std::slice::from_ref(&fixture.home),
            &WalkOptions::default(),
            &AtomicBool::new(false),
        );
        assert!(reconciled.changes.is_empty());
    }

    fn options() -> WalkOptions {
        WalkOptions {
            background: false,
            ..WalkOptions::default()
        }
    }

    /// A `.gitignore` changed in place leaves its folder's time as it was,
    /// so a reconciling walk reads nothing again; a re-check (#186) reads
    /// every folder under it, a few at a time, and applies the rules as
    /// they are now.
    #[test]
    fn a_recheck_reads_every_folder_a_few_at_a_time_under_the_rules_as_they_are_now() {
        let fixture = indexed_with(&[
            "repo/.git/HEAD",
            "repo/.gitignore",
            "repo/src/deep/trace.draft",
            "repo/src/main.rs",
            "Documents/plan.txt",
        ]);
        let home = &fixture.home;
        let repo = home.join("repo");
        assert_eq!(names(&fixture.index, "trace"), ["trace.draft"]);
        fs::write(repo.join(".gitignore"), "*.draft\n").unwrap();

        let unchanged = reconcile(
            &fixture.index,
            &fixture.scope,
            std::slice::from_ref(home),
            &options(),
            &AtomicBool::new(false),
        );
        assert_eq!(unchanged.folders_read, 0);
        assert!(unchanged.changes.is_empty(), "{:?}", unchanged.changes);

        let mut pending = vec![repo.clone()];
        let first = recheck(
            &fixture.index,
            &fixture.scope,
            &mut pending,
            &options(),
            &AtomicBool::new(false),
            1,
        );
        assert_eq!(first.folders_compared, 1, "one folder at a time");
        assert!(first.recheck.is_empty());
        assert!(!pending.is_empty(), "the folders under it are left");
        fixture.index.apply(&first.changes).unwrap();
        while !pending.is_empty() {
            let next = recheck(
                &fixture.index,
                &fixture.scope,
                &mut pending,
                &options(),
                &AtomicBool::new(false),
                1,
            );
            fixture.index.apply(&next.changes).unwrap();
        }
        assert!(names(&fixture.index, "trace").is_empty());
        assert_eq!(names(&fixture.index, "main"), ["main.rs"]);
        assert_eq!(names(&fixture.index, "plan"), ["plan.txt"]);

        // The line taken out again: what it hid comes back.
        fs::write(repo.join(".gitignore"), "").unwrap();
        let mut pending = vec![repo];
        let again = recheck(
            &fixture.index,
            &fixture.scope,
            &mut pending,
            &options(),
            &AtomicBool::new(false),
            usize::MAX,
        );
        assert!(pending.is_empty());
        fixture.index.apply(&again.changes).unwrap();
        assert_eq!(names(&fixture.index, "trace"), ["trace.draft"]);
    }

    /// A folder read again because its time changed names itself for a
    /// re-check when it holds an ignore file or a repository, which may
    /// have been added or replaced with that change (#186).
    #[test]
    fn a_folder_read_again_holding_an_ignore_file_is_named_for_a_recheck() {
        let fixture = indexed_with(&[
            "repo/.git/HEAD",
            "repo/src/trace.draft",
            "Documents/plan.txt",
        ]);
        let home = &fixture.home;
        // While Pane was not running: a .gitignore made in the repository,
        // and a file in Documents.
        fs::write(home.join("repo/.gitignore"), "*.draft\n").unwrap();
        fs::write(home.join("Documents/later.txt"), b"x").unwrap();
        touch(&home.join("repo"));
        touch(&home.join("Documents"));
        let reconciled = reconcile(
            &fixture.index,
            &fixture.scope,
            std::slice::from_ref(home),
            &options(),
            &AtomicBool::new(false),
        );
        assert_eq!(reconciled.recheck, [home.join("repo")]);
        fixture.index.apply(&reconciled.changes).unwrap();
        assert_eq!(names(&fixture.index, "later"), ["later.txt"]);

        let mut pending = reconciled.recheck;
        let rechecked = recheck(
            &fixture.index,
            &fixture.scope,
            &mut pending,
            &options(),
            &AtomicBool::new(false),
            usize::MAX,
        );
        fixture.index.apply(&rechecked.changes).unwrap();
        assert!(names(&fixture.index, "trace").is_empty());
    }

    /// A folder that does not answer keeps what the index holds of it, in a
    /// re-check as in a reconciling walk, and is named however many there
    /// are.
    #[test]
    fn a_recheck_keeps_what_a_folder_that_does_not_answer_holds() {
        let fixture = indexed();
        let music = fixture.home.join("Music");
        walker::STALLED
            .lock()
            .unwrap()
            .push((music.clone(), std::time::Duration::from_secs(2)));
        let mut pending = vec![fixture.home.clone()];
        let rechecked = recheck(
            &fixture.index,
            &fixture.scope,
            &mut pending,
            &WalkOptions {
                background: false,
                hung_after: Some(std::time::Duration::from_millis(100)),
                ..WalkOptions::default()
            },
            &AtomicBool::new(false),
            usize::MAX,
        );
        walker::STALLED
            .lock()
            .unwrap()
            .retain(|(path, _)| *path != music);
        assert_eq!(rechecked.hung_folders, [music]);
        fixture.index.apply(&rechecked.changes).unwrap();
        assert_eq!(names(&fixture.index, "song"), ["song.mp3"]);
        assert_eq!(names(&fixture.index, "plan"), ["plan.txt"]);
    }
}
