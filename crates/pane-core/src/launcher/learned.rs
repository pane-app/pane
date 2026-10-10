//! What root search learned from what the user chooses (ADR 0030, #199):
//! every root result the user invokes from root search — with its action
//! dispatched, not refused as unavailable or paused — earns **frecency**,
//! a score that decays with a ten-day half-life and never falls below the
//! score of a result never used, and the **query** the user typed before
//! choosing it, folded as matching folds it. The last three distinct
//! ones are remembered per result, newest first; they weigh in ranking
//! (the comparator's steps 3, 6 and 7, see `search`) only while the
//! result's frecency is above 1 and it was last opened within 17 days.
//!
//! A use is keyed by the result's identity, as a quick slot is (ADR
//! 0026): a registered command by its command id, an indexed result —
//! such as an application, whose id survives an update into a new
//! version folder (#124) — by its own id under the command that supplies
//! it; never a row's title or position. Not recorded: a use a global
//! hotkey opens, a computed answer or other computed result, a file, a
//! fallback row, and Pane's own install and management rows — none of
//! them a root result chosen from root search. Invoking a quick slot
//! from the pinned home records a use with no query.
//!
//! The record is Pane's own — `learned.json` beside `installed.json`,
//! following the house record rules of `aliases.json` (see `choices`):
//! versioned, validated, written atomically one change at a time; an
//! unreadable record is reported and never replaced, and nothing is
//! recorded while it cannot be read. A write that fails puts back what
//! the record last held. Uninstalling a package forgets its entries, as
//! its aliases are forgotten; disabling keeps them. An entry whose score
//! has decayed to 1 and that was last opened more than 17 days ago is
//! dropped: it ranks nothing. Recording a use does not re-sort the list
//! on screen; the next search ranks with it.
//!
//! The user's controls over what is learned (#200): the Actions panel's
//! "Reset Ranking" clears one result's frecency and queries (the whole
//! entry, by its identity), the Launcher page's "Reset ranking…" clears
//! every result's, and the page's "Learn from what I choose" switch
//! stops any use being recorded and ranking weighing what was learned
//! — turned off, ranking acts as if nothing was learned, and what was
//! learned is kept until it is reset, so turning it on again uses it.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;

use serde_json::{Map, Value};

use super::actions::{ResultAction, ResultActionItem};
use super::choices::{Choices, Record};
use super::{Entry, Launcher, Screen, State, Status, off_thread};
use crate::packages::PackageIdentity;
use crate::search::Learned;

/// A frecency score's half-life, in days: the score halves every ten
/// days since the use it was anchored at.
const HALF_LIFE_DAYS: f64 = 10.0;

/// How many days a use keeps its result's learned queries counting, and
/// past which an entry decayed to the floor is dropped.
const WITHIN_DAYS: f64 = 17.0;

/// How many distinct queries are remembered per result, newest first.
const KEPT_QUERIES: usize = 3;

/// A day, in milliseconds.
const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

/// One result's learned use: its frecency score, anchored at the last
/// use, and the queries it was chosen with.
#[derive(Clone, Debug, Default, PartialEq)]
struct Use {
    /// The score anchored at `last_opened`: what the decayed score was,
    /// plus 1.
    score: f64,
    /// When the result was last opened, in milliseconds by the
    /// launcher's clock.
    last_opened: u64,
    /// The queries the result was chosen with, newest first, each
    /// distinct, non-empty and folded.
    queries: Vec<String>,
}

/// What root search learned, by the result's identity (a command's id,
/// or an indexed result's `<command id>:<result id>`), recorded in
/// `learned.json` as `{ "version": 1, "uses": { "<id>": { "score": 2,
/// "lastOpened": …, "queries": […] } } }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct LearnedChoices {
    uses: BTreeMap<String, Use>,
}

