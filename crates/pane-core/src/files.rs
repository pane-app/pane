//! The files of the folder the user granted a package: the grant, listing
//! it for the package's commands, and checking a file before it is opened.
//!
//! A WASI guest has no folders to read (its WASI context preopens none), so
//! Pane's host lists a folder for the Files default extension through the
//! `pane:extension/files` import. The host owns every part that decides what
//! is reached ([ADR 0017](../../../docs/adr/0017-host-lists-a-granted-folder-for-an-extension.md)):
//!
//! - **The grant.** The user chooses the folder with Pane's own "Choose
//!   folder" row and the system's picker; [`check_grant`] refuses the file
//!   system's root, the home folder itself, a hidden folder and network
//!   (UNC) paths, and Pane records the canonical folder per package
//!   identity in its own `folders.json`, never in the extension's data. The
//!   guest never names a path.
//! - **The listing** ([`walk`], [`Limits`]): bounded, breadth first, counting
//!   entries and checking for cancellation while it reads each folder, on
//!   one worker thread per package, which takes only the newest request. It
//!   is kept for the visit of root search it was made in, so keystrokes
//!   filter the kept listing; the guest's call never waits for it.
//! - **Opening**: a result names a file by the id the host gave it
//!   ([`FileAccess::checked_file`]); before it is opened the host checks
//!   again that it is a regular file inside the grant, not a link, and not
//!   a program or script ([`runs_as_program`]).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use tokio::sync::watch;

use crate::atomic::{Readers, write_atomically};
use crate::generation::Generation;
use crate::runtime::{GuestState, bindings, lock};

use bindings::pane::extension::files as wit;

/// How many folders below the granted one Pane enters.
pub const MAX_DEPTH: usize = 8;

/// How many files one listing holds at most.
pub const MAX_FILES: usize = 5_000;

/// How many folder entries (files, folders, links, anything) one listing
/// looks at, at most.
pub const MAX_ENTRIES: usize = 20_000;

/// How long a listing waits after it is asked for before it starts, so that
/// leaving root search at once starts none.
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// The file Pane records the granted folders in, beside `installed.json`.
pub const RECORD_FILE: &str = "folders.json";

/// One file a listing found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// Its absolute path, inside the listed folder.
    pub path: PathBuf,
    /// Its path below the listed folder, with `/` between names.
    pub relative: String,
}

/// What a listing found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderListing {
    /// The files, breadth first, each folder's entries in name order.
    pub files: Vec<Listed>,
    /// The listing did not look at everything: it stopped at one of its
    /// [`Limits`], or a subfolder could not be read.
    pub truncated: bool,
}

/// Lists the files of folders; replaceable, for tests.
pub trait Folders: Send + Sync + 'static {
    /// Lists `folder`, a canonical granted folder, within `limits`. Called
    /// on the package's worker thread; once `cancelled` returns true the
    /// answer is not wanted any more (root search was left, the grant
    /// changed or the package stopped), so the listing should stop.
    fn list(
        &self,
        folder: &Path,
        limits: &Limits,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FolderListing, String>;
}

/// This system's folders, listed by [`walk`].
pub fn native() -> Arc<dyn Folders> {
    Arc::new(Native)
}

struct Native;

impl Folders for Native {
    fn list(
        &self,
        folder: &Path,
        limits: &Limits,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FolderListing, String> {
        walk(folder, limits, cancelled)
    }
}

/// The bounds of one listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How many folders below the listed one are entered.
    pub depth: usize,
    /// How many files are returned at most.
    pub files: usize,
    /// How many entries are looked at, at most.
    pub entries: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            depth: MAX_DEPTH,
            files: MAX_FILES,
            entries: MAX_ENTRIES,
        }
    }
}

