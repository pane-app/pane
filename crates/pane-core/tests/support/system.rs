//! A recording fake of the system the `system` host functions act on
//! (#145): what commands copied, opened, revealed and moved to the Recycle
//! Bin, in order, with a clipboard of its own and a list of installed
//! applications for Open With…; and, for #148, pastes (when it is told it
//! can paste), the application in front and the selected text, which it
//! answers as it is told to, else "not available" as the real adapters do.
//! Passed to the launcher with
//! `Launcher::with_system`, as the link opener and the clipboard are, and
//! to the runtime with `Runtime::set_applications`. Shared by the test
//! binaries that drive the launcher.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pane_core::applications::{Application, Applications};
use pane_core::system::{Clip, FrontApplication, NotTrashed, System, SystemError};

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
    /// It had the application in front paste what its clipboard held then.
    Pasted(Option<Clip>),
}

/// What the fake says of paste, the front application and the selected
/// text until it is told to answer them: not available, as the real
/// adapters say until #125.
pub const NOT_YET: &str = "Not available in this test";

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
    /// Whether it can paste.
    pastes: Mutex<bool>,
    /// Why its paste fails, when it does.
    paste_fails: Mutex<Option<String>>,
    /// What another program copies while it pastes.
    copied_meanwhile: Mutex<Option<Clip>>,
    /// What it answers for the front application, once told.
    front: Mutex<Option<Result<Option<FrontApplication>, SystemError>>>,
    /// What it answers for the selected text, once told.
    selection: Mutex<Option<Result<Option<String>, SystemError>>>,
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

    /// From now on, it can paste.
    pub fn support_paste(&self) {
        *self.pastes.lock().unwrap() = true;
    }

    /// From now on, its paste fails with `why` (after the application was
    /// asked to paste).
    pub fn fail_paste(&self, why: &str) {
        *self.paste_fails.lock().unwrap() = Some(why.into());
    }

    /// While it pastes, another program copies `clip`.
    pub fn copy_while_pasting(&self, clip: Clip) {
        *self.copied_meanwhile.lock().unwrap() = Some(clip);
    }

    /// From now on, it answers `answer` for the front application.
    pub fn set_front_application(&self, answer: Result<Option<FrontApplication>, SystemError>) {
        *self.front.lock().unwrap() = Some(answer);
    }

    /// From now on, it answers `answer` for the selected text.
    pub fn set_selected_text(&self, answer: Result<Option<String>, SystemError>) {
        *self.selection.lock().unwrap() = Some(answer);
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

    fn can_paste(&self) -> Result<(), SystemError> {
        if *self.pastes.lock().unwrap() {
            Ok(())
        } else {
            Err(SystemError::NotAvailable(NOT_YET.into()))
        }
    }

    fn paste_clipboard(&self) -> Result<(), SystemError> {
        self.note(Done::Pasted(self.clipboard()));
        if let Some(clip) = self.copied_meanwhile.lock().unwrap().take() {
            self.set_clipboard(Some(clip));
        }
        match self.paste_fails.lock().unwrap().clone() {
            Some(why) => Err(SystemError::Failed(why)),
            None => Ok(()),
        }
    }

    fn front_application(&self) -> Result<Option<FrontApplication>, SystemError> {
        self.front
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Err(SystemError::NotAvailable(NOT_YET.into())))
    }

    fn selected_text(&self) -> Result<Option<String>, SystemError> {
        self.selection
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Err(SystemError::NotAvailable(NOT_YET.into())))
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
