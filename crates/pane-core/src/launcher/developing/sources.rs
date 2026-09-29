//! What development last saw of a package's source folder, so that only a
//! change of it is a save.
//!
//! The file watchers tell of more than saves, and differently on each
//! system: FSEvents (macOS) may tell of files written just before it
//! started, carries a file's earlier flags (created, modified) on each of its
//! later events, and reports changes of metadata alone, such as copying a
//! file by cloning it; `ReadDirectoryChangesW` (Windows) reports a folder
//! whose entries changed as modified. Pane's own copies of the components
//! into the folder are events too. So each event is checked against what
//! was last seen of its path: a save is a file whose modification time,
//! size or (when it was written within the clock's resolution of when Pane
//! last looked, so that a same-sized write could keep the time) bytes
//! changed, or a file or folder that appeared or went.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::develop::{Build, is_save};

/// Files up to this size are remembered by their bytes too.
const DIGESTED: u64 = 1 << 20;

/// How long after a file's modification time a same-sized write may keep
/// that time, where the time has sub-second precision (Linux's coarse clock
/// ticks every few milliseconds) and where it has not (HFS+, FAT).
const FINE_RESOLUTION: Duration = Duration::from_millis(100);
const COARSE_RESOLUTION: Duration = Duration::from_secs(2);

/// The source folder as development last saw it: each path that could be
/// a save, relative to the folder.
pub(super) struct Sources {
    /// The source folder, canonical.
    root: PathBuf,
    build: Arc<dyn Build>,
    seen: HashMap<PathBuf, Seen>,
    /// Top-level folders that appeared since the last call of
    /// [`Sources::take_new_folders`].
    new_folders: Vec<PathBuf>,
}

enum Seen {
    Folder,
    File(FileState),
}

#[derive(Clone, PartialEq, Eq)]
struct FileState {
    modified: Option<SystemTime>,
    len: u64,
    /// A hash of its bytes, if it was small enough or Pane wrote it.
    digest: Option<u64>,
    /// When Pane looked.
    observed: SystemTime,
}

impl FileState {
    fn read(path: &Path, meta: &std::fs::Metadata, any_size: bool) -> FileState {
        let digest = (meta.is_file() && (any_size || meta.len() <= DIGESTED))
            .then(|| std::fs::read(path).ok())
            .flatten()
            .map(|bytes| {
                let mut hasher = DefaultHasher::new();
                hasher.write(&bytes);
                hasher.finish()
            });
        FileState {
            modified: meta.modified().ok(),
            len: meta.len(),
            digest,
            observed: SystemTime::now(),
        }
    }

    /// Whether a write after Pane looked could have kept the modification
    /// time.
    fn ambiguous(&self) -> bool {
        let Some(modified) = self.modified else {
            return true;
        };
        let coarse = modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .is_ok_and(|since| since.subsec_nanos() == 0);
        let resolution = if coarse {
            COARSE_RESOLUTION
        } else {
            FINE_RESOLUTION
        };
        self.observed
            .duration_since(modified)
            .map_or(true, |age| age < resolution)
    }
}

impl Sources {
    /// What is in `root` now, as far as `build` could be saved in it.
    pub(super) fn new(root: &Path, build: Arc<dyn Build>) -> Sources {
        let mut sources = Sources {
            root: root.to_path_buf(),
            build,
            seen: HashMap::new(),
            new_folders: Vec::new(),
        };
        sources.record_contents(Path::new(""));
        sources.new_folders.clear();
        sources
    }

