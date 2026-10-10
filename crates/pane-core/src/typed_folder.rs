//! The entries of a folder the user typed into root search (#204), which
//! Pane's host lists for a command that answers root results for such a
//! query: the Files extension, and the files samples in each language,
//! answer them as `open-file` results.
//!
//! No folder is granted, unlike the folder a package asks for
//! ([`crate::files`], ADR 0017): the user named it, and ADR 0034's rule
//! that the host, not the extension, reads the file system covers it. The
//! bounds are the scan policy's, kept to one folder:
//!
//! - the **direct entries** only, folders first and each in name order;
//! - **at most [`MAX_ENTRIES`]** entries, with a partial listing saying so
//!   (`truncated`);
//! - hidden entries (a name starting with `.`, and on Windows the hidden
//!   and system attributes), links and names that are not Unicode are
//!   neither listed nor counted, as the granted folder's walk skips them.
//!
//! The text the user typed is resolved as the launcher resolves a typed
//! path ([`crate::launcher::typed_query`]): `~` alone or followed by a
//! separator is the home folder (the one the file index is configured
//! with, else the environment's), and `file://` is taken off. A listing is
//! Pane's to keep: the entries are named by the ids it gives them, and it
//! checks an entry again before opening it — still in the latest listing,
//! still the kind it was listed as, not a link, still inside the folder
//! the user typed — as [`crate::files`] checks a file of a granted folder.
//!
//! The listing runs on a thread of its own, since a folder may block: the
//! runtime thread awaits it, serving other packages' calls meanwhile, and
//! the wait is Pane's time, never the guest's computing (#18, #136), as
//! the system host functions are.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::files::{canonical, hidden, is_network_path, program_at, runs_as_program, unreadable};
use crate::launcher::typed_query;
use crate::runtime::{GuestState, bindings, lock};

use bindings::pane::extension::typed_folder as wit;

/// How many direct entries one typed folder's listing holds, at most.
pub(crate) const MAX_ENTRIES: usize = 500;

/// One direct entry of a typed folder, as the host found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypedEntry {
    /// The id Pane gave the entry for this listing, which an `open-file`
    /// result names it by.
    pub(crate) id: String,
    /// Its absolute path, inside the typed folder.
    pub(crate) path: PathBuf,
    /// Its own name.
    pub(crate) name: String,
    /// It is a folder.
    pub(crate) folder: bool,
    /// Whether opening it would run a program, as it was listed.
    pub(crate) program: bool,
}

/// What one typed folder's listing found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypedListing {
    /// The folder listed, resolved.
    pub(crate) folder: PathBuf,
    /// The home folder `~` resolves to, for the entries' folders as people
    /// see them; `None` when none is known.
    pub(crate) home: Option<PathBuf>,
    /// The direct entries, folders first, each in name order.
    pub(crate) entries: Vec<TypedEntry>,
    /// The folder holds more entries than [`MAX_ENTRIES`].
    pub(crate) truncated: bool,
}

/// A typed folder's entry as a row lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypedKnown {
    /// Its absolute path.
    pub(crate) path: PathBuf,
    /// Its own name, as Pane found it.
    pub(crate) name: String,
    /// Its folder for people: below the home folder as `~/…`, elsewhere
    /// the typed folder's own path.
    pub(crate) within: String,
    /// It is a folder.
    pub(crate) folder: bool,
    /// Whether opening it would run a program, as it was listed.
    pub(crate) program: bool,
}

/// The typed folders listed for each package, shared by the runtime (the
/// guests' import) and the launcher (the rows, and opening an entry).
/// Each package's latest listing is kept, replacing the one before it.
/// Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct TypedFolders(Arc<Mutex<BTreeMap<String, Kept>>>);

/// A package's latest typed listing, with the version its ids carry.
struct Kept {
    /// Grows with every listing of the package, so an id of an earlier
    /// one is not found.
    version: u64,
    listing: TypedListing,
}

impl TypedFolders {
    /// The entries of the folder `text`, the path the user typed, names,
    /// resolved as a typed path is, within the module's bounds, and kept
    /// as the package with identity key `owner`'s latest listing. An error
    /// explains a folder that cannot be listed.
    pub(crate) fn list(
        &self,
        owner: &str,
        text: &str,
        home: Option<&Path>,
    ) -> Result<TypedListing, String> {
        let mut listing = entries_of(text, home)?;
        // The version is taken before the ids are given, so a listing that
        // ends after a newer one of the same package cannot replace it:
        // the newer one's version is the greater.
        let version = lock(&self.0)
            .get(owner)
            .map(|kept| kept.version)
            .unwrap_or(0)
            + 1;
        for (index, entry) in listing.entries.iter_mut().enumerate() {
            entry.id = format!("t{version}.{index}");
        }
        let answer = listing.clone();
        let mut state = lock(&self.0);
        if version > state.get(owner).map(|kept| kept.version).unwrap_or(0) {
            state.insert(owner.to_owned(), Kept { version, listing });
        }
        Ok(answer)
    }

