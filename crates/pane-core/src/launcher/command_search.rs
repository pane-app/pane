//! Searching inside an opened command: a command whose manifest entry sets
//! `"search": true`, such as one searching an online service, gets a search
//! field of its own once the user opens it. Pane sends it the text typed
//! there and lists what it finds; root search never asks it, so what the
//! user types there reaches no such command or its service.
//!
//! Each change of the text starts a new search and stops the one before, in
//! the runtime (where it waits, with its web requests) as well as here: an
//! answer to an older text is never shown, whether it arrives late or is
//! the older search's error. The runtime waits a moment
//! ([`crate::runtime::SEARCH_DEBOUNCE`]) before it starts a search, so
//! typing on stops each one before it has asked anything. Leaving the
//! command stops its search too. An error the command answers with, such
//! as a service that is down, is shown in place of results, and counts
//! against the extension no more than any error it answers with: it is not
//! paused for it.
//!
//! A result may be a file of the folder granted to the command's package,
//! by the id Pane gave it (Search Files, #150): Pane lists it with the
//! file's own name and folder and gives it its own file actions (see
//! `own_actions`). A search answered while Pane was still listing that
//! folder is asked again once the listing ends, unless a newer text (or
//! leaving the command) stopped it first.
//!
//! A search in progress is on its package's generation's undo list
//! ("command search", see `generation`): the generation's end stops it,
//! as a newer text does.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::{
    CommandList, Entry, Launcher, Row, Screen, State, Status, files, owner, stopped,
    until_cancelled,
};
use crate::extension_data::PackageData;
use crate::generation::Registration;
use crate::runtime::{CallError, SearchResult, StopSearch, View};

/// The search of an open command that searches as the user types.
pub(super) struct Searching {
    /// The command's manifest id, sent with each search.
    command: String,
    /// The command's own list as it last answered it, shown at once while
    /// its search field is blank; `None` until the text is first set.
    list: Option<CommandList>,
    /// Stops the search in progress, if one is; dropping it stops it.
    in_progress: Option<InProgress>,
    /// Kept while a search waits for the granted folder's listing before it
    /// is asked again: dropping it (a newer text, the command left) ends
    /// that wait.
    waiting: Option<tokio::sync::oneshot::Sender<()>>,
    /// Search Files browsed by Pane itself, when the command is Pane's
    /// registered Files command (#177, see `search_files`): the command is
    /// then never asked to search.
    pub(super) files: Option<super::search_files::Browsing>,
}

impl Searching {
    pub(super) fn new(command: String) -> Searching {
        Searching {
            command,
            list: None,
            in_progress: None,
            waiting: None,
            files: None,
        }
    }

    /// Keeps `list` as the command's own list, shown once the search field
    /// is cleared: the list drawn again while it holds text.
    pub(super) fn keep(&mut self, list: CommandList) {
        self.list = Some(list);
    }
}

/// A search in progress: dropping it stops the search, and so does the end
/// of the generation it runs in, whose undo list holds it meanwhile.
struct InProgress {
    /// Stops the search when it is taken and dropped: by this, or by the
    /// generation's end.
    stop: Arc<Mutex<Option<StopSearch>>>,
    /// Its place on its generation's undo list, taken off when this is
    /// dropped.
    _undo: Option<Registration>,
}

impl InProgress {
    /// The search `stop` stops, running in the generation of `data`.
    fn new(stop: StopSearch, data: Option<&PackageData>) -> InProgress {
        let stop = Arc::new(Mutex::new(Some(stop)));
        let undo = data.map(|data| {
            let stop = stop.clone();
            data.generation().on_end("command search", move || {
                // Dropped here, it stops the search.
                drop(
                    stop.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .take(),
                );
                Ok(())
            })
        });
        InProgress { stop, _undo: undo }
    }
}

impl Drop for InProgress {
    fn drop(&mut self) {
        // Stopped now, whatever still holds the handle.
        drop(
            self.stop
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take(),
        );
    }
}

/// A search started, to be awaited for its answer to be shown.
type Pending = Pin<Box<dyn Future<Output = ()> + Send>>;