impl Choices for LearnedChoices {
    const FILE: &'static str = "learned.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "what root search learned";

    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let mut learned = LearnedChoices::default();
        let Some(uses) = fields.get("uses") else {
            return Ok(learned);
        };
        let uses = uses.as_object().ok_or("`uses` is not an object")?;
        for (id, entry) in uses {
            // An entry that cannot be used is left out, as an unusable
            // alias is; the rest of the record reads.
            let Some(entry) = entry.as_object() else {
                continue;
            };
            let (Some(score), Some(last_opened)) = (
                entry.get("score").and_then(Value::as_f64),
                entry.get("lastOpened").and_then(Value::as_u64),
            ) else {
                continue;
            };
            // A use leaves a score above the floor of 1; a hand-edited
            // entry below it ranks nothing and is not kept.
            if !score.is_finite() || score <= 1.0 {
                continue;
            }
            let mut queries = Vec::new();
            if let Some(learned) = entry.get("queries").and_then(Value::as_array) {
                for query in learned.iter().filter_map(Value::as_str) {
                    if !query.trim().is_empty() && !queries.iter().any(|kept| kept == query) {
                        queries.push(query.to_owned());
                    }
                }
            }
            queries.truncate(KEPT_QUERIES);
            learned.uses.insert(
                id.clone(),
                Use {
                    score,
                    last_opened,
                    queries,
                },
            );
        }
        Ok(learned)
    }

    fn write(&self) -> Map<String, Value> {
        let uses = self
            .uses
            .iter()
            .map(|(id, use_)| {
                (
                    id.clone(),
                    serde_json::json!({
                        "score": use_.score,
                        "lastOpened": use_.last_opened,
                        "queries": use_.queries,
                    }),
                )
            })
            .collect();
        Map::from_iter([("uses".to_string(), Value::Object(uses))])
    }

    fn restore(&mut self, id: &str, other: &Self) {
        match other.uses.get(id) {
            Some(use_) => self.uses.insert(id.to_owned(), use_.clone()),
            None => self.uses.remove(id),
        };
    }

    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = self.uses.len();
        self.uses.retain(|id, _| keep(id));
        self.uses.len() != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.learned
    }
}

/// The days from `then` to `now`, both by the launcher's clock.
fn days_since(now: u64, then: u64) -> f64 {
    now.saturating_sub(then) as f64 / DAY
}

/// The frecency of `use_` as of `now`: its score, anchored at the last
/// use, halved every ten days since and never below the score of a
/// result never used.
fn frecency(use_: &Use, now: u64) -> f64 {
    let elapsed = days_since(now, use_.last_opened);
    (use_.score * 2f64.powf(-elapsed / HALF_LIFE_DAYS)).max(1.0)
}

/// Whether `use_`'s queries still count at `now`: the result's frecency
/// is above the floor of 1, and it was last opened within 17 days.
fn counting(use_: &Use, now: u64) -> bool {
    frecency(use_, now) > 1.0 && days_since(now, use_.last_opened) <= WITHIN_DAYS
}

impl LearnedChoices {
    /// What ranking sees of the learned uses as of `now`, by the row id
    /// of the result each was recorded for: its decayed frecency, and
    /// its queries while they still count.
    pub(super) fn ranked(&self, now: u64) -> HashMap<String, Learned> {
        self.uses
            .iter()
            .map(|(id, use_)| {
                let frecency = frecency(use_, now);
                let queries = counting(use_, now)
                    .then(|| use_.queries.clone())
                    .unwrap_or_default();
                (id.clone(), Learned { frecency, queries })
            })
            .collect()
    }

    /// Forgets what was learned about the result `id` — its frecency and
    /// its queries, the whole entry; whether anything went.
    fn forget_one(&mut self, id: &str) -> bool {
        self.uses.remove(id).is_some()
    }

    /// Forgets everything that was learned; whether anything went.
    fn forget_all(&mut self) -> bool {
        if self.uses.is_empty() {
            return false;
        }
        self.uses.clear();
        true
    }
}

/// The Actions panel's "Reset Ranking" entry for root search's selected
/// row, a result root search can learn about (see `use_of`): it clears
/// what was learned for it — its frecency and its queries. It cannot run
/// while the record cannot be read.
pub(super) fn reset_item(state: &State) -> ResultActionItem {
    ResultActionItem {
        action: ResultAction::ResetRanking,
        label: "Reset Ranking".into(),
        available: state.learned.unreadable().is_none(),
    }
}

/// The use root search records of activating `entry` on the selected row:
/// the identity of the root result the row is — a registered command's
/// id, or an indexed result's `<command id>:<result id>` — and the query
/// typed. `None` off root search; for a row without a lasting identity
/// (Pane's own rows, an alias's or a fallback's text, a computed answer,
/// a file), which is never a root result; and for an action that does
/// not dispatch, one refused as unavailable or paused, or one the window
/// performs instead of the launcher: no use of those is recorded.
pub(super) fn use_of(state: &State, entry: &Entry) -> Option<(String, String)> {
    if !matches!(state.view.screen, Screen::Root { .. }) {
        return None;
    }
    match entry {
        Entry::Open(_) | Entry::OpenApplication { .. } | Entry::OpenTarget { .. } => {}
        _ => return None,
    }
    let index = state.view.selected?;
    let row = state.view.rows.get(index)?;
    let query = state.view.query()?.to_owned();
    // A root result a quick slot can hold by identity (`quick_slots`):
    // a registered command, or an indexed result under its command.
    // Pane's own rows and every other row have none.
    state
        .root
        .iter()
        .chain(state.indexes.results())
        .find(|result| result.row.id == row.id)
        .filter(|result| result.pin.is_some())
        .map(|_| (row.id.clone(), query))
}

