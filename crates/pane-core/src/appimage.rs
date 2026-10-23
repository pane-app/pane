//! Running from an AppImage on Linux (#464): the one file a user keeps,
//! which the AppImage runtime mounts and starts the `pane` program inside.
//!
//! The program the system reports (`std::env::current_exe`) is then the
//! one inside the mount (`/tmp/.mount_…/usr/bin/pane`), which is read only
//! and gone once Pane exits. What Pane names as its own program has to be
//! the AppImage file instead: an update replaces that file (ADR 0049), and
//! the autostart entry starts it. The runtime tells the program where both
//! are, in its environment: `APPIMAGE`, the AppImage file's absolute path,
//! and `APPDIR`, the folder it is mounted at.
//!
//! Those variables, and the `ARGV0` and `OWD` the runtime sets beside
//! them, would also reach every program Pane starts, which would then
//! think it runs from Pane's AppImage (an Electron application's updater
//! reads `APPIMAGE` to decide how to update itself). [`adopt`] therefore
//! reads them once, as Pane starts, and takes them out of Pane's
//! environment; [`running`] answers from what it read.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The variables the AppImage runtime sets for the program it starts.
const RUNTIME_VARIABLES: [&str; 4] = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"];

/// The AppImage [`adopt`] found Pane running from, if any.
static RUNNING: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Reads, once, whether this Pane runs from an AppImage, and takes the
/// AppImage runtime's variables out of the environment so the programs
/// Pane starts do not inherit them. Pane's `main` calls it first thing.
/// Off Linux, and in a Pane not started by an AppImage runtime, it finds
/// none and changes nothing.
///
/// # Safety
///
/// It changes the process's environment, so it must be called before any
/// other thread is started, as Rust's `std::env::remove_var` requires.
pub unsafe fn adopt() {
    let found = found(
        std::env::var_os("APPIMAGE").map(PathBuf::from),
        std::env::var_os("APPDIR").map(PathBuf::from),
        std::env::current_exe().ok(),
    );
    if found.is_some() {
        for variable in RUNTIME_VARIABLES {
            // SAFETY: the caller guarantees no other thread runs yet, so
            // nothing reads the environment while it changes.
            unsafe { std::env::remove_var(variable) };
        }
    }
    let _ = RUNNING.set(found);
}

/// The AppImage file this Pane runs from, as [`adopt`] found it: `None`
/// for a Pane that runs its program directly (or that never adopted).
pub fn running() -> Option<&'static Path> {
    RUNNING.get().and_then(Option::as_deref)
}

/// The AppImage that `appimage` and `appdir` (the runtime's variables)
/// name for the program at `exe`: only when the program runs from inside
/// the mount `appdir` names, so a program that merely inherited the
/// variables (one started by another AppImage's program) is not taken
/// for an AppImage, and only for an absolute path to a file.
fn found(
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
    exe: Option<PathBuf>,
) -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let (appimage, appdir, exe) = (appimage?, appdir?, exe?);
    let inside = !appdir.as_os_str().is_empty() && exe.starts_with(&appdir);
    (inside && appimage.is_absolute() && appimage.is_file()).then_some(appimage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_program_inside_the_mount_runs_from_the_appimage() {
        let folder = tempfile::tempdir().unwrap();
        let appimage = folder.path().join("Pane-0.1.0-x86_64.AppImage");
        std::fs::write(&appimage, b"an AppImage").unwrap();
        let mount = PathBuf::from("/tmp/.mount_PaneXYZ");
        let inside = mount.join("usr/bin/pane");
        let found = |image: Option<&Path>, dir: Option<&Path>, exe: &Path| {
            super::found(
                image.map(Path::to_path_buf),
                dir.map(Path::to_path_buf),
                Some(exe.to_path_buf()),
            )
        };
        if cfg!(target_os = "linux") {
            assert_eq!(
                found(Some(&appimage), Some(&mount), &inside),
                Some(appimage.clone())
            );
        } else {
            // Only Linux runs AppImages.
            assert_eq!(found(Some(&appimage), Some(&mount), &inside), None);
        }
        // A program elsewhere that inherited the variables, an AppImage
        // that is not a file, a relative path, or a variable missing.
        let elsewhere = Path::new("/home/u/.local/bin/pane");
        assert_eq!(found(Some(&appimage), Some(&mount), elsewhere), None);
        assert_eq!(
            found(
                Some(&folder.path().join("gone.AppImage")),
                Some(&mount),
                &inside
            ),
            None
        );
        assert_eq!(
            found(Some(Path::new("Pane.AppImage")), Some(&mount), &inside),
            None
        );
        assert_eq!(found(None, Some(&mount), &inside), None);
        assert_eq!(found(Some(&appimage), None, &inside), None);
        assert_eq!(found(Some(&appimage), Some(Path::new("")), &inside), None);
    }
}