/// Lists the regular files in `folder` and its subfolders, breadth first
/// and each folder in name order, within `limits`. Hidden entries (a name
/// starting with `.`; on Windows also the hidden and system attributes),
/// symbolic links and other links, and names that are not Unicode are
/// neither listed nor entered. Entries are counted, and `cancelled` checked,
/// as each folder is read, so a huge folder stops at the entry limit; a
/// subfolder that cannot be read makes the listing `truncated`. Only the
/// paths of the folders still to list are queued, not open handles.
pub fn walk(
    folder: &Path,
    limits: &Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<FolderListing, String> {
    let shown = folder.display().to_string();
    if !folder.is_absolute() {
        return Err(format!("“{shown}” is not a full path"));
    }
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(format!("{shown} is a file, not a folder")),
        Err(error) => return Err(unreadable(&shown, &error)),
    }
    let mut listing = FolderListing::default();
    let mut seen = 0;
    // Folders still to list: their path, their path below `folder` and how
    // deep they are.
    let mut pending = VecDeque::from([(folder.to_path_buf(), String::new(), 0)]);
    while let Some((dir, below, depth)) = pending.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if below.is_empty() => return Err(unreadable(&shown, &error)),
            Err(_) => {
                listing.truncated = true;
                continue;
            }
        };
        let mut read = Vec::new();
        for entry in entries {
            if cancelled() {
                return Err("the listing was cancelled".into());
            }
            seen += 1;
            if seen > limits.entries {
                listing.truncated = true;
                break;
            }
            match entry {
                Ok(entry) => read.push(entry),
                Err(_) => listing.truncated = true,
            }
        }
        read.sort_by_key(fs::DirEntry::file_name);
        for entry in read {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if hidden(&name, &entry) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                listing.truncated = true;
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let relative = if below.is_empty() {
                name
            } else {
                format!("{below}/{name}")
            };
            if kind.is_dir() {
                if depth >= limits.depth {
                    listing.truncated = true;
                } else {
                    pending.push_back((entry.path(), relative, depth + 1));
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            if listing.files.len() == limits.files {
                listing.truncated = true;
                return Ok(listing);
            }
            listing.files.push(Listed {
                path: entry.path(),
                relative,
            });
        }
        if seen > limits.entries {
            return Ok(listing);
        }
    }
    Ok(listing)
}

/// Why `folder` cannot be listed, for the user.
fn unreadable(folder: &str, error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => format!("{folder} does not exist"),
        io::ErrorKind::PermissionDenied => format!("Pane may not read {folder}"),
        _ => format!("Pane cannot read {folder}: {error}"),
    }
}

/// Whether the entry `name` is hidden: its name starts with a dot, or on
/// Windows it has the hidden or system attribute.
fn hidden(name: &str, entry: &fs::DirEntry) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    if let Ok(metadata) = entry.metadata() {
        return hidden_attributes(&metadata);
    }
    #[cfg(not(windows))]
    let _ = entry;
    false
}

/// Whether `metadata`, a directory entry's own, has the hidden or system
/// attribute.
#[cfg(windows)]
fn hidden_attributes(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

/// Whether `path` names a network or device location Pane never grants or
/// opens: on Windows a UNC path (`\\server\share`, `\\?\UNC\...`) or a
/// device path, told from the text alone, before any file system call.
pub fn is_network_path(path: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let text = path.to_string_lossy();
    let text = text.replace('/', "\\");
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.len() < 3 || rest.get(1..3) != Some(":\\"),
        None => text.starts_with(r"\\"),
    }
}

/// `path` without Windows' verbatim prefix (`\\?\C:\...` becomes
/// `C:\...`), as canonicalizing gives it; other paths are unchanged.
fn plain(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\")
            && rest.get(1..3) == Some(":\\")
        {
            return PathBuf::from(rest);
        }
    }
    path
}

/// The canonical path of `path`, with no verbatim prefix.
fn canonical(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path).map(plain)
}

/// The home folder, canonical, if the environment names one.
fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(var)?;
    canonical(Path::new(&home)).ok()
}

