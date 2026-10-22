//! Windows: the shortcuts in the Start menu's Programs folders, on the
//! user's and every user's Desktop and pinned to the taskbar, and the
//! packaged (AppX/MSIX) apps of the shell's Apps folder (`shell:AppsFolder`),
//! such as Calculator on Windows 11. A shortcut is a shell link (`.lnk`), an
//! internet shortcut (`.url`) whose scheme has a registered handler (a game
//! launcher's `steam://rungameid/...`), or a ClickOnce application reference
//! (`.appref-ms`). All are opened as Explorer opens them (the shell's
//! `ShellExecuteEx`): a shortcut by its path, a packaged app by
//! `shell:AppsFolder\<AppUserModelID>`.
//!
//! A shell link is identified by what it opens: its target and arguments as
//! the shell's shortcut interface reads them, on single-threaded COM
//! apartments in parallel, with version folders as a wildcard
//! ([`Key::program`]); an MSI-advertised shortcut is resolved through the
//! installer to the program it installs; a shortcut whose AppUserModelID is
//! a packaged app's is that app. An internet shortcut is identified by its
//! URL and a ClickOnce reference by its deployment ([`Key::Link`]). A
//! packaged app is identified by its package family
//! ([`super::identity::packaged_keys`]). A shell link Pane cannot read is
//! identified by its own path.
//!
//! Left out: the Startup folders, uninstallers (by the shortcut's name or
//! its program's), a shell link whose target is missing, empty, a folder or
//! a document rather than a program, an internet shortcut to a web page or
//! to a scheme nothing handles, and folders that are symbolic links or
//! junctions.
//!
//! A shell link is titled with the name Explorer shows for it (the shell's
//! display name, which its folder's localized names translate), its file
//! name staying an untranslated name that still finds it; its target is
//! the program whose name may be an alternate title
//! ([`super::names`]). A packaged app keeps the shell's display name. An
//! internet shortcut or a ClickOnce reference is titled by its file name
//! and has no program.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use super::identity::packaged_keys;
use super::{
    Changes, Discovery, Key, Source, Watch, env_dir, has_extension, id_path, sorted_entries,
    watching,
};

/// How deep Pane looks into the Programs folders' subfolders.
const MAX_DEPTH: usize = 8;

/// The extensions of the shortcut files Pane finds applications by.
const SHORTCUT_EXTENSIONS: [&str; 3] = ["lnk", "url", "appref-ms"];

/// The extensions of the files a shell link may target and still be an
/// application: programs, scripts the system runs, management consoles and
/// Control Panel items. Anything else (a text file, a manual, a web page)
/// is a document.
const PROGRAM_EXTENSIONS: [&str; 10] = [
    "exe", "com", "bat", "cmd", "msc", "cpl", "vbs", "vbe", "wsf", "wsh",
];

/// The URL schemes an internet shortcut is a web page or a document by,
/// not an application, whatever handles them.
const DOCUMENT_SCHEMES: [&str; 6] = ["http", "https", "ftp", "file", "mailto", "news"];

/// The folder below a Start menu's Programs folder whose shortcuts Windows
/// runs at sign-in: programs, but not ones to list.
const STARTUP: &str = "Startup";

/// What a packaged app's source path starts with, before its
/// AppUserModelID.
const APPS_FOLDER: &str = r"shell:AppsFolder\";

/// Where a shortcut was found, the most preferred first: the primary
/// source of an application found in several places is the one in the
/// earliest ([`super::identity::primary`]), as Explorer prefers the user's
/// own shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Place {
    /// The user's Desktop.
    UserDesktop,
    /// Every user's Desktop (`%PUBLIC%\Desktop`).
    AllUsersDesktop,
    /// The user's Start menu Programs folder.
    UserStartMenu,
    /// Every user's Start menu Programs folder.
    AllUsersStartMenu,
    /// The shortcuts pinned to the taskbar.
    TaskbarPins,
    /// The Apps folder's packaged apps.
    PackagedApps,
}

impl Place {
    /// The places sharing a layout: a shortcut at the same path below one
    /// of them as below an earlier one is not listed again (the all-users
    /// Start menu repeats the user's).
    fn layout(self) -> u8 {
        match self {
            Place::UserDesktop | Place::AllUsersDesktop => 0,
            Place::UserStartMenu | Place::AllUsersStartMenu => 1,
            Place::TaskbarPins => 2,
            Place::PackagedApps => 3,
        }
    }
}

/// A folder of shortcuts and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutFolder {
    pub path: PathBuf,
    pub place: Place,
    /// Whether its subfolders are looked into too (the Start menu's are;
    /// the Desktop's are not).
    pub subfolders: bool,
}

/// What a shortcut opens, as the shell's shortcut interface reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcut {
    /// The program or file it targets, with environment variables
    /// expanded; empty when it targets no file (a packaged app, a shell
    /// item).
    pub target: String,
    /// The arguments it passes.
    pub arguments: String,
    /// Its AppUserModelID, when it has one.
    pub app_user_model_id: Option<String>,
    /// The name Explorer shows for the shortcut file, which the folder's
    /// own localized names (`desktop.ini`'s `LocalizedFileNames`) or the
    /// shortcut's localized name resource translate; `None` when the shell
    /// gives none, and the file's name is shown instead.
    pub display_name: Option<String>,
}

/// What a shell link's target is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutTarget {
    /// Nothing is there: the shortcut is broken.
    Missing,
    /// A folder.
    Folder,
    /// A file, a program or a document by its extension.
    File,
}

/// Reads shortcuts: for each path, what it opens, or `None` when it cannot
/// be read.
pub type Resolver = Arc<dyn Fn(&[PathBuf]) -> Vec<Option<Shortcut>> + Send + Sync>;

/// Says what a shell link's target is on disk.
type Targets = Arc<dyn Fn(&str) -> ShortcutTarget + Send + Sync>;

/// Says whether a URL scheme (in lowercase) has a registered handler.
type Handlers = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// What a shortcut file was when it was read: its size and when it was
/// last written, so an unchanged shortcut is not read again.
type Fingerprint = (u64, Option<SystemTime>);

/// The shortcuts read by earlier scans, by path, with what each file was
/// when it was read.
type ReadShortcuts = Arc<Mutex<HashMap<PathBuf, (Fingerprint, Option<Shortcut>)>>>;

/// The shortcuts in some folders and, when enabled, the packaged apps in
/// the Apps folder.
#[derive(Clone)]
pub struct StartMenu {
    /// The shortcut folders.
    folders: Vec<ShortcutFolder>,
    /// Whether the Apps folder's packaged apps are listed too.
    packaged: bool,
    /// Reads what each shortcut opens.
    resolve: Resolver,
    /// Says what a shell link's target is on disk.
    targets: Targets,
    /// Says whether a URL scheme has a handler.
    handlers: Handlers,
    /// The shortcuts read by earlier scans, by path.
    read: ReadShortcuts,
}