/// Records a use of the result `id` at `now`, with `query` typed (or
/// `None`, a use with no query): the decayed score gains 1 and is
/// anchored at `now`, and the query, folded and not one already held,
/// becomes the newest of the three remembered. Entries whose score has
/// decayed to 1 and that were last opened more than 17 days ago are
/// dropped — they rank nothing. Whether anything changed: nothing is
/// recorded while the record cannot be read, or is kept nowhere.
pub(super) fn record(state: &mut State, id: &str, query: Option<&str>, now: u64) -> bool {
    let record = &mut state.learned;
    if record.unreadable().is_some() || !record.kept() {
        return false;
    }
    let query = query
        .map(crate::search::fold)
        .filter(|query| !query.is_empty());
    let uses = &mut record.chosen.uses;
    uses.retain(|_, use_| {
        !(frecency(use_, now) <= 1.0 && days_since(now, use_.last_opened) > WITHIN_DAYS)
    });
    let use_ = uses.entry(id.to_owned()).or_default();
    // A use adds 1 to the decayed score, re-anchoring it; a result never
    // used stands at the floor of 1.
    use_.score = frecency(use_, now) + 1.0;
    use_.last_opened = now;
    if let Some(query) = query {
        use_.queries.retain(|learned| *learned != query);
        use_.queries.insert(0, query);
        use_.queries.truncate(KEPT_QUERIES);
    }
    true
}

/// What the status line says while an unreadable record keeps root
/// search from learning anything.
pub(super) fn unreadable_report(problem: &str) -> String {
    format!(
        "Pane could not read what root search learned, so it ranks as if nothing was learned: \
         {problem}"
    )
}

impl Launcher {
    /// Records that the user chose the root result `id` from root search,
    /// with `query` typed (or `None`, a use with no query, as a quick
    /// slot's is), by the launcher's clock, and writes what was learned
    /// off this thread, one write at a time. The use takes effect in
    /// ranking with the next search, and never re-sorts the list on
    /// screen; a record that cannot be written goes back to what it last
    /// held, and one that cannot be read is never replaced — it was
    /// reported when Pane started.
    pub(super) fn record_use(&self, id: &str, query: Option<&str>) {
        let mut state = self.lock();
        // "Learn from what I choose" turned off: no use is recorded, and
        // what was learned is kept until it is reset.
        if !state.learning {
            return;
        }
        let now = state.clock.now();
        let recorded = record(&mut state, id, query, now);
        if !recorded {
            return;
        }
        // The rows kept for a query were ranked from what was learned
        // then: rank them again for the next search, not for the list on
        // screen (#199 with #202's `statics` generation).
        state.statics += 1;
        let id = id.to_owned();
        let saves = state.learned_saves.clone();
        saves.begin();
        let ended = saves.clone();
        let launcher = self.clone();
        let saving = std::thread::Builder::new()
            .name("pane-learned".into())
            .spawn(move || {
                if let Err(error) = launcher.save::<LearnedChoices>(Some(&id)) {
                    crate::diagnostic!("Pane could not record what root search learned: {error}");
                }
                ended.end();
            });
        if let Err(error) = saving {
            saves.end();
            crate::diagnostic!("Pane could not record what root search learned: {error}");
        }
    }

    /// Why what root search learned could not be read, if it could not:
    /// nothing is recorded then and root search ranks as if nothing was
    /// learned, until the record is readable again.
    pub fn learned_problem(&self) -> Option<String> {
        self.lock().learned.unreadable().map(str::to_owned)
    }