    /// The entry with id `id` in the package with identity key `owner`'s
    /// latest typed listing, as a row lists it; `None` for an id Pane did
    /// not give it.
    pub(crate) fn known(&self, owner: &str, id: &str) -> Option<TypedKnown> {
        let state = lock(&self.0);
        let kept = state.get(owner)?;
        let index = parse_id(id, kept.version)?;
        let entry = kept.listing.entries.get(index)?;
        let (name, within) = describe(&entry.path, kept.listing.home.as_deref());
        Some(TypedKnown {
            path: entry.path.clone(),
            name,
            within,
            folder: entry.folder,
            program: entry.program,
        })
    }

    /// Whether the package with identity key `owner`'s latest typed
    /// listing is partial: the folder holds more entries than Pane listed.
    pub(crate) fn partial(&self, owner: &str) -> bool {
        lock(&self.0)
            .get(owner)
            .is_some_and(|kept| kept.listing.truncated)
    }

    /// The entry with id `id` in the package with identity key `owner`'s
    /// latest typed listing, checked again now that Pane is to act on it:
    /// it is still in that listing, not on a network path, still there and
    /// of the kind it was listed as, not a link, and its canonical path is
    /// still inside the typed folder's. Unless `programs`, opening it must
    /// not run a program either, as [`crate::files`] checks a granted
    /// folder's file. Blocking.
    pub(crate) fn checked(&self, owner: &str, id: &str, programs: bool) -> Result<PathBuf, String> {
        let (path, is_folder, folder) = {
            let state = lock(&self.0);
            let kept = state
                .get(owner)
                .ok_or("it is not in the extension's latest listing; search again")?;
            let index = parse_id(id, kept.version)
                .ok_or("it is not in the extension's latest listing; search again")?;
            let entry = kept
                .listing
                .entries
                .get(index)
                .ok_or("it is not in the extension's latest listing; search again")?;
            (
                entry.path.clone(),
                entry.folder,
                kept.listing.folder.clone(),
            )
        };
        if is_network_path(&path) {
            return Err("it is on a network location".into());
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err("it no longer exists".into());
            }
            Err(error) => return Err(format!("Pane cannot read it: {error}")),
        };
        if metadata.file_type().is_symlink() {
            return Err("it is now a link; Pane opens only entries of the folder you typed".into());
        }
        if metadata.is_dir() != is_folder {
            return Err(if is_folder {
                "it is now a file".into()
            } else {
                "it is now a folder".into()
            });
        }
        if !is_folder && !metadata.is_file() {
            return Err("it is no longer a regular file".into());
        }
        let resolved = canonical(&path).map_err(|error| format!("Pane cannot read it: {error}"))?;
        let within = canonical(&folder)
            .map_err(|error| format!("Pane cannot read the folder you typed: {error}"))?;
        if !resolved.starts_with(&within) {
            return Err("it is no longer inside the folder you typed".into());
        }
        if !programs && !is_folder && runs_as_program(&resolved, &metadata) {
            return Err("it is a program or script, which opening would run".into());
        }
        Ok(resolved)
    }
}

/// The entries of the folder `text` names, within the module's bounds (see
/// the module docs); an error explains a folder that cannot be listed.
fn entries_of(text: &str, home: Option<&Path>) -> Result<TypedListing, String> {
    let shown = text.trim();
    let Some(resolved) = typed_query::path_like(shown, home) else {
        return Err(format!("“{shown}” is not a path"));
    };
    let folder = PathBuf::from(&resolved);
    if is_network_path(&folder) {
        return Err(format!(
            "{resolved} is a network location; Pane lists only folders on this computer"
        ));
    }
    if !folder.is_absolute() {
        return Err(format!("“{resolved}” is not a full path"));
    }
    match fs::metadata(&folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(format!("{resolved} is a file, not a folder")),
        Err(error) => return Err(unreadable(&resolved, &error)),
    }
    let read = fs::read_dir(&folder).map_err(|error| unreadable(&resolved, &error))?;
    // Entries are counted as the folder is read, so a huge folder stops at
    // the entry bound rather than being read whole first.
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in read {
        if entries.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => truncated = true,
        }
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut listed = Vec::new();
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if hidden(&name, &entry) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            truncated = true;
            continue;
        };
        // A link is listed by neither the index nor a granted folder's
        // walk, and never followed.
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            listed.push(TypedEntry {
                id: String::new(),
                path,
                name,
                folder: true,
                program: false,
            });
        } else if kind.is_file() {
            listed.push(TypedEntry {
                id: String::new(),
                path,
                name,
                folder: false,
                program: program_at(&path),
            });
        }
        // Anything else (a socket, a fifo) is neither.
    }
    // Folders first, the name order kept within each: a stable sort over
    // what is already in name order.
    listed.sort_by_key(|entry| !entry.folder);
    Ok(TypedListing {
        folder,
        home: home.map(PathBuf::from),
        entries: listed,
        truncated,
    })
}

