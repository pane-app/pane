//! The applications installed on the system: finding them, knowing which
//! application each is, and opening them.
//!
//! A WASI guest cannot see or start the system's applications, so Pane's host
//! does it for the Applications default extension through the
//! `pane:extension/applications` import ([`Applications`]). Each system has
//! its own adapter, a [`Discovery`] chosen by [`native`], which reports the
//! [`Source`]s it finds:
//!
//! - Windows: the shortcuts (shell links, internet shortcuts with a
//!   registered scheme, ClickOnce references) in the Start menu's Programs
//!   folders, on the Desktops and pinned to the taskbar, and the packaged
//!   apps of the Apps folder ([`StartMenu`]);
//! - macOS: the application bundles in the Applications folders ([`AppBundles`]);
//! - Linux: the XDG desktop entries in the `applications` data folders
//!   ([`DesktopEntries`]).
//!
//! The host turns those sources into applications with a stable identity
//! ([`identity`], ADR 0038): sources with one [`Key`] are one application,
//! whose id is a digest of the key, and the host keeps the map from each id
//! to the source that opens it ([`Cached`]), so an id survives an update that
//! moves the program, and an id from before identities (a source's path)
//! still finds its application. Each application is titled as the system
//! shows it (localized) and carries the other names and words that find it
//! and what tells it apart from applications of the same name ([`names`]).
//!
//! The host's list stays current by itself while a package that asked for
//! it runs ([`Cached`], ADR 0038): each adapter watches the folders it
//! finds sources in ([`Discovery::watch`]), the list is rescanned once
//! their changes settle, and an application whose last source went stays
//! listed for a grace before it leaves.
//!
//! The host also keeps each application's own icon ([`icons`], #172): it
//! extracts it at 256 pixels in the background into a cache in Pane's cache
//! folder, and returns an icon reference with each application
//! ([`icon_reference`]), so root search, quick slots and any extension's
//! list draw it.
//!
//! Finding is plain file system work and is compiled on every system, so each
//! adapter's discovery is tested everywhere with fixture folders; opening uses
//! the system's own launcher and works only on its system.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod app_bundles;
mod cached;
mod desktop_entries;
pub mod icons;
pub mod identity;
pub mod names;
mod plist;
mod start_menu;
mod watching;

pub use app_bundles::AppBundles;
pub use cached::{Cached, DEBOUNCE, GRACE};
pub use desktop_entries::DesktopEntries;
pub use identity::{Catalog, Identified, Key, Source};
pub use start_menu::{Place, Shortcut, ShortcutFolder, ShortcutTarget, StartMenu};

/// An installed application, as an extension receives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Application {
    /// Its stable id ([`Key::id`]), which identifies it to
    /// [`Applications::open`]: opaque, and the same across updates and
    /// restarts.
    pub id: String,
    /// Its name, as the system shows it (localized): its title.
    pub name: String,
    /// Where it was found, for people.
    pub location: String,
    /// The other names that find it: its untranslated name, another
    /// source's name, its program's name ([`names::alternate_titles`]).
    pub alternate_titles: Vec<String>,
    /// Words that find it besides its names (a desktop entry's
    /// `Keywords`).
    pub keywords: Vec<String>,
    /// What tells it apart from other applications with its name, when
    /// there are any ([`names::distinctions`]); `None` when it is alone
    /// with its name.
    pub distinction: Option<String>,
}

/// Finds and opens the system's installed applications.
pub trait Applications: Send + Sync + 'static {
    /// The installed applications, in no particular order. A location that is
    /// missing or cannot be read adds none.
    fn installed(&self) -> Result<Vec<Application>, String>;

    /// Opens (launches) the application `id`, as [`Applications::installed`]
    /// gave it or as it was given before identities (the path of its
    /// shortcut, bundle or desktop entry), without waiting for it to finish.
    /// An error explains why the system did not open it.
    fn open(&self, id: &str) -> Result<(), String>;

    /// What the system opens for the application `id` (current, or a path
    /// from before identities): its primary source's path, which the
    /// system's opener takes as the application to open something with.
    /// `None` when `id` names no application this host knows. It may scan
    /// the system's folders, so call it off the window's thread.
    fn source(&self, _id: &str) -> Option<String> {
        None
    }

    /// The current id of the application `id` names, when `id` is another
    /// id for it, such as the path its id was before identities. It looks
    /// only at the list already kept and never scans, so it may be called
    /// anywhere; `None` when no list is kept or `id` names nothing in it.
    fn current_id(&self, _id: &str) -> Option<String> {
        None
    }

    /// Has `changed` called, from a thread of the list's own, each time the
    /// installed applications change by themselves: a watcher saw an
    /// install or a removal, an application's grace ended, a rescan found
    /// something new. A list that never changes by itself never calls it.
    fn on_change(&self, _changed: Arc<dyn Fn() + Send + Sync>) {}

    /// No package that asked for the installed applications can run any
    /// more: drop the kept list and stop watching, until the next
    /// [`Applications::installed`]. It must not block.
    fn release(&self) {}

    /// What the icon of the application `id` is extracted from
    /// ([`icons`]): its primary source's path, or, for a packaged app also
    /// found by a shortcut, its `shell:AppsFolder\<AppUserModelID>`, whose
    /// logo has light and dark variants. `None` when `id` names no
    /// application this host knows. It may scan the system's folders, so
    /// call it off the window's thread.
    fn icon_source(&self, id: &str) -> Option<String> {
        self.source(id)
    }
}

