//! Submenus in the Actions panel (#140): an action that leads to further
//! choices, such as "Open With…" or "Move to List…", opens them in place in
//! the panel instead of calling the command back, so they are not all
//! listed at once.
//!
//! A submenu has its own title, which the panel shows as its context while
//! it is open, and its own entries. Each entry is an action of its own: a
//! callback with its style, section and shortcut, or a further submenu. The
//! command gives the entries in one of two ways (`docs/list-tree.md`):
//!
//! - **at once**, with the item in its tree, and the submenu lists them as
//!   soon as it opens;
//! - **when it opens**: Pane hands the submenu's callback id to the
//!   command's `handle-event`, as for any action, and the answer's
//!   `entries` are the submenu's. It is asked once per opening; until it
//!   answers the submenu is loading, and an error (its own, a crash, an
//!   answer Pane cannot read) is shown as the submenu's one entry, without
//!   closing the panel. Asking draws nothing again: the list is unchanged.
//!
//! The submenus open over the selected item form a stack, outermost first:
//! [`Launcher::open_submenu`] opens one from the level shown (the item's
//! actions, or the innermost submenu's entries), [`Launcher::close_submenu`]
//! steps back one level (Escape), and [`Launcher::close_submenus`] closes
//! them all (the panel closed). They belong to the item: once another item
//! is selected, or another screen shown, none is open. An answer that
//! arrives after its submenu closed, or after the selection moved to
//! another item, is discarded.
//!
//! Choosing an entry ([`Launcher::run_submenu_entry`]) runs its callback as
//! choosing an action does, and closes the submenus with the panel: a
//! repeat of the same choice finds nothing open and runs nothing. An entry's
//! shortcut is bound by the same rules as an item's actions' (see
//! `item_actions`), within its own submenu, and works only while that
//! submenu is shown; the window handles the keys.

use std::future::Future;
use std::path::PathBuf;

use super::item_actions::{self, ItemAction, selected_listed};
use super::{Launcher, State, looks, own_actions, stopped};
use crate::extension_data::PackageData;
use crate::keyboard::Binding;
use crate::runtime::{Action, CallError, SubmenuEntries};

/// The submenus open over the selected item, outermost first.
#[derive(Debug, Default)]
pub(super) struct Submenus {
    levels: Vec<Level>,
    /// How many submenus have opened: each opening's number, by which an
    /// answer finds the opening that asked for it.
    openings: u64,
}

impl Submenus {
    /// Closes every submenu: an answer still on its way finds its opening
    /// gone.
    pub(super) fn close_all(&mut self) {
        self.levels.clear();
    }
}

/// One open submenu.
#[derive(Clone, Debug)]
struct Level {
    /// Which opening this is.
    opening: u64,
    /// The id of the item it belongs to.
    target: String,
    /// The submenu's title.
    title: String,
    entries: Entries,
}

/// An open submenu's entries, or why there are none yet.
#[derive(Clone, Debug)]
enum Entries {
    /// Asked for, not answered yet.
    Loading,
    /// Why the command could not give them.
    Failed(String),
    /// Given at once, or answered.
    Listed(Vec<Action>),
}

/// What to ask for a submenu's entries.
struct Ask {
    opening: u64,
    target: String,
    from: AskFrom,
}

/// Who gives a submenu's entries when it opens.
enum AskFrom {
    /// The open command, through its `handle-event`.
    Command {
        component: PathBuf,
        /// The open command's manifest id, which the host functions the
        /// call makes act for.
        command: Option<String>,
        callback: String,
        data: Option<PackageData>,
    },
    /// Pane itself, for a row whose actions it performs (a file's Open
    /// With…, see `own_actions`).
    Pane,
}

/// The submenu the Actions panel shows over the selected item: the
/// innermost one open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenSubmenu {
    /// The id of the item whose actions it belongs to: the panel's target.
    pub target: String,
    /// Its title, which the panel shows as its context.
    pub title: String,
    /// How deep it is: 1 for a submenu of one of the item's actions, 2 for
    /// one of that submenu's entries' own, and so on.
    pub depth: usize,
    /// Its entries, or why there are none to list yet.
    pub state: SubmenuState,
}

/// What an open submenu lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmenuState {
    /// Pane asked the command for the entries and waits for its answer.
    Loading,
    /// The command could not give them: why, which the panel shows as an
    /// entry of its own.
    Failed(String),
    /// Its entries, in order, their shortcuts bound by the rules of an
    /// item's actions within this submenu.
    Listed(Vec<ItemAction>),
}

