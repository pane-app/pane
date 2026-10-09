//! Pane's own actions on the rows it lists itself (#150): a file of a
//! package's granted folder, in root search's file results and in Search
//! Files' results, and a computed answer, such as the calculator's.
//!
//! Such a row has actions as an item of a command's list has (see
//! `item_actions`): the first is Enter's, the second Ctrl+Enter's, the
//! third Ctrl+Shift+Enter's, and the Actions panel lists them all, with
//! "Open With…" opening a submenu of the installed applications. Pane
//! performs them itself; no extension is called.
//!
//! **A file** (ADR 0017 and ADR 0034: the extension names it by the id
//! Pane gave it, never by a path). Pane checks it again before acting, as it
//! always did before opening one (`crate::files`, and for the file index
//! `crate::file_index::Indexer::checked`). A folder the index found has
//! Open (Enter: the file manager), Show in Explorer, Copy Path, Copy Name,
//! Copy File and Move to Recycle Bin. A document's actions are
//! Open (Enter), Show in Explorer (Ctrl+Enter; Finder or the File Manager
//! elsewhere), Open With…, Copy Path, Copy Name (#177), Copy File and Move
//! to Recycle Bin (destructive, confirmed first). File search's own Enter never runs a
//! program by accident (ADR 0037's exception, keeping ADR 0017's intent):
//! for a program or script, Enter shows it in Explorer, Ctrl+Enter is Open
//! With…, and only the explicit Run action runs it. Each closes the window
//! after acting, as the standard actions do, and says what it did in a
//! HUD ("Opened plan.md", "Copied to Clipboard"). What failed stays on
//! screen, in the status line.
//!
//! **A computed answer.** Copy answer (Enter: the window puts the text on
//! the clipboard, `Launcher::selected_copy`) and Paste answer (Ctrl+Enter):
//! Pane pastes it into the application that was in front, closing its
//! window, or where it cannot paste yet (#125) copies it instead and says
//! so in a HUD ([`crate::system::PASTE_FALLBACK`]).

use std::path::PathBuf;

use super::files::FileRow;
use super::item_actions::Listed;
use super::{Entry, Launcher, Screen, State, Status, off_thread};
use crate::feedback::{Caller, GivenConfirmation, Hud, ToastStyle, WindowPresence};
use crate::runtime::{Action, ActionKind, ActionStyle, ActionSubmenu, SubmenuEntries};
use crate::system::{self, Clip, PASTE_FALLBACK, SystemError};

/// The callback ids of Pane's own actions. A row of Pane's never reaches a
/// command's `handle-event`, so they cannot meet a command's own ids.
const OPEN: &str = "pane.open";
const RUN: &str = "pane.run";
const REVEAL: &str = "pane.reveal";
/// Open With…'s submenu; its entries are `pane.open-with/<application id>`.
const OPEN_WITH: &str = "pane.open-with";
const COPY_PATH: &str = "pane.copy-path";
const COPY_NAME: &str = "pane.copy-name";
const COPY_FILE: &str = "pane.copy-file";
const TRASH: &str = "pane.trash";
const COPY_ANSWER: &str = "pane.copy-answer";
const PASTE_ANSWER: &str = "pane.paste-answer";

/// What the HUD says once a copy is made.
pub(super) const COPIED: &str = "Copied to Clipboard";

/// What the system's file manager is called.
pub(super) fn file_manager() -> &'static str {
    if cfg!(target_os = "windows") {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    }
}

/// What the system's trash is called.
pub(super) fn trash_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Recycle Bin"
    } else {
        "Trash"
    }
}

/// An action titled `title` with the callback `callback`.
fn action(title: impl Into<String>, callback: &str) -> Action {
    Action {
        title: Some(title.into()),
        kind: ActionKind::Callback(callback.to_owned()),
        section: None,
        style: ActionStyle::Default,
        shortcut: None,
        icon: None,
    }
}

/// Open With…: a submenu of the installed applications, asked for when it
/// opens.
fn open_with() -> Action {
    Action {
        kind: ActionKind::Submenu(ActionSubmenu {
            title: "Open With".into(),
            entries: SubmenuEntries::Asked(OPEN_WITH.into()),
        }),
        ..action("Open With…", OPEN_WITH)
    }
}

