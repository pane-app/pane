//! The system as a command reaches it through Pane (`wit/system.wit`, ADR
//! 0037): the clipboard (copy, concealed or not, and read), opening
//! anything (a URL of any scheme, a file, a folder or an application, with
//! the system's handler or a named application), showing a path in the
//! file manager, and moving paths to the Recycle Bin.
//!
//! Opening is unfiltered, as Raycast's is: the extension is trusted (ADR
//! 0002) and can already run programs (ADR 0033), so a filter would protect
//! nothing. Each function does only what it names; none closes the window
//! or tells the user anything. The SDKs' standard actions compose them with
//! the window and feedback host functions (`crate::feedback`).
//!
//! The launcher reaches the system through one trait, [`System`], given to
//! it as the link opener and the clipboard are
//! ([`crate::Launcher::with_system`]): the app passes [`native`], tests a
//! recording fake. A launcher given none answers each function with
//! [`none`]'s refusal. Each adapter is called off the runtime's thread, so
//! it may block for as long as the system takes (a handler starting, the
//! clipboard held open by another program).
//!
//! - Windows: the clipboard through the clipboard adapter's own window
//!   (text as `CF_UNICODETEXT`, a file as `CF_HDROP`; a concealed copy
//!   carries `ExcludeClipboardContentFromMonitorProcessing`,
//!   `CanIncludeInClipboardHistory` 0 and `CanUploadToCloudClipboard` 0),
//!   opening with `ShellExecuteExW` (an application by its path, its Start
//!   menu shortcut or its `shell:AppsFolder` id, the target as its
//!   argument), File Explorer's selection with
//!   `SHOpenFolderAndSelectItems`, and the Recycle Bin with
//!   `SHFileOperationW` (`FOF_ALLOWUNDO`, warning before a path that would
//!   be deleted for good, such as one on a network drive);
//! - macOS: the general pasteboard (text, a file URL, the
//!   `org.nspasteboard.ConcealedType` marker), `/usr/bin/open` (with `-a`
//!   for an application, `-R` to reveal), and `NSFileManager`'s
//!   `trashItemAtURL`;
//! - Linux: the X11 clipboard for text (X11 has no "do not record" marker,
//!   so a concealed copy is a plain one there), `xdg-open` (an application's
//!   desktop entry through `gio launch`), the file manager's
//!   `org.freedesktop.FileManager1.ShowItems` (else the folder opens), and
//!   `gio trash`. Copying a file and reading the clipboard answer that
//!   they are not available on Linux yet.

use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod programs;
#[cfg(target_os = "windows")]
mod windows;

/// The most text `read-clipboard` answers, in bytes of UTF-8: a command
/// reading more is told so rather than given it, since its memory is
/// bounded (`crate::GUEST_MEMORY`).
pub const MAX_CLIPBOARD_TEXT: usize = 4 * 1024 * 1024;

/// What is put on the clipboard, or read from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    /// Text.
    Text(String),
    /// A file or folder, by its absolute path, as copying it in the file
    /// manager puts it there.
    File(PathBuf),
}

/// A path [`System::trash`] did not move, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotTrashed {
    pub path: PathBuf,
    pub reason: String,
}

/// The system Pane runs on, as the `system` host functions reach it. Each
/// is called off the runtime's thread and may block; an error explains to
/// the user why the system did not do it.
pub trait System: Send + Sync + 'static {
    /// Puts `clip` on the clipboard, replacing what was there; `concealed`
    /// marks it with the system's "do not record" markers where it has
    /// them, so clipboard managers (Pane's own history among them) skip it.
    fn copy(&self, clip: &Clip, concealed: bool) -> Result<(), String>;

    /// What the clipboard holds now: its text, or the first file copied in
    /// the file manager; `None` when it holds neither.
    fn read_clipboard(&self) -> Result<Option<Clip>, String>;

    /// Opens `target` (a URL of any scheme, a file, a folder or an
    /// application) with the system's handler, or with `application` (a
    /// path, or an installed application's id) when one is named, without
    /// waiting for what opens it.
    fn open(&self, target: &str, application: Option<&str>) -> Result<(), String>;

    /// Shows `path`, which exists, selected in the system's file manager.
    fn reveal(&self, path: &Path) -> Result<(), String>;

    /// Moves each of `paths`, absolute, to the Recycle Bin (the system's
    /// trash); answers those it did not move, each with why.
    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed>;
}

/// This system's adapter: Windows', macOS' or Linux's, or one that
/// explains that Pane reaches no system functions here.
pub fn native() -> Arc<dyn System> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsSystem)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacosSystem)
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(linux::LinuxSystem::new())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Arc::new(Unavailable(format!(
            "Not available on {}: Pane reaches the clipboard, opens things and recycles them \
             only on Windows, macOS and Linux",
            std::env::consts::OS
        )))
    }
}

/// The system of a launcher given none: every function says that this
/// Pane does not reach the system.
pub fn none() -> Arc<dyn System> {
    Arc::new(Unavailable(
        "Not available: this Pane does not reach the system's clipboard, open things or \
         recycle them"
            .into(),
    ))
}

/// A system Pane reaches nothing of, saying why.
struct Unavailable(String);

