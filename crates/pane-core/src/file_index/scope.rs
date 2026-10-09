//! The index scope (#126, "Scope and rules"): the roots, and the rules
//! deciding what under them is indexed. Ignore files are read as Git reads
//! them, without running `git`, by ripgrep's `ignore` crate.
//!
//! What the rules learn of a folder when a change is looked at (its ignore
//! files, whether it is in a repository) is kept by the scope between
//! batches of changes (#186, [`Scope::admits_kept`]) until a change drops
//! it ([`Scope::forget_kept`]), never for a folder no change is reported
//! from ([`Scope::keep_nothing_under`]); a scope built again keeps nothing.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder, gitconfig_excludes_path};
use serde::{Deserialize, Serialize};

use super::format::{SEPARATOR, path_key};
use super::volume::{Asked, VOLUME_ANSWER, VolumeKind, VolumeKinds, ask_within, volume_kind};
use crate::util::lock;

/// The most folders a scope keeps between batches of changes; past it,
/// everything kept is dropped and learned again.
const KEPT_FOLDERS: usize = 20_000;

/// What the user and Pane decide is indexed. Every switch here is a user
/// setting, and starts as [`ScopeRules::for_home`] says. The rules an index
/// was built under are kept with it ([`super::IndexRecord::rules`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeRules {
    /// The folders indexed, with everything under them the rules admit.
    pub roots: Vec<PathBuf>,
    /// The home folder, whose `AppData` (Windows) or `Library` (macOS) is
    /// excluded by default.
    pub home: Option<PathBuf>,
    /// Index hidden entries: a name starting with `.` on every system, the
    /// hidden or system attribute on Windows.
    pub include_hidden: bool,
    /// Leave out what `.gitignore` (inside a Git repository), `.ignore`,
    /// the repository's `.git/info/exclude` and the user's global Git ignore
    /// file exclude.
    pub use_ignore_files: bool,
    /// Leave out `node_modules`, folders named `tmp`, `temp`, `cache` or
    /// `caches` in any letter case, `*.tmp` and `*.temp` files, and the home
    /// folder's `AppData` (Windows) or `Library` (macOS).
    pub default_exclusions: bool,
    /// Index network shares and removable drives: a root on one, and one
    /// mounted under a root. A network share included is not watched, only
    /// reconciled now and then.
    pub include_other_volumes: bool,
    /// Folders the user excluded, with everything under them.
    pub excluded_folders: Vec<PathBuf>,
    /// Patterns the user excluded, in `.gitignore` syntax, matched below
    /// each root.
    pub excluded_patterns: Vec<String>,
    /// Pane's own data, cache and log folders and the index itself: never
    /// indexed, whatever the user sets.
    pub always_excluded: Vec<PathBuf>,
}

impl ScopeRules {
    /// The defaults: the home folder, without hidden entries, what ignore
    /// files exclude, the default exclusions or other volumes.
    pub fn for_home(home: PathBuf) -> ScopeRules {
        ScopeRules {
            roots: vec![home.clone()],
            home: Some(home),
            include_hidden: false,
            use_ignore_files: true,
            default_exclusions: true,
            include_other_volumes: false,
            excluded_folders: Vec::new(),
            excluded_patterns: Vec::new(),
            always_excluded: Vec::new(),
        }
    }
}

/// Why an entry is not indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Excluded {
    /// Pane's own folders, the system's recycle and setup folders, and
    /// folders tagged as caches (`CACHEDIR.TAG`).
    Always,
    Hidden,
    /// `node_modules`, temporary and cache folders, application data.
    Default,
    /// A folder the user excluded.
    Folder,
    /// A pattern the user excluded.
    Pattern,
    /// An ignore file.
    IgnoreFile,
    /// A network or removable volume.
    OtherVolume,
}

/// Names never indexed, in any letter case: the system's recycle and
/// setup folders, which Raycast skips too.
const SYSTEM_NAMES: [&str; 8] = [
    "$RECYCLE.BIN",
    "System Volume Information",
    "$Windows.~BT",
    "$Windows.~WS",
    "$WinREAgent",
    "$SysReset",
    "Config.Msi",
    "$GetCurrent",
];

/// Folder names excluded by default, in any letter case.
const DEFAULT_FOLDER_NAMES: [&str; 5] = ["node_modules", "tmp", "temp", "cache", "caches"];

/// The file a folder holding a cache is tagged with
/// (<https://bford.info/cachedir/>).
pub(crate) const CACHE_TAG: &str = "CACHEDIR.TAG";

/// The ignore files in force at a folder: its own, then its parents'.
#[derive(Default)]
pub(crate) struct IgnoreNode {
    parent: Option<Arc<IgnoreNode>>,
    /// The folder's own, in precedence order: `.ignore`, `.gitignore`,
    /// `.git/info/exclude`.
    matchers: Vec<Gitignore>,
}