/// The actions of `file`, in order (see the module docs).
pub(super) fn file_actions(file: &FileRow) -> Vec<Action> {
    let reveal = action(format!("Show in {}", file_manager()), REVEAL);
    let mut actions = if file.folder {
        // A folder opens in the file manager.
        vec![action("Open", OPEN), reveal]
    } else if file.program {
        vec![reveal, open_with(), action("Run", RUN)]
    } else {
        vec![action("Open", OPEN), reveal, open_with()]
    };
    actions.push(action("Copy Path", COPY_PATH));
    actions.push(action("Copy Name", COPY_NAME));
    actions.push(action("Copy File", COPY_FILE));
    actions.push(Action {
        style: ActionStyle::Destructive,
        ..action(format!("Move to {}", trash_name()), TRASH)
    });
    actions
}

/// The actions of a computed answer, in order.
fn answer_actions() -> Vec<Action> {
    vec![
        action("Copy answer", COPY_ANSWER),
        action("Paste answer", PASTE_ANSWER),
    ]
}

/// What Enter does with `file`, as the footer names it.
pub(super) fn primary_title(file: &FileRow) -> String {
    file_actions(file)
        .into_iter()
        .next()
        .and_then(|action| action.title)
        .unwrap_or_default()
}

/// The actions of the row at `index`, as an item's, when Pane performs them
/// itself: a file's, or a computed answer's.
pub(super) fn listed(state: &State, index: usize) -> Option<Listed> {
    if !matches!(
        state.view.screen,
        Screen::Root { .. } | Screen::Command | Screen::CommandSearch { .. }
    ) {
        return None;
    }
    let row = state.view.rows.get(index)?;
    let actions = match state.entries.get(index)? {
        Entry::File(file) => file_actions(file),
        Entry::Copy(_) => answer_actions(),
        _ => return None,
    };
    Some(Listed {
        id: row.id.clone(),
        title: row.title.clone(),
        actions,
    })
}

/// A row Pane acts on itself.
#[derive(Clone)]
pub(super) enum Own {
    File(FileRow),
    /// A computed answer, by its text.
    Answer(String),
}

/// The selected row, if Pane performs its actions itself.
pub(super) fn selected(state: &State) -> Option<Own> {
    let index = state.view.selected?;
    listed(state, index)?;
    match state.entries.get(index)? {
        Entry::File(file) => Some(Own::File(file.clone())),
        Entry::Copy(text) => Some(Own::Answer(text.clone())),
        _ => None,
    }
}

/// What one of Pane's own actions does, once chosen.
pub(super) enum Work {
    Open(FileRow),
    Run(FileRow),
    Reveal(FileRow),
    OpenWith {
        file: FileRow,
        /// The installed application's id.
        application: String,
        /// Its name.
        name: String,
    },
    CopyPath(FileRow),
    CopyName(FileRow),
    CopyFile(FileRow),
    Trash(FileRow),
    Paste(String),
}

/// The work the action with `callback`, titled `title`, of `own` asks for;
/// `None` for one Pane does not do here: a submenu's opening, and Copy
/// answer, which is Enter's (the window copies).
pub(super) fn work(own: Own, callback: &str, title: &str) -> Option<Work> {
    match own {
        Own::Answer(text) => (callback == PASTE_ANSWER).then_some(Work::Paste(text)),
        Own::File(file) => {
            if let Some(application) = callback
                .strip_prefix(OPEN_WITH)
                .and_then(|rest| rest.strip_prefix('/'))
            {
                return Some(Work::OpenWith {
                    file,
                    application: application.to_owned(),
                    name: title.to_owned(),
                });
            }
            match callback {
                OPEN => Some(Work::Open(file)),
                RUN => Some(Work::Run(file)),
                REVEAL => Some(Work::Reveal(file)),
                COPY_PATH => Some(Work::CopyPath(file)),
                COPY_NAME => Some(Work::CopyName(file)),
                COPY_FILE => Some(Work::CopyFile(file)),
                TRASH => Some(Work::Trash(file)),
                _ => None,
            }
        }
    }
}