/// Checks that `folder`, as the user chose it, may be granted, and returns
/// its canonical path: an absolute path to a folder the user can read, not
/// a network (UNC) path, not the file system's root or a drive's, not the
/// home folder itself, and not hidden.
pub fn check_grant(folder: &Path) -> Result<PathBuf, String> {
    let shown = folder.display().to_string();
    if is_network_path(folder) {
        return Err(format!(
            "{shown} is a network location; Pane lists only folders on this computer"
        ));
    }
    if !folder.is_absolute() {
        return Err(format!("“{shown}” is not a full path"));
    }
    let resolved = canonical(folder).map_err(|error| unreadable(&shown, &error))?;
    if is_network_path(&resolved) {
        return Err(format!(
            "{shown} is a network location; Pane lists only folders on this computer"
        ));
    }
    let metadata = fs::metadata(&resolved).map_err(|error| unreadable(&shown, &error))?;
    if !metadata.is_dir() {
        return Err(format!("{shown} is a file, not a folder"));
    }
    if resolved.parent().is_none() {
        return Err(format!(
            "{shown} is the whole disk; choose a folder inside it"
        ));
    }
    if home().is_some_and(|home| home == resolved) {
        return Err(format!(
            "{shown} is your home folder; choose a folder inside it"
        ));
    }
    let name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    #[cfg(windows)]
    let hidden = name.starts_with('.') || hidden_attributes(&metadata);
    #[cfg(not(windows))]
    let hidden = name.starts_with('.');
    if hidden {
        return Err(format!(
            "{shown} is a hidden folder; Pane does not list hidden folders"
        ));
    }
    fs::read_dir(&resolved).map_err(|error| unreadable(&shown, &error))?;
    Ok(resolved)
}

/// The file types that run a program when the system opens them, on any of
/// the systems Pane runs on: file search's Enter never opens them, anywhere
/// (it reveals them; only the explicit Run action runs them).
const PROGRAM_EXTENSIONS: &[&str] = &[
    // Windows
    "exe", "bat", "cmd", "com", "lnk", "js", "jse", "vbs", "vbe", "wsf", "wsh", "hta", "msi", "msp",
    "scr", "pif", "ps1", "cpl", "reg", "url", // macOS
    "app", "command", "tool", "terminal", "workflow", // Linux
    "desktop",
];

/// Whether opening `path` would run it as a program: its type is one of
/// [`PROGRAM_EXTENSIONS`] (compared caselessly), a folder above it is a
/// macOS application bundle (`.app`), or on macOS and Linux it has an
/// executable bit.
pub fn runs_as_program(path: &Path, metadata: &fs::Metadata) -> bool {
    if program_named(path) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            return true;
        }
    }
    #[cfg(not(unix))]
    let _ = metadata;
    false
}

/// Whether `path`'s name alone says that opening it runs a program: its
/// type is one of [`PROGRAM_EXTENSIONS`], or a folder above it is a macOS
/// application bundle.
fn program_named(path: &Path) -> bool {
    let program_type = |name: &std::ffi::OsStr| {
        Path::new(name)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                PROGRAM_EXTENSIONS
                    .iter()
                    .any(|program| program.eq_ignore_ascii_case(extension))
            })
    };
    if path.file_name().is_some_and(program_type) {
        return true;
    }
    path.ancestors().skip(1).any(|folder| {
        folder
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    })
}

/// Whether the file a listing found at `path` runs as a program
/// ([`runs_as_program`]), as it is now; by its name alone when it cannot be
/// read.
fn program_at(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => runs_as_program(path, &metadata),
        Err(_) => program_named(path),
    }
}

/// Where a package's granted folder's listing is, as its commands see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FolderState {
    NotGranted,
    Listing,
    /// The kept listing: each file's id and path below the folder.
    Ready {
        files: Vec<(String, String)>,
        truncated: bool,
    },
}

