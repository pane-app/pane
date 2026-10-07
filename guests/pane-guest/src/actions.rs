//! The standard actions, as Raycast's built-in ones behave: Copy, Paste,
//! Open, Open With…, Show in Explorer (named for the system) and Move to
//! Recycle Bin. Each does its work through [`crate::system`], then closes
//! the window, as Raycast's do; Copy and Move to Recycle Bin then say what
//! they did in a HUD ("Copied to Clipboard"). Asked to keep the window
//! open, one says it in a toast instead. Paste closes the window by nature;
//! where Pane cannot paste yet, it copies instead and says so in a HUD
//! ("Copied — paste is not available here yet").
//!
//! ```ignore
//! use pane_guest::actions;
//! use pane_guest::system::Clip;
//!
//! Item::new("note", "Note").actions([
//!     actions::copy(Clip::Text("hunter2".into())).concealed().into(),
//!     actions::paste(Clip::Text("Kind regards".into())).into(),
//!     actions::open("https://example.com").into(),
//!     actions::open_with(r"C:\Notes\todo.txt").into(),
//!     actions::show_in_file_manager(r"C:\Notes\todo.txt").into(),
//!     actions::move_to_trash([r"C:\Notes\todo.txt"]).into(),
//! ])
//! ```
//!
//! They are compositions of host functions, not part of Pane's contract
//! (ADR 0037): an author can compose the same steps differently. Each is
//! an [`Action`] once converted, so it can be given a section or a
//! shortcut like any other.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::feedback::{Toast, ToastStyle, show_hud, show_toast};
use crate::list::{Action, Submenu};
use crate::system::{self, Clip, SystemError};
use crate::window::{PopToRootType, close};

/// A standard action, before it becomes an [`Action`]: what it does, its
/// title, and whether it keeps the window open.
pub struct Standard {
    does: Does,
    title: Option<String>,
    keep_open: bool,
}

/// What a standard action does.
enum Does {
    Copy {
        content: Clip,
        concealed: bool,
    },
    Paste {
        content: Clip,
    },
    Open {
        target: String,
        application: Option<String>,
    },
    OpenWith {
        target: String,
    },
    Reveal {
        path: String,
    },
    Trash {
        paths: Vec<String>,
    },
}

/// Copy (titled "Copy to Clipboard"): puts `content` on the clipboard,
/// closes the window and shows "Copied to Clipboard" in a HUD.
pub fn copy(content: Clip) -> Standard {
    Standard::new(Does::Copy {
        content,
        concealed: false,
    })
}

/// Paste (titled "Paste"): closes the window and pastes `content` into the
/// application that was in front before Pane, putting back what the
/// clipboard held. Where Pane cannot paste yet, it copies `content`
/// instead, closes the window and says so in a HUD ([`PASTE_FALLBACK`]).
/// It always closes the window: [`Standard::keep_window_open`] does not
/// apply.
pub fn paste(content: Clip) -> Standard {
    Standard::new(Does::Paste { content })
}

/// What Paste says in a HUD when it copied instead, where Pane cannot paste
/// yet.
pub const PASTE_FALLBACK: &str = "Copied — paste is not available here yet";

/// Open: opens `target` (a URL of any scheme, a file, a folder or an
/// application) with the system's handler, then closes the window.
pub fn open(target: impl Into<String>) -> Standard {
    Standard::new(Does::Open {
        target: target.into(),
        application: None,
    })
}

/// Open With…: a submenu of the installed applications, by name; the one
/// chosen opens `target`, then the window closes.
pub fn open_with(target: impl Into<String>) -> Standard {
    Standard::new(Does::OpenWith {
        target: target.into(),
    })
}

/// Show in Explorer ("Show in Finder" on macOS, "Show in File Manager"
/// elsewhere): shows `path` selected in the file manager, then closes the
/// window.
pub fn show_in_file_manager(path: impl Into<String>) -> Standard {
    Standard::new(Does::Reveal { path: path.into() })
}

/// Move to Recycle Bin ("Move to Trash" elsewhere), in the destructive
/// style: moves `paths` to the Recycle Bin, closes the window and says so
/// in a HUD. If some could not be moved, it fails, naming them.
pub fn move_to_trash(paths: impl IntoIterator<Item = impl Into<String>>) -> Standard {
    Standard::new(Does::Trash {
        paths: paths.into_iter().map(Into::into).collect(),
    })
}

impl Standard {
    fn new(does: Does) -> Standard {
        Standard {
            does,
            title: None,
            keep_open: false,
        }
    }