/// What Enter does with `file`: opens a document, reveals a program or
/// script.
pub(super) fn primary(file: FileRow) -> Work {
    if file.program {
        Work::Reveal(file)
    } else {
        Work::Open(file)
    }
}

/// Notes that `work` begins, while the launcher is locked: the status line
/// is about it from now on, and says it runs (Move to Recycle Bin and
/// Paste answer first ask or close).
pub(super) fn begin(state: &mut State, work: &Work) {
    state.sent_from = None;
    if !matches!(work, Work::Trash(_) | Work::Paste(_)) {
        state.view.status = Status::Running;
    }
}

/// The entries of a file's Open With…: the installed applications, by
/// name.
pub(super) async fn open_with_entries(launcher: &Launcher) -> Result<Vec<Action>, String> {
    let applications = launcher
        .runtime()
        .map_err(|error| error.to_string())?
        .applications();
    let mut installed = off_thread(move || applications.installed()).await?;
    installed.sort_by_key(|application| application.name.to_lowercase());
    Ok(installed
        .into_iter()
        .map(|application| action(application.name, &format!("{OPEN_WITH}/{}", application.id)))
        .collect())
}

/// How one of Pane's own actions ended.
enum Ended {
    /// It acted: the window closes and a HUD says `.0`, as the standard
    /// actions' do.
    Hud(String),
    /// It did nothing (the user did not confirm, or the paste closed the
    /// window itself): nothing more is said.
    Quiet,
    /// It failed: the status says why, and the window stays.
    Failed(String),
}