impl OpenSubmenu {
    /// Its entries; none while it is loading or failed.
    pub fn entries(&self) -> &[ItemAction] {
        match &self.state {
            SubmenuState::Listed(entries) => entries,
            SubmenuState::Loading | SubmenuState::Failed(_) => &[],
        }
    }

    /// The indexes of the entries whose title holds `query`, ignoring case
    /// and the spaces around it; all of them for a blank one. Filtering
    /// flattens the sections, as it does for the item's actions.
    pub fn matching(&self, query: &str) -> Vec<usize> {
        item_actions::matching(self.entries(), query)
    }

    /// The index of the entry `binding` runs while this submenu is shown,
    /// if one of them has it bound.
    pub fn bound_to(&self, binding: &Binding) -> Option<usize> {
        item_actions::bound_to(self.entries(), binding)
    }
}

impl Launcher {
    /// The submenu the Actions panel shows: the innermost one open over the
    /// selected item of an open command's list. `None` when none is open,
    /// or the item it belongs to is no longer the selected one.
    pub fn submenu(&self) -> Option<OpenSubmenu> {
        let state = self.lock();
        let level = current(&state)?;
        Some(OpenSubmenu {
            target: level.target.clone(),
            title: level.title.clone(),
            depth: state.submenus.levels.len(),
            state: match &level.entries {
                Entries::Loading => SubmenuState::Loading,
                Entries::Failed(why) => SubmenuState::Failed(why.clone()),
                Entries::Listed(entries) => SubmenuState::Listed(looks::with_action_icons(
                    &state,
                    entries,
                    item_actions::bind(entries, &state.pane_keys),
                )),
            },
        })
    }