/// A file a package's listing found, as the host knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KnownFile {
    /// The file's own name.
    pub name: String,
    /// Its folder, for people: the granted folder's name and the
    /// subfolders below it.
    pub within: String,
    /// Whether it is a program or script ([`runs_as_program`]), as it was
    /// when it was listed: file search's Enter reveals it rather than open
    /// it.
    pub program: bool,
}

/// The granted folders, the listings kept for them and the workers making
/// them, shared by the runtime (the guests' import) and the launcher (the
/// grant, the rows and opening). Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct FileAccess(Arc<Access>);

#[derive(Default)]
struct Access {
    folders: Mutex<Option<Arc<dyn Folders>>>,
    record: Mutex<Option<PathBuf>>,
    state: Mutex<AccessState>,
}

#[derive(Default)]
struct AccessState {
    /// Increments on each visit of root search (and on leaving it).
    visit: u64,
    /// The granted folder of each package, by identity key, canonical.
    grants: BTreeMap<String, PathBuf>,
    /// Increments on every grant change, so a listing of an older grant is
    /// told apart.
    version: u64,
    packages: HashMap<String, PackageFiles>,
}

struct PackageFiles {
    kept: Option<Kept>,
    /// The visit and grant version of the listing being made, if any.
    running: Option<(u64, u64)>,
    /// The visit and grant version whose listing the package's latest
    /// `list-folder()` was told is still being made, if it was told that.
    told_listing: Option<(u64, u64)>,
    /// The visit and grant version of the latest listing that ended, kept
    /// or not. Both only grow, so a wait for one listing ends when it, or a
    /// newer one that replaced it, has ended, and never when an older one
    /// stopped late.
    done: watch::Sender<(u64, u64)>,
    /// The package's worker, started with its first listing.
    worker: Option<mpsc::Sender<Job>>,
}

impl Default for PackageFiles {
    fn default() -> PackageFiles {
        PackageFiles {
            kept: None,
            running: None,
            told_listing: None,
            done: watch::Sender::new((0, 0)),
            worker: None,
        }
    }
}

/// A listing kept for one visit and grant.
struct Kept {
    visit: u64,
    version: u64,
    folder: PathBuf,
    result: Result<FolderListing, String>,
    /// Whether each listed file is a program or script, in the listing's
    /// order, looked at on the worker with the listing.
    programs: Vec<bool>,
}

/// A listing a package's worker is asked to make.
struct Job {
    owner: String,
    folder: PathBuf,
    visit: u64,
    version: u64,
    generation: Option<Generation>,
}