impl Launcher {
    /// Does `work`, one of Pane's own actions chosen on the screen of
    /// `epoch` (see the module docs).
    pub(super) async fn do_own(&self, epoch: u64, work: Work) {
        let ended = match work {
            // Found in the file index: checked again, then opened, or shown
            // in the file manager if it turned out to be a program (its
            // executable bit), never run (#175, ADR 0037).
            Work::Open(file) if file.indexed => {
                let links = self.links.clone();
                let system = self.system();
                let name = file.name.clone();
                let manager = file_manager();
                let opened = self
                    .on_entry(&file, move |checked| {
                        if checked.program {
                            system.reveal(&checked.path).map(|()| true)
                        } else {
                            links.open_file(&checked.path).map(|()| false)
                        }
                    })
                    .await;
                match opened {
                    Ok(false) => Ended::Hud(format!("Opened {name}")),
                    Ok(true) => Ended::Hud(format!("Showed {name} in {manager}")),
                    Err(why) => Ended::Failed(format!("Could not open {name}: {why}")),
                }
            }
            Work::Open(file) => {
                let links = self.links.clone();
                let name = file.name.clone();
                match self
                    .on_file(&file, false, move |path| links.open_file(&path))
                    .await
                {
                    Ok(()) => Ended::Hud(format!("Opened {name}")),
                    Err(why) => Ended::Failed(format!("Could not open {name}: {why}")),
                }
            }
            Work::Run(file) => {
                let links = self.links.clone();
                let name = file.name.clone();
                match self
                    .on_file(&file, true, move |path| links.open_file(&path))
                    .await
                {
                    Ok(()) => Ended::Hud(format!("Ran {name}")),
                    Err(why) => Ended::Failed(format!("Could not run {name}: {why}")),
                }
            }
            Work::Reveal(file) => {
                let system = self.system();
                let name = file.name.clone();
                let manager = file_manager();
                match self
                    .on_file(&file, true, move |path| system.reveal(&path))
                    .await
                {
                    Ok(()) => Ended::Hud(format!("Showed {name} in {manager}")),
                    Err(why) => Ended::Failed(format!("Could not show {name} in {manager}: {why}")),
                }
            }
            Work::OpenWith {
                file,
                application,
                name: app,
            } => {
                let system = self.system();
                let name = file.name.clone();
                let applications = self.runtime().ok().map(|runtime| runtime.applications());
                let opened = self
                    .on_file(&file, true, move |path| {
                        // The installed application is opened by its source.
                        let application = match &applications {
                            Some(applications) => {
                                crate::applications::opener(applications.as_ref(), &application)
                            }
                            None => application,
                        };
                        system.open(&path.to_string_lossy(), Some(application.as_str()))
                    })
                    .await;
                match opened {
                    Ok(()) => Ended::Hud(format!("Opened {name} with {app}")),
                    Err(why) => Ended::Failed(format!("Could not open {name} with {app}: {why}")),
                }
            }
            Work::CopyPath(file) => {
                let system = self.system();
                let name = file.name.clone();
                let copied = self
                    .on_file(&file, true, move |path| {
                        system.copy(&Clip::Text(path.display().to_string()), false)
                    })
                    .await;
                match copied {
                    Ok(()) => Ended::Hud(COPIED.into()),
                    Err(why) => Ended::Failed(format!("Could not copy the path of {name}: {why}")),
                }
            }
            // The name as the system has it now, once the entry is
            // checked again (#177).
            Work::CopyName(file) => {
                let system = self.system();
                let name = file.name.clone();
                let copied = self
                    .on_file(&file, true, move |path| {
                        let named = path
                            .file_name()
                            .map(|named| named.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        system.copy(&Clip::Text(named), false)
                    })
                    .await;
                match copied {
                    Ok(()) => Ended::Hud(COPIED.into()),
                    Err(why) => Ended::Failed(format!("Could not copy the name of {name}: {why}")),
                }
            }
            Work::CopyFile(file) => {
                let system = self.system();
                let name = file.name.clone();
                let copied = self
                    .on_file(&file, true, move |path| {
                        system.copy(&Clip::File(path), false)
                    })
                    .await;
                match copied {
                    Ok(()) => Ended::Hud(COPIED.into()),
                    Err(why) => Ended::Failed(format!("Could not copy {name}: {why}")),
                }
            }
            Work::Trash(file) => self.trash_file(epoch, file).await,
            Work::Paste(text) => {
                let system = self.system();
                let copied = text.clone();
                self.paste_or_copy(
                    epoch,
                    Some(Clip::Text(text)),
                    Box::new(move || system.copy(&Clip::Text(copied), false)),
                )
                .await;
                // It said how it went itself.
                return;
            }
        };
        self.end_own(epoch, ended);
    }

    /// Asks the user to confirm moving `file` to the Recycle Bin, then
    /// moves it.
    async fn trash_file(&self, epoch: u64, file: FileRow) -> Ended {
        let trash = trash_name();
        let name = file.name.clone();
        let caller = Caller {
            component: file.component.clone(),
            command: None,
            windowed: true,
        };
        let asked = self.ask_to_confirm(
            &caller,
            GivenConfirmation {
                title: format!("Move “{name}” to the {trash}?"),
                message: Some(format!("You can put it back from the {trash}.")),
                primary: format!("Move to {trash}"),
                destructive: true,
                dismiss: None,
                remember: None,
            },
        );
        match asked.await {
            Ok(true) => {}
            Ok(false) => return Ended::Quiet,
            Err(why) => return Ended::Failed(why),
        }
        if let Some(mut state) = self.lock_if_current(epoch) {
            state.view.status = Status::Running;
        }
        let system = self.system();
        let moved = self
            .on_file(&file, true, move |path| {
                match system.trash(&[path]).into_iter().next() {
                    None => Ok(()),
                    Some(not) => Err(not.reason),
                }
            })
            .await;
        match moved {
            Ok(()) => Ended::Hud(format!("Moved to {trash}")),
            Err(why) => Ended::Failed(format!("Could not move {name} to the {trash}: {why}")),
        }
    }

    /// Checks `file` again, off the calling thread (see
    /// `crate::files::FileAccess::checked_file`; a program or script only
    /// where `programs`), then does `act` with its checked path there.
    async fn on_file(
        &self,
        file: &FileRow,
        programs: bool,
        act: impl FnOnce(PathBuf) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        if file.indexed {
            // Run, Reveal, Open With…, the copies and the bin act on a
            // program as on any file; only Enter's Open tells them apart.
            return self.on_entry(file, move |checked| act(checked.path)).await;
        }
        let files = self.lock().files.clone();
        let (owner, id) = (file.owner.clone(), file.id.clone());
        off_thread(move || {
            let files =
                files.ok_or_else(|| String::from("Pane's extension runtime is unavailable"))?;
            act(files.checked_file(&owner, &id, programs)?)
        })
        .await
    }

    /// Checks the file index's entry `file` again, off the calling thread
    /// (see `crate::file_index::Indexer::checked`), then does `act` with
    /// what the check found there.
    async fn on_entry<T: Send + 'static>(
        &self,
        file: &FileRow,
        act: impl FnOnce(crate::file_index::Checked) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let files = self.lock().files.clone();
        let (owner, id) = (file.owner.clone(), file.id.clone());
        off_thread(move || {
            let files =
                files.ok_or_else(|| String::from("Pane's extension runtime is unavailable"))?;
            act(files.indexer().checked(&owner, &id)?)
        })
        .await
    }