    /// Waits until every use recorded so far has been written; `false` if
    /// one has not within `limit`. For tests and development builds, which
    /// so wait for the record without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_learned_recorded(&self, limit: std::time::Duration) -> bool {
        let saves = self.lock().learned_saves.clone();
        saves.settled(limit)
    }

    /// Resets what root search learned about `target` — the Actions
    /// panel's "Reset Ranking" (#200), for a root result that can be
    /// learned and is still the selected row: its frecency and its
    /// learned queries are cleared, and the status line says "Ranking
    /// reset for <title>". Whether the reset ran, and the future that
    /// records it, off the window's thread, one write at a time — a write
    /// that fails puts the entry back (see [`Launcher::save`]) and says
    /// why. A record that cannot be read is never replaced; the panel's
    /// entry stays unavailable while it is, and the status line says why.
    /// The list on screen ranks again at once: the reset is a change the
    /// user made, not a use recorded, which never re-sorts it.
    pub fn reset_ranking(&self, target: &str) -> (bool, impl Future<Output = ()> + Send + 'static) {
        let mut guard = self.lock();
        let state = &mut *guard;
        // The panel's target must still be the selected row, one root
        // search learns about: `result_action_ready` checked the same
        // before the panel ran it.
        let title = super::quick_slots::pin_of_selected(state)
            .filter(|pin| pin.key() == target)
            .and_then(|_| {
                state
                    .view
                    .selected
                    .and_then(|index| state.view.rows.get(index))
                    .map(|row| row.title.clone())
            });
        // The write the reset left to make: the entry's return to the
        // record, which the future records.
        let mut pending = None;
        let mut ran = false;
        if let Some(title) = title {
            ran = true;
            if let Some(problem) = state.learned.unreadable() {
                state.view.status = Status::Error(unreadable_report(problem));
            } else {
                state.view.status = Status::Result(format!("Ranking reset for {title}"));
                if state.learned.chosen.forget_one(target) {
                    pending = Some((state.screen_epoch, target.to_owned()));
                    super::reranked(state);
                    self.changed();
                }
            }
        }
        let launcher = self.clone();
        let recording = async move {
            let Some((epoch, target)) = pending else {
                return;
            };
            let writer = launcher.clone();
            let written = off_thread(move || writer.save::<LearnedChoices>(Some(&target))).await;
            if let Err(problem) = written {
                let mut state = launcher.lock();
                // The write put the entry back (see `Launcher::save`):
                // ranking sees it again at once.
                super::reranked(&mut state);
                launcher.changed();
                if state.screen_epoch == epoch {
                    state.view.status =
                        Status::Error(format!("Could not reset the ranking: {problem}"));
                }
            }
        };
        (ran, recording)
    }

    /// Resets everything root search learned — the Launcher page's
    /// "Reset ranking…" (#200): every result's frecency and learned
    /// queries are cleared. Whether anything was learned to reset, and
    /// the future that records it, off the window's thread, one write at
    /// a time — `Err` when the record cannot be written or read, with
    /// everything put back as the record last held it. A record that
    /// cannot be read is never replaced. The list on screen ranks again
    /// at once: the reset is a change the user made, not a use recorded,
    /// which never re-sorts it.
    pub fn reset_all_learned(
        &self,
    ) -> (bool, impl Future<Output = Result<(), String>> + Send + 'static) {
        let mut guard = self.lock();
        let state = &mut *guard;
        // Nothing runs while the record cannot be read: it is never
        // replaced (see `choices`), and the page's entry says so.
        let refused = state.learned.unreadable().map(str::to_owned);
        let reset = refused.is_none().then(|| state.learned.chosen.forget_all()).flatten();
        if reset.is_some() {
            super::reranked(state);
            self.changed();
        }
        let launcher = self.clone();
        let recording = async move {
            if let Some(problem) = refused {
                return Err(problem);
            }
            if reset.is_none() {
                return Ok(());
            }
            let writer = launcher.clone();
            let written = off_thread(move || writer.save::<LearnedChoices>(None)).await;
            if written.is_err() {
                // Everything goes back to what the record last held (see
                // `Record::revert`): ranking sees it again at once.
                let mut state = launcher.lock();
                state.learned.revert();
                super::reranked(&mut state);
                launcher.changed();
            }
            written
        };
        (reset.is_some(), recording)
    }

    /// Forgets what root search learned about the uninstalled package
    /// with `identity` (its commands and its results), as its aliases are
    /// forgotten. Returns what writes the record without them, to run off
    /// the window's thread; nothing to write if it had nothing learned.
    pub(super) fn forget_learned_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.learned.forget(identity) {
            return None;
        }
        let launcher = self.clone();
        Some(move || launcher.save::<LearnedChoices>(None))
    }
}