impl FileAccess {
    fn state(&self) -> std::sync::MutexGuard<'_, AccessState> {
        lock(&self.0.state)
    }

    /// Lists folders with `folders` from now on, instead of this system's
    /// own.
    pub(crate) fn set_folders(&self, folders: Arc<dyn Folders>) {
        *lock(&self.0.folders) = Some(folders);
    }

    fn folders(&self) -> Arc<dyn Folders> {
        lock(&self.0.folders).clone().unwrap_or_else(native)
    }

    /// Reads the grants recorded in `dir`'s [`RECORD_FILE`], and records
    /// later changes there. An unreadable record grants nothing.
    pub(crate) fn open_record(&self, dir: &Path) {
        let file = dir.join(RECORD_FILE);
        let grants = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.get("folders").and_then(Value::as_object).cloned())
            .unwrap_or_default();
        let mut state = self.state();
        state.grants = grants
            .into_iter()
            .filter_map(|(owner, folder)| Some((owner, PathBuf::from(folder.as_str()?))))
            .collect();
        state.version += 1;
        drop(state);
        *lock(&self.0.record) = Some(file);
    }

    fn write_record(&self, grants: &BTreeMap<String, PathBuf>) -> Result<(), String> {
        let Some(file) = lock(&self.0.record).clone() else {
            return Ok(());
        };
        let folders: Map<String, Value> = grants
            .iter()
            .map(|(owner, folder)| (owner.clone(), Value::from(folder.to_string_lossy())))
            .collect();
        let record = serde_json::json!({ "version": 1, "folders": folders });
        let text = serde_json::to_string_pretty(&record).expect("a record is JSON");
        write_atomically(&file, text.as_bytes(), Readers::Default)
            .map_err(|error| format!("Pane could not record the folder: {error}"))
    }

    /// The folder granted to the package with identity key `owner`.
    pub(crate) fn granted(&self, owner: &str) -> Option<PathBuf> {
        self.state().grants.get(owner).cloned()
    }

    /// Grants the package with identity key `owner` the folder `folder`,
    /// once [`check_grant`] accepts it, replacing any earlier grant, and
    /// records it. Returns the canonical folder. Blocking.
    pub(crate) fn grant(&self, owner: &str, folder: &Path) -> Result<PathBuf, String> {
        let folder = check_grant(folder)?;
        self.change(owner, Some(folder.clone()))?;
        Ok(folder)
    }

    /// Takes back the folder granted to the package with identity key
    /// `owner`, and records that. Blocking.
    pub(crate) fn revoke(&self, owner: &str) -> Result<(), String> {
        self.change(owner, None)
    }

    fn change(&self, owner: &str, folder: Option<PathBuf>) -> Result<(), String> {
        let mut grants = self.state().grants.clone();
        match &folder {
            Some(folder) => grants.insert(owner.to_owned(), folder.clone()),
            None => grants.remove(owner),
        };
        self.write_record(&grants)?;
        let mut state = self.state();
        match folder {
            Some(folder) => state.grants.insert(owner.to_owned(), folder),
            None => state.grants.remove(owner),
        };
        state.version += 1;
        if let Some(package) = state.packages.get_mut(owner) {
            package.kept = None;
        }
        Ok(())
    }

    /// Notes a new visit of root search, or that it was left: the listings
    /// kept are dropped, and those being made stop.
    pub(crate) fn new_visit(&self) {
        let mut state = self.state();
        state.visit += 1;
        for package in state.packages.values_mut() {
            package.kept = None;
        }
    }

    /// The granted folder's listing for the package with identity key
    /// `owner`, starting one on its worker if none is kept or being made for
    /// this visit. Never waits.
    pub(crate) fn folder_state(
        &self,
        owner: &str,
        generation: Option<&Generation>,
    ) -> Result<FolderState, String> {
        let mut state = self.state();
        let (visit, version) = (state.visit, state.version);
        let Some(folder) = state.grants.get(owner).cloned() else {
            return Ok(FolderState::NotGranted);
        };
        let package = state.packages.entry(owner.to_owned()).or_default();
        if let Some(kept) = &package.kept
            && kept.visit == visit
            && kept.version == version
        {
            package.told_listing = None;
            return match &kept.result {
                Ok(listing) => Ok(FolderState::Ready {
                    files: listing
                        .files
                        .iter()
                        .enumerate()
                        .map(|(index, file)| (token(visit, version, index), file.relative.clone()))
                        .collect(),
                    truncated: listing.truncated,
                }),
                Err(problem) => Err(problem.clone()),
            };
        }
        if package.running != Some((visit, version)) {
            package.running = Some((visit, version));
            let job = Job {
                owner: owner.to_owned(),
                folder,
                visit,
                version,
                generation: generation.cloned(),
            };
            let worker = match &package.worker {
                Some(worker) => worker.clone(),
                None => {
                    let worker = self.start_worker()?;
                    package.worker = Some(worker.clone());
                    worker
                }
            };
            if worker.send(job).is_err() {
                package.running = None;
                package.worker = None;
                package.told_listing = None;
                return Err("Pane's folder listing for this extension stopped".into());
            }
        }
        package.told_listing = Some((visit, version));
        Ok(FolderState::Listing)
    }

    /// When the package with identity key `owner` was last told this
    /// visit's listing is still being made: resolves once that listing ends
    /// (at once if it already has), or a newer one that replaced it. An
    /// older listing that ends meanwhile (one stopped when root search was
    /// left) does not resolve it. `None` when the package was not told so,
    /// or was told so in an earlier visit or of an earlier grant.
    pub(crate) fn listed(&self, owner: &str) -> Option<impl Future<Output = ()> + Send + 'static> {
        let state = self.state();
        let current = (state.visit, state.version);
        let package = state.packages.get(owner)?;
        if package.told_listing != Some(current) {
            return None;
        }
        let mut done = package.done.subscribe();
        Some(async move {
            let _ = done.wait_for(|ended| *ended >= current).await;
        })
    }

    /// The file with id `id` in the latest listing of the package with
    /// identity key `owner`, as the host names it.
    pub(crate) fn known(&self, owner: &str, id: &str) -> Option<KnownFile> {
        let (_, relative, folder, program) = self.lookup(owner, id)?;
        let name = relative.rsplit('/').next().unwrap_or(&relative).to_owned();
        let folder_name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.display().to_string());
        let within = match relative.rsplit_once('/') {
            Some((parent, _)) => format!("{folder_name}/{parent}"),
            None => folder_name,
        };
        Some(KnownFile {
            name,
            within,
            program,
        })
    }

    /// The path, path below the folder and folder of the file with id `id`,
    /// and whether it was a program or script when it was listed.
    fn lookup(&self, owner: &str, id: &str) -> Option<(PathBuf, String, PathBuf, bool)> {
        let state = self.state();
        let kept = state.packages.get(owner)?.kept.as_ref()?;
        let listing = kept.result.as_ref().ok()?;
        let index = parse_token(id, kept.visit, kept.version)?;
        let file = listing.files.get(index)?;
        Some((
            file.path.clone(),
            file.relative.clone(),
            kept.folder.clone(),
            kept.programs.get(index).copied().unwrap_or(false),
        ))
    }

    /// The file with id `id` of the package with identity key `owner`,
    /// checked again now that Pane is to act on it: it is in the package's
    /// latest listing, its folder is still the package's grant, it is not a
    /// network path, it is a regular file and not a link, and its canonical
    /// path is inside the grant's. Unless `programs`, opening it must not
    /// run a program either: what file search's Open (and Enter) checks,
    /// where Run, Reveal, Open With…, the copies and the Recycle Bin act on
    /// a program as on any file. Blocking.
    pub(crate) fn checked_file(
        &self,
        owner: &str,
        id: &str,
        programs: bool,
    ) -> Result<PathBuf, String> {
        let (path, _, folder, _) = self
            .lookup(owner, id)
            .ok_or("it is not in the extension's latest listing; search again")?;
        if self.granted(owner).as_ref() != Some(&folder) {
            return Err("its folder is no longer the one granted to the extension".into());
        }
        if is_network_path(&path) {
            return Err("it is on a network location".into());
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err("it no longer exists".into());
            }
            Err(error) => return Err(format!("Pane cannot read it: {error}")),
        };
        if metadata.file_type().is_symlink() {
            return Err("it is now a link; Pane opens only files in the granted folder".into());
        }
        if !metadata.is_file() {
            return Err("it is no longer a regular file".into());
        }
        let resolved = canonical(&path).map_err(|error| format!("Pane cannot read it: {error}"))?;
        let within = canonical(&folder)
            .map_err(|error| format!("Pane cannot read the granted folder: {error}"))?;
        if within != folder || !resolved.starts_with(&within) {
            return Err("it is no longer inside the granted folder".into());
        }
        if !programs && runs_as_program(&resolved, &metadata) {
            return Err("it is a program or script, which opening would run".into());
        }
        Ok(resolved)
    }

    /// Starts the worker thread that lists the granted folder of the package
    /// with identity key `owner`, one listing at a time, the newest asked.
    fn start_worker(&self) -> Result<mpsc::Sender<Job>, String> {
        let (jobs, received) = mpsc::channel::<Job>();
        let access = Arc::downgrade(&self.0);
        std::thread::Builder::new()
            .name("pane-files".into())
            .spawn(move || {
                while let Ok(mut job) = received.recv() {
                    // Only the newest request is listed.
                    while let Ok(newer) = received.try_recv() {
                        job = newer;
                    }
                    let Some(access) = access.upgrade() else {
                        return;
                    };
                    FileAccess(access).run(job);
                }
            })
            .map_err(|error| format!("Pane could not start listing the folder: {error}"))?;
        Ok(jobs)
    }

    /// Whether `job` is no longer wanted.
    fn stale(&self, job: &Job) -> bool {
        let state = self.state();
        state.visit != job.visit
            || state.version != job.version
            || job.generation.as_ref().is_some_and(|g| g.ended().is_some())
    }

    /// Makes the listing `job` asks for, on the worker, and keeps it if it
    /// is still wanted.
    fn run(&self, job: Job) {
        let debounced = Instant::now() + DEBOUNCE;
        while Instant::now() < debounced && !self.stale(&job) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = if self.stale(&job) {
            None
        } else {
            let folders = self.folders();
            let cancelled = || self.stale(&job);
            let result = folders.list(&job.folder, &Limits::default(), &cancelled);
            // Which files are programs, looked at here, off every caller's
            // thread, so a row knows what Enter does without reading the
            // disk.
            let programs = match &result {
                Ok(listing) => listing
                    .files
                    .iter()
                    .map(|file| program_at(&file.path))
                    .collect(),
                Err(_) => Vec::new(),
            };
            Some((result, programs))
        };
        let mut state = self.state();
        let current = (state.visit, state.version);
        let package = state.packages.entry(job.owner.clone()).or_default();
        if package.running == Some((job.visit, job.version)) {
            package.running = None;
        }
        if let Some((result, programs)) = result
            && current == (job.visit, job.version)
            && !job.generation.as_ref().is_some_and(|g| g.ended().is_some())
        {
            package.kept = Some(Kept {
                visit: job.visit,
                version: job.version,
                folder: job.folder,
                result,
                programs,
            });
        }
        let ended = (job.visit, job.version);
        package.done.send_if_modified(|done| {
            let newer = ended > *done;
            if newer {
                *done = ended;
            }
            newer
        });
    }
}