    /// Says how one of Pane's own actions ended: after one that acted, the
    /// window closes and a HUD says what it did; what failed is said on the
    /// screen of `epoch` if it is still on display.
    fn end_own(&self, epoch: u64, ended: Ended) {
        if let Some(mut state) = self.lock_if_current(epoch) {
            state.view.status = match &ended {
                Ended::Failed(why) => Status::Error(why.clone()),
                Ended::Hud(_) | Ended::Quiet => Status::Idle,
            };
        }
        match ended {
            Ended::Hud(title) => self.show_hud(Hud::new(ToastStyle::Success, title)),
            Ended::Quiet | Ended::Failed(_) => self.changed(),
        }
    }

    /// Pastes `clip` into the application that was in front before Pane,
    /// closing Pane's window first; where Pane cannot paste on this system
    /// yet, does `fallback` (a copy) instead and says so in a HUD
    /// ([`PASTE_FALLBACK`]), which closes the window too. A failure is said
    /// in the status line of the screen of `epoch`, and in a HUD once the
    /// window has closed. With no `clip` — content the system cannot paste
    /// through [`Clip`], such as a copied image (#167) — it does `fallback`
    /// as where Pane cannot paste.
    pub(super) async fn paste_or_copy(
        &self,
        epoch: u64,
        clip: Option<Clip>,
        fallback: Box<dyn FnOnce() -> Result<(), String> + Send>,
    ) {
        let system = self.system();
        let asked = system.clone();
        // Asked first, so that a paste this system cannot make leaves the
        // window and the clipboard as they were.
        let can_paste = match clip {
            Some(clip) => off_thread(move || asked.can_paste()).await.map(|()| clip),
            None => Err(SystemError::NotAvailable(String::new())),
        };
        let pasted = match can_paste {
            Ok(clip) => {
                // The application that was in front can only come back
                // once Pane's window has gone.
                self.close_after_acting();
                off_thread(move || system::paste(system.as_ref(), &clip)).await
            }
            Err(not) => Err(not),
        };
        let failed = match pasted {
            Ok(()) => None,
            Err(SystemError::NotAvailable(_)) => match off_thread(fallback).await {
                Ok(()) => {
                    if let Some(mut state) = self.lock_if_current(epoch) {
                        state.view.status = Status::Idle;
                    }
                    self.show_hud(Hud::new(ToastStyle::Success, PASTE_FALLBACK));
                    return;
                }
                Err(why) => Some(format!("Could not copy it: {why}")),
            },
            Err(SystemError::Failed(why)) => Some(format!("Could not paste: {why}")),
        };
        let hidden = {
            let mut state = self.lock();
            if state.screen_epoch == epoch {
                state.view.status = match &failed {
                    Some(why) => Status::Error(why.clone()),
                    None => Status::Idle,
                };
            }
            state.feedback.presence == WindowPresence::Hidden
        };
        match failed {
            Some(why) if hidden => self.show_hud(Hud::new(ToastStyle::Failure, why)),
            _ => self.changed(),
        }
    }
}
