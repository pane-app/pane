//! The index scope (#126, "Scope and rules"): the roots, and the rules
//! deciding what under them is indexed. Ignore files are read as Git reads
//! them, without running `git`, by ripgrep's `ignore` crate.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};

use super::format::{SEPARATOR, path_key};

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
    /// Enter network and removable volumes mounted under a root.
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
    pub fn new(rules: ScopeRules) -> Scope {
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
        Scope {
            rules,
            patterns,
            global,
            excluded,
            home,
        }
    }

    pub fn rules(&self) -> &ScopeRules {
        &self.rules
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
    /// root, reading the ignore files on the way as a walk would have.
    /// `known` keeps what was learned about folders between calls, for a
    /// batch of changes in the same folders.
    pub fn admits(&self, path: &Path, is_dir: bool, known: &mut Admitted) -> bool {
        let Some(root) = self.root_of(path) else {
            return false;
        };
        if path == self.rules.roots[root] {
            return true;
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
            Some(self.root_context(root, OwnFiles::read(folder)))
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
        known.folders.insert(folder.to_path_buf(), context.clone());
        context
    }
}

/// What [`Scope::admits`] learned about folders.
#[derive(Default)]
pub struct Admitted {
    folders: HashMap<PathBuf, Option<Context>>,
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
