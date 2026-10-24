//! Root search ready before its first use, as Raycast's is: the enabled
//! commands that supply indexed results, such as Applications, are asked
//! for them as soon as they can run (as Pane starts, and once one is
//! installed, enabled, updated, reloaded, set up or no longer waiting),
//! rather than when root search is first shown or used. So first setup, or
//! an update applied in the background, lists the installed applications,
//! and the icon cache extracts their icons (`application_icons`), before
//! the user's first search: that search draws real icons, not placeholders.
//!
//! Only a command not asked since it could run is asked here. One whose
//! results are kept, even stale, is asked again when root search is shown
//! or used, as before (see `indexed`); one that failed says so there. The
//! answer is kept and listed as a query's would be, on a thread of its own.
//! The guest call runs on the extension runtime like any other, and the
//! icons are extracted by the cache's own worker at the lowest priority, so
//! neither a start nor typing waits for them.
//!
//! Pane's own program turns this on ([`Launcher::with_eager_indexed_results`]).
//! A launcher built without it, as most tests build one, asks only when
//! root search is shown or used, so what those tests count stays theirs.

use std::path::PathBuf;

use super::{Launcher, State};

impl Launcher {
    /// This launcher asking the commands that supply indexed results for
    /// them as soon as they can run, now and after every change to the
    /// installed packages (see the module docs), rather than at root
    /// search's first use.
    pub fn with_eager_indexed_results(self) -> Self {
        {
            let mut state = self.lock();
            state.eager_indexes = true;
            self.ask_eagerly(&mut state);
        }
        self
    }

    /// Asks, in the background, the enabled commands that supply indexed
    /// results and were not asked since they could run, if this launcher
    /// asks eagerly. Called with the state locked wherever the installed
    /// packages change.
    pub(super) fn ask_eagerly(&self, state: &mut State) {
        if !state.eager_indexes {
            return;
        }
        let waiting = &state.waiting;
        let indexes = &state.indexes;
        let commands: Vec<_> = state
            .packages
            .iter()
            .filter(|package| state.runs(package))
            .flat_map(|package| {
                let data = self
                    .installation
                    .as_ref()
                    .map(|installation| installation.data.owned_by(&package.identity));
                package
                    .indexed_result_commands()
                    .into_iter()
                    // As a query asks (see `ask_for_indexed_results`): not
                    // one whose required preferences are unset, nor one a
                    // use is narrowed into waiting.
                    .filter(move |command| {
                        !indexes.knows(&command.component)
                            && !self.needs_setup(package, command.manifest_id())
                            && waiting
                                .reason_for(&package.identity, command.manifest_id())
                                .is_none()
                    })
                    .map(move |command| (command, data.clone()))
            })
            .collect();
        let asking = state.indexes.begin_asking(commands);
        if asking.is_empty() {
            return;
        }
        let components: Vec<PathBuf> = asking
            .iter()
            .map(|(command, _)| command.component.clone())
            .collect();
        let launcher = self.clone();
        let started = std::thread::Builder::new()
            .name("pane-indexed-results".into())
            .spawn(move || {
                futures::executor::block_on(launcher.show_indexed_results(asking));
                launcher.changed();
            });
        if let Err(error) = started {
            // Not asked after all: root search's next use asks them.
            state
                .indexes
                .retain(|component| !components.iter().any(|asked| asked == component));
            crate::diagnostic!("Pane could not ask for root search's results ahead: {error}");
        }
    }
}
