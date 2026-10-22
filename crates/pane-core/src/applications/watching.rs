//! Watching the folders an adapter finds applications in, through the
//! native file watcher development mode uses (`notify`: ReadDirectoryChangesW
//! on Windows, FSEvents on macOS, inotify on Linux). What a watcher sees is
//! told as a [`Change`]; the host's list ([`super::Cached`]) debounces the
//! changes and rescans, so nothing here reads what changed.
//!
//! A folder that does not exist yet is watched for from the nearest folder
//! above it that does (only the folders on the way to it matter there), and
//! the watch is partial ([`Watch::complete`]): the host's list makes it
//! again after each rescan, so the folder is watched once it appears.

use std::path::{Path, PathBuf};

use notify::event::{EventKind, MetadataKind, ModifyKind};
use notify::{RecursiveMode, Watcher};

use super::{Change, Changes, Watch};

/// A folder to watch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Folder {
    path: PathBuf,
    /// Whether its subfolders are watched too.
    subfolders: bool,
    /// Whether only entries appearing or going in it matter, and say that
    /// the system is completing a change ([`Change::Completing`]): the
    /// folder of the packages registered for the user.
    packages: bool,
}

impl Folder {
    /// `path` and every folder in it.
    pub fn walked(path: PathBuf) -> Folder {
        Folder {
            path,
            subfolders: true,
            packages: false,
        }
    }

    /// `path` alone, without its subfolders.
    pub fn flat(path: PathBuf) -> Folder {
        Folder {
            path,
            subfolders: false,
            packages: false,
        }
    }

    /// `path`, holding a folder for each package registered for the user,
    /// whose appearing or going alone matters.
    pub fn packages(path: PathBuf) -> Folder {
        Folder {
            path,
            subfolders: false,
            packages: true,
        }
    }
}

/// The paths a watch's events are judged by, each as given and as the
/// system resolves it (FSEvents reports `/private/var/...` for
/// `/var/...`).
#[derive(Clone, Debug, Default)]
struct Watched {
    /// The folders watched themselves.
    folders: Vec<PathBuf>,
    /// The folders of packages registered for the user.
    packages: Vec<PathBuf>,
    /// The folders that do not exist yet, watched for from above.
    awaited: Vec<PathBuf>,
}

/// `path` as given, and as the system resolves it when that differs; a
/// path that does not exist is resolved through its nearest existing
/// folder.
fn forms(path: &Path) -> Vec<PathBuf> {
    let mut forms = vec![path.to_path_buf()];
    let resolved = path.ancestors().find_map(|ancestor| {
        let canonical = std::fs::canonicalize(ancestor).ok()?;
        let rest = path.strip_prefix(ancestor).ok()?;
        Some(canonical.join(rest))
    });
    forms.extend(resolved.filter(|resolved| resolved != path));
    forms
}

/// Watches `folders`, telling `changes` of what changes in them from the
/// watcher's own thread until the returned watch is dropped. Fails only
/// when the system's watcher cannot start; a folder that cannot be watched
/// makes the watch partial.
pub(super) fn watch(folders: Vec<Folder>, changes: Changes) -> Result<Watch, String> {
    let mut watched = Watched::default();
    let mut targets: Vec<(PathBuf, RecursiveMode)> = Vec::new();
    let mut complete = true;
    for folder in &folders {
        let Some((path, mode)) = watched_for(folder) else {
            complete = false;
            continue;
        };
        if path == folder.path {
            watched.folders.extend(forms(&folder.path));
            if folder.packages {
                watched.packages.extend(forms(&folder.path));
            }
        } else {
            complete = false;
            watched.awaited.extend(forms(&folder.path));
        }
        if !targets.iter().any(|(target, _)| *target == path) {
            targets.push((path, mode));
        }
    }
    let judged = watched.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Some(change) = change_of(event, &judged) {
            changes(change);
        }
    })
    .map_err(|error| format!("Pane could not watch for applications: {error}"))?;
    for (path, mode) in targets {
        if watcher.watch(&path, mode).is_err() {
            complete = false;
        }
    }
    Ok(if complete {
        Watch::new(watcher)
    } else {
        Watch::partial(watcher)
    })
}

/// Where `folder` is watched from, and how: the folder itself, or the
/// nearest folder above it that exists (without its subfolders) when it
/// does not exist yet; `None` when neither does.
fn watched_for(folder: &Folder) -> Option<(PathBuf, RecursiveMode)> {
    if folder.path.is_dir() {
        let mode = if folder.subfolders {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        return Some((folder.path.clone(), mode));
    }
    folder
        .path
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.is_dir())
        .map(|ancestor| (ancestor.to_path_buf(), RecursiveMode::NonRecursive))
}