/// The id of the file at `index` in the listing of `visit` and `version`.
fn token(visit: u64, version: u64, index: usize) -> String {
    format!("{visit}.{version}.{index}")
}

/// The index of the file with id `id` in the listing of `visit` and
/// `version`, if it is one of its ids.
fn parse_token(id: &str, visit: u64, version: u64) -> Option<usize> {
    let mut parts = id.split('.');
    let (v, g, index) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || v != visit.to_string() || g != version.to_string() {
        return None;
    }
    index.parse().ok()
}

/// The host side of `pane:extension/files`: answers the calling package's
/// granted folder's listing, never waiting for one.
impl wit::Host for GuestState {
    fn list_folder(&mut self) -> Result<wit::FolderState, String> {
        // Answers from what the listing worker found; never waits for it.
        let _host = self.host();
        // Stopped code starts no more work: checked once the host call is
        // marked, from when Pane no longer decides to give up on the thread.
        if let Some(end) = self.stopped() {
            return Err(crate::runtime::stopped_code(end));
        }
        let Some(owner) = self.owner() else {
            return Ok(wit::FolderState::NotGranted);
        };
        let generation = self.generation().cloned();
        Ok(
            match self
                .file_access()
                .folder_state(&owner, generation.as_ref())?
            {
                FolderState::NotGranted => wit::FolderState::NotGranted,
                FolderState::Listing => wit::FolderState::Listing,
                FolderState::Ready { files, truncated } => {
                    wit::FolderState::Ready(wit::FolderListing {
                        files: files
                            .into_iter()
                            .map(|(id, relative)| wit::FoundFile { id, relative })
                            .collect(),
                        truncated,
                    })
                }
            },
        )
    }

