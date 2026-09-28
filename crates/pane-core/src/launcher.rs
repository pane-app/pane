//! The launcher model: the public host interface driven by the native window
//! and by tests alike.
//!
//! Every user action is a method on [`Launcher`]; [`Launcher::view`] returns a
//! snapshot of what the window should show. Actions that call into an
//! extension update the snapshot immediately (for example to "running") and
//! return a future that applies the extension's reply when awaited.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::runtime::{CallError, Runtime};

/// A command offered in root search, backed by one extension component.
#[derive(Clone, Debug)]
pub struct CommandRegistration {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub component: PathBuf,
}

/// Which screen the launcher shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Root search: the installed commands.
    Root,
    /// An opened command's list view.
    Command,
}

/// A selectable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
}

/// Feedback about the most recent action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    /// An extension call is in progress.
    Running,
    /// The extension's answer to the most recent action.
    Result(String),
    /// Why the most recent action failed.
    Error(String),
}

/// A snapshot of what the launcher shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherView {
    pub screen: Screen,
    pub title: String,
    pub rows: Vec<Row>,
    /// Index into `rows`; `None` when there are no rows.
    pub selected: Option<usize>,
    pub status: Status,
}

/// The launcher. Cloning shares the same state.
#[derive(Clone)]
pub struct Launcher {
    runtime: Result<Runtime, CallError>,
    commands: Arc<[CommandRegistration]>,
    state: Arc<Mutex<State>>,
}

struct State {
    view: LauncherView,
    /// The command whose view is open, as an index into `commands`.
    open: Option<usize>,
    /// Incremented on every navigation, so a reply that arrives after the
    /// user has left the screen it was requested from is discarded.
    screen_generation: u64,
}

impl Launcher {
    /// Creates a launcher at root search. A runtime that failed to start
    /// leaves navigation usable and explains the failure when a command opens.
    pub fn new(runtime: Result<Runtime, CallError>, commands: Vec<CommandRegistration>) -> Self {
        let commands: Arc<[CommandRegistration]> = commands.into();
        let state = State {
            view: root_view(&commands),
            open: None,
            screen_generation: 0,
        };
        Launcher {
            runtime,
            commands,
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn view(&self) -> LauncherView {
        self.lock().view.clone()
    }

    /// Moves the selection by `delta` rows, clamped to the list.
    pub fn move_selection(&self, delta: isize) {
        let mut state = self.lock();
        let view = &mut state.view;
        if let Some(selected) = view.selected {
            let last = view.rows.len() - 1;
            view.selected = Some(selected.saturating_add_signed(delta).min(last));
        }
    }

    /// Selects the row at `index`, if there is one.
    pub fn select(&self, index: usize) {
        let mut state = self.lock();
        if index < state.view.rows.len() {
            state.view.selected = Some(index);
        }
    }

    /// Leaves an open command for root search.
    pub fn back(&self) {
        let mut state = self.lock();
        if state.view.screen == Screen::Command {
            state.open = None;
            state.screen_generation += 1;
            state.view = root_view(&self.commands);
        }
    }

    /// Opens the selected command (root) or runs the selected item's action
    /// (command view). Await the returned future to apply the reply.
    pub fn activate_selected(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let selected = state.view.selected;
        let call = match (state.view.screen, state.open, selected) {
            (Screen::Root, _, Some(index)) => Some(Call::Open(index)),
            (Screen::Command, Some(command), Some(index)) => {
                Some(Call::Run(command, state.view.rows[index].id.clone()))
            }
            _ => None,
        };
        if call.is_some() {
            state.view.status = Status::Running;
        }
        let generation = state.screen_generation;
        drop(state);
        let launcher = self.clone();
        async move {
            match call {
                Some(Call::Open(index)) => launcher.open_command(generation, index).await,
                Some(Call::Run(command, item_id)) => {
                    launcher.run_action(generation, command, item_id).await
                }
                None => {}
            }
        }
    }

    async fn run_action(&self, generation: u64, index: usize, item_id: String) {
        let component = &self.commands[index].component;
        let result = match self.runtime() {
            Ok(runtime) => runtime.run_action(component, &item_id).await,
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(generation) else {
            return;
        };
        state.view.status = match result {
            Ok(answer) => Status::Result(answer),
            Err(error) => Status::Error(error.to_string()),
        };
    }

    async fn open_command(&self, generation: u64, index: usize) {
        let component = &self.commands[index].component;
        let result = match self.runtime() {
            Ok(runtime) => runtime.get_view(component).await,
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(generation) else {
            return;
        };
        match result {
            Ok(view) => {
                let rows: Vec<Row> = view
                    .items
                    .into_iter()
                    .map(|item| Row {
                        id: item.id,
                        title: item.title,
                        subtitle: item.subtitle,
                    })
                    .collect();
                state.open = Some(index);
                state.screen_generation += 1;
                state.view = LauncherView {
                    screen: Screen::Command,
                    title: view.title,
                    selected: first_index(&rows),
                    rows,
                    status: Status::Idle,
                };
            }
            Err(error) => state.view.status = Status::Error(error.to_string()),
        }
    }

    fn runtime(&self) -> Result<&Runtime, CallError> {
        self.runtime.as_ref().map_err(Clone::clone)
    }

    /// Locks the state only if the screen is still the one of `generation`.
    fn lock_if_current(&self, generation: u64) -> Option<MutexGuard<'_, State>> {
        let state = self.lock();
        (state.screen_generation == generation).then_some(state)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

enum Call {
    /// Open the command at this index.
    Open(usize),
    /// Run an item's action in the command at this index.
    Run(usize, String),
}

fn root_view(commands: &[CommandRegistration]) -> LauncherView {
    let rows: Vec<Row> = commands
        .iter()
        .map(|command| Row {
            id: command.id.clone(),
            title: command.title.clone(),
            subtitle: command.subtitle.clone(),
        })
        .collect();
    LauncherView {
        screen: Screen::Root,
        title: "Pane".into(),
        selected: first_index(&rows),
        rows,
        status: Status::Idle,
    }
}

fn first_index(rows: &[Row]) -> Option<usize> {
    (!rows.is_empty()).then_some(0)
}