    /// This action titled `title` instead of its standard title.
    pub fn title(mut self, title: impl Into<String>) -> Standard {
        self.title = Some(title.into());
        self
    }

    /// For Copy: a concealed copy, which clipboard managers (Pane's own
    /// clipboard history among them) do not keep. Other actions ignore it.
    pub fn concealed(mut self) -> Standard {
        if let Does::Copy { concealed, .. } = &mut self.does {
            *concealed = true;
        }
        self
    }

    /// For Open: opens with `application` (its path, or an installed
    /// application's id) instead of the system's handler. Other actions
    /// ignore it.
    pub fn application(mut self, application: impl Into<String>) -> Standard {
        if let Does::Open {
            application: chosen,
            ..
        } = &mut self.does
        {
            *chosen = Some(application.into());
        }
        self
    }

    /// Keeps the window open after the action: what it did is then said in
    /// a toast instead of a HUD. Paste ignores it: it closes the window by
    /// nature.
    pub fn keep_window_open(mut self) -> Standard {
        self.keep_open = true;
        self
    }

    /// Its standard title, as Raycast names it, named for the system where
    /// the system's names differ.
    fn standard_title(&self) -> String {
        match &self.does {
            Does::Copy { .. } => "Copy to Clipboard".into(),
            Does::Paste { .. } => "Paste".into(),
            Does::Open { .. } => "Open".into(),
            Does::OpenWith { .. } => "Open With…".into(),
            Does::Reveal { .. } => format!("Show in {}", system::file_manager_name()),
            Does::Trash { .. } => format!("Move to {}", system::trash_name()),
        }
    }
}

impl From<Standard> for Action {
    fn from(standard: Standard) -> Action {
        let title = standard
            .title
            .clone()
            .unwrap_or_else(|| standard.standard_title());
        let keep_open = standard.keep_open;
        match standard.does {
            Does::Copy { content, concealed } => Action::new(title, move || async move {
                system::copy(&content, concealed)?;
                finish(keep_open, "Copied to Clipboard", true);
                Ok::<(), String>(())
            }),
            Does::Paste { content } => Action::new(title, move || async move {
                match system::paste(&content) {
                    Ok(()) => Ok(()),
                    Err(SystemError::NotAvailable(_)) => {
                        system::copy(&content, false)?;
                        finish(false, PASTE_FALLBACK, true);
                        Ok(())
                    }
                    Err(SystemError::Failed(why)) => Err(why),
                }
            }),
            Does::Open {
                target,
                application,
            } => Action::new(title, move || async move {
                system::open(&target, application.as_deref())?;
                finish(keep_open, "Opened", false);
                Ok::<(), String>(())
            }),
            Does::OpenWith { target } => Action::submenu(
                title,
                Submenu::lazy("Open With", move || async move {
                    applications_for(target, keep_open)
                }),
            ),
            Does::Reveal { path } => Action::new(title, move || async move {
                system::reveal(&path)?;
                finish(
                    keep_open,
                    &format!("Shown in {}", system::file_manager_name()),
                    false,
                );
                Ok::<(), String>(())
            }),
            Does::Trash { paths } => Action::new(title, move || async move {
                if let Err(not_trashed) = system::trash(&paths) {
                    return Err(system::describe_not_trashed(&not_trashed, paths.len()));
                }
                finish(
                    keep_open,
                    &format!("Moved to {}", system::trash_name()),
                    true,
                );
                Ok::<(), String>(())
            })
            .destructive(),
        }
    }
}

/// The entries of Open With… for `target`: the installed applications, by
/// name, each opening it.
fn applications_for(target: String, keep_open: bool) -> Result<Vec<Action>, String> {
    let mut installed = crate::applications::installed()?;
    installed.sort_by_key(|application| application.name.to_lowercase());
    Ok(installed
        .into_iter()
        .map(|application| {
            let target = target.clone();
            let id = application.id;
            let name = application.name;
            Action::new(name.clone(), move || async move {
                system::open(&target, Some(id.as_str()))?;
                finish(keep_open, &format!("Opened with {name}"), false);
                Ok::<(), String>(())
            })
        })
        .collect())
}

/// What a standard action does once it has acted: closes the window, then,
/// with `hud`, says `said` in a HUD; or, kept open, says it in a toast.
fn finish(keep_open: bool, said: &str, hud: bool) {
    if keep_open {
        show_toast(Toast::success(said.to_owned()));
        return;
    }
    close(false, PopToRootType::Default);
    if hud {
        show_hud(said, ToastStyle::Success);
    }
}