impl IgnoreNode {
    /// Whether the nearest ignore file deciding about `path` ignores it.
    fn decides(&self, path: &Path, is_dir: bool) -> Option<bool> {
        let mut node = Some(self);
        while let Some(current) = node {
            for matcher in &current.matchers {
                match matcher.matched(path, is_dir) {
                    Match::Ignore(_) => return Some(true),
                    Match::Whitelist(_) => return Some(false),
                    Match::None => {}
                }
            }
            node = current.parent.as_deref();
        }
        None
    }
}

/// What the rules know at a folder that was admitted: which root it is
/// under, how deep, the ignore files in force inside it and whether it is
/// inside a Git repository.
#[derive(Clone)]
pub(crate) struct Context {
    pub(crate) root: usize,
    pub(crate) ignore: Arc<IgnoreNode>,
    pub(crate) in_repository: bool,
    /// Whether the folder is the home folder.
    pub(crate) is_home: bool,
}

/// An entry of a folder being judged.
pub(crate) struct Candidate<'a> {
    pub(crate) path: &'a Path,
    pub(crate) name: &'a OsStr,
    pub(crate) is_dir: bool,
    /// The hidden or system attribute (Windows).
    pub(crate) hidden_attribute: bool,
}

/// The rules, ready to apply.
pub struct Scope {
    rules: ScopeRules,
    /// Per root, the user's patterns.
    patterns: Vec<Option<Gitignore>>,
    global: Option<Gitignore>,
    /// Keys of the folders excluded by the user or always.
    excluded: Vec<(Vec<u8>, Excluded)>,
    home: Option<Vec<u8>>,
    /// What kind of volume holds a folder.
    volumes: VolumeKinds,
    /// Per root, the kind of volume holding it, asked once it is there.
    root_volumes: Vec<OnceLock<RootVolume>>,
    /// What [`Scope::admits_kept`] learned of folders, kept between batches
    /// of changes until a change drops it (#186).
    kept: Mutex<Admitted>,
    /// Folders of which nothing is kept, with everything under them: not
    /// watched live, so no change would drop it ([`Scope::keep_nothing_under`]).
    unkept: Mutex<HashSet<PathBuf>>,
    /// The files Git reads to find the user's global ignore file, and that
    /// file, each as it was before `global` was read.
    global_files: Vec<(PathBuf, Stamp)>,
    /// The global ignore file `global` was read from, and how it was then.
    global_file: Option<(PathBuf, Stamp)>,
}

/// What the system said of the volume holding a root.
#[derive(Clone, Copy, Debug)]
struct RootVolume {
    kind: VolumeKind,
    /// `false` when it did not answer in time ([`VOLUME_ANSWER`]), and the
    /// root is taken for a network share.
    answered: bool,
}

/// How a file was: its size and modified time, `None` when it was not
/// there.
type Stamp = Option<(u64, Option<SystemTime>)>;

fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.len(), metadata.modified().ok()))
}

/// The files Git reads to find the user's global ignore file
/// (`core.excludesFile`), as the `ignore` crate reads them
/// (`ignore::gitignore::gitconfig_excludes_path`): `GIT_CONFIG_GLOBAL`, the
/// home folder's `.gitconfig`, `git/config` in `XDG_CONFIG_HOME` (or
/// `~/.config`), and the system's (`GIT_CONFIG_SYSTEM`, or
/// `/etc/gitconfig`).
fn git_config_files() -> Vec<PathBuf> {
    let set = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    // The home folder as the `ignore` crate finds it.
    #[allow(deprecated)]
    let home = std::env::home_dir();
    let mut files = Vec::new();
    files.extend(set("GIT_CONFIG_GLOBAL"));
    if let Some(home) = &home {
        files.push(home.join(".gitconfig"));
    }
    let config = set("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|home| home.join(".config")));
    if let Some(config) = config {
        files.push(config.join("git").join("config"));
    }
    let system = set("GIT_CONFIG_SYSTEM");
    files.push(system.unwrap_or_else(|| PathBuf::from("/etc/gitconfig")));
    files
}

fn matcher(root: &Path, files: &[PathBuf]) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    for file in files {
        // A line that does not parse is skipped; the others still apply.
        let _ = builder.add(file);
    }
    builder.build().ok().filter(|matcher| !matcher.is_empty())
}

fn has(folder: &Path, name: &str) -> bool {
    folder.join(name).symlink_metadata().is_ok()
}

impl Scope {
    /// The rules, told what kind of volume holds a folder by the system
    /// ([`volume_kind`]).
    pub fn new(rules: ScopeRules) -> Scope {
        Scope::with_volumes(rules, Arc::new(volume_kind))
    }