    /// Opens the submenu of the action at `index` of the level the panel
    /// shows for the item `target` (its actions while no submenu is open,
    /// else the innermost submenu's entries), if `target` is still the
    /// selected item and that action opens a submenu. Entries given at once
    /// are listed now; for entries asked for when the submenu opens, it is
    /// loading now, and awaiting the returned future asks the command once
    /// and lists its answer, unless the submenu closed or the selection
    /// moved meanwhile. Nothing happens otherwise.
    pub fn open_submenu(
        &self,
        target: &str,
        index: usize,
    ) -> impl Future<Output = ()> + Send + 'static {
        let ask = {
            let mut state = self.lock();
            self.begin_opening(&mut state, target, index)
        };
        let launcher = self.clone();
        async move {
            if let Some(ask) = ask {
                launcher.ask_for_entries(ask).await;
            }
        }
    }

    /// Closes the submenu shown, stepping back to the level above it (the
    /// item's actions, or the submenu it opened from), as Escape does in
    /// the panel. Whether one was open.
    pub fn close_submenu(&self) -> bool {
        self.lock().submenus.levels.pop().is_some()
    }

    /// Closes every open submenu, as closing the Actions panel does.
    pub fn close_submenus(&self) {
        self.lock().submenus.close_all();
    }

    /// Runs the entry at `index` of the submenu shown over the item
    /// `target`, as the panel chooses it: only if `target` is still the
    /// selected item, its submenu is listed and the entry calls the command
    /// back (one that opens a submenu is [`Launcher::open_submenu`]'s). The
    /// submenus close, as the panel does. Await the returned future to apply
    /// the answer, as for any action; nothing happens otherwise.
    pub fn run_submenu_entry(
        &self,
        target: &str,
        index: usize,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let chosen = current(&state)
            .filter(|level| level.target == target)
            .and_then(|level| match &level.entries {
                Entries::Listed(entries) => entries.get(index),
                Entries::Loading | Entries::Failed(_) => None,
            })
            .and_then(|entry| {
                let callback = entry.callback()?.to_owned();
                Some((callback, item_actions::title(entry)))
            });
        if chosen.is_some() {
            // Chosen: the panel closes with its submenus, and a repeat of
            // the choice finds nothing to run.
            state.submenus.close_all();
        }
        self.run_chosen(state, chosen)
    }

    /// Opens the submenu (see [`Launcher::open_submenu`]) while the
    /// launcher is locked; what to ask the command, for one whose entries
    /// it gives when the submenu opens.
    fn begin_opening(&self, state: &mut State, target: &str, index: usize) -> Option<Ask> {
        let selected = selected_listed(state).is_some_and(|listed| listed.id == target);
        let elsewhere = state
            .submenus
            .levels
            .first()
            .is_some_and(|level| level.target != target);
        if !selected || elsewhere {
            // Submenus of another item, or of none: gone.
            state.submenus.close_all();
        }
        if !selected {
            return None;
        }
        let submenu = match state.submenus.levels.last() {
            None => selected_listed(state)?
                .actions
                .get(index)?
                .submenu()?
                .clone(),
            Some(level) => match &level.entries {
                Entries::Listed(entries) => entries.get(index)?.submenu()?.clone(),
                Entries::Loading | Entries::Failed(_) => return None,
            },
        };
        let (entries, ask) = match submenu.entries {
            SubmenuEntries::Given(entries) => (Entries::Listed(entries), None),
            // A row whose actions Pane performs: Pane gives the entries.
            SubmenuEntries::Asked(_) if own_actions::selected(state).is_some() => {
                (Entries::Loading, Some(AskFrom::Pane))
            }
            SubmenuEntries::Asked(callback) => {
                let component = state.open.clone()?;
                let data = self.data_in(state, &component);
                let command = state.open_command.clone();
                let from = AskFrom::Command {
                    component,
                    command,
                    callback,
                    data,
                };
                (Entries::Loading, Some(from))
            }
        };
        state.submenus.openings += 1;
        let opening = state.submenus.openings;
        state.submenus.levels.push(Level {
            opening,
            target: target.to_owned(),
            title: submenu.title,
            entries,
        });
        ask.map(|from| Ask {
            opening,
            target: target.to_owned(),
            from,
        })
    }

    /// Asks the open command for the entries of the submenu `ask` opened,
    /// and lists them, or why it could not give them, if that opening is
    /// still open over the selected item.
    async fn ask_for_entries(&self, ask: Ask) {
        let Ask {
            opening,
            target,
            from,
        } = ask;
        // The entries, or why there are none; and the command asked, whose
        // generation may end meanwhile.
        let (answer, asked) = match from {
            AskFrom::Pane => (own_actions::open_with_entries(self).await, None),
            AskFrom::Command {
                component,
                command,
                callback,
                data,
            } => {
                let answer = match self.updating(&component) {
                    // Its package's code is being replaced (an update Pane
                    // applies by itself), which would stop the call:
                    // refused, as an action is.
                    Some(problem) => Err(problem),
                    None => match self.runtime() {
                        Ok(runtime) => runtime
                            .handle_event_with(
                                &component,
                                command.as_deref(),
                                &callback,
                                "{}",
                                data.clone(),
                            )
                            .await
                            .map_err(|error| error.to_string())
                            .and_then(|answer| {
                                answer.entries.ok_or_else(|| {
                                    CallError::Unreadable(
                                        "its answer to opening a submenu has no `entries`".into(),
                                    )
                                    .to_string()
                                })
                            }),
                        Err(error) => Err(error.to_string()),
                    },
                };
                (answer, Some((component, data)))
            }
        };
        let mut state = self.lock();
        let state = &mut *state;
        let Some(position) = state
            .submenus
            .levels
            .iter()
            .position(|level| level.opening == opening)
        else {
            // The panel closed, or stepped back past this submenu: the
            // answer is discarded.
            return;
        };
        if !selected_listed(state).is_some_and(|listed| listed.id == target) {
            // Another item is selected now: its submenus are gone, and the
            // answer with them.
            state.submenus.close_all();
            return;
        }
        let ended = asked
            .as_ref()
            .and_then(|(component, data)| stopped(state, component, data));
        let entries = match (ended, answer) {
            // Stopped while it was asked (disabled, reloaded, paused): its
            // answer is not shown.
            (Some(problem), _) => Entries::Failed(problem),
            (None, Ok(entries)) => {
                // Their web images and system icons start loading, as the
                // list's do (#142).
                looks::want_action_icons(state, &entries);
                Entries::Listed(entries)
            }
            (None, Err(why)) => Entries::Failed(why),
        };
        state.submenus.levels[position].entries = entries;
    }
}

/// The submenu shown now: the innermost one open, while the item it
/// belongs to is still the selected one.
fn current(state: &State) -> Option<&Level> {
    let level = state.submenus.levels.last()?;
    selected_listed(state).filter(|listed| listed.id == level.target)?;
    Some(level)
}
