//! macOS: the application bundles (`.app` folders) in the Applications
//! folders, opened by the system's `open` command (Launch Services).

use std::path::{Path, PathBuf};

use super::{Application, Applications, env_dir, has_extension, id_path, sorted_entries};

/// How deep Pane looks into folders that are not bundles, such as
/// `/Applications/Utilities`.
const MAX_DEPTH: usize = 2;

/// The application bundles in some folders.
#[derive(Clone, Debug)]
pub struct AppBundles {
    folders: Vec<PathBuf>,
}

impl AppBundles {
    /// The bundles in `folders` and their subfolders (not inside bundles).
    pub fn new(folders: Vec<PathBuf>) -> AppBundles {
        AppBundles { folders }
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
        AppBundles { folders }
    }
}

fn collect(dir: &Path, depth: usize, found: &mut Vec<Application>) {
    for entry in sorted_entries(dir) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if has_extension(&path, "app") {
            let Some(name) = path.file_stem() else {
                continue;
            };
            found.push(Application {
                id: path.to_string_lossy().into_owned(),
                name: name.to_string_lossy().into_owned(),
                location: dir.display().to_string(),
            });
        } else if depth < MAX_DEPTH {
            collect(&path, depth + 1, found);
        }
    }
}

impl Applications for AppBundles {
    fn installed(&self) -> Result<Vec<Application>, String> {
        let mut found = Vec::new();
        for folder in &self.folders {
            collect(folder, 0, &mut found);
        }
        Ok(found)
    }

    fn open(&self, id: &str) -> Result<(), String> {
        let path = id_path(id, "app", "an application bundle")?;
        open_bundle(&path)
    }
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
