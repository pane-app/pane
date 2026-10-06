//! A recording fake of the system the `system` host functions act on
//! (#145): what commands copied, opened, revealed and moved to the Recycle
//! Bin, in order, with a clipboard of its own and a list of installed
//! applications for Open With…. Passed to the launcher with
//! `Launcher::with_system`, as the link opener and the clipboard are, and
//! to the runtime with `Runtime::set_applications`. Shared by the test
//! binaries that drive the launcher.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pane_core::applications::{Application, Applications};
use pane_core::system::{Clip, NotTrashed, System};

/// What a command had the system do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    Copied {
        clip: Clip,
        concealed: bool,
    },
    Opened {
        target: String,
        application: Option<String>,
    },
    Revealed(PathBuf),
    /// The paths it was asked to move, whether it moved them or not.
    Trashed(Vec<PathBuf>),
}

/// The file name a path the fake does not move to the Recycle Bin has,
/// with why it does not.
pub const KEPT: (&str, &str) = ("Keep me.txt", "It is open in another program");

/// A system that records what it is asked to do and does it to a
/// clipboard of its own; it moves every path to the Recycle Bin but one
/// named [`KEPT`].
#[derive(Default)]
pub struct RecordingSystem {
    done: Mutex<Vec<Done>>,
    clipboard: Mutex<Option<Clip>>,
}

impl RecordingSystem {
    /// What it was asked to do so far, in order, forgotten once read.
    pub fn take(&self) -> Vec<Done> {
        std::mem::take(&mut *self.done.lock().unwrap())
    }

    /// Puts `clip` on its clipboard, as another program copying would.
    pub fn set_clipboard(&self, clip: Option<Clip>) {
        *self.clipboard.lock().unwrap() = clip;
    }

    /// What its clipboard holds.
    pub fn clipboard(&self) -> Option<Clip> {
        self.clipboard.lock().unwrap().clone()
    }

    fn note(&self, done: Done) {
        self.done.lock().unwrap().push(done);
    }
}

impl System for RecordingSystem {
    fn copy(&self, clip: &Clip, concealed: bool) -> Result<(), String> {
        self.set_clipboard(Some(clip.clone()));
        self.note(Done::Copied {
            clip: clip.clone(),
            concealed,
        });
        Ok(())
    }

    fn read_clipboard(&self) -> Result<Option<Clip>, String> {
        Ok(self.clipboard())
    }

    fn open(&self, target: &str, application: Option<&str>) -> Result<(), String> {
        self.note(Done::Opened {
            target: target.to_owned(),
            application: application.map(str::to_owned),
        });
        Ok(())
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        self.note(Done::Revealed(path.to_path_buf()));
        Ok(())
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed> {
        self.note(Done::Trashed(paths.to_vec()));
        paths
            .iter()
            .filter(|path| path.file_name().is_some_and(|name| name == KEPT.0))
            .map(|path| NotTrashed {
                path: path.clone(),
                reason: KEPT.1.into(),
            })
            .collect()
    }
}

/// The applications Open With… lists, in the order the system gives them
/// (not by name).
pub fn installed_applications() -> Vec<Application> {
    ["Zed", "Notepad", "code editor"]
        .into_iter()
        .map(|name| Application {
            id: format!("app:{name}"),
            name: name.into(),
            location: "/apps".into(),
        })
        .collect()
}

impl Applications for RecordingSystem {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(installed_applications())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        System::open(self, id, None)
    }
}