/// The index of the entry with id `id` in the listing of `version`, if it
/// is one of its ids.
fn parse_id(id: &str, version: u64) -> Option<usize> {
    let rest = id.strip_prefix('t')?;
    let (of, index) = rest.split_once('.')?;
    (of == version.to_string()).then(|| index.parse().ok())?
}

/// The name of the entry at `path` and its folder for people, as the file
/// index describes an entry: below the home folder as `~/…`, elsewhere its
/// full path.
fn describe(path: &Path, home: Option<&Path>) -> (String, String) {
    crate::file_index::describe(path, home)
}

/// Runs `work` on a thread of its own, since a folder may block (a network
/// mount the host could not tell apart), and answers what it answered;
/// what a thread that could not start answers instead.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    gone: impl FnOnce() -> T,
) -> T {
    let (reply, response) = tokio::sync::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("pane-typed-folder".into())
        .spawn(move || {
            let _ = reply.send(work());
        });
    if started.is_err() {
        return gone();
    }
    response.await.unwrap_or_else(|_| gone())
}

/// What a command is told when the listing's thread could not start.
fn failed() -> String {
    "Pane could not list the folder (its thread failed)".into()
}

/// The home folder `~` in a typed path resolves to: the one the file index
/// is configured with — the same folder the launcher resolves `~` to —
/// else the environment's.
fn home(access: &crate::files::FileAccess) -> Option<PathBuf> {
    access
        .indexer()
        .base_rules()
        .and_then(|rules| rules.home)
        .or_else(crate::launcher::home_folder)
}

