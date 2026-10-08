//! The installed applications changed by themselves (the host's live list,
//! ADR 0038): the enabled commands with indexed results that asked for
//! them are asked for their results again, at once if root search is on
//! screen with a query, otherwise at the next query (each visit of root
//! search asks again anyway). Root search is listed again in place: the
//! selected row stays on the same result, or at the same position if that
//! result left, so the list never jumps under the user.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::indexed::Refresh;
use super::{Launcher, State, aliases, root_rows};

/// How long a command being asked for its results is waited for before it
/// is asked again for a change, at most.
const ASKING_WAIT: Duration = Duration::from_secs(30);

/// How often a command being asked is looked at again meanwhile.
const ASKING_POLL: Duration = Duration::from_millis(50);

impl Launcher {
    /// The installed applications changed; `components` asked for them and
    /// may still run. The work is done on a thread of its own: this is
    /// called from the list's.
    pub(super) fn applications_changed(&self, components: Vec<PathBuf>) {
        let launcher = self.clone();
        // Without a thread, the next visit of root search asks anyway.
        let _ = std::thread::Builder::new()
            .name("pane-application-changes".into())
            .spawn(move || launcher.ask_again(components));
    }

    /// Marks the results of `components` stale and, while root search is
    /// on screen with a query, asks for them again and lists them in
    /// place. A command being asked is waited for first, as its answer may
    /// predate the change.
    fn ask_again(&self, mut components: Vec<PathBuf>) {
        let deadline = Instant::now() + ASKING_WAIT;
        loop {
            let asking = {
                let mut state = self.lock();
                let state = &mut *state;
                components.retain(|component| state.indexes.changed(component) == Refresh::Asking);
                match state.view.query().map(str::to_owned) {
                    Some(query) => self.ask_for_indexed_results(state, &query),
                    None => Vec::new(),
                }
            };
            if !asking.is_empty() {
                futures::executor::block_on(
                    self.show_indexed_results_with(asking, relist_in_place),
                );
                self.changed();
            }
            if components.is_empty() || Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(ASKING_POLL);
        }
    }
}

/// Lists root search for `query` again after its results changed in the
/// background: the selected row stays selected, wherever it moved, or the
/// row at its position is if it left.
fn relist_in_place(state: &mut State, query: &str) {
    let selected = state.view.selected;
    let keep = selected
        .and_then(|index| state.view.rows.get(index))
        .map(|row| row.id.clone());
    let (rows, entries) = root_rows(state, query);
    state.view.selected = keep
        .and_then(|id| rows.iter().position(|row| row.id == id))
        .or_else(|| {
            selected
                .filter(|_| !rows.is_empty())
                .map(|index| index.min(rows.len() - 1))
        })
        .or_else(|| aliases::first_choice(&entries));
    state.view.rows = rows;
    state.entries = entries;
}