impl Launcher {
    /// Sets the text of the open command's search field to `query`: the
    /// search in progress, if any, is stopped. A blank text shows the
    /// command's own list again, as it was last listed, then asks the
    /// command for it anew (its items may have changed since, such as a
    /// setting it shows), which the returned future lists; any other text
    /// asks the command to search, whose answer the returned future shows
    /// (the rows listed meanwhile stay, with a running status). `None` when
    /// nothing is asked.
    pub(super) fn search_in_command(&self, state: &mut State, query: &str) -> Option<Pending> {
        // Search Files lists the file index itself (#177).
        if super::search_files::browsing(state) {
            return self.ask_files(state, query, 0);
        }
        let blank = query.trim().is_empty();
        let command = self.set_search_text(state, query)?;
        let component = state.open.clone()?;
        // The package's generation as of now: disabling or reloading it
        // stops the search.
        let data = self.data_in(state, &component);
        let runtime = match self.runtime() {
            Ok(runtime) => runtime.clone(),
            Err(error) => {
                // Nothing listed is an answer to the text typed.
                state.view.rows.clear();
                state.entries.clear();
                state.view.selected = None;
                state.view.status = Status::Error(error.to_string());
                return None;
            }
        };
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        let epoch = state.screen_epoch;
        let search = state.search_epoch;
        let launcher = self.clone();
        if blank {
            // Drawn with the record its screen was opened with.
            let launch = state.launch.clone();
            return Some(Box::pin(async move {
                let answer = runtime
                    .render_launched_with(&component, Some(command.as_str()), &launch, data.clone())
                    .await;
                launcher.show_listed_again(epoch, search, component, data, answer);
            }));
        }
        let query = query.trim().to_owned();
        let (stop, answer) = runtime.search_with(&component, &command, &query, data.clone());
        if let Some(searching) = state.searching.as_mut() {
            searching.in_progress = Some(InProgress::new(stop, data.as_ref()));
        }
        Some(Box::pin(async move {
            let answer = answer.await;
            let listing = launcher.show_search_results(epoch, search, component, data, answer);
            // Answered while the granted folder was still being listed:
            // asked again once it is, unless a newer text stopped it.
            if let Some((listed, mut ended)) = listing
                && until_cancelled(listed, &mut ended).await.is_some()
                && let Some(again) = launcher.search_again(epoch, search, &query)
            {
                again.await;
            }
        }))
    }

    /// Asks the open command again for `query`, the text of the search
    /// `search` on the screen of `epoch`, once the granted folder its
    /// answer waited for is listed; `None` when that text or screen is
    /// gone, or the command can no longer be asked.
    fn search_again(&self, epoch: u64, search: u64, query: &str) -> Option<Pending> {
        let mut state = self.lock_if_current(epoch)?;
        if state.search_epoch != search {
            return None;
        }
        let component = state.open.clone()?;
        let command = state.searching.as_ref()?.command.clone();
        let data = self.data_in(&state, &component);
        let runtime = self.runtime().ok()?.clone();
        let (stop, answer) = runtime.search_with(&component, &command, query, data.clone());
        state.searching.as_mut()?.in_progress = Some(InProgress::new(stop, data.as_ref()));
        drop(state);
        let launcher = self.clone();
        Some(Box::pin(async move {
            let answer = answer.await;
            // Asked once more at most: the listing is kept now.
            let _ = launcher.show_search_results(epoch, search, component, data, answer);
        }))
    }

    /// Clears the open command's search field (Escape): the search in
    /// progress, if any, is stopped, and the command's own list is shown as
    /// it was last listed, without asking the command.
    pub(super) fn clear_search_in_command(&self, state: &mut State) {
        if super::search_files::browsing(state) {
            self.clear_files_search(state);
            return;
        }
        let _ = self.set_search_text(state, "");
    }

    /// Sets the open command's search text to `query`, stopping the search
    /// in progress; a blank text shows the command's own list as it was
    /// last listed. The command's manifest id; `None` if no open command
    /// searches.
    fn set_search_text(&self, state: &mut State, query: &str) -> Option<String> {
        let searching = state.searching.as_mut()?;
        // Stopped at once: its answer, if it still comes, is not shown, and
        // a wait for the folder's listing ends.
        searching.in_progress = None;
        searching.waiting = None;
        state.search_epoch += 1;
        if matches!(&state.view.screen, Screen::CommandSearch { query } if query.trim().is_empty())
        {
            // Leaving the command's own list: kept as it is on screen.
            searching.list = Some(CommandList {
                rows: state.view.rows.clone(),
                entries: state.entries.clone(),
            });
        }
        let kept = if query.trim().is_empty() {
            searching.list.clone()
        } else {
            None
        };
        let command = searching.command.clone();
        state.view.screen = Screen::CommandSearch {
            query: query.to_owned(),
        };
        if let Some(list) = kept {
            self.show_command_list(state, list, Status::Idle);
        }
        Some(command)
    }