impl std::fmt::Debug for StartMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartMenu")
            .field("folders", &self.folders)
            .field("packaged", &self.packaged)
            .finish_non_exhaustive()
    }
}

impl StartMenu {
    /// The shortcuts in `folders` and their subfolders, the first being the
    /// user's Start menu and the others every user's. Packaged apps are not
    /// listed.
    pub fn new(folders: Vec<PathBuf>) -> StartMenu {
        let folders = folders
            .into_iter()
            .enumerate()
            .map(|(index, path)| ShortcutFolder {
                path,
                place: if index == 0 {
                    Place::UserStartMenu
                } else {
                    Place::AllUsersStartMenu
                },
                subfolders: true,
            })
            .collect();
        StartMenu::with_folders(folders)
    }

    /// The shortcuts in `folders`. Packaged apps are not listed.
    pub fn with_folders(folders: Vec<ShortcutFolder>) -> StartMenu {
        StartMenu {
            folders,
            packaged: false,
            resolve: Arc::new(resolve_shortcuts),
            targets: Arc::new(target_on_disk),
            handlers: Arc::new(has_handler),
            read: Arc::default(),
        }
    }

    /// The current user's Start menu
    /// (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`) and every user's
    /// (`%ProgramData%\Microsoft\Windows\Start Menu\Programs`), with their
    /// subfolders; the user's Desktop and every user's (`%PUBLIC%\Desktop`),
    /// wherever the shell keeps them, and the shortcuts pinned to the
    /// taskbar
    /// (`%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`),
    /// without subfolders; then the Apps folder's packaged apps.
    pub fn from_env() -> StartMenu {
        let programs = |root: PathBuf| root.join(r"Microsoft\Windows\Start Menu\Programs");
        let folder = |place: Place, subfolders: bool| {
            move |path: PathBuf| ShortcutFolder {
                path,
                place,
                subfolders,
            }
        };
        let folders = env_dir("APPDATA")
            .map(programs)
            .map(folder(Place::UserStartMenu, true))
            .into_iter()
            .chain(
                env_dir("ProgramData")
                    .map(programs)
                    .map(folder(Place::AllUsersStartMenu, true)),
            )
            .chain(
                desktop(false)
                    .or_else(|| env_dir("USERPROFILE").map(|home| home.join("Desktop")))
                    .map(folder(Place::UserDesktop, false)),
            )
            .chain(
                desktop(true)
                    .or_else(|| env_dir("PUBLIC").map(|public| public.join("Desktop")))
                    .map(folder(Place::AllUsersDesktop, false)),
            )
            .chain(
                env_dir("APPDATA")
                    .map(|root| {
                        root.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
                    })
                    .map(folder(Place::TaskbarPins, false)),
            )
            .collect();
        let mut menu = StartMenu::with_folders(folders);
        menu.packaged = true;
        menu
    }

    /// The folders it finds shortcuts in.
    pub fn folders(&self) -> &[ShortcutFolder] {
        &self.folders
    }

    /// This, reading shortcuts with `resolve` instead of the shell, which
    /// exists only on Windows: what tests use to give shortcuts targets on
    /// every system. Those targets name files of no particular disk, so
    /// every one is taken to exist as a file until [`StartMenu::with_targets`]
    /// says otherwise.
    pub fn with_resolver(
        mut self,
        resolve: impl Fn(&[PathBuf]) -> Vec<Option<Shortcut>> + Send + Sync + 'static,
    ) -> StartMenu {
        self.resolve = Arc::new(resolve);
        self.targets = Arc::new(|_| ShortcutTarget::File);
        self.read = Arc::default();
        self
    }

    /// This, asking `targets` what a shell link's target is instead of the
    /// disk.
    pub fn with_targets(
        mut self,
        targets: impl Fn(&str) -> ShortcutTarget + Send + Sync + 'static,
    ) -> StartMenu {
        self.targets = Arc::new(targets);
        self
    }

    /// This, asking `handlers` whether a URL scheme (in lowercase) has a
    /// registered handler instead of the system's registry.
    pub fn with_handlers(
        mut self,
        handlers: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> StartMenu {
        self.handlers = Arc::new(handlers);
        self
    }

    /// What each of `paths` opens: read again only when the file changed
    /// since an earlier scan read it.
    fn shortcuts(&self, paths: &[PathBuf]) -> Vec<Option<Shortcut>> {
        let fingerprints: Vec<Fingerprint> = paths
            .iter()
            .map(|path| {
                std::fs::metadata(path).map_or((0, None), |metadata| {
                    (metadata.len(), metadata.modified().ok())
                })
            })
            .collect();
        let mut read = self
            .read
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (unread, unread_fingerprints): (Vec<PathBuf>, Vec<Fingerprint>) = paths
            .iter()
            .zip(fingerprints)
            .filter(|(path, fingerprint)| {
                read.get(*path).is_none_or(|(known, _)| {
                    // A file whose time is unknown is always read again.
                    known != fingerprint || known.1.is_none()
                })
            })
            .map(|(path, fingerprint)| (path.clone(), fingerprint))
            .unzip();
        if !unread.is_empty() {
            let shortcuts = (self.resolve)(&unread);
            for ((path, fingerprint), shortcut) in
                unread.into_iter().zip(unread_fingerprints).zip(shortcuts)
            {
                read.insert(path, (fingerprint, shortcut));
            }
        }
        // Forget the shortcuts no longer found.
        let found: HashSet<&PathBuf> = paths.iter().collect();
        read.retain(|path, _| found.contains(path));
        paths
            .iter()
            .map(|path| read.get(path).and_then(|(_, shortcut)| shortcut.clone()))
            .collect()
    }
}

/// What kind of shortcut file was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A shell link (`.lnk`).
    Link,
    /// An internet shortcut (`.url`).
    Internet,
    /// A ClickOnce application reference (`.appref-ms`).
    ClickOnce,
}

impl Kind {
    /// The kind of shortcut `path` is by its extension, if it is one.
    fn of(path: &Path) -> Option<Kind> {
        let kinds = [Kind::Link, Kind::Internet, Kind::ClickOnce];
        SHORTCUT_EXTENSIONS
            .iter()
            .zip(kinds)
            .find(|(extension, _)| has_extension(path, extension))
            .map(|(_, kind)| kind)
    }
}

/// A shortcut file found, before Pane knows what it opens.
struct Found {
    path: PathBuf,
    name: String,
    location: String,
    place: Place,
    kind: Kind,
}

/// What a shortcut file found opens, as Pane read it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Opens {
    /// A shell link, as the shell's shortcut interface read it; `None` when
    /// it could not be read.
    Link(Option<Shortcut>),
    /// An internet shortcut's URL ([`internet_shortcut_url`]); `None` when
    /// it names none.
    Url(Option<String>),
    /// A ClickOnce reference's deployment ([`click_once_deployment`]);
    /// `None` when it names none.
    ClickOnce(Option<String>),
}

