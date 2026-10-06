//! The system functions on Linux (see the parent module): text through the
//! X11 clipboard adapter, which serves it for as long as Pane runs;
//! `xdg-open` for opening, and `gio launch` for an application's desktop
//! entry; the file manager's `org.freedesktop.FileManager1.ShowItems`
//! through `dbus-send` for revealing (else the folder opens); and
//! `gio trash` for the trash. X11 has no "do not record" marker, so a
//! concealed copy is a plain one; copying a file and reading the clipboard
//! say that they are not available on Linux yet.

use std::path::{Path, PathBuf};

use super::{Clip, NotTrashed, System, missing, programs};
use crate::clipboard::{ClipboardSystem, LinuxClipboard};

/// The system functions on Linux.
pub(super) struct LinuxSystem {
    /// This session's X11 clipboard, kept for as long as Pane runs: it
    /// serves what a command copied. Why there is none otherwise (Wayland,
    /// no display).
    clipboard: Result<LinuxClipboard, String>,
}

impl LinuxSystem {
    pub(super) fn new() -> LinuxSystem {
        LinuxSystem {
            clipboard: crate::clipboard::linux::native(),
        }
    }
}

impl System for LinuxSystem {
    fn copy(&self, clip: &Clip, _concealed: bool) -> Result<(), String> {
        let clipboard = self.clipboard.as_ref().map_err(Clone::clone)?;
        match clip {
            Clip::Text(text) => clipboard.write_text(text),
            Clip::File(_) => Err("Copying a file is not available on Linux yet".into()),
        }
    }

    fn read_clipboard(&self) -> Result<Option<Clip>, String> {
        Err("Reading the clipboard is not available on Linux yet".into())
    }

    fn open(&self, target: &str, application: Option<&str>) -> Result<(), String> {
        match application {
            None => programs::start("xdg-open", &[target]),
            Some(entry) if entry.ends_with(".desktop") => {
                programs::start("gio", &["launch", entry, target])
            }
            Some(program) => programs::start(program, &[target]),
        }
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        if let Some(missing) = missing(path) {
            return Err(missing);
        }
        let address = format!("file://{}", path.display());
        let shown = programs::run(
            "dbus-send",
            &[
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{address}"),
                "string:",
            ],
        );
        match shown {
            Ok(()) => Ok(()),
            // No file manager answers it: the folder opens instead.
            Err(_) => {
                let folder = path.parent().unwrap_or(path);
                programs::start("xdg-open", &[&folder.to_string_lossy()])
            }
        }
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed> {
        paths
            .iter()
            .filter_map(|path| {
                programs::run("gio", &["trash", "--", &path.to_string_lossy()])
                    .err()
                    .map(|reason| NotTrashed {
                        path: path.clone(),
                        reason,
                    })
            })
            .collect()
    }
}