    /// Shows `list`, the open command's own list, with `status`.
    fn show_command_list(&self, state: &mut State, list: CommandList, status: Status) {
        state.view.selected = super::first_index(&list.rows);
        state.view.rows = list.rows;
        state.entries = list.entries;
        state.view.status = status;
    }

    /// Lists the open command's `answer` when asked for its own list again
    /// by the search `search`, unless the screen or its text has changed
    /// since. A failure keeps the list as it was, with the error.
    fn show_listed_again(
        &self,
        epoch: u64,
        search: u64,
        component: PathBuf,
        data: Option<PackageData>,
        answer: Result<View, CallError>,
    ) {
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        if state.search_epoch != search {
            return;
        }
        let state = &mut *state;
        match (stopped(state, &component, &data), answer) {
            (Some(problem), _) => {
                state.view.rows.clear();
                state.entries.clear();
                state.view.selected = None;
                state.view.status = Status::Error(problem);
            }
            (None, Ok(view)) => {
                super::looks::remember(state, &component, &view.items);
                // Pane's folder rows lead it again, as when it opened.
                let list = self.command_list(state, &component, view.items);
                if let Some(searching) = state.searching.as_mut() {
                    searching.list = Some(list.clone());
                }
                self.show_command_list(state, list, Status::Idle);
            }
            (None, Err(error)) => state.view.status = Status::Error(error.to_string()),
        }
    }

    /// Lists the open command's `answer` to the search `search`, unless the
    /// screen or its text has changed since. When the command was told that
    /// its package's granted folder is still being listed, answers what
    /// resolves once it is, and what resolves once a newer text (or leaving
    /// the command) makes that wait pointless; the status says it runs
    /// until then.
    fn show_search_results(
        &self,
        epoch: u64,
        search: u64,
        component: PathBuf,
        data: Option<PackageData>,
        answer: Result<Vec<SearchResult>, CallError>,
    ) -> Option<(ListedWait, tokio::sync::oneshot::Receiver<()>)> {
        let mut state = self.lock_if_current(epoch)?;
        if state.search_epoch != search {
            return None;
        }
        let state = &mut *state;
        if let Some(searching) = state.searching.as_mut() {
            searching.in_progress = None;
        }
        let owner = owner(&state.packages, &component).map(|package| package.identity.key());
        let access = state.files.clone();
        let (list, status) = match (stopped(state, &component, &data), answer) {
            // Stopped while it was running: its answer is not shown.
            (Some(problem), _) => (CommandList::default(), Status::Error(problem)),
            // Replaced by a newer search, whose answer is shown instead.
            (None, Err(CallError::Cancelled)) => return None,
            (None, Ok(results)) => {
                let (rows, entries) = results
                    .into_iter()
                    .filter_map(|result| match result.file {
                        None => {
                            let entry = Entry::Run(result.listing.id.clone());
                            Some((Row::listed(result.listing, None), entry))
                        }
                        // A file Pane listed, by the name Pane found; one
                        // it did not list is left out.
                        Some(file) => {
                            let (row, file) = files::file_row(
                                access.as_ref()?,
                                owner.as_deref()?,
                                &component,
                                file,
                                result.listing.id,
                            )?;
                            Some((row, Entry::File(file)))
                        }
                    })
                    .unzip();
                (CommandList { rows, entries }, Status::Idle)
            }
            (None, Err(error)) => (CommandList::default(), Status::Error(error.to_string())),
        };
        // The system icons of the files the file index found (#142).
        super::file_search::want_icons(state, &list.entries);
        // Told the folder is still being listed: asked again once it is.
        let listed = match (&status, &owner, &access) {
            (Status::Idle, Some(owner), Some(access)) => access.listed(owner),
            _ => None,
        };
        // Still listing a granted folder: the wait goes on, stamped anew.
        let status = if listed.is_some() {
            Status::Running {
                since: Instant::now(),
            }
        } else {
            status
        };
        self.show_command_list(state, list, status);
        let listed: ListedWait = Box::pin(listed?);
        let (waiting, ended) = tokio::sync::oneshot::channel();
        state.searching.as_mut()?.waiting = Some(waiting);
        Some((listed, ended))
    }
}

/// What resolves once a granted folder's listing ends.
type ListedWait = Pin<Box<dyn Future<Output = ()> + Send>>;