    /// Whether `relative`, which may be a save (see [`is_save`]), changed
    /// since it was last seen; it is remembered as it is now.
    pub(super) fn changed(&mut self, relative: &Path) -> bool {
        let path = self.root.join(relative);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            let known = self.seen.contains_key(relative);
            self.seen.retain(|seen, _| !seen.starts_with(relative));
            return known;
        };
        if meta.is_dir() {
            return match self.seen.get(relative) {
                // A folder whose entries changed: each entry tells.
                Some(Seen::Folder) => false,
                _ => {
                    self.seen.retain(|seen, _| !seen.starts_with(relative));
                    self.record_folder(relative);
                    true
                }
            };
        }
        let Some(Seen::File(old)) = self.seen.get(relative) else {
            self.seen.retain(|seen, _| !seen.starts_with(relative));
            let state = FileState::read(&path, &meta, false);
            self.seen.insert(relative.to_path_buf(), Seen::File(state));
            return true;
        };
        let changed = if meta.modified().ok() != old.modified || meta.len() != old.len {
            true
        } else if old.ambiguous() {
            let now = FileState::read(&path, &meta, old.digest.is_some());
            now.digest.is_none() || now.digest != old.digest
        } else {
            return false;
        };
        let any_size = old.digest.is_some() && old.len > DIGESTED;
        let state = FileState::read(&path, &meta, any_size);
        self.seen.insert(relative.to_path_buf(), Seen::File(state));
        changed
    }

    /// Looks at the whole folder again, as when the watcher lost events;
    /// returns whether anything changed.
    pub(super) fn rescan(&mut self) -> bool {
        let mut paths: Vec<PathBuf> = self.seen.keys().cloned().collect();
        self.list(Path::new(""), &mut paths);
        paths.sort();
        paths.dedup();
        let mut changed = false;
        for path in paths {
            changed |= self.changed(&path);
        }
        changed
    }

    /// Remembers `relative` as it is now, after Pane wrote it, so that its
    /// events are not a save.
    pub(super) fn wrote(&mut self, relative: &Path) {
        if !is_save(relative, &*self.build) {
            return;
        }
        let path = self.root.join(relative);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_dir() => {
                let state = FileState::read(&path, &meta, true);
                self.seen.insert(relative.to_path_buf(), Seen::File(state));
            }
            _ => {
                self.changed(relative);
            }
        }
    }

    /// The top-level folders that appeared since the last call, to be
    /// watched too.
    pub(super) fn take_new_folders(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.new_folders)
    }

    fn record_folder(&mut self, relative: &Path) {
        self.seen.insert(relative.to_path_buf(), Seen::Folder);
        if relative.components().count() == 1 {
            self.new_folders.push(self.root.join(relative));
        }
        self.record_contents(relative);
    }

    /// Remembers what is in the folder `relative`, at any depth.
    fn record_contents(&mut self, relative: &Path) {
        let Ok(entries) = std::fs::read_dir(self.root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let child = relative.join(entry.file_name());
            if !is_save(&child, &*self.build) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                self.record_folder(&child);
            } else {
                let state = FileState::read(&entry.path(), &meta, false);
                self.seen.insert(child, Seen::File(state));
            }
        }
    }

    /// Adds the paths in the folder `relative` that could be saves to
    /// `paths`.
    fn list(&self, relative: &Path, paths: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(self.root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let child = relative.join(entry.file_name());
            if !is_save(&child, &*self.build) {
                continue;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                self.list(&child, paths);
            }
            paths.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develop::{BuildJob, BuildOutcome};
    use std::fs;

    struct Ignoring;

    impl Build for Ignoring {
        fn command(&self) -> String {
            String::new()
        }
        fn ignores(&self, path: &Path) -> bool {
            path == Path::new("out.wasm")
        }
        fn run(&self, _: &BuildJob) -> BuildOutcome {
            BuildOutcome::Built
        }
    }

    fn sources(root: &Path) -> Sources {
        Sources::new(root, Arc::new(Ignoring))
    }

    #[test]
    fn only_a_change_of_content_or_of_what_is_there_is_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "one").unwrap();
        let mut sources = sources(root);
        let lib = Path::new("src/lib.rs");

        // Told of a file or folder as it already was: not a save.
        assert!(!sources.changed(lib));
        assert!(!sources.changed(Path::new("src")));
        // Read, or its metadata alone changed: not a save.
        fs::read(root.join(lib)).unwrap();
        let mut permissions = fs::metadata(root.join(lib)).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(root.join(lib), permissions.clone()).unwrap();
        assert!(!sources.changed(lib));
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(root.join(lib), permissions).unwrap();

        // Written with other bytes of the same size, at once: a save, once.
        fs::write(root.join(lib), "two").unwrap();
        assert!(sources.changed(lib));
        assert!(!sources.changed(lib));
        fs::write(root.join(lib), "three").unwrap();
        assert!(sources.changed(lib));

        // Appearing and going are saves; a folder that appears is watched.
        fs::create_dir(root.join("lib")).unwrap();
        fs::write(root.join("lib/util.rs"), "").unwrap();
        assert!(sources.changed(Path::new("lib")));
        assert_eq!(sources.take_new_folders(), [root.join("lib")]);
        assert!(!sources.changed(Path::new("lib/util.rs")));
        fs::remove_file(root.join("lib/util.rs")).unwrap();
        assert!(sources.changed(Path::new("lib/util.rs")));
        assert!(!sources.changed(Path::new("lib/util.rs")));
        fs::remove_dir(root.join("lib")).unwrap();
        assert!(sources.changed(Path::new("lib")));
        assert!(sources.take_new_folders().is_empty());
    }

    #[test]
    fn what_pane_wrote_is_not_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("command.wasm"), "old").unwrap();
        let mut sources = sources(root);
        fs::remove_file(root.join("command.wasm")).unwrap();
        fs::write(root.join("command.wasm"), "new").unwrap();
        sources.wrote(Path::new("command.wasm"));
        assert!(!sources.changed(Path::new("command.wasm")));
    }

    #[test]
    fn a_rescan_finds_what_changed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.txt"), "a").unwrap();
        let mut sources = sources(root);
        assert!(!sources.rescan());
        fs::write(root.join("b.txt"), "b").unwrap();
        assert!(sources.rescan());
        assert!(!sources.rescan());
        fs::remove_file(root.join("a.txt")).unwrap();
        assert!(sources.rescan());
        // The build's own output is never looked at.
        fs::write(root.join("out.wasm"), "built").unwrap();
        assert!(!sources.rescan());
    }
}