/// Whether a shortcut named `name` is an uninstaller's ("Uninstall Tool",
/// "Tool Uninstaller"), which is not an application to open.
fn is_uninstaller(name: &str) -> bool {
    name.to_lowercase().contains("uninstall")
}

/// Whether `target`, a shell link's target, is an uninstaller's program
/// (Inno Setup's `unins000.exe`, `uninst.exe`, `uninstall.exe`).
fn is_uninstaller_program(target: &str) -> bool {
    Path::new(&target.replace('\\', "/"))
        .file_stem()
        .is_some_and(|stem| stem.to_string_lossy().to_lowercase().starts_with("unins"))
}

/// Whether `target`, a shell link's target, is a program by its extension
/// rather than a document ([`PROGRAM_EXTENSIONS`]).
fn is_program(target: &str) -> bool {
    let target = PathBuf::from(target.replace('\\', "/"));
    PROGRAM_EXTENSIONS
        .iter()
        .any(|extension| has_extension(&target, extension))
}

/// The text of a small shortcut file: UTF-16 when it starts with that
/// encoding's byte order mark (as `.appref-ms` files do), else UTF-8 (or
/// the ANSI of `.url` files, read as far as it is UTF-8).
pub(super) fn shortcut_text(bytes: &[u8]) -> String {
    if let Some(utf16) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = utf16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// The URL an internet shortcut's `text` names: the `URL` key of its
/// `[InternetShortcut]` section, both without case; `None` when it names
/// none.
fn internet_shortcut_url(text: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            in_section = section.trim().eq_ignore_ascii_case("InternetShortcut");
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("URL")
        {
            let url = value.trim();
            return (!url.is_empty()).then(|| url.to_owned());
        }
    }
    None
}

/// The scheme of `url`, in lowercase: letters, digits, `+`, `-` and `.`
/// after a letter, before the first `:`. A single letter is a drive (`C:`),
/// not a scheme.
fn url_scheme(url: &str) -> Option<String> {
    let (scheme, _) = url.split_once(':')?;
    let valid = scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| scheme.to_ascii_lowercase())
}

/// Whether an internet shortcut to `url` is an application: its scheme is
/// not a web page's or a document's ([`DOCUMENT_SCHEMES`]) and `handles`
/// says it has a registered handler (a game launcher's `steam:`).
fn is_application_url(url: &str, handles: &dyn Fn(&str) -> bool) -> bool {
    url_scheme(url)
        .is_some_and(|scheme| !DOCUMENT_SCHEMES.contains(&scheme.as_str()) && handles(&scheme))
}

/// The deployment a ClickOnce application reference's `text` names: its
/// first line, the deployment's URL with the application's identity
/// (`https://host/App.application#App.application, Culture=neutral, ...`);
/// `None` when it names none.
pub(super) fn click_once_deployment(text: &str) -> Option<String> {
    let line = text.trim_matches(['\0', '\u{feff}']).lines().next()?.trim();
    line.to_lowercase()
        .contains(".application")
        .then(|| line.to_owned())
}

fn collect(
    folder: &ShortcutFolder,
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<(u8, String)>,
    found: &mut Vec<Found>,
) {
    let start_menu = matches!(
        folder.place,
        Place::UserStartMenu | Place::AllUsersStartMenu
    );
    for entry in sorted_entries(dir) {
        let path = entry.path();
        if path.is_dir() {
            // A symbolic link or a junction could lead anywhere, or back.
            let linked = entry
                .file_type()
                .map_or(true, |file_type| file_type.is_symlink());
            // Windows runs the Startup folder's shortcuts at sign-in.
            let startup =
                start_menu && depth == 0 && entry.file_name().eq_ignore_ascii_case(STARTUP);
            if folder.subfolders && depth < MAX_DEPTH && !linked && !startup {
                collect(folder, &path, depth + 1, seen, found);
            }
            continue;
        }
        let Some(kind) = Kind::of(&path) else {
            continue;
        };
        let Some(name) = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        // Uninstallers are shortcuts too, but not applications to open.
        if is_uninstaller(&name) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&folder.path) else {
            continue;
        };
        let relative = relative.to_string_lossy().to_lowercase();
        if !seen.insert((folder.place.layout(), relative)) {
            continue;
        }
        found.push(Found {
            path,
            name,
            location: dir.display().to_string(),
            place: folder.place,
            kind,
        });
    }
}

/// Whether `parsing`, an Apps folder item's parsing name, is a packaged
/// app's AppUserModelID (`<package family>!<app>`), not a file or a folder.
fn is_app_user_model_id(parsing: &str) -> bool {
    parsing.contains('!') && !parsing.contains(['\\', '/', ':', '{'])
}

/// The key of a shell link that opens `shortcut`, or `None` when it is not
/// an application: a packaged app's key when its AppUserModelID is one in
/// `package_key`; else, when its target is a program that is there
/// (`targets`) and not an uninstaller, its target and arguments. An empty
/// target opens a shell item, or is an MSI-advertised shortcut whose
/// product is not installed (the reader resolves an installed one to its
/// program); a missing target is a broken shortcut; a folder or a document
/// is not an application.
fn link_key(
    shortcut: &Shortcut,
    package_key: &HashMap<String, Key>,
    targets: &dyn Fn(&str) -> ShortcutTarget,
) -> Option<Key> {
    if let Some(key) = shortcut
        .app_user_model_id
        .as_ref()
        .and_then(|aumid| package_key.get(&aumid.to_lowercase()))
    {
        return Some(key.clone());
    }
    let target = shortcut.target.trim();
    let admitted = !target.is_empty()
        && is_program(target)
        && !is_uninstaller_program(target)
        && targets(target) == ShortcutTarget::File;
    admitted.then(|| Key::program(target, &shortcut.arguments))
}