    /// The rules, told what kind of volume holds a folder by `volumes` (a
    /// test's own answer, or the system's).
    pub fn with_volumes(rules: ScopeRules, volumes: VolumeKinds) -> Scope {
        let patterns = rules
            .roots
            .iter()
            .map(|root| {
                if rules.excluded_patterns.is_empty() {
                    return None;
                }
                let mut builder = GitignoreBuilder::new(root);
                for pattern in &rules.excluded_patterns {
                    let _ = builder.add_line(None, pattern);
                }
                builder.build().ok().filter(|matcher| !matcher.is_empty())
            })
            .collect();
        // Each as it was before the global ignore file is read, so that a
        // change made while it is read is seen as one later.
        let (global_files, global_file) = if rules.use_ignore_files {
            let mut files: Vec<(PathBuf, Stamp)> = git_config_files()
                .into_iter()
                .map(|file| {
                    let then = stamp(&file);
                    (file, then)
                })
                .collect();
            let global_file = gitconfig_excludes_path().map(|file| {
                let then = stamp(&file);
                (file, then)
            });
            files.extend(global_file.clone());
            (files, global_file)
        } else {
            (Vec::new(), None)
        };
        let global = rules
            .use_ignore_files
            .then(|| Gitignore::global().0)
            .filter(|matcher| !matcher.is_empty());
        let mut excluded: Vec<(Vec<u8>, Excluded)> = rules
            .always_excluded
            .iter()
            .map(|folder| (path_key(folder), Excluded::Always))
            .collect();
        excluded.extend(
            rules
                .excluded_folders
                .iter()
                .map(|folder| (path_key(folder), Excluded::Folder)),
        );
        let home = rules.home.as_deref().map(path_key);
        let root_volumes = rules.roots.iter().map(|_| OnceLock::new()).collect();
        Scope {
            rules,
            patterns,
            global,
            excluded,
            home,
            volumes,
            root_volumes,
            kept: Mutex::new(Admitted::default()),
            unkept: Mutex::new(HashSet::new()),
            global_files,
            global_file,
        }
    }

    pub fn rules(&self) -> &ScopeRules {
        &self.rules
    }

    /// Whether the user's global Git ignore file, or a file Git reads to
    /// find it (`core.excludesFile` in `.gitconfig`), changed since these
    /// rules read it: the rules are to be built again (#186). Never while
    /// ignore files are not used.
    pub fn global_ignore_changed(&self) -> bool {
        self.global_files
            .iter()
            .any(|(file, then)| stamp(file) != *then)
    }

    /// Whether these rules and `other` read the same global ignore file, as
    /// it was then: if not, what it leaves out may have changed.
    pub fn same_global_ignore(&self, other: &Scope) -> bool {
        self.global_file == other.global_file
    }

    /// Whether the rules admit `path`, as [`Scope::admits`] says, told with
    /// what the scope keeps of the folders it was asked about before (#186):
    /// the ignore files of a folder and the folders above it are read once,
    /// and kept between batches of changes until [`Scope::forget_kept`]
    /// drops them. Shared by the coordinator's batches and the check before
    /// a file is acted on. Under a folder [`Scope::keep_nothing_under`]
    /// named, the ignore files are read again each time.
    pub fn admits_kept(&self, path: &Path, is_dir: bool) -> bool {
        let unkept = {
            let unkept = lock(&self.unkept);
            !unkept.is_empty() && path.ancestors().any(|folder| unkept.contains(folder))
        };
        if unkept {
            return self.admits(path, is_dir, &mut Admitted::default());
        }
        let mut kept = lock(&self.kept);
        if kept.folders.len() > KEPT_FOLDERS {
            kept.clear();
        }
        self.admits(path, is_dir, &mut kept)
    }

    /// Keeps nothing from now on of `folders` and the folders under them,
    /// and drops what is kept of them: they are not watched live (a network
    /// share, a folder past Linux's watch limit, a root when watching could
    /// not start), so no change would say when their ignore files change.
    pub fn keep_nothing_under(&self, folders: &[PathBuf]) {
        if folders.is_empty() {
            return;
        }
        lock(&self.unkept).extend(folders.iter().cloned());
        let mut kept = lock(&self.kept);
        for folder in folders {
            kept.forget(folder);
        }
    }

    /// The folders [`Scope::keep_nothing_under`] named, for the scope built
    /// in this one's place.
    pub fn kept_nothing_under(&self) -> Vec<PathBuf> {
        lock(&self.unkept).iter().cloned().collect()
    }

    /// Drops what is kept of `folder` and every folder under it, so that
    /// their ignore files are read again when next asked: `folder` was
    /// created, deleted, renamed or changed, or an ignore file, a
    /// repository or a cache tag in it was.
    pub fn forget_kept(&self, folder: &Path) {
        lock(&self.kept).forget(folder);
    }

    /// Drops everything kept.
    pub fn forget_all_kept(&self) {
        lock(&self.kept).clear();
    }

    /// The kind of volume holding the folder `path`, a mount the walker
    /// meets under a root: the system's answer within [`VOLUME_ANSWER`]
    /// (one that does not answer in time is taken for a network share, one
    /// gone meanwhile is of an unknown kind).
    pub fn volume_kind(&self, path: &Path) -> VolumeKind {
        match ask_within(&self.volumes, path, VOLUME_ANSWER) {
            Asked::Kind(kind) => kind,
            Asked::Away => VolumeKind::Unknown,
            Asked::NoAnswer => VolumeKind::Network,
        }
    }