impl System for Unavailable {
    fn copy(&self, _clip: &Clip, _concealed: bool) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn read_clipboard(&self) -> Result<Option<Clip>, String> {
        Err(self.0.clone())
    }

    fn open(&self, _target: &str, _application: Option<&str>) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn reveal(&self, _path: &Path) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed> {
        paths
            .iter()
            .map(|path| NotTrashed {
                path: path.clone(),
                reason: self.0.clone(),
            })
            .collect()
    }
}

/// `path`, as a command gave it, if it is absolute; otherwise why Pane
/// does not act on it (a relative path would be resolved against Pane's
/// own working folder, which the command knows nothing of).
pub(crate) fn absolute(path: &str) -> Result<PathBuf, String> {
    let given = PathBuf::from(path);
    if path.trim().is_empty() {
        return Err("No path was given".into());
    }
    if !given.is_absolute() {
        return Err(format!(
            "“{path}” is not an absolute path; name the file or folder from the root of its drive"
        ));
    }
    Ok(given)
}

/// Checks `paths` as a command gave them to `trash`: the absolute ones, for
/// the system to move, and the others, which Pane does not move, each with
/// why.
pub(crate) fn trashable(paths: &[String]) -> (Vec<PathBuf>, Vec<NotTrashed>) {
    let mut kept = Vec::new();
    let mut refused = Vec::new();
    for path in paths {
        match absolute(path) {
            Ok(absolute) => kept.push(absolute),
            Err(reason) => refused.push(NotTrashed {
                path: PathBuf::from(path),
                reason,
            }),
        }
    }
    (kept, refused)
}

/// Why `path` is not one to reveal or recycle, if it does not exist: the
/// adapters check this before asking the system, whose own answer would
/// be less clear.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
pub(crate) fn missing(path: &Path) -> Option<String> {
    path.symlink_metadata()
        .is_err()
        .then(|| format!("{} does not exist", path.display()))
}

/// The names a concealed copy carries on Windows, with their DWORD value:
/// clipboard monitors must not look at it, and Windows' clipboard history
/// and cloud clipboard must not keep it. Pane's own history skips any copy
/// carrying one of them (`crate::clipboard::Markers`).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const CONCEALED_MARKERS: [(&str, u32); 3] = [
    ("ExcludeClipboardContentFromMonitorProcessing", 0),
    ("CanIncludeInClipboardHistory", 0),
    ("CanUploadToCloudClipboard", 0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launcher_given_no_system_says_so_for_each_function() {
        let system = none();
        let why = system.copy(&Clip::Text("x".into()), false).unwrap_err();
        assert!(why.starts_with("Not available"), "{why}");
        assert_eq!(system.read_clipboard(), Err(why.clone()));
        assert_eq!(system.open("https://example.com", None), Err(why.clone()));
        assert_eq!(system.reveal(Path::new("/x")), Err(why.clone()));
        assert_eq!(
            system.trash(&[PathBuf::from("/x")]),
            [NotTrashed {
                path: PathBuf::from("/x"),
                reason: why
            }]
        );
    }

    #[test]
    fn only_an_absolute_path_is_acted_on() {
        assert!(
            absolute("notes.txt")
                .unwrap_err()
                .contains("not an absolute path")
        );
        assert!(absolute("  ").is_err());
        let here = std::env::temp_dir();
        assert_eq!(absolute(&here.to_string_lossy()), Ok(here));
    }

    #[test]
    fn trash_refuses_relative_paths_and_keeps_the_others() {
        let folder = tempfile::tempdir().unwrap();
        let present = folder.path().join("present.txt");
        std::fs::write(&present, "x").unwrap();
        let gone = folder.path().join("missing.txt");
        let (kept, refused) = trashable(&[
            present.to_string_lossy().into_owned(),
            gone.to_string_lossy().into_owned(),
            "relative.txt".into(),
        ]);
        // The system says what became of a missing one.
        assert_eq!(kept, [present.clone(), gone.clone()]);
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].path, PathBuf::from("relative.txt"));
        assert!(refused[0].reason.contains("not an absolute path"));

        assert_eq!(missing(&present), None);
        assert_eq!(
            missing(&gone),
            Some(format!("{} does not exist", gone.display()))
        );
    }

    #[test]
    fn a_concealed_copy_is_one_pane_s_history_skips() {
        // The markers a concealed copy carries on Windows are the ones the
        // clipboard adapter reads as forbidding a copy to be kept.
        let markers = crate::clipboard::Markers {
            exclude_from_monitoring: CONCEALED_MARKERS
                .iter()
                .any(|(name, _)| *name == "ExcludeClipboardContentFromMonitorProcessing"),
            include_in_history: CONCEALED_MARKERS
                .iter()
                .find(|(name, _)| *name == "CanIncludeInClipboardHistory")
                .map(|(_, value)| *value != 0),
            upload_to_cloud: CONCEALED_MARKERS
                .iter()
                .find(|(name, _)| *name == "CanUploadToCloudClipboard")
                .map(|(_, value)| *value != 0),
        };
        assert_eq!(
            markers,
            crate::clipboard::Markers {
                exclude_from_monitoring: true,
                include_in_history: Some(false),
                upload_to_cloud: Some(false),
            }
        );
        assert!(!markers.allow());
    }
}