/// The sources the shortcuts `found` and the Apps folder's `items` (each
/// display name and parsing name) are, given what each shortcut opens
/// (`opens`, in the order of `found`), what a shell link's target is on
/// disk (`targets`) and whether a URL scheme has a handler (`handles`).
///
/// A shell link is keyed by the packaged app its AppUserModelID names, or
/// by its target and arguments ([`link_key`], which leaves out what is not
/// an application), or, when it could not be read, by its own path. An
/// internet shortcut whose URL is an application's
/// ([`is_application_url`]) is keyed by its URL and a ClickOnce reference
/// by its deployment, in lowercase. The Apps folder's packaged apps are
/// keyed by package family; one that no shortcut opens but whose name a
/// shortcut has is left out, as the Apps folder lists the shortcuts'
/// programs too.
fn sources(
    found: Vec<Found>,
    opens: Vec<Opens>,
    items: Vec<(String, String)>,
    targets: &dyn Fn(&str) -> ShortcutTarget,
    handles: &dyn Fn(&str) -> bool,
) -> Vec<Source> {
    let mut packaged: Vec<(String, String)> = Vec::new();
    let mut aumids: HashSet<String> = HashSet::new();
    for (name, parsing) in items {
        if is_app_user_model_id(&parsing)
            && !name.trim().is_empty()
            && aumids.insert(parsing.to_lowercase())
        {
            packaged.push((name, parsing));
        }
    }
    let keys = packaged_keys(
        &packaged
            .iter()
            .map(|(_, parsing)| parsing.clone())
            .collect::<Vec<_>>(),
    );
    let package_key: HashMap<String, Key> = packaged
        .iter()
        .zip(&keys)
        .map(|((_, parsing), key)| (parsing.to_lowercase(), key.clone()))
        .collect();

    let mut sources: Vec<Source> = found
        .into_iter()
        .zip(opens)
        .filter_map(|(found, opens)| {
            let path = found.path.to_string_lossy().into_owned();
            let mut program = None;
            let mut arguments = false;
            let mut shown = None;
            let key = match opens {
                Opens::Link(Some(shortcut)) => {
                    let key = link_key(&shortcut, &package_key, targets)?;
                    if matches!(key, Key::Program { .. }) {
                        program = Some(shortcut.target.trim().to_owned());
                        arguments = !shortcut.arguments.trim().is_empty();
                    }
                    shown = shortcut.display_name.as_deref().map(shown_name);
                    key
                }
                Opens::Link(None) => Key::Path(path.to_lowercase()),
                Opens::Url(Some(url)) if is_application_url(&url, handles) => {
                    Key::Link(url.to_lowercase())
                }
                Opens::ClickOnce(Some(deployment)) => Key::Link(deployment.to_lowercase()),
                Opens::Url(_) | Opens::ClickOnce(None) => return None,
            };
            // Titled as Explorer shows it; the file's own name, when the
            // shell translates it, still finds it.
            let (name, untranslated) = match shown.filter(|shown| !shown.is_empty()) {
                Some(shown) if shown != found.name => (shown, Some(found.name)),
                _ => (found.name, None),
            };
            Some(Source {
                untranslated,
                arguments,
                program,
                ..Source::new(key, path, name, found.location, found.place as usize)
            })
        })
        .collect();

    let shortcut_keys: HashSet<Key> = sources.iter().map(|source| source.key.clone()).collect();
    let shortcut_names: HashSet<String> = sources
        .iter()
        .map(|source| source.name.to_lowercase())
        .collect();
    for ((name, parsing), key) in packaged.into_iter().zip(keys) {
        if !shortcut_keys.contains(&key) && shortcut_names.contains(&name.to_lowercase()) {
            continue;
        }
        sources.push(Source::new(
            key,
            format!("{APPS_FOLDER}{parsing}"),
            name,
            "Apps folder (packaged apps)",
            Place::PackagedApps as usize,
        ));
    }
    sources
}

/// `display_name`, the name the shell shows for a shortcut, as a title:
/// trimmed, without the `.lnk` the shell shows only when told to show
/// every extension.
fn shown_name(display_name: &str) -> String {
    let name = display_name.trim();
    let stem = name.len().checked_sub(4).and_then(|end| {
        (name.is_char_boundary(end) && name[end..].eq_ignore_ascii_case(".lnk"))
            .then(|| &name[..end])
    });
    stem.unwrap_or(name).trim().to_owned()
}

impl Discovery for StartMenu {
    fn sources(&self) -> Result<Vec<Source>, String> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for folder in &self.folders {
            collect(folder, &folder.path, 0, &mut seen, &mut found);
        }
        // Only shell links are read through the shell; the other kinds are
        // small text files.
        let links: Vec<PathBuf> = found
            .iter()
            .filter(|found| found.kind == Kind::Link)
            .map(|found| found.path.clone())
            .collect();
        let mut shortcuts = self.shortcuts(&links).into_iter();
        let text = |path: &Path| std::fs::read(path).ok().map(|bytes| shortcut_text(&bytes));
        let opens: Vec<Opens> = found
            .iter()
            .map(|found| match found.kind {
                Kind::Link => Opens::Link(shortcuts.next().flatten()),
                Kind::Internet => {
                    Opens::Url(text(&found.path).as_deref().and_then(internet_shortcut_url))
                }
                Kind::ClickOnce => {
                    Opens::ClickOnce(text(&found.path).as_deref().and_then(click_once_deployment))
                }
            })
            .collect();
        let items = if self.packaged {
            apps_folder()?
        } else {
            Vec::new()
        };
        Ok(sources(
            found,
            opens,
            items,
            self.targets.as_ref(),
            self.handlers.as_ref(),
        ))
    }

    fn open(&self, path: &str) -> Result<(), String> {
        if let Some(aumid) = path.strip_prefix(APPS_FOLDER) {
            if aumid.is_empty() || aumid.contains(['\\', '/']) {
                return Err(format!("{path} is not a packaged app"));
            }
            return shell_execute(path);
        }
        let extension = SHORTCUT_EXTENSIONS
            .into_iter()
            .find(|extension| has_extension(Path::new(path), extension))
            .unwrap_or("lnk");
        let path = id_path(path, extension, "a Start menu shortcut")?;
        shell_execute(&path.to_string_lossy())
    }

    /// Watches each shortcut folder (with its subfolders where they are
    /// looked into) and, when packaged apps are listed, the folder Windows
    /// makes for each package registered for the user
    /// (`%LOCALAPPDATA%\Packages`): a package's folder appearing or going
    /// is an install or a removal, which Windows completes after it, so it
    /// is looked at again a little later ([`super::Change::Completing`]).
    fn watch(&self, changes: Changes) -> Result<Watch, String> {
        let mut folders: Vec<watching::Folder> = self
            .folders
            .iter()
            .map(|folder| {
                if folder.subfolders {
                    watching::Folder::walked(folder.path.clone())
                } else {
                    watching::Folder::flat(folder.path.clone())
                }
            })
            .collect();
        if self.packaged {
            folders.extend(
                env_dir("LOCALAPPDATA")
                    .map(|local| watching::Folder::packages(local.join("Packages"))),
            );
        }
        watching::watch(folders, changes)
    }
}

/// What `target`, a shell link's target, is on this disk. A target Pane may
/// not look at is taken to be there.
fn target_on_disk(target: &str) -> ShortcutTarget {
    match std::fs::metadata(target) {
        Ok(metadata) if metadata.is_dir() => ShortcutTarget::Folder,
        Ok(_) => ShortcutTarget::File,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ShortcutTarget::Missing,
        Err(_) => ShortcutTarget::File,
    }
}