    /// The kind of volume holding root `root`, asked once for this scope
    /// within [`VOLUME_ANSWER`]: one that does not answer in time is taken
    /// for a network share ([`Scope::roots_not_answering`]). A root that is
    /// not there (an unplugged drive) is asked again each time until it is
    /// back, taken meanwhile as a local disk so that it keeps its entries.
    pub fn root_volume(&self, root: usize) -> VolumeKind {
        let (Some(known), Some(path)) = (self.root_volumes.get(root), self.rules.roots.get(root))
        else {
            return VolumeKind::Local;
        };
        if let Some(answer) = known.get() {
            return answer.kind;
        }
        let answer = match ask_within(&self.volumes, path, VOLUME_ANSWER) {
            Asked::Away => return VolumeKind::Local,
            Asked::Kind(kind) => RootVolume {
                kind,
                answered: true,
            },
            Asked::NoAnswer => RootVolume {
                kind: VolumeKind::Network,
                answered: false,
            },
        };
        known.get_or_init(|| answer).kind
    }

    /// The roots whose volume the system did not say within
    /// [`VOLUME_ANSWER`] (a stalled network mount), taken for network
    /// shares: left out unless other volumes are included, never watched.
    pub fn roots_not_answering(&self) -> Vec<PathBuf> {
        self.rules
            .roots
            .iter()
            .zip(&self.root_volumes)
            .filter(|(_, known)| known.get().is_some_and(|answer| !answer.answered))
            .map(|(root, _)| root.clone())
            .collect()
    }

    /// Whether the rules leave root `root` out whole: it is on a network
    /// share, a removable drive or a volume the system says nothing about,
    /// and other volumes are not included.
    pub fn leaves_out_root(&self, root: usize) -> bool {
        !self.rules.include_other_volumes && self.root_volume(root) != VolumeKind::Local
    }

    /// The roots the rules keep: every root but those
    /// [`Scope::leaves_out_root`] leaves out.
    pub fn kept_roots(&self) -> Vec<PathBuf> {
        self.roots_where(|root| !self.leaves_out_root(root))
    }

    /// The roots watched for live changes: those kept, but a network
    /// share's, which is reconciled now and then instead (#126 leaves live
    /// watching of network shares out).
    pub fn watched_roots(&self) -> Vec<PathBuf> {
        self.roots_where(|root| {
            !self.leaves_out_root(root) && self.root_volume(root) != VolumeKind::Network
        })
    }

    /// The roots kept that are on a network share: never watched, and
    /// reconciled now and then.
    pub fn network_roots(&self) -> Vec<PathBuf> {
        self.roots_where(|root| {
            !self.leaves_out_root(root) && self.root_volume(root) == VolumeKind::Network
        })
    }

    fn roots_where(&self, wanted: impl Fn(usize) -> bool) -> Vec<PathBuf> {
        self.rules
            .roots
            .iter()
            .enumerate()
            .filter(|(root, _)| wanted(*root))
            .map(|(_, path)| path.clone())
            .collect()
    }

    /// Whether the folder at `path`, on volume `volume` in a folder on
    /// volume `parent_volume`, is a network share, a removable drive or a
    /// volume the system says nothing about, mounted there, that the rules
    /// leave out (other volumes not included).
    /// On Windows another volume under a root is reached only through a
    /// mount point or a junction, a link never followed, so the volumes
    /// listed are always the folder's own.
    pub(crate) fn leaves_out_mount(&self, path: &Path, volume: u64, parent_volume: u64) -> bool {
        !self.rules.include_other_volumes
            && volume != parent_volume
            && self.volume_kind(path) != VolumeKind::Local
    }

    /// The context at root `root` itself, once listed: `own_files` says
    /// which ignore files and repository folder it holds.
    pub(crate) fn root_context(&self, root: usize, own_files: OwnFiles) -> Context {
        let path = &self.rules.roots[root];
        let base = Context {
            root,
            ignore: Arc::new(IgnoreNode::default()),
            in_repository: false,
            is_home: false,
        };
        self.folder_context(&base, path, own_files)
    }

    /// The context inside `folder`, a folder admitted in `parent`'s
    /// context, given the files `own_files` it holds.
    pub(crate) fn folder_context(&self, parent: &Context, folder: &Path, own: OwnFiles) -> Context {
        let in_repository = parent.in_repository || own.git;
        let mut files = Vec::new();
        if self.rules.use_ignore_files {
            if own.ignore {
                files.push(folder.join(".ignore"));
            }
            if own.gitignore && in_repository {
                files.push(folder.join(".gitignore"));
            }
            if own.git {
                files.push(folder.join(".git").join("info").join("exclude"));
            }
        }
        let matchers: Vec<Gitignore> = files
            .iter()
            .filter_map(|file| matcher(folder, std::slice::from_ref(file)))
            .collect();
        let ignore = if matchers.is_empty() {
            parent.ignore.clone()
        } else {
            Arc::new(IgnoreNode {
                parent: Some(parent.ignore.clone()),
                matchers,
            })
        };
        Context {
            root: parent.root,
            ignore,
            in_repository,
            is_home: self.home.as_deref() == Some(path_key(folder).as_slice()),
        }
    }

