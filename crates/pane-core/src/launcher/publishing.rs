//! Root search's timing of a query's list (#201, #202): when the query
//! is asked of the commands that compute results for it, when the rows
//! shown become the query typed, and what a late answer does to them.
//!
//! Ranking command metadata and the results kept ahead of the query is
//! synchronous, as it always was. For the results computed from the query
//! the launcher waits before it shows the new query's list: it publishes
//! it once every provider asked for the query has answered, or 200 ms
//! after the query changed, whichever comes first — so a provider that is
//! slow or hangs holds the list no longer than the budget — and until
//! then the window keeps showing the previous query's list while the
//! search field shows what was typed at once. A provider that misses the
//! budget keeps running until its answer or cancellation: the budget
//! bounds the wait, not the scheduling, and the runtime still serves one
//! call at a time.
//!
//! The commands that compute results are not asked the moment the query
//! changes: the search waits out a short quiet period first (#202), so a
//! burst of keystrokes asks once, and a call a query replaced is
//! cancelled only when the newer query needs the runtime — as its quiet
//! period ends — keeping the instance of a call that answers within it
//! (see [`Launcher::set_query`]).
//!
//! An answer that arrives after the list was published is merged into it,
//! coalesced within 16 ms: answers arriving close together become one
//! update. A merge moves the selection to the new first row when the
//! first row was selected (see `relist_root`), so a preselected fallback
//! (ADR 0031) gives way to a late result, and it never takes the
//! selection from a row the user moved to. Answers for an older query or
//! an earlier search of the same one are discarded as before
//! (`search_epoch`).
//!
//! The budget and the coalescing are timed by the launcher's clock
//! ([`crate::clipboard::Clock`]): a clock a test advances publishes what
//! is due through [`crate::clipboard::Clock::on_change`], while a thread
//! of the launcher's own waits the time out for the system's clock, which
//! only moves by time passing. [`Launcher::list_published`] tells whether
//! the current query's list is published, which the keys the window holds
//! for it read (#203).

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Launcher, State, relist_root};

/// How long a query's list waits for its providers before it is published
/// anyway (#201).
pub(super) const BUDGET: Duration = Duration::from_millis(200);

/// How long the search waits after the last keystroke before it asks the
/// commands that compute results for the query (#202): a burst of
/// keystrokes asks once, well within the budget. It is waited out in real
/// time rather than by the launcher's clock: a clock a test holds still
/// would never let a query be asked.
pub(super) const QUIET: Duration = Duration::from_millis(40);

/// How close together late answers merge into one update of the published
/// list (#201).
const MERGE_WITHIN: Duration = Duration::from_millis(16);

/// While the current query's list is not yet published: what still holds
/// it (see [`State::holding`]). The rows shown stay the previous query's,
/// which the field's own query runs ahead of.
pub(super) struct Holding {
    /// The query whose list is held.
    pub(super) query: String,
    /// The components of the providers asked for the query that have not
    /// answered: the list is published once none is left.
    pub(super) awaiting: Vec<PathBuf>,
    /// When the budget ends, in milliseconds by the launcher's clock.
    pub(super) deadline: u64,
}

impl Launcher {
    /// Whether the list shown is the current query's (#201): published,
    /// once every provider asked for the query has answered or its budget
    /// ended. While it is not, the rows shown stay the previous query's
    /// and the field shows what was typed; the keys the window holds for
    /// it (#203) wait for this.
    pub fn list_published(&self) -> bool {
        self.lock().holding.is_none()
    }

    /// Notes that a late answer must merge into the published list
    /// (#201), if none is coalescing yet: one arriving within 16 ms of
    /// another becomes the same update.
    pub(super) fn merge_soon(&self, state: &mut State) {
        if state.merge.is_none() {
            let at = state.clock.now().saturating_add(milliseconds(MERGE_WITHIN));
            state.merge = Some(at);
            self.wait_for(MERGE_WITHIN);
        }
    }

    /// Looks at what is due to be published once `wait` has passed
    /// (#201): a thread of its own waits the time out, for the system's
    /// clock, which only moves by time passing — a clock a test advances
    /// calls the same look through [`crate::clipboard::Clock::on_change`]
    /// when it is set.
    pub(super) fn wait_for(&self, wait: Duration) {
        let launcher = self.downgrade();
        let started = std::thread::Builder::new()
            .name("pane-publish".into())
            .spawn(move || {
                std::thread::sleep(wait);
                if let Some(launcher) = launcher.upgrade() {
                    launcher.publish_due();
                }
            });
        if let Err(error) = started {
            crate::diagnostic!(
                "Pane cannot wait out root search's budget on its own thread: {error}"
            );
        }
    }

    /// Publishes what is due by the launcher's clock now (#201): the held
    /// list whose budget ended, and the merge whose coalescing closed.
    /// Whichever way the clock reached it — time passing on the system's,
    /// or a test advancing its own — this is the one look both take.
    pub(super) fn publish_due(&self) {
        let mut guard = self.lock();
        let now = guard.clock.now();
        let mut changed = false;
        if guard
            .holding
            .as_ref()
            .is_some_and(|held| now >= held.deadline)
        {
            publish(&mut guard);
            changed = true;
        }
        if guard.merge.is_some_and(|at| now >= at) {
            guard.merge = None;
            let query = guard.view.query().map(str::to_owned);
            if let Some(query) = query {
                relist_root(&mut guard, &query);
                changed = true;
            }
        }
        drop(guard);
        if changed {
            // The window redraws from the launcher, as it does for any
            // change made in the background.
            self.changed();
        }
    }
}

/// Publishes the held list of the query it names (#201): every provider
/// asked answered, or the budget ended. The rows shown become the
/// query's — the answers that arrived while it was held included, in
/// place of every answer of an earlier query — and the selection follows
/// a merge's rule (see `relist_root`): the new first row when the first
/// row was selected, its own row by id otherwise.
pub(super) fn publish(state: &mut State) {
    let Some(holding) = state.holding.take() else {
        return;
    };
    state.computed = std::mem::take(&mut state.staged);
    state.published = holding.query.clone();
    relist_root(state, &holding.query);
}

/// Notes that `component` answered the query whose list is held (#201),
/// publishing the list once every provider asked has.
pub(super) fn answered(state: &mut State, component: &Path) {
    let due = state.holding.as_mut().is_some_and(|holding| {
        holding
            .awaiting
            .retain(|asked| asked.as_path() != component);
        holding.awaiting.is_empty()
    });
    if due {
        publish(state);
    }
}

/// `duration` in whole milliseconds, as the clock counts them.
pub(super) fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