/// Whether the URL scheme `scheme` has a registered handler: its key under
/// `HKEY_CLASSES_ROOT` (the user's classes over the machine's) is marked as
/// a URL protocol and says how to open it (`shell`).
#[cfg(windows)]
fn has_handler(scheme: &str) -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CLASSES_ROOT, KEY_READ, RRF_RT_ANY, RegCloseKey, RegGetValueW, RegOpenKeyExW,
    };
    use windows::core::HSTRING;

    // Only a scheme's own characters reach the registry, never a path.
    if url_scheme(&format!("{scheme}:")).as_deref() != Some(scheme) {
        return false;
    }
    let (key, marker) = (HSTRING::from(scheme), HSTRING::from("URL Protocol"));
    // SAFETY: asks whether the value exists; every pointer is valid or
    // absent.
    let marked = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            &key,
            &marker,
            RRF_RT_ANY,
            None,
            None,
            None,
        )
    };
    if marked != ERROR_SUCCESS {
        return false;
    }
    let shell = HSTRING::from(format!(r"{scheme}\shell"));
    let mut opened = HKEY::default();
    // SAFETY: `opened` is filled in when the call succeeds and closed once
    // below.
    if unsafe { RegOpenKeyExW(HKEY_CLASSES_ROOT, &shell, None, KEY_READ, &mut opened) }
        != ERROR_SUCCESS
    {
        return false;
    }
    // SAFETY: the handle the open returned, closed once.
    let _ = unsafe { RegCloseKey(opened) };
    true
}

#[cfg(not(windows))]
fn has_handler(_scheme: &str) -> bool {
    false
}

/// The user's Desktop (`all_users` false) or every user's (`true`), where
/// the shell keeps it (a user may move theirs, to OneDrive for instance).
#[cfg(windows)]
fn desktop(all_users: bool) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_PublicDesktop, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };

    let folder = if all_users {
        FOLDERID_PublicDesktop
    } else {
        FOLDERID_Desktop
    };
    // SAFETY: the returned string is owned by the caller and freed here.
    unsafe {
        let path = SHGetKnownFolderPath(&folder, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        text.filter(|text| !text.is_empty()).map(PathBuf::from)
    }
}

#[cfg(not(windows))]
fn desktop(_all_users: bool) -> Option<PathBuf> {
    None
}

/// COM on the thread for as long as it is held: the shell may use it to
/// enumerate the Apps folder, read a shortcut or open one.
#[cfg(windows)]
use crate::windows_shell::Com;

/// How many threads read shortcuts at once, at most.
#[cfg(windows)]
const READERS: usize = 8;

/// What each shortcut at `paths` opens, read by the shell's shortcut
/// interface on single-threaded COM apartments, several threads at once.
#[cfg(windows)]
fn resolve_shortcuts(paths: &[PathBuf]) -> Vec<Option<Shortcut>> {
    if paths.is_empty() {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .clamp(1, READERS)
        .min(paths.len());
    let chunk = paths.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let readers: Vec<_> = paths
            .chunks(chunk)
            .map(|chunk| (chunk.len(), scope.spawn(move || read_shortcuts(chunk))))
            .collect();
        readers
            .into_iter()
            .flat_map(|(count, reader)| reader.join().unwrap_or_else(|_| vec![None; count]))
            .collect()
    })
}

#[cfg(not(windows))]
fn resolve_shortcuts(paths: &[PathBuf]) -> Vec<Option<Shortcut>> {
    vec![None; paths.len()]
}

/// What each shortcut at `paths` opens, on this thread's own apartment.
#[cfg(windows)]
fn read_shortcuts(paths: &[PathBuf]) -> Vec<Option<Shortcut>> {
    let Ok(_com) = Com::new() else {
        return vec![None; paths.len()];
    };
    paths.iter().map(|path| read_shortcut(path).ok()).collect()
}

/// What the shortcut at `path` opens, without resolving a target that
/// moved (which could search the disk): its target as recorded, with
/// environment variables expanded, its arguments and its AppUserModelID;
/// and the name Explorer shows for it.
#[cfg(windows)]
fn read_shortcut(path: &Path) -> windows::core::Result<Shortcut> {
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToString};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        IShellItem, IShellLinkW, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY, ShellLink,
    };
    use windows::core::{GUID, Interface, PCWSTR};

    /// `PKEY_AppUserModel_ID`.
    const APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };
    let text = |buffer: &[u16]| -> String {
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end]).trim().to_owned()
    };
    let file = crate::util::wide(path);
    // SAFETY: plain COM calls on interfaces the shell returns, on a thread
    // whose COM apartment outlives them; every buffer outlives its call.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.cast::<IPersistFile>()?
            .Load(PCWSTR(file.as_ptr()), STGM_READ)?;
        let mut target = vec![0u16; 4096];
        // No target file (a packaged app's shortcut) is not an error: the
        // target is empty then.
        let _ = link.GetPath(&mut target, std::ptr::null_mut(), 0);
        // An MSI-advertised shortcut names the product it installs, not a
        // file: its target is the program that product installed, or none
        // when it is not installed.
        let target = match advertised_target(&file) {
            Some(installed) => installed.unwrap_or_default(),
            None => text(&target),
        };
        let mut arguments = vec![0u16; 4096];
        let _ = link.GetArguments(&mut arguments);
        let app_user_model_id = link.cast::<IPropertyStore>().ok().and_then(|store| {
            let mut value = store.GetValue(&APP_USER_MODEL_ID).ok()?;
            let mut buffer = vec![0u16; 1024];
            let read = PropVariantToString(&value, &mut buffer);
            let _ = PropVariantClear(&mut value);
            read.ok()?;
            Some(text(&buffer)).filter(|aumid| !aumid.is_empty())
        });
        // The name Explorer shows for the file, localized through its
        // folder's `desktop.ini` or the shortcut's own name resource.
        let display_name =
            SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(file.as_ptr()), None)
                .ok()
                .and_then(|item| {
                    let shown = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
                    let name = shown.to_string().ok();
                    CoTaskMemFree(Some(shown.0 as *const _));
                    name
                })
                .filter(|name| !name.trim().is_empty());
        Ok(Shortcut {
            target,
            arguments: text(&arguments),
            app_user_model_id,
            display_name,
        })
    }
}

