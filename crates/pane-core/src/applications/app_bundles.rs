//! macOS: the application bundles (`.app` folders) in the Applications
//! folders, opened by the system's `open` command (Launch Services). A
//! bundle is identified by its bundle identifier (`CFBundleIdentifier` in
//! its `Info.plist`), so moving or renaming it keeps the application; one
//! without an identifier is identified by its path.
//!
//! A bundle is titled with its display name as Finder shows it
//! (`NSFileManager`'s display name, localized for the user's languages);
//! its folder name, when it differs, still finds it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    Changes, Discovery, Key, Source, Watch, env_dir, has_extension, id_path, plist, sorted_entries,
    watching,
};

/// How deep Pane looks into folders that are not bundles, such as
/// `/Applications/Utilities`.
const MAX_DEPTH: usize = 2;

/// Gives a bundle's display name, as Finder shows it; `None` when there is
/// none to give.
type DisplayNames = Arc<dyn Fn(&Path) -> Option<String> + Send + Sync>;

/// The application bundles in some folders.
#[derive(Clone)]
pub struct AppBundles {
    folders: Vec<PathBuf>,
    /// Gives each bundle's display name.
    display_name: DisplayNames,
}

impl std::fmt::Debug for AppBundles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBundles")
            .field("folders", &self.folders)
            .finish_non_exhaustive()
    }
}

impl AppBundles {
    /// The bundles in `folders` and their subfolders (not inside bundles),
    /// a bundle in an earlier folder preferred to one with the same
    /// identifier in a later folder.
    pub fn new(folders: Vec<PathBuf>) -> AppBundles {
        AppBundles {
            folders,
            display_name: Arc::new(display_name),
        }
    }

    /// The bundles in `/Applications`, `/System/Applications` and
    /// `~/Applications` (`$HOME/Applications`), and their subfolders such as
    /// `Utilities`.
    pub fn from_env() -> AppBundles {
        let mut folders = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
        ];
        folders.extend(env_dir("HOME").map(|home| home.join("Applications")));
        AppBundles::new(folders)
    }

    /// This, naming bundles with `display_name` instead of Finder, which
    /// exists only on macOS: what tests use to give bundles localized names
    /// on every system.
    pub fn with_display_names(
        mut self,
        display_name: impl Fn(&Path) -> Option<String> + Send + Sync + 'static,
    ) -> AppBundles {
        self.display_name = Arc::new(display_name);
        self
    }
}

/// The key of the bundle at `bundle`: its identifier, in lowercase, or its
/// path when its `Info.plist` names none.
fn bundle_key(bundle: &Path) -> Key {
    std::fs::read(bundle.join("Contents/Info.plist"))
        .ok()
        .and_then(|info| plist::string(&info, "CFBundleIdentifier"))
        .map(|identifier| identifier.trim().to_lowercase())
        .filter(|identifier| !identifier.is_empty())
        .map_or_else(
            || Key::Path(bundle.to_string_lossy().into_owned()),
            Key::Bundle,
        )
}

/// `shown`, a bundle's display name, as a title: trimmed, without the
/// `.app` Finder shows only when told to show every extension.
fn shown_name(shown: &str) -> String {
    let shown = shown.trim();
    let stem = shown.len().checked_sub(4).and_then(|end| {
        (shown.is_char_boundary(end) && shown[end..].eq_ignore_ascii_case(".app"))
            .then(|| &shown[..end])
    });
    stem.unwrap_or(shown).trim().to_owned()
}

impl AppBundles {
    fn collect(&self, dir: &Path, depth: usize, place: usize, found: &mut Vec<Source>) {
        for entry in sorted_entries(dir) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if has_extension(&path, "app") {
                let Some(folder_name) = path.file_stem() else {
                    continue;
                };
                let folder_name = folder_name.to_string_lossy().into_owned();
                let shown = (self.display_name)(&path)
                    .map(|shown| shown_name(&shown))
                    .filter(|shown| !shown.is_empty());
                let (name, untranslated) = match shown {
                    Some(shown) if shown != folder_name => (shown, Some(folder_name)),
                    _ => (folder_name, None),
                };
                found.push(Source {
                    untranslated,
                    ..Source::new(
                        bundle_key(&path),
                        path.to_string_lossy(),
                        name,
                        dir.display().to_string(),
                        place,
                    )
                });
            } else if depth < MAX_DEPTH {
                self.collect(&path, depth + 1, place, found);
            }
        }
    }
}

impl Discovery for AppBundles {
    fn sources(&self) -> Result<Vec<Source>, String> {
        let mut found = Vec::new();
        for (place, folder) in self.folders.iter().enumerate() {
            self.collect(folder, 0, place, &mut found);
        }
        Ok(found)
    }

    fn open(&self, path: &str) -> Result<(), String> {
        let path = id_path(path, "app", "an application bundle")?;
        open_bundle(&path)
    }

    /// Watches the Applications folders and everything in them (FSEvents
    /// on macOS); one that does not exist yet is watched for from the
    /// nearest folder above it that does.
    fn watch(&self, changes: Changes) -> Result<Watch, String> {
        watching::watch(
            self.folders
                .iter()
                .map(|folder| watching::Folder::walked(folder.clone()))
                .collect(),
            changes,
        )
    }
}

/// The display name Finder shows for the bundle at `bundle`, localized for
/// the user's languages.
#[cfg(target_os = "macos")]
fn display_name(bundle: &Path) -> Option<String> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSFileManager, NSString};

    autoreleasepool(|_| {
        let path = NSString::from_str(&bundle.to_string_lossy());
        let shown = NSFileManager::defaultManager().displayNameAtPath(&path);
        Some(shown.to_string()).filter(|shown| !shown.trim().is_empty())
    })
}

/// Bundles are named by Finder only on macOS: elsewhere, by their folder.
#[cfg(not(target_os = "macos"))]
fn display_name(_bundle: &Path) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn open_bundle(path: &Path) -> Result<(), String> {
    // `open` returns once Launch Services has started (or activated) the
    // application, and explains a failure on its standard error.
    let output = std::process::Command::new("/usr/bin/open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("cannot run /usr/bin/open: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if reason.is_empty() {
        format!("/usr/bin/open failed with {}", output.status)
    } else {
        reason
    })
}

#[cfg(not(target_os = "macos"))]
fn open_bundle(path: &Path) -> Result<(), String> {
    Err(format!(
        "{} is a macOS application bundle; Pane opens those only on macOS",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_display_name_loses_the_extension_finder_may_show() {
        assert_eq!(shown_name("Calculator.app"), "Calculator");
        assert_eq!(shown_name(" Máy tính "), "Máy tính");
        assert_eq!(shown_name("App"), "App");
        assert_eq!(shown_name(".app"), "");
    }
}