/// What a watcher's `event` tells the host's list, if anything: a lost
/// event or a failure asks for a full rescan; reading a file says nothing;
/// in a folder of packages, only an entry appearing, going or being renamed
/// says something, that a package is being registered or removed; above a
/// folder that does not exist yet, only a change on the way to it does.
fn change_of(event: notify::Result<notify::Event>, watched: &Watched) -> Option<Change> {
    let Ok(event) = event else {
        return Some(Change::Lost);
    };
    if event.need_rescan() {
        return Some(Change::Lost);
    }
    let reading = matches!(
        event.kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    );
    if reading {
        return None;
    }
    if event.paths.is_empty() {
        return Some(Change::Changed);
    }
    let under = |folders: &[PathBuf], path: &Path| folders.iter().any(|f| path.starts_with(f));
    let in_packages = |path: &Path| under(&watched.packages, path);
    let relevant = |path: &Path| {
        under(&watched.folders, path) || watched.awaited.iter().any(|f| f.starts_with(path))
    };
    if event.paths.iter().all(|path| in_packages(path)) {
        let entry = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        );
        return entry.then_some(Change::Completing);
    }
    event
        .paths
        .iter()
        .any(|path| relevant(path))
        .then_some(Change::Changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, Flag, RemoveKind};

    fn event(kind: EventKind, path: &str) -> notify::Result<notify::Event> {
        Ok(notify::Event::new(kind).add_path(PathBuf::from(path)))
    }

    fn watched() -> Watched {
        Watched {
            folders: vec![PathBuf::from("/menu"), PathBuf::from("/local/Packages")],
            packages: vec![PathBuf::from("/local/Packages")],
            awaited: vec![PathBuf::from("/home/ann/.local/share/applications")],
        }
    }

    #[test]
    fn a_change_in_a_folder_is_a_change_and_reading_is_none() {
        let created = EventKind::Create(CreateKind::File);
        let written = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        assert_eq!(
            change_of(event(created, "/menu/Editor.lnk"), &watched()),
            Some(Change::Changed)
        );
        assert_eq!(
            change_of(event(written, "/menu/Suite/Editor.lnk"), &watched()),
            Some(Change::Changed)
        );
        assert_eq!(
            change_of(
                event(EventKind::Access(AccessKind::Any), "/menu/Editor.lnk"),
                &watched()
            ),
            None
        );
    }

    #[test]
    fn only_a_package_folder_appearing_or_going_says_a_package_changed() {
        let package = "/local/Packages/Contoso.Editor_8wekyb3d8bbwe";
        let created = EventKind::Create(CreateKind::Folder);
        let removed = EventKind::Remove(RemoveKind::Folder);
        assert_eq!(
            change_of(event(created, package), &watched()),
            Some(Change::Completing)
        );
        assert_eq!(
            change_of(event(removed, package), &watched()),
            Some(Change::Completing)
        );
        // A package writing its own data changes its folder's time.
        let written = EventKind::Modify(ModifyKind::Any);
        assert_eq!(change_of(event(written, package), &watched()), None);
    }

    #[test]
    fn above_a_folder_that_does_not_exist_yet_only_the_way_to_it_matters() {
        let created = EventKind::Create(CreateKind::Folder);
        assert_eq!(
            change_of(
                event(created, "/home/ann/.local/share/applications"),
                &watched()
            ),
            Some(Change::Changed)
        );
        assert_eq!(
            change_of(event(created, "/home/ann/.local/share"), &watched()),
            Some(Change::Changed)
        );
        let written = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        assert_eq!(
            change_of(
                event(written, "/home/ann/.local/share/recently-used.xbel"),
                &watched()
            ),
            None
        );
    }

    #[test]
    fn lost_events_and_failures_ask_for_a_full_rescan() {
        let lost = notify::Event::new(EventKind::Other).set_flag(Flag::Rescan);
        assert_eq!(change_of(Ok(lost), &watched()), Some(Change::Lost));
        let failed = Err(notify::Error::generic("the buffer overflowed"));
        assert_eq!(change_of(failed, &watched()), Some(Change::Lost));
    }

    #[test]
    fn a_missing_folder_is_watched_for_from_the_nearest_one_above_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("share/applications");
        assert_eq!(
            watched_for(&Folder::walked(missing)),
            Some((dir.path().to_path_buf(), RecursiveMode::NonRecursive))
        );
        assert_eq!(
            watched_for(&Folder::walked(dir.path().to_path_buf())),
            Some((dir.path().to_path_buf(), RecursiveMode::Recursive))
        );
        assert_eq!(
            watched_for(&Folder::flat(dir.path().to_path_buf())),
            Some((dir.path().to_path_buf(), RecursiveMode::NonRecursive))
        );
    }

    #[test]
    fn a_path_is_judged_as_given_and_as_the_system_resolves_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("share/applications");
        let resolved = std::fs::canonicalize(dir.path())
            .unwrap()
            .join("share/applications");
        let forms = forms(&missing);
        assert_eq!(forms[0], missing);
        assert!(forms.contains(&resolved), "{forms:?}");
    }
}