/// What the shortcut `file` (a NUL-terminated path) installs when the
/// Windows Installer advertises it: `None` when it is not an advertised
/// shortcut, `Some(None)` when its product or component is not installed,
/// and `Some(Some(path))` with the installed program's path. Read through
/// the installer's documented `MsiGetShortcutTargetW` and
/// `MsiGetComponentPathW`.
#[cfg(windows)]
fn advertised_target(file: &[u16]) -> Option<Option<String>> {
    use windows::Win32::System::ApplicationInstallationAndServicing::{
        INSTALLSTATE_LOCAL, INSTALLSTATE_MOREDATA, INSTALLSTATE_SOURCE, MsiGetComponentPathW,
        MsiGetShortcutTargetW,
    };
    use windows::core::{PCWSTR, PWSTR};

    /// A GUID in braces and its NUL (`MAX_GUID_CHARS` + 1), which is also
    /// room for a feature id (`MAX_FEATURE_CHARS` + 1).
    const GUID: usize = 39;
    let mut product = [0u16; GUID];
    let mut feature = [0u16; GUID];
    let mut component = [0u16; GUID];
    // SAFETY: `file` is NUL-terminated and every buffer holds the size the
    // function documents, all outliving the call.
    let read = unsafe {
        MsiGetShortcutTargetW(
            PCWSTR(file.as_ptr()),
            Some(PWSTR(product.as_mut_ptr())),
            Some(PWSTR(feature.as_mut_ptr())),
            Some(PWSTR(component.as_mut_ptr())),
        )
    };
    // Not an advertised shortcut, or one naming no component.
    if read != 0 || component[0] == 0 {
        return None;
    }
    let mut buffer = vec![0u16; 1024];
    for _ in 0..2 {
        let mut size = buffer.len() as u32;
        // SAFETY: `buffer` holds `size` characters and outlives the call;
        // the GUIDs are NUL-terminated.
        let state = unsafe {
            MsiGetComponentPathW(
                PCWSTR(product.as_ptr()),
                PCWSTR(component.as_ptr()),
                Some(PWSTR(buffer.as_mut_ptr())),
                Some(&mut size),
            )
        };
        if state == INSTALLSTATE_MOREDATA {
            buffer = vec![0u16; size as usize + 1];
            continue;
        }
        if state != INSTALLSTATE_LOCAL && state != INSTALLSTATE_SOURCE {
            return Some(None);
        }
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        let path = String::from_utf16_lossy(&buffer[..end]).trim().to_owned();
        return Some(Some(path).filter(|path| !path.is_empty()));
    }
    Some(None)
}

/// The Apps folder's items, each (display name, parsing name).
#[cfg(windows)]
fn apps_folder() -> Result<Vec<(String, String)>, String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT,
        SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    };

    let failed = |error: windows::core::Error| format!("cannot list the Apps folder: {error}");
    let _com = Com::new()?;
    let name = |item: &IShellItem, kind: SIGDN| -> Result<String, String> {
        // SAFETY: the returned string is owned by the caller and freed here.
        unsafe {
            let text = item.GetDisplayName(kind).map_err(failed)?;
            let name = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0 as *const _));
            Ok(name)
        }
    };
    let mut items = Vec::new();
    // SAFETY: plain COM calls on interfaces the shell returns, on a thread
    // with COM initialized for their lifetime.
    unsafe {
        let folder: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(failed)?;
        let entries: IEnumShellItems = folder
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(failed)?;
        loop {
            let mut item = [None];
            let mut fetched = 0;
            entries
                .Next(&mut item, Some(&mut fetched))
                .map_err(failed)?;
            let Some(item) = item[0].take().filter(|_| fetched == 1) else {
                break;
            };
            items.push((
                name(&item, SIGDN_NORMALDISPLAY)?,
                name(&item, SIGDN_PARENTRELATIVEPARSING)?,
            ));
        }
    }
    Ok(items)
}

#[cfg(not(windows))]
fn apps_folder() -> Result<Vec<(String, String)>, String> {
    Err("the Apps folder exists only on Windows".into())
}

/// Opens `file`, a shortcut's path or `shell:AppsFolder\<AppUserModelID>`,
/// as Explorer does.
#[cfg(windows)]
fn shell_execute(file: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let file = crate::util::wide(file);
    let _com = Com::new()?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait until the shell has started it (the thread ends next), and
        // report a failure here instead of in a dialog.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size and a valid,
    // NUL-terminated file name that outlives the call.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| error.message())
}

