//! Root results commands supply ahead of the query, such as the installed
//! applications, kept by the launcher so that searching only ranks them.
//!
//! Each enabled command with `"indexedResults": true` is asked for its
//! results once root search is used (a query that is not blank) and they
//! are kept for later queries, so typing never waits for them. Coming back to
//! root search marks them stale: the next query asks again, listing the kept
//! results until the answer replaces them. A disabled or replaced command's
//! results are forgotten at once, and an answer from it arriving afterwards
//! is discarded.

use std::path::{Path, PathBuf};

use super::{CommandRegistration, Entry, RootResult, Row};
use crate::runtime::{CallError, IndexedAction, IndexedResult};
use crate::search::Keys;

/// The kept results of each command that supplies them.
#[derive(Default)]
pub(super) struct Indexes {
    commands: Vec<Index>,
}

/// One command's kept results.
struct Index {
    component: PathBuf,
    /// Whether its results are being asked for.
    asking: bool,
    /// Whether its results were asked for since root search was last shown.
    fresh: bool,
    /// Its results, or the row explaining why it could not supply them.
    results: Vec<RootResult>,
    /// Why it could not supply them, listed for every query that is not
    /// blank.
    failure: Option<(Row, Entry)>,
}

impl Indexes {
    /// Marks every command's results stale, to be asked for again with the
    /// next query; they stay listed meanwhile.
    pub(super) fn stale(&mut self) {
        for index in &mut self.commands {
            index.fresh = false;
        }
    }

    /// Of `commands`, the enabled commands that supply results ahead of the
    /// query, those to ask now: not asked since root search was shown, and
    /// not being asked. They are marked as being asked.
    pub(super) fn begin_asking<T>(
        &mut self,
        commands: Vec<(CommandRegistration, T)>,
    ) -> Vec<(CommandRegistration, T)> {
        commands
            .into_iter()
            .filter(|(command, _)| {
                let index = match self
                    .commands
                    .iter_mut()
                    .position(|index| index.component == command.component)
                {
                    Some(position) => &mut self.commands[position],
                    None => {
                        self.commands.push(Index {
                            component: command.component.clone(),
                            asking: false,
                            fresh: false,
                            results: Vec::new(),
                            failure: None,
                        });
                        self.commands.last_mut().expect("pushed above")
                    }
                };
                if index.asking || index.fresh {
                    return false;
                }
                index.asking = true;
                index.fresh = true;
                true
            })
            .collect()
    }

    /// Keeps `command`'s `answer`, unless its results were forgotten while
    /// it was being asked (it was disabled or replaced).
    pub(super) fn answer(
        &mut self,
        command: &CommandRegistration,
        answer: Result<Vec<IndexedResult>, CallError>,
    ) {
        let Some(index) = self
            .commands
            .iter_mut()
            .find(|index| index.component == command.component && index.asking)
        else {
            return;
        };
        index.asking = false;
        match answer {
            Ok(results) => {
                index.failure = None;
                index.results = results
                    .into_iter()
                    .map(|result| indexed_result(command, result))
                    .collect();
            }
            Err(error) => {
                let row = Row {
                    id: format!("{}:failed", command.id),
                    title: command.title.clone(),
                    subtitle: Some(format!("Could not list: {error}")),
                    unavailable: None,
                };
                let problem = format!("{} could not list its results: {error}", command.title);
                index.results.clear();
                index.failure = Some((row, Entry::Broken(problem)));
            }
        }
    }

    /// Forgets the results of the commands whose component `keep` rejects,
    /// such as those of a disabled or replaced package; an answer from them
    /// being awaited is discarded.
    pub(super) fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        self.commands.retain(|index| keep(&index.component));
    }

    /// Every kept result, in the order the commands were first asked and
    /// their answers give them.
    pub(super) fn results(&self) -> impl Iterator<Item = &RootResult> {
        self.commands.iter().flat_map(|index| &index.results)
    }

    /// The rows explaining why a command could not supply its results.
    pub(super) fn failures(&self) -> impl Iterator<Item = &(Row, Entry)> {
        self.commands
            .iter()
            .filter_map(|index| index.failure.as_ref())
    }
}

/// The root result for one of `command`'s indexed results.
fn indexed_result(command: &CommandRegistration, result: IndexedResult) -> RootResult {
    let entry = match result.action {
        IndexedAction::OpenApplication(id) => Entry::OpenApplication {
            id,
            name: result.listing.title.clone(),
        },
    };
    let row = Row::listed(result.listing, Some(&command.id));
    let keys = Keys::new(&row.title, row.subtitle.as_deref(), None);
    RootResult {
        row,
        entry,
        keys,
        target: None,
    }
}