    fn limits(&mut self) -> wit::ScanLimits {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        wit::ScanLimits {
            depth: count(MAX_DEPTH),
            files: count(MAX_FILES),
            entries: count(MAX_ENTRIES),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Condvar;
    use std::time::Duration;

    use futures::FutureExt;
    use futures::executor::block_on;

    use super::*;

    /// A folder lister whose listings each return only when the test lets
    /// one return, whether cancelled or not: a listing that stops late.
    #[derive(Default)]
    struct Gated {
        state: Mutex<Gate>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct Gate {
        started: usize,
        may_return: usize,
    }

    impl Gated {
        fn let_one_return(&self) {
            lock(&self.state).may_return += 1;
            self.changed.notify_all();
        }

        fn wait_until_started(&self, count: usize) {
            let mut state = lock(&self.state);
            while state.started < count {
                let (next, timeout) = self
                    .changed
                    .wait_timeout(state, Duration::from_secs(10))
                    .unwrap();
                assert!(!timeout.timed_out(), "listing {count} never started");
                state = next;
            }
        }
    }

    impl Folders for Gated {
        fn list(
            &self,
            folder: &Path,
            _limits: &Limits,
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<FolderListing, String> {
            let mut state = lock(&self.state);
            state.started += 1;
            let number = state.started;
            self.changed.notify_all();
            while state.may_return < number {
                state = self.changed.wait(state).unwrap();
            }
            Ok(FolderListing {
                files: vec![Listed {
                    path: folder.join("report.txt"),
                    relative: "report.txt".into(),
                }],
                truncated: false,
            })
        }
    }

    #[test]
    fn a_listing_of_a_left_visit_ending_late_does_not_end_the_next_visit_s_wait() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Granted");
        fs::create_dir_all(&folder).unwrap();
        let gated = Arc::new(Gated::default());
        let access = FileAccess::default();
        access.set_folders(gated.clone());
        access.grant("owner", &folder).unwrap();

        // The first visit's listing starts, then root search is left and
        // visited again before it returns.
        assert_eq!(access.folder_state("owner", None), Ok(FolderState::Listing));
        gated.wait_until_started(1);
        access.new_visit();
        assert_eq!(access.folder_state("owner", None), Ok(FolderState::Listing));
        let mut listed = Box::pin(access.listed("owner").expect("a listing is being made"));

        // The old listing returns; the worker has moved on to the new one.
        gated.let_one_return();
        gated.wait_until_started(2);
        assert!(
            (&mut listed).now_or_never().is_none(),
            "the wait ended with the old listing"
        );

        gated.let_one_return();
        block_on(listed);
        assert!(matches!(
            access.folder_state("owner", None),
            Ok(FolderState::Ready { files, .. }) if files.len() == 1
        ));
    }

    #[test]
    fn a_listing_that_ended_before_the_wait_is_made_is_waited_for_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Granted");
        fs::create_dir_all(&folder).unwrap();
        let gated = Arc::new(Gated::default());
        let access = FileAccess::default();
        access.set_folders(gated.clone());
        access.grant("owner", &folder).unwrap();

        // The package is told the folder is listing, and the listing ends
        // before the launcher asks what to wait for.
        assert_eq!(access.folder_state("owner", None), Ok(FolderState::Listing));
        gated.let_one_return();
        gated.wait_until_started(1);
        while access.state().packages["owner"].running.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }
        let listed = access
            .listed("owner")
            .expect("the package was told the folder is listing");
        assert!(listed.now_or_never().is_some());

        // Told the listing, it has nothing more to wait for.
        assert!(matches!(
            access.folder_state("owner", None),
            Ok(FolderState::Ready { .. })
        ));
        assert!(access.listed("owner").is_none());
    }
}
