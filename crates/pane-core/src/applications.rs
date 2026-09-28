//! The applications installed on the system: finding them and opening them.
//!
//! A WASI guest cannot see or start the system's applications, so Pane's host
//! does it for the Applications default extension through the
//! `pane:extension/applications` import ([`Applications`]). Each system has
//! its own adapter, chosen by [`native`]:
//!
//! - Windows: the shortcuts in the Start menu's Programs folders ([`StartMenu`]);
//! - macOS: the application bundles in the Applications folders ([`AppBundles`]);
//! - Linux: the XDG desktop entries in the `applications` data folders
//!   ([`DesktopEntries`]).
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
mod start_menu;

pub use app_bundles::AppBundles;
pub use cached::Cached;
pub use desktop_entries::DesktopEntries;
pub use start_menu::StartMenu;

/// An installed application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    /// Identifies it to [`Applications::open`]: the path of the shortcut,
    /// bundle or desktop entry it was found by.
    pub id: String,
    /// Its name, as the system shows it.
    pub name: String,
    /// Where it was found, for people.
    pub location: String,
}

/// Finds and opens the system's installed applications.
pub trait Applications: Send + Sync + 'static {
    /// The installed applications, in no particular order. A location that is
    /// missing or cannot be read adds none.
    fn installed(&self) -> Result<Vec<Application>, String>;

    /// Opens (launches) the application `id`, as [`Applications::installed`]
    /// gave it, without waiting for it to finish. An error explains why the
    /// system did not open it.
    fn open(&self, id: &str) -> Result<(), String>;
}

/// How old the kept list of applications may get before a guest asking for
/// it has it rescanned in the background ([`Cached`]).
pub const RESCAN_AFTER: Duration = Duration::from_secs(10);

/// This system's adapter, reading the usual locations from the environment,
/// behind a [`Cached`] list rescanned in the background once older than
/// [`RESCAN_AFTER`].
pub fn native() -> Arc<dyn Applications> {
    let adapter: Arc<dyn Applications> = if cfg!(target_os = "windows") {
        Arc::new(StartMenu::from_env())
    } else if cfg!(target_os = "macos") {
        Arc::new(AppBundles::from_env())
    } else if cfg!(target_os = "linux") {
        Arc::new(DesktopEntries::from_env())
    } else {
        Arc::new(Unsupported)
    };
    Arc::new(Cached::new(adapter, RESCAN_AFTER))
}

/// A system Pane does not find applications on.
struct Unsupported;

impl Applications for Unsupported {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Err(unsupported())
    }

    fn open(&self, _id: &str) -> Result<(), String> {
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