#[cfg(not(windows))]
fn shell_execute(file: &str) -> Result<(), String> {
    Err(format!(
        "{file} is a Windows shortcut or app; Pane opens those only on Windows"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, place: Place) -> Found {
        Found {
            path: PathBuf::from(format!(r"C:\Menu\{name}.lnk")),
            name: name.into(),
            location: r"C:\Menu".into(),
            place,
            kind: Kind::Link,
        }
    }

    /// The sources of shell links read as `shortcuts`, whose targets all
    /// exist, with no URL scheme handled.
    fn link_sources(
        found: Vec<Found>,
        shortcuts: Vec<Option<Shortcut>>,
        items: Vec<(String, String)>,
    ) -> Vec<Source> {
        let opens = shortcuts.into_iter().map(Opens::Link).collect();
        sources(found, opens, items, &|_| ShortcutTarget::File, &|_| false)
    }

    fn target(target: &str) -> Option<Shortcut> {
        Some(Shortcut {
            target: target.into(),
            ..Shortcut::default()
        })
    }

    fn item(name: &str, parsing: &str) -> (String, String) {
        (name.to_owned(), parsing.to_owned())
    }

    #[test]
    fn packaged_apps_without_a_shortcut_are_added_by_their_package_family() {
        let found = vec![
            found("Notepad++", Place::UserStartMenu),
            found("Paint", Place::UserStartMenu),
        ];
        let shortcuts = vec![target(r"C:\Program Files\Notepad++\notepad++.exe"), None];
        let sources = link_sources(
            found,
            shortcuts,
            vec![
                item(
                    "Calculator",
                    "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                ),
                // A shortcut of the same name is already listed.
                item("paint", "Microsoft.Paint_8wekyb3d8bbwe!App"),
                // A desktop program's entry: covered by its shortcut, or not an app.
                item("Notepad++", r"C:\Program Files\Notepad++\notepad++.exe"),
                item("Control Panel", "{26EE0668-A00A-44D7-9371-BEB064C98683}\\0"),
                item("Help", "https://example.com/help"),
                item("", "Nameless_1234!App"),
                // Listed twice by the shell: once.
                item(
                    "Calculator",
                    "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                ),
            ],
        );

        let names: Vec<&str> = sources.iter().map(|source| source.name.as_str()).collect();
        assert_eq!(names, ["Notepad++", "Paint", "Calculator"]);
        assert_eq!(
            sources[2].path,
            r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );
        assert_eq!(
            sources[2].key,
            Key::Package {
                family: "microsoft.windowscalculator_8wekyb3d8bbwe".into(),
                app: None
            }
        );
        assert_eq!(sources[2].place, Place::PackagedApps as usize);
        assert_eq!(
            sources[0].key,
            Key::program(r"C:\Program Files\Notepad++\notepad++.exe", "")
        );
        // A shortcut that could not be read is keyed by its own path.
        assert_eq!(sources[1].key, Key::Path(r"c:\menu\paint.lnk".into()));
    }

    #[test]
    fn a_shortcut_to_a_packaged_app_is_that_app() {
        let shortcut = Some(Shortcut {
            app_user_model_id: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
            ..Shortcut::default()
        });
        let sources = link_sources(
            vec![found("Terminal", Place::UserStartMenu)],
            vec![shortcut],
            vec![item(
                "Terminal",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            )],
        );

        // Both are sources of one application: the shortcut, then the app.
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].key, sources[1].key);
        let catalog = super::super::Catalog::new(sources);
        let applications = catalog.applications();
        assert_eq!(applications.len(), 1);
        assert_eq!(
            catalog.find(&applications[0].id).unwrap().primary().path,
            r"C:\Menu\Terminal.lnk"
        );
    }

    #[test]
    fn a_desktop_program_s_own_app_user_model_id_does_not_make_it_a_packaged_app() {
        let shortcut = Some(Shortcut {
            target: r"C:\Tool\tool.exe".into(),
            app_user_model_id: Some("Vendor.Tool".into()),
            ..Shortcut::default()
        });
        let sources = link_sources(
            vec![found("Tool", Place::UserStartMenu)],
            vec![shortcut],
            vec![],
        );
        assert_eq!(sources[0].key, Key::program(r"C:\Tool\tool.exe", ""));
    }

    #[test]
    fn a_shortcut_is_titled_as_explorer_shows_it_and_its_file_name_still_finds_it() {
        let localized = Some(Shortcut {
            target: r"C:\Windows\System32\mspaint.exe".into(),
            display_name: Some("Ứng dụng Vẽ".into()),
            ..Shortcut::default()
        });
        // Explorer told to show every extension shows `.lnk` too.
        let with_extension = Some(Shortcut {
            target: r"C:\Tool\tool.exe".into(),
            display_name: Some(" Tool.LNK ".into()),
            ..Shortcut::default()
        });
        let unnamed = target(r"C:\Other\other.exe");
        let sources = link_sources(
            vec![
                found("Paint", Place::UserStartMenu),
                found("Tool", Place::UserStartMenu),
                found("Other", Place::UserStartMenu),
            ],
            vec![localized, with_extension, unnamed],
            vec![],
        );

        assert_eq!(sources[0].name, "Ứng dụng Vẽ");
        assert_eq!(sources[0].untranslated.as_deref(), Some("Paint"));
        assert_eq!(
            (sources[1].name.as_str(), &sources[1].untranslated),
            ("Tool", &None)
        );
        assert_eq!(
            (sources[2].name.as_str(), &sources[2].untranslated),
            ("Other", &None)
        );
    }

    #[test]
    fn a_shortcut_s_program_and_whether_it_passes_arguments_are_kept() {
        let browser = r"C:\Browser\chrome.exe";
        let web_app = Some(Shortcut {
            target: browser.into(),
            arguments: "--app-id=mail".into(),
            ..Shortcut::default()
        });
        let packaged = Some(Shortcut {
            app_user_model_id: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
            ..Shortcut::default()
        });
        let sources = link_sources(
            vec![
                found("Browser", Place::UserStartMenu),
                found("Mail", Place::UserStartMenu),
                found("Terminal", Place::UserStartMenu),
                found("Broken", Place::UserStartMenu),
            ],
            vec![target(browser), web_app, packaged, None],
            vec![item(
                "Terminal",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            )],
        );

        assert_eq!(sources[0].program.as_deref(), Some(browser));
        assert!(!sources[0].arguments);
        assert_eq!(sources[1].program.as_deref(), Some(browser));
        assert!(sources[1].arguments);
        // A packaged app runs no program Pane knows; nor does an unreadable
        // shortcut.
        assert_eq!(sources[2].program, None);
        assert_eq!(sources[3].program, None);
    }

    #[test]
    fn a_packaged_app_keeps_the_display_name_the_shell_gives_it() {
        let sources = link_sources(
            vec![],
            vec![],
            vec![item(
                "Máy tính",
                "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
            )],
        );
        assert_eq!(sources[0].name, "Máy tính");
        assert_eq!(sources[0].untranslated, None);
    }

    #[test]
    fn places_are_preferred_as_explorer_prefers_them() {
        assert!(Place::UserDesktop < Place::AllUsersDesktop);
        assert!(Place::AllUsersDesktop < Place::UserStartMenu);
        assert!(Place::UserStartMenu < Place::AllUsersStartMenu);
        assert!(Place::AllUsersStartMenu < Place::TaskbarPins);
        assert!(Place::TaskbarPins < Place::PackagedApps);
    }

    fn found_file(name: &str, place: Place) -> Found {
        let path = PathBuf::from(format!(r"C:\Menu\{name}"));
        Found {
            kind: Kind::of(&path).unwrap(),
            // The file's own stem: off Windows a `\` is no separator, so
            // the stem of the whole `C:\Menu\…` path would keep its folder.
            name: Path::new(name)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            path,
            location: r"C:\Menu".into(),
            place,
        }
    }

    #[test]
    fn shortcut_files_are_known_by_their_extension() {
        let kind = |name: &str| Kind::of(Path::new(name));
        assert_eq!(kind(r"C:\Menu\Tool.lnk"), Some(Kind::Link));
        assert_eq!(kind(r"C:\Menu\Game.URL"), Some(Kind::Internet));
        assert_eq!(kind(r"C:\Menu\Orders.appref-ms"), Some(Kind::ClickOnce));
        assert_eq!(kind(r"C:\Menu\desktop.ini"), None);
        assert_eq!(kind(r"C:\Menu\Readme.txt"), None);
    }

    #[test]
    fn uninstallers_are_known_by_their_name_or_their_program() {
        assert!(is_uninstaller("Uninstall Tool"));
        assert!(is_uninstaller("Tool Uninstaller"));
        assert!(is_uninstaller("UNINSTALL"));
        assert!(!is_uninstaller("Installer Helper"));
        assert!(!is_uninstaller("Tool"));
        assert!(is_uninstaller_program(
            r"C:\Program Files\Tool\unins000.exe"
        ));
        assert!(is_uninstaller_program(r"C:\Tool\Uninstall.exe"));
        assert!(is_uninstaller_program(r"C:\Tool\uninst.exe"));
        assert!(!is_uninstaller_program(r"C:\Uninstall\tool.exe"));
        assert!(!is_uninstaller_program(r"C:\Tool\tool.exe"));
    }

    #[test]
    fn programs_are_known_by_their_extension_and_the_rest_are_documents() {
        for program in [
            r"C:\Tool\tool.exe",
            r"C:\Tool\TOOL.EXE",
            r"C:\Tool\run.cmd",
            r"C:\Tool\run.bat",
            r"C:\Windows\system32\compmgmt.msc",
            r"C:\Windows\system32\desk.cpl",
        ] {
            assert!(is_program(program), "{program}");
        }
        for document in [
            r"C:\Tool\readme.txt",
            r"C:\Tool\manual.pdf",
            r"C:\Tool\help.chm",
            r"C:\Tool\site.html",
            r"C:\Tool",
            r"C:\Tool\exe",
            "",
        ] {
            assert!(!is_program(document), "{document}");
        }
    }

    #[test]
    fn an_internet_shortcut_s_url_is_read_from_its_section() {
        assert_eq!(
            internet_shortcut_url(
                "[{000214A0-0000-0000-C000-000000000046}]\r\nProp3=19,0\r\n\
                 [InternetShortcut]\r\nIDList=\r\nURL=steam://rungameid/570\r\n\
                 IconIndex=0\r\n"
            )
            .as_deref(),
            Some("steam://rungameid/570")
        );
        assert_eq!(
            internet_shortcut_url("[internetshortcut]\nurl = com.epicgames.launcher://apps/x\n")
                .as_deref(),
            Some("com.epicgames.launcher://apps/x")
        );
        // A URL key outside the section, or none at all.
        assert_eq!(internet_shortcut_url("[Other]\nURL=steam://x\n"), None);
        assert_eq!(internet_shortcut_url("[InternetShortcut]\nURL=\n"), None);
        assert_eq!(internet_shortcut_url(""), None);
    }

    #[test]
    fn an_internet_shortcut_is_an_application_when_its_scheme_has_a_handler() {
        let handles = |scheme: &str| matches!(scheme, "steam" | "https" | "com.epicgames.launcher");
        assert!(is_application_url("steam://rungameid/570", &handles));
        assert!(is_application_url("STEAM://rungameid/570", &handles));
        assert!(is_application_url(
            "com.epicgames.launcher://apps/fortnite?action=launch",
            &handles
        ));
        // No registered handler.
        assert!(!is_application_url("unknown-launcher://game/1", &handles));
        // A web page or a document, whatever opens it.
        assert!(!is_application_url("https://example.com/", &handles));
        assert!(!is_application_url("file:///C:/notes.txt", &handles));
        // A drive letter, or no scheme.
        assert!(!is_application_url(r"C:\Games\game.exe", &handles));
        assert!(!is_application_url("rungameid/570", &handles));
        assert_eq!(url_scheme("Steam://x").as_deref(), Some("steam"));
        assert_eq!(url_scheme("1steam://x"), None);
        assert_eq!(url_scheme(r"st\eam://x"), None);
    }

    #[test]
    fn a_click_once_reference_names_its_deployment_in_utf_16() {
        let deployment = "http://apps.example.com/Orders/Orders.application#Orders.application, \
                          Culture=neutral, PublicKeyToken=0123456789abcdef, \
                          processorArchitecture=msil";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(deployment.encode_utf16().flat_map(u16::to_le_bytes));
        let text = shortcut_text(&bytes);
        assert_eq!(click_once_deployment(&text).as_deref(), Some(deployment));
        // UTF-8 too, with its byte order mark.
        let utf8 = [&[0xEF, 0xBB, 0xBF][..], deployment.as_bytes()].concat();
        assert_eq!(
            click_once_deployment(&shortcut_text(&utf8)).as_deref(),
            Some(deployment)
        );
        assert_eq!(click_once_deployment(""), None);
        assert_eq!(click_once_deployment("not a deployment"), None);
    }

    #[test]
    fn shortcuts_that_are_not_applications_are_left_out() {
        let shortcut = |target: &str| {
            Opens::Link(Some(Shortcut {
                target: target.into(),
                ..Shortcut::default()
            }))
        };
        let targets = |target: &str| match target {
            r"C:\Gone\gone.exe" => ShortcutTarget::Missing,
            r"C:\Projects" => ShortcutTarget::Folder,
            _ => ShortcutTarget::File,
        };
        let found = [
            "Tool.lnk",
            "Broken.lnk",
            "Projects.lnk",
            "Readme.lnk",
            "Remove Tool.lnk",
            "Shell item.lnk",
            "Not installed.lnk",
            "Web site.url",
            "Unhandled.url",
            "Empty.url",
            "Game.url",
            "Orders.appref-ms",
            "Empty.appref-ms",
        ]
        .into_iter()
        .map(|name| found_file(name, Place::UserDesktop))
        .collect();
        let opens = vec![
            shortcut(r"C:\Tool\tool.exe"),
            shortcut(r"C:\Gone\gone.exe"),
            shortcut(r"C:\Projects"),
            shortcut(r"C:\Tool\readme.txt"),
            shortcut(r"C:\Tool\unins000.exe"),
            // A shell item, or an MSI-advertised shortcut whose product is
            // not installed: the reader gives no target.
            shortcut(""),
            Opens::Link(Some(Shortcut::default())),
            Opens::Url(Some("https://example.com/".into())),
            Opens::Url(Some("unknown-launcher://game/1".into())),
            Opens::Url(None),
            Opens::Url(Some("steam://rungameid/570".into())),
            Opens::ClickOnce(Some(
                "http://apps.example.com/Orders.application#Orders.application".into(),
            )),
            Opens::ClickOnce(None),
        ];

        let sources = sources(found, opens, vec![], &targets, &|scheme| scheme == "steam");

        let names: Vec<&str> = sources.iter().map(|source| source.name.as_str()).collect();
        assert_eq!(names, ["Tool", "Game", "Orders"]);
        assert_eq!(sources[1].key, Key::Link("steam://rungameid/570".into()));
        assert_eq!(
            sources[2].key,
            Key::Link("http://apps.example.com/orders.application#orders.application".into())
        );
        assert_eq!(sources[1].place, Place::UserDesktop as usize);
    }

    #[test]
    fn an_advertised_shortcut_resolved_to_its_program_is_that_program_s_application() {
        // The reader resolves an MSI-advertised shortcut to the program its
        // product installed: it is one application with a plain shortcut
        // to that program, neither dropped nor listed twice.
        let word = r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE";
        let sources = link_sources(
            vec![
                found("Word", Place::AllUsersStartMenu),
                found("Word", Place::UserDesktop),
            ],
            vec![target(word), target(&word.to_lowercase())],
            vec![],
        );

        let catalog = super::super::Catalog::new(sources);
        let applications = catalog.applications();
        assert_eq!(applications.len(), 1);
        assert_eq!(applications[0].id, Key::program(word, "").id());
        assert_eq!(
            catalog.find(&applications[0].id).unwrap().primary().place,
            Place::UserDesktop as usize
        );
    }

    #[test]
    fn one_link_on_the_desktop_and_in_the_start_menu_is_one_application() {
        let found = vec![
            found_file("Game.url", Place::UserStartMenu),
            found_file("Game.url", Place::AllUsersDesktop),
        ];
        let opens = vec![
            Opens::Url(Some("steam://rungameid/570".into())),
            Opens::Url(Some("STEAM://RunGameId/570".into())),
        ];
        let sources = sources(found, opens, vec![], &|_| ShortcutTarget::File, &|_| true);

        let catalog = super::super::Catalog::new(sources);
        assert_eq!(catalog.applications().len(), 1);
        assert_eq!(
            catalog.identified()[0].primary().place,
            Place::AllUsersDesktop as usize
        );
    }
}