    /// Why `entry`, in a folder of context `context`, is not indexed, or
    /// `None` when it is.
    pub(crate) fn excludes(&self, context: &Context, entry: &Candidate<'_>) -> Option<Excluded> {
        let name = entry.name.to_string_lossy();
        if SYSTEM_NAMES
            .iter()
            .any(|system| system.eq_ignore_ascii_case(&name))
        {
            return Some(Excluded::Always);
        }
        if !self.excluded.is_empty() {
            let key = path_key(entry.path);
            for (folder, why) in &self.excluded {
                if key.starts_with(folder)
                    && (key.len() == folder.len() || key[folder.len()] == SEPARATOR)
                {
                    return Some(*why);
                }
            }
        }
        if !self.rules.include_hidden && (name.starts_with('.') || entry.hidden_attribute) {
            return Some(Excluded::Hidden);
        }
        if self.rules.default_exclusions {
            let lower = name.to_lowercase();
            let excluded = if entry.is_dir {
                DEFAULT_FOLDER_NAMES.contains(&lower.as_str())
                    || (context.is_home
                        && ((cfg!(windows) && lower == "appdata")
                            || (cfg!(target_os = "macos") && lower == "library")))
            } else {
                lower.ends_with(".tmp") || lower.ends_with(".temp")
            };
            if excluded {
                return Some(Excluded::Default);
            }
        }
        if let Some(Some(patterns)) = self.patterns.get(context.root)
            && patterns.matched(entry.path, entry.is_dir).is_ignore()
        {
            return Some(Excluded::Pattern);
        }
        if self.rules.use_ignore_files {
            match context.ignore.decides(entry.path, entry.is_dir) {
                Some(true) => return Some(Excluded::IgnoreFile),
                Some(false) => return None,
                None => {}
            }
            if context.in_repository
                && let Some(global) = &self.global
                && global.matched(entry.path, entry.is_dir).is_ignore()
            {
                return Some(Excluded::IgnoreFile);
            }
        }
        None
    }

    /// The root `path` is under (the deepest one), if any.
    pub(crate) fn root_of(&self, path: &Path) -> Option<usize> {
        let key = path_key(path);
        self.rules
            .roots
            .iter()
            .enumerate()
            .filter(|(_, root)| {
                let root = path_key(root);
                key.starts_with(&root)
                    && (key.len() == root.len()
                        || key[root.len()] == SEPARATOR
                        || root.last() == Some(&SEPARATOR))
            })
            .max_by_key(|(_, root)| root.as_os_str().len())
            .map(|(at, _)| at)
    }

    /// Whether the rules admit `path` and every folder above it up to its
    /// root (a root on a network share or a removable drive admits nothing
    /// unless other volumes are included), reading the ignore files on the
    /// way as a walk would have. `known` keeps what was learned about
    /// folders between calls, for a batch of changes in the same folders.
    pub fn admits(&self, path: &Path, is_dir: bool, known: &mut Admitted) -> bool {
        let Some(root) = self.root_of(path) else {
            return false;
        };
        if path == self.rules.roots[root] {
            return !self.leaves_out_root(root);
        }
        let Some(parent) = path.parent() else {
            return false;
        };
        let Some(context) = self.context_at(root, parent, known) else {
            return false;
        };
        let Some(name) = path.file_name() else {
            return false;
        };
        let entry = Candidate {
            path,
            name,
            is_dir,
            hidden_attribute: hidden_attribute(path),
        };
        self.excludes(&context, &entry).is_none()
    }

    /// The context inside `folder` under root `root`, or `None` when the
    /// folder is not admitted.
    pub(crate) fn context_at(
        &self,
        root: usize,
        folder: &Path,
        known: &mut Admitted,
    ) -> Option<Context> {
        if let Some(known) = known.folders.get(folder) {
            return known.clone();
        }
        let context = if folder == self.rules.roots[root] {
            (!self.leaves_out_root(root)).then(|| self.root_context(root, OwnFiles::read(folder)))
        } else {
            let parent = folder.parent()?;
            let parent_context = self.context_at(root, parent, known)?;
            let entry = Candidate {
                path: folder,
                name: folder.file_name()?,
                is_dir: true,
                hidden_attribute: hidden_attribute(folder),
            };
            if self.excludes(&parent_context, &entry).is_some() || has(folder, CACHE_TAG) {
                None
            } else {
                Some(self.folder_context(&parent_context, folder, OwnFiles::read(folder)))
            }
        };
        // Known under its folder, which is known already, so that forgetting
        // that one forgets it too.
        if folder != self.rules.roots[root]
            && let Some(parent) = folder.parent()
        {
            known
                .below
                .entry(parent.to_path_buf())
                .or_default()
                .push(folder.to_path_buf());
        }
        known.folders.insert(folder.to_path_buf(), context.clone());
        context
    }
}

