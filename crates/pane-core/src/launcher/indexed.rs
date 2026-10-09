//! Root results commands supply ahead of the query, such as the installed
//! applications, kept by the launcher so that searching only ranks them.
//! A result's alternate titles and keywords find it too (see `search`).
//!
//! Each enabled command with `"indexedResults": true` is asked for its
//! results once root search is used (a query that is not blank) and they
//! are kept for later queries, so typing never waits for them. Coming back to
//! root search marks them stale: the next query asks again, listing the kept
//! results until the answer replaces them. So does a change of what they
//! are made from, the installed applications (`application_changes`),
//! which asks again at once while root search shows a query. A disabled or
//! replaced command's
//! results are forgotten at once, and an answer from it arriving afterwards
//! is discarded.

use std::path::{Path, PathBuf};

use super::quick_slots::PinTarget;
use super::{CommandRegistration, Entry, RootResult, Row};
use crate::packages::{CommandMatches, CommandWhen};
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
    /// Whether it ever answered (with results, or failing): until it
    /// does, a quick slot pinning one of its results waits for it.
    answered: bool,
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

    /// What supplies the results of the command with component
    /// `component` changed (the installed applications it lists): its kept
    /// results are marked stale, to be asked for again, unless it is being
    /// asked now, whose answer may predate the change.
    pub(super) fn changed(&mut self, component: &Path) -> Refresh {
        let Some(index) = self
            .commands
            .iter_mut()
            .find(|index| index.component == component)
        else {
            return Refresh::NotKept;
        };
        if index.asking {
            return Refresh::Asking;
        }
        index.fresh = false;
        Refresh::Stale
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
                            answered: false,
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
        index.answered = true;
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

    /// Where the results of the command with component `component` stand:
    /// a quick slot pinning one of them says so while it cannot be found.
    pub(super) fn listing(&self, component: &Path) -> Listing {
        match self
            .commands
            .iter()
            .find(|index| index.component == component)
        {
            None => Listing::NotAsked,
            Some(index) if index.asking => Listing::Asking,
            Some(Index {
                failure: Some((_, Entry::Broken(problem))),
                ..
            }) => Listing::Failed(problem.clone()),
            Some(index) if index.answered => Listing::Listed,
            Some(_) => Listing::NotAsked,
        }
    }

    /// Whether the command with component `component` ever answered.
    pub(super) fn answered(&self, component: &Path) -> bool {
        self.commands
            .iter()
            .any(|index| index.component == component && index.answered)
    }

    /// The rows explaining why a command could not supply its results.
    pub(super) fn failures(&self) -> impl Iterator<Item = &(Row, Entry)> {
        self.commands
            .iter()
            .filter_map(|index| index.failure.as_ref())
    }
}

/// What [`Indexes::changed`] did with a command's kept results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refresh {
    /// It was never asked: its first query asks it.
    NotKept,
    /// It is being asked: it is to be marked once it answered.
    Asking,
    /// They were marked stale.
    Stale,
}

/// Where one command's kept results stand (see [`Indexes::listing`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Listing {
    /// Never asked for, or forgotten since.
    NotAsked,
    /// Being asked for now, for the first time.
    Asking,
    /// Answered with its results.
    Listed,
    /// Answered with a failure, which says why.
    Failed(String),
}

/// The root result for one of `command`'s indexed results. A quick slot
/// can hold it by its identity: the result's own id scoped to `command`.
fn indexed_result(command: &CommandRegistration, result: IndexedResult) -> RootResult {
    let pin = PinTarget::Indexed {
        command: command.id.clone(),
        result: result.listing.id.clone(),
    };
    let entry = match result.action {
        IndexedAction::OpenApplication(id) => Entry::OpenApplication {
            id,
            name: result.listing.title.clone(),
        },
        IndexedAction::Open {
            target,
            application,
        } => Entry::OpenTarget {
            target,
            application,
            name: result.listing.title.clone(),
        },
    };
    let row = Row::listed(result.listing, Some(&command.id));
    // An alternate title finds it as its title does, and a keyword as its
    // subtitle does; the row shows its title whichever matched.
    let keys = Keys::new(&row.title, row.subtitle.as_deref(), None)
        .with_alternates(&result.alternate_titles, &result.keywords);
    RootResult {
        row,
        entry,
        keys,
        target: None,
        pin: Some(pin),
        // An indexed result is matched by its titles and keywords, as ever.
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    }
}