/// The host side of `pane:extension/typed-folder`: lists the folder the
/// user typed for the command's package's answer of root results.
impl wit::Host for GuestState {
    async fn list_entries(&mut self, folder: String) -> Result<wit::FolderListing, String> {
        // Answers from what the listing thread found; the runtime thread
        // awaits it, serving other packages' calls meanwhile.
        let _host = self.host();
        // Stopped code starts no more work: checked once the host call is
        // marked, from when Pane no longer decides to give up on the
        // thread.
        if let Some(end) = self.stopped() {
            return Err(crate::runtime::stopped_code(end));
        }
        let Some(owner) = self.owner() else {
            return Err("Pane lists a typed folder for installed extensions".into());
        };
        let access = self.file_access();
        let home = home(&access);
        self.hosted(off_thread(
            move || access.typed().list(&owner, folder.trim(), home.as_deref()),
            || Err(failed()),
        ))
        .await
        .map(|listing| wit::FolderListing {
            entries: listing
                .entries
                .into_iter()
                .map(|entry| wit::FolderEntry {
                    id: entry.id,
                    name: entry.name,
                    folder: entry.folder,
                    program: entry.program,
                })
                .collect(),
            truncated: listing.truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of controlled fixtures:
    ///
    /// ```text
    /// Pane typed — ñ/
    ///   Zed notes ü.md
    ///   alpha.txt
    ///   .hidden.txt
    ///   run plan.bat
    ///   notes/
    ///     plan.md
    ///     link.md -> ../alpha.txt   (Unix only)
    /// ```
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Pane typed — ñ");
        for (file, text) in [
            ("Zed notes ü.md", "zed"),
            ("alpha.txt", "alpha"),
            (".hidden.txt", "hidden"),
            ("run plan.bat", "@echo off"),
            ("notes/plan.md", "plan"),
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("alpha.txt"), root.join("notes/link.md")).unwrap();
        (dir, root)
    }

    /// The listing of the folder `path` names, as `owner`'s.
    fn listed(owner: &str, path: &Path) -> TypedListing {
        TypedFolders::default()
            .list(owner, &path.to_string_lossy(), None)
            .unwrap()
    }

    #[test]
    fn a_typed_folder_lists_its_direct_entries_folders_first_in_name_order() {
        let (_dir, root) = fixture();
        let listing = listed("owner", &root);
        let names: Vec<&str> = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        // Folders first, then the files by the bytes of their names; the
        // hidden file is skipped, and so is the link.
        assert_eq!(
            names,
            ["notes", "Zed notes ü.md", "alpha.txt", "run plan.bat"]
        );
        assert!(listing.entries[0].folder);
        assert!(!listing.entries[1].folder);
        assert!(!listing.truncated);
        for entry in &listing.entries {
            assert!(entry.path.starts_with(&root), "{:?}", entry.path);
            assert!(!entry.id.is_empty(), "the entry has an id");
        }
        // The program is told apart, so Enter can show it rather than run
        // it.
        assert!(listing.entries[3].program);
        assert!(!listing.entries[1].program);
    }

    #[test]
    fn a_folder_with_more_than_500_entries_is_listed_partially() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("many");
        fs::create_dir(&root).unwrap();
        for number in 0..=MAX_ENTRIES {
            fs::write(root.join(format!("file {number:04}.txt")), "x").unwrap();
        }
        let listing = listed("owner", &root);
        assert_eq!(listing.entries.len(), MAX_ENTRIES);
        assert!(listing.truncated, "the listing says it is partial");
        // The first 500 in name order: the last file is not among them.
        assert_eq!(listing.entries[0].name, "file 0000.txt");
        assert_eq!(listing.entries.last().unwrap().name, "file 0499.txt");
        assert!(
            !listing
                .entries
                .iter()
                .any(|entry| entry.name == "file 0500.txt")
        );
    }

    #[test]
    fn a_folder_that_cannot_be_listed_is_an_error() {
        let (_dir, root) = fixture();
        let missing = root.parent().unwrap().join("not there");
        let problem = TypedFolders::default()
            .list("owner", &missing.to_string_lossy(), None)
            .unwrap_err();
        assert!(problem.contains("does not exist"), "{problem}");
        // A file is not a folder, and words are not a path.
        let file = root.join("alpha.txt");
        let problem = TypedFolders::default()
            .list("owner", &file.to_string_lossy(), None)
            .unwrap_err();
        assert!(problem.contains("is a file, not a folder"), "{problem}");
        let problem = TypedFolders::default()
            .list("owner", "hello there", None)
            .unwrap_err();
        assert!(problem.contains("is not a path"), "{problem}");
    }

    #[test]
    fn a_tilde_resolves_to_the_home_folder_and_file_urls_are_taken_off() {
        let (_dir, root) = fixture();
        let home = root.parent().unwrap().to_path_buf();
        let below = format!("~/{}/", root.file_name().unwrap().to_string_lossy());
        let listing = TypedFolders::default()
            .list("owner", &below, Some(&home))
            .unwrap();
        assert_eq!(listing.folder, root);
        assert_eq!(listing.home.as_deref(), Some(home.as_path()));
        let url = format!("file://{}", root.to_string_lossy());
        let listing = TypedFolders::default()
            .list("owner", &url, Some(&home))
            .unwrap();
        assert_eq!(listing.folder, root);
    }

    #[test]
    fn an_entry_is_found_by_its_id_and_checked_again_before_it_opens() {
        let (_dir, root) = fixture();
        let folders = TypedFolders::default();
        let listing = folders
            .list("owner", &root.to_string_lossy(), None)
            .unwrap();
        let alpha = listing
            .entries
            .iter()
            .find(|entry| entry.name == "alpha.txt")
            .unwrap();
        // The row knows the entry by its id, and its folder for people.
        let known = folders.known("owner", &alpha.id).unwrap();
        assert_eq!(known.name, "alpha.txt");
        assert_eq!(known.within, root.to_string_lossy());
        assert!(!known.program);
        // Checked again, it opens (its canonical path); a program is
        // refused as an open, and only Run and Reveal may act on it.
        let opened = folders.checked("owner", &alpha.id, false).unwrap();
        assert_eq!(
            fs::canonicalize(&opened).unwrap(),
            fs::canonicalize(&alpha.path).unwrap()
        );
        let bat = listing
            .entries
            .iter()
            .find(|entry| entry.name == "run plan.bat")
            .unwrap();
        assert!(
            folders
                .checked("owner", &bat.id, false)
                .unwrap_err()
                .contains("would run")
        );
        assert!(folders.checked("owner", &bat.id, true).is_ok());
        // An id of an earlier listing is not found: the listing replaced
        // it.
        let again = folders
            .list("owner", &root.to_string_lossy(), None)
            .unwrap();
        assert!(folders.known("owner", &alpha.id).is_none());
        assert!(folders.known("owner", &again.entries[0].id).is_some());
    }
}