/// What [`Scope::admits`] learned about folders.
#[derive(Default)]
pub struct Admitted {
    folders: HashMap<PathBuf, Option<Context>>,
    /// The folders known directly in each folder known.
    below: HashMap<PathBuf, Vec<PathBuf>>,
}

impl Admitted {
    /// Forgets `folder` and every folder known under it.
    fn forget(&mut self, folder: &Path) {
        // A folder is known only once the one it is in is: one not known
        // has nothing known under it.
        if self.folders.remove(folder).is_none() {
            return;
        }
        if let Some(parent) = folder.parent()
            && let Some(siblings) = self.below.get_mut(parent)
        {
            siblings.retain(|sibling| sibling != folder);
        }
        let mut pending = self.below.remove(folder).unwrap_or_default();
        while let Some(next) = pending.pop() {
            self.folders.remove(&next);
            if let Some(below) = self.below.remove(&next) {
                pending.extend(below);
            }
        }
    }

    fn clear(&mut self) {
        self.folders.clear();
        self.below.clear();
    }
}

/// The files of a folder the rules read: its ignore files and whether it
/// holds a Git repository.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OwnFiles {
    pub(crate) ignore: bool,
    pub(crate) gitignore: bool,
    pub(crate) git: bool,
}

impl OwnFiles {
    fn read(folder: &Path) -> OwnFiles {
        OwnFiles {
            ignore: has(folder, ".ignore"),
            gitignore: has(folder, ".gitignore"),
            git: has(folder, ".git"),
        }
    }
}