/// The icon reference of the installed application `id`, which the host
/// returns with it (`icon` in `wit/applications.wit`): an extension shows
/// the application's own icon by naming it as an item's icon,
/// `{"application": <reference>}` (`docs/list-tree.md`, "Icons"). Opaque to
/// extensions, like the id.
pub fn icon_reference(id: &str) -> String {
    id.to_owned()
}

/// What the system opens for `application`, an installed application's id
/// or a path: the application's source when `applications` knows it, else
/// `application` as it is.
pub fn opener(applications: &dyn Applications, application: &str) -> String {
    applications
        .source(application)
        .unwrap_or_else(|| application.to_owned())
}

/// One system's way of finding its applications: an adapter, which the
/// host's list ([`Cached`]) turns into applications by identity.
pub trait Discovery: Send + Sync + 'static {
    /// Every source of an application found, in a deterministic order. A
    /// location that is missing or cannot be read adds none.
    fn sources(&self) -> Result<Vec<Source>, String>;

    /// Opens (launches) the source at `path` ([`Source::path`]), without
    /// waiting for it to finish. An error explains why the system did not
    /// open it.
    fn open(&self, path: &str) -> Result<(), String>;

    /// Starts watching the places sources are found in: `changes` is told
    /// of what changes there, from a thread of the adapter's own, until the
    /// returned watch is dropped. An adapter that cannot watch says why;
    /// the host's list then relies on its periodic rescan.
    fn watch(&self, changes: Changes) -> Result<Watch, String> {
        let _ = changes;
        Err("this system's applications are not watched".into())
    }
}

/// What a watcher saw in the places an adapter finds sources in
/// ([`Discovery::watch`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Something changed: the host's list is rescanned once the changes
    /// settle ([`DEBOUNCE`]).
    Changed,
    /// The system began a change it completes later (a packaged app being
    /// registered or removed): rescanned once the changes settle, and again
    /// a few seconds later.
    Completing,
    /// The watcher lost changes (its buffer overflowed) or failed: the
    /// whole list is rescanned at once.
    Lost,
}

/// Where an adapter tells what its watcher saw.
pub type Changes = Arc<dyn Fn(Change) + Send + Sync>;

/// Watching an adapter's places: its watchers stop when this is dropped.
pub struct Watch {
    _watchers: Box<dyn Send>,
    complete: bool,
}

impl Watch {
    /// A watch of every place, which stops when `watchers` is dropped.
    pub fn new(watchers: impl Send + 'static) -> Watch {
        Watch {
            _watchers: Box::new(watchers),
            complete: true,
        }
    }

    /// A watch missing some places (one that does not exist yet, or could
    /// not be watched), which stops when `watchers` is dropped: the host's
    /// list makes it again after each rescan, to watch them once it can.
    pub fn partial(watchers: impl Send + 'static) -> Watch {
        Watch {
            _watchers: Box::new(watchers),
            complete: false,
        }
    }

    /// Whether it watches every place.
    pub fn complete(&self) -> bool {
        self.complete
    }
}

/// How often the host's list is rescanned in full while it is kept,
/// whatever its watchers saw, to reconcile a change they missed.
pub const RECONCILE_EVERY: Duration = Duration::from_secs(30 * 60);

/// This system's adapter, reading the usual locations from the environment,
/// behind the host's live [`Cached`] list of applications by identity,
/// watched while a package that asked for it runs and reconciled every
/// [`RECONCILE_EVERY`].
pub fn native() -> Arc<dyn Applications> {
    let adapter: Arc<dyn Discovery> = if cfg!(target_os = "windows") {
        Arc::new(StartMenu::from_env())
    } else if cfg!(target_os = "macos") {
        Arc::new(AppBundles::from_env())
    } else if cfg!(target_os = "linux") {
        Arc::new(DesktopEntries::from_env())
    } else {
        Arc::new(Unsupported)
    };
    Arc::new(Cached::new(adapter, RECONCILE_EVERY))
}

/// A system Pane does not find applications on.
struct Unsupported;

impl Discovery for Unsupported {
    fn sources(&self) -> Result<Vec<Source>, String> {
        Err(unsupported())
    }

    fn open(&self, _path: &str) -> Result<(), String> {
        Err(unsupported())
    }
}

fn unsupported() -> String {
    format!(
        "Pane finds applications only on Windows, macOS and Linux, not on {}",
        std::env::consts::OS
    )
}

/// The folder in environment variable `name`, if it is set and not empty.
fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The entries of `dir`, sorted by name so that discovery is deterministic;
/// none if it cannot be read.
fn sorted_entries(dir: &Path) -> Vec<std::fs::DirEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries
}

/// Whether `path`'s extension is `extension`, ignoring case.
fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|found| found.to_str())
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

/// `path` as text, or why it is not an application Pane found.
fn id_path(id: &str, extension: &str, kind: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(id);
    if !path.is_absolute() || !has_extension(&path, extension) {
        return Err(format!("{id} is not {kind}"));
    }
    if !path.exists() {
        return Err(format!("{id} no longer exists"));
    }
    Ok(path)
}