/// Whether `path` has the hidden or system attribute (Windows only).
fn hidden_attribute(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        path.symlink_metadata()
            .is_ok_and(|metadata| metadata.file_attributes() & (0x2 | 0x4) != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn home_with(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        for file in files {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if file.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::write(&path, b"").unwrap();
            }
        }
        fs::create_dir_all(&home).unwrap();
        (dir, home)
    }

    fn admitted(scope: &Scope, home: &Path, relative: &str) -> bool {
        let path = home.join(relative);
        scope.admits(&path, path.is_dir(), &mut Admitted::default())
    }

    #[test]
    fn hidden_entries_and_the_default_exclusions_are_left_out_until_switched_off() {
        let (_dir, home) = home_with(&[
            "notes.txt",
            ".secret.txt",
            ".config/app.toml",
            "project/node_modules/left-pad/index.js",
            "project/Cache/blob",
            "project/TMP/x",
            "project/draft.tmp",
            "project/main.rs",
        ]);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        assert!(admitted(&scope, &home, "notes.txt"));
        assert!(admitted(&scope, &home, "project/main.rs"));
        for left_out in [
            ".secret.txt",
            ".config/app.toml",
            "project/node_modules/left-pad/index.js",
            "project/Cache/blob",
            "project/TMP/x",
            "project/draft.tmp",
        ] {
            assert!(!admitted(&scope, &home, left_out), "{left_out}");
        }
        let mut rules = ScopeRules::for_home(home.clone());
        rules.include_hidden = true;
        rules.default_exclusions = false;
        let scope = Scope::new(rules);
        assert!(admitted(&scope, &home, ".config/app.toml"));
        assert!(admitted(
            &scope,
            &home,
            "project/node_modules/left-pad/index.js"
        ));
        assert!(admitted(&scope, &home, "project/draft.tmp"));
    }

    #[test]
    fn gitignore_applies_inside_a_repository_and_ignore_files_everywhere() {
        let (_dir, home) = home_with(&[
            "repo/.git/HEAD",
            "repo/.gitignore",
            "repo/build/out.o",
            "repo/src/lib.rs",
            "repo/src/keep.log",
            "repo/src/other.log",
            "loose/.gitignore",
            "loose/build/out.o",
            "plain/.ignore",
            "plain/secret.key",
        ]);
        fs::write(home.join("repo/.gitignore"), "build/\n*.log\n!keep.log\n").unwrap();
        fs::write(home.join("loose/.gitignore"), "build/\n").unwrap();
        fs::write(home.join("plain/.ignore"), "*.key\n").unwrap();
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        assert!(!admitted(&scope, &home, "repo/build/out.o"));
        assert!(!admitted(&scope, &home, "repo/build"));
        assert!(!admitted(&scope, &home, "repo/src/other.log"));
        assert!(admitted(&scope, &home, "repo/src/keep.log"));
        assert!(admitted(&scope, &home, "repo/src/lib.rs"));
        // Outside a repository Git reads no .gitignore.
        assert!(admitted(&scope, &home, "loose/build/out.o"));
        assert!(!admitted(&scope, &home, "plain/secret.key"));

        let mut rules = ScopeRules::for_home(home.clone());
        rules.use_ignore_files = false;
        let scope = Scope::new(rules);
        assert!(admitted(&scope, &home, "repo/build/out.o"));
        assert!(admitted(&scope, &home, "plain/secret.key"));
    }

    #[test]
    fn what_is_kept_of_a_folder_holds_until_it_or_a_folder_above_it_is_forgotten() {
        let (_dir, home) = home_with(&[
            "repo/.git/HEAD",
            "repo/.gitignore",
            "repo/src/trace.txt",
            "other/notes.txt",
        ]);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let traced = home.join("repo/src/trace.txt");
        assert!(scope.admits_kept(&traced, false));
        assert!(scope.admits_kept(&home.join("other/notes.txt"), false));

        // Kept: the ignore file is not read again until its folder is
        // forgotten.
        fs::write(home.join("repo/.gitignore"), "*.txt\n").unwrap();
        assert!(scope.admits_kept(&traced, false));
        assert!(!scope.admits(&traced, false, &mut Admitted::default()));
        scope.forget_kept(&home.join("other"));
        assert!(
            scope.admits_kept(&traced, false),
            "another folder forgotten"
        );
        scope.forget_kept(&home.join("repo"));
        assert!(!scope.admits_kept(&traced, false));

        // Forgetting a folder forgets every folder under it.
        fs::write(home.join("repo/.gitignore"), "").unwrap();
        assert!(!scope.admits_kept(&traced, false));
        scope.forget_kept(&home);
        assert!(scope.admits_kept(&traced, false));

        // And everything at once.
        fs::write(home.join("repo/.gitignore"), "src/\n").unwrap();
        assert!(scope.admits_kept(&traced, false));
        scope.forget_all_kept();
        assert!(!scope.admits_kept(&traced, false));
        assert!(!scope.admits_kept(&home.join("repo/src"), true));
    }

    /// Of a folder no change is reported from (not watched live), nothing
    /// is kept: its ignore files are read each time (#186).
    #[test]
    fn nothing_is_kept_of_a_folder_not_watched_live() {
        let (_dir, home) = home_with(&[
            "repo/.git/HEAD",
            "repo/.gitignore",
            "repo/src/trace.txt",
            "other/notes.txt",
        ]);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let traced = home.join("repo/src/trace.txt");
        let notes = home.join("other/notes.txt");
        assert!(scope.admits_kept(&traced, false));
        assert!(scope.admits_kept(&notes, false));
        scope.keep_nothing_under(&[home.join("repo")]);

        fs::write(home.join("repo/.gitignore"), "*.txt\n").unwrap();
        assert!(!scope.admits_kept(&traced, false), "read again");
        fs::write(home.join("repo/.gitignore"), "").unwrap();
        assert!(scope.admits_kept(&traced, false), "read again");
        // Elsewhere what was learned is kept as before.
        fs::write(home.join("other/.ignore"), "*.txt\n").unwrap();
        assert!(scope.admits_kept(&notes, false), "kept");

        // A scope built in its place is told so.
        let again = Scope::new(ScopeRules::for_home(home.clone()));
        again.keep_nothing_under(&scope.kept_nothing_under());
        assert_eq!(again.kept_nothing_under(), [home.join("repo")]);
    }

    #[test]
    fn the_users_folders_and_patterns_and_panes_own_folders_are_left_out() {
        let (_dir, home) = home_with(&[
            "private/diary.txt",
            "work/report.bak",
            "work/report.txt",
            "pane-cache/file-index/1.seg",
            "folder with CACHEDIR.TAG/CACHEDIR.TAG",
            "folder with CACHEDIR.TAG/blob",
        ]);
        let mut rules = ScopeRules::for_home(home.clone());
        rules.excluded_folders = vec![home.join("private")];
        rules.excluded_patterns = vec!["*.bak".into()];
        rules.always_excluded = vec![home.join("pane-cache")];
        let scope = Scope::new(rules);
        assert!(!admitted(&scope, &home, "private/diary.txt"));
        assert!(!admitted(&scope, &home, "work/report.bak"));
        assert!(admitted(&scope, &home, "work/report.txt"));
        assert!(!admitted(&scope, &home, "pane-cache/file-index/1.seg"));
        assert!(!admitted(&scope, &home, "folder with CACHEDIR.TAG/blob"));
        assert!(!scope.admits(
            Path::new("/somewhere/else.txt"),
            false,
            &mut Admitted::default()
        ));
    }

    #[test]
    fn the_system_recycle_and_setup_folders_are_never_indexed() {
        let (_dir, home) = home_with(&["$Recycle.Bin/x", "System Volume Information/y"]);
        let mut rules = ScopeRules::for_home(home.clone());
        rules.include_hidden = true;
        rules.default_exclusions = false;
        let scope = Scope::new(rules);
        assert!(!admitted(&scope, &home, "$Recycle.Bin/x"));
        assert!(!admitted(&scope, &home, "System Volume Information/y"));
    }

    #[test]
    fn a_root_on_a_share_or_a_removable_drive_is_left_out_until_other_volumes_are_included() {
        // As Windows (GetDriveTypeW) or macOS (statfs) would answer for a
        // mapped drive or a mounted share, and a USB stick.
        let (dir, home) = home_with(&["notes.txt"]);
        let share = dir.path().join("share");
        let stick = dir.path().join("stick");
        for root in [&share, &stick] {
            fs::create_dir_all(root.join("Projects")).unwrap();
            fs::write(root.join("Projects/plan.txt"), b"").unwrap();
        }
        let mut rules = ScopeRules::for_home(home.clone());
        rules.roots.extend([share.clone(), stick.clone()]);
        let volumes = crate::file_index::volume::tests::fake_volumes(&share, &stick);
        let scope = Scope::with_volumes(rules.clone(), volumes.clone());
        assert!(admitted(&scope, &home, "notes.txt"));
        for root in [&share, &stick] {
            assert!(!scope.admits(root, true, &mut Admitted::default()));
            assert!(!admitted(&scope, root, "Projects/plan.txt"));
        }
        assert_eq!(scope.kept_roots(), std::slice::from_ref(&home));
        assert_eq!(scope.watched_roots(), std::slice::from_ref(&home));
        assert!(scope.network_roots().is_empty());

        // Included: both are indexed, and the share is reconciled rather
        // than watched.
        rules.include_other_volumes = true;
        let scope = Scope::with_volumes(rules, volumes);
        for root in [&share, &stick] {
            assert!(scope.admits(root, true, &mut Admitted::default()));
            assert!(admitted(&scope, root, "Projects/plan.txt"));
        }
        assert_eq!(
            scope.kept_roots(),
            [home.clone(), share.clone(), stick.clone()]
        );
        assert_eq!(scope.watched_roots(), [home, stick]);
        assert_eq!(scope.network_roots(), [share]);
    }

    /// A root away when it is first asked about (an unplugged drive) keeps
    /// its entries, and is asked about again once it is back (#184): a
    /// removable drive plugged in later is left out from then on.
    #[test]
    fn a_root_away_is_asked_about_again_once_it_is_back() {
        let (dir, home) = home_with(&["notes.txt"]);
        let stick = dir.path().join("stick");
        let mut rules = ScopeRules::for_home(home.clone());
        rules.roots.push(stick.clone());
        let share = dir.path().join("share");
        let volumes = crate::file_index::volume::tests::fake_volumes(&share, &stick);
        let scope = Scope::with_volumes(rules, volumes);
        assert_eq!(scope.kept_roots(), [home.clone(), stick.clone()]);
        fs::create_dir_all(&stick).unwrap();
        assert_eq!(scope.root_volume(1), VolumeKind::Removable);
        assert_eq!(scope.kept_roots(), [home]);
    }

    /// A root whose volume the system does not say in time (a stalled
    /// network mount) is taken for a network share and noted, holding up
    /// nothing longer than that time; a volume mounted under a root that
    /// the system says nothing about is left out as another volume (#184).
    #[test]
    fn a_volume_that_does_not_answer_or_says_nothing_is_left_out() {
        let (dir, home) = home_with(&["notes.txt", "mounted/x.txt"]);
        let slow = dir.path().join("slow");
        fs::create_dir_all(&slow).unwrap();
        let mut rules = ScopeRules::for_home(home.clone());
        rules.roots.push(slow.clone());
        let stalled = slow.clone();
        let volumes: VolumeKinds = Arc::new(move |path: &Path| {
            if path.starts_with(&stalled) {
                std::thread::sleep(VOLUME_ANSWER * 2);
                VolumeKind::Local
            } else if path.ends_with("mounted") {
                VolumeKind::Unknown
            } else {
                VolumeKind::Local
            }
        });
        let mounted = home.join("mounted");

        let scope = Scope::with_volumes(rules.clone(), volumes.clone());
        assert_eq!(scope.kept_roots(), std::slice::from_ref(&home));
        assert_eq!(scope.roots_not_answering(), std::slice::from_ref(&slow));
        assert!(scope.leaves_out_mount(&mounted, 2, 1));

        rules.include_other_volumes = true;
        let scope = Scope::with_volumes(rules, volumes);
        assert_eq!(scope.watched_roots(), [home]);
        assert_eq!(scope.network_roots(), [slow]);
        assert!(!scope.leaves_out_mount(&mounted, 2, 1));
    }

    #[cfg(windows)]
    #[test]
    fn application_data_in_the_home_folder_is_left_out_on_windows() {
        let (_dir, home) = home_with(&["AppData/Local/x.txt", "work/AppData/y.txt"]);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        assert!(!admitted(&scope, &home, "AppData/Local/x.txt"));
        assert!(admitted(&scope, &home, "work/AppData/y.txt"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_library_in_the_home_folder_is_left_out_on_macos() {
        let (_dir, home) = home_with(&["Library/Caches/x", "work/Library/y.txt"]);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        assert!(!admitted(&scope, &home, "Library/Caches/x"));
        assert!(admitted(&scope, &home, "work/Library/y.txt"));
    }
}
