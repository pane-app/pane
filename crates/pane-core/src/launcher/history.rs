//! Root search's recent queries (#206): the queries the search cleared —
//! by Escape, by the field being emptied, by a command's opening taking
//! the query (returning to root always starts empty), or by root search
//! being shown fresh over one — recorded as Pane's own record
//! (`search-history.json`, the `aliases.json` house rules, see `choices`)
//! so that Up on an empty query can restore them, each with the argument
//! values typed with it (#205, see `argument_fields`) except a
//! password's, which is recorded empty and its text never written. The
//! record is on this computer only and never sent anywhere; it holds at
//! most [`KEPT`] entries, newest first, and a consecutive duplicate is
//! not added.
//!
//! Recording follows the "Learn from what I choose" switch (#200: one
//! switch for both): turned off, nothing is recorded, and what was
//! recorded is kept until the Launcher page's "Reset search history"
//! resets it. An unreadable record is reported on root search's status
//! line and never replaced, and nothing is recorded while it cannot be
//! read; a write that fails puts the record back to what it last held.
//! Uninstalling a package forgets the entries that carry its commands'
//! values, as its aliases are forgotten; disabling keeps them.

use std::future::Future;

use serde_json::{Map, Value};

use super::argument_fields;
use super::choices::{self, Choices, Record};
use super::{Launcher, State, off_thread};
use crate::arguments::ArgumentKind;
use crate::packages::PackageIdentity;

/// How many entries the history keeps, newest first; the oldest beyond
/// them is dropped when one is added.
const KEPT: usize = 64;

/// One recorded search: the query as it was typed, and the argument
/// values typed with it for the command they belong to — a password's
/// empty, its text never recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// The query, trimmed as it was recorded.
    query: String,
    /// The command whose argument values the entry carries, by its id
    /// in Pane's records; `None` while the search held no values.
    command: Option<String>,
    /// The values by argument name; a password's empty.
    values: Vec<(String, String)>,
}

/// Root search's recent queries, newest first, recorded in
/// `search-history.json` as `{ "version": 1, "queries": [ { "query":
/// "…", "command": "…", "values": { "…": "…" } } ] }` — the command and
/// its values only while the entry carries any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RecentQueries {
    queries: Vec<Entry>,
}

impl Choices for RecentQueries {
    const FILE: &'static str = "search-history.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "the search history";

    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let mut history = RecentQueries::default();
        let Some(queries) = fields.get("queries") else {
            return Ok(history);
        };
        let queries = queries.as_array().ok_or("`queries` is not an array")?;
        for entry in queries {
            // An entry that cannot be used is left out, as an unusable
            // alias is; the rest of the record reads.
            let Some(entry) = entry.as_object() else {
                continue;
            };
            let Some(query) = entry.get("query").and_then(Value::as_str) else {
                continue;
            };
            if query.trim().is_empty() {
                continue;
            }
            let command = entry
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let mut values = Vec::new();
            if let Some(recorded) = entry.get("values").and_then(Value::as_object) {
                for (name, value) in recorded {
                    if let Some(value) = value.as_str() {
                        values.push((name.clone(), value.to_owned()));
                    }
                }
            }
            history.queries.push(Entry {
                query: query.to_owned(),
                command: command.filter(|_| !values.is_empty()),
                values,
            });
        }
        history.queries.truncate(KEPT);
        Ok(history)
    }

    fn write(&self) -> Map<String, Value> {
        let queries = self
            .queries
            .iter()
            .map(|entry| {
                let mut fields = Map::new();
                fields.insert("query".into(), entry.query.clone().into());
                if let Some(command) = &entry.command {
                    fields.insert("command".into(), command.clone().into());
                    let values = entry
                        .values
                        .iter()
                        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                        .collect();
                    fields.insert("values".into(), Value::Object(values));
                }
                Value::Object(fields)
            })
            .collect();
        Map::from_iter([("queries".to_string(), Value::Array(queries))])
    }

    fn restore(&mut self, _command: &str, _other: &Self) {
        // The history's writes never undo one command's choices (see
        // `Launcher::save`): an entry is appended or every entry is
        // reset, and a write that fails reverts the record as a whole
        // through `Record::revert`.
    }

    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = self.queries.len();
        // An entry with no command is no package's: it is always kept.
        self.queries
            .retain(|entry| entry.command.as_deref().is_none_or(keep));
        self.queries.len() != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.history
    }
}

impl RecentQueries {
    /// The entry `back` entries back from the newest: `back` of 0 the
    /// most recent one, the oldest beyond the last. For Up's walk
    /// through the history (#206).
    fn entry(&self, back: usize) -> Option<&Entry> {
        self.queries.get(back)
    }

    /// Forgets every entry; whether any went.
    fn forget_all(&mut self) -> bool {
        if self.queries.is_empty() {
            return false;
        }
        self.queries.clear();
        true
    }
}

/// The argument values the search's inline fields hold (#205), as they
/// are recorded with a query: the command they belong to and the values
/// by argument name, a password's empty — never its text — and nothing
/// while the search holds no values.
fn values_of(state: &State) -> Option<(String, Vec<(String, String)>)> {
    let (command, typed) = state.arguments.recorded()?;
    // A password's value is the one argument that is never recorded: it
    // is recorded empty, so no record of Pane's holds its text.
    let declared = declared_of(state, command);
    let values = typed
        .iter()
        .map(|(name, value)| {
            let password = declared.iter().any(|argument| {
                argument.name == *name && matches!(argument.kind, ArgumentKind::Password)
            });
            let value = if password {
                String::new()
            } else {
                value.clone()
            };
            (name.clone(), value)
        })
        .collect();
    Some((command.to_owned(), values))
}

/// The arguments the command with Pane's id `command` declares, to know
/// which of the values typed are passwords; empty for one built into
/// Pane or no longer installed.
fn declared_of(state: &State, command: &str) -> Vec<crate::arguments::ManifestArgument> {
    let (key, manifest) = choices::split(command);
    state
        .packages
        .iter()
        .find(|package| package.identity.key() == key)
        .map(|package| package.arguments_of(manifest).to_vec())
        .unwrap_or_default()
}

/// What the status line says while an unreadable record keeps Up from
/// recalling anything.
pub(super) fn unreadable_report(problem: &str) -> String {
    format!("Pane could not read the search history, so Up recalls nothing: {problem}")
}

impl Launcher {
    /// Records root search's `query` as cleared, with the argument values
    /// typed with it (#206): a recent query Up can restore. Nothing
    /// happens while the query is blank, while "Learn from what I choose"
    /// is off (#200: one switch for both), or while the record cannot be
    /// read or is kept nowhere. A consecutive duplicate is not added, and
    /// at most [`KEPT`] entries are kept. The record is written off the
    /// window's thread, one write at a time; a write that fails puts it
    /// back to what it last held.
    pub(super) fn record_cleared_query(&self, state: &mut State, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        // "Learn from what I choose" turned off: no query is recorded,
        // and what was recorded is kept until it is reset.
        if !state.learning {
            return;
        }
        let values = values_of(state);
        let entry = Entry {
            query: query.to_owned(),
            command: values.as_ref().map(|(command, _)| command.clone()),
            values: values.map(|(_, values)| values).unwrap_or_default(),
        };
        let record = &mut state.history;
        if record.unreadable().is_some() || !record.kept() {
            return;
        }
        if record.chosen.queries.first() == Some(&entry) {
            return;
        }
        record.chosen.queries.insert(0, entry);
        record.chosen.queries.truncate(KEPT);
        let launcher = self.clone();
        let saves = state.history_saves.clone();
        let ended = saves.clone();
        saves.begin();
        let saving = std::thread::Builder::new()
            .name("pane-history".into())
            .spawn(move || {
                if let Err(error) = launcher.save::<RecentQueries>(None) {
                    // The write that failed puts the record back to what
                    // it last held, as `Record::revert` does for a reset.
                    launcher.lock().history.revert();
                    crate::diagnostic!("Pane could not record the search history: {error}");
                }
                ended.end();
            });
        if let Err(error) = saving {
            saves.end();
            crate::diagnostic!("Pane could not record the search history: {error}");
        }
    }

    /// The recent query `back` entries back from the newest, for Up on an
    /// empty query to restore (#206): `None` when there is none that far
    /// back, or while the record cannot be read — Up then recalls
    /// nothing. The window walks the entries, holding which it is at;
    /// the query is the entry's, trimmed as it was recorded.
    pub fn recent_query(&self, back: usize) -> Option<String> {
        let state = self.lock();
        if state.history.unreadable().is_some() {
            return None;
        }
        state
            .history
            .chosen
            .entry(back)
            .map(|entry| entry.query.clone())
    }

    /// Restores the argument values of the recent query `back` entries
    /// back (#206), after the window searched for the query the same
    /// entry gave — the search took the values the query's clear had
    /// recorded (#205): they become the search's own state again, the
    /// command's fields' values, a password's empty as it was recorded,
    /// no argument marked missing. Nothing happens when there is no
    /// entry that far back.
    pub fn restore_recent_arguments(&self, back: usize) {
        let mut state = self.lock();
        let Some(entry) = state.history.chosen.entry(back) else {
            return;
        };
        // Cloned out first: the values are put into the search's own
        // state, which the entry is not part of.
        let (command, values) = match (&entry.command, &entry.values) {
            (Some(command), values) => (command.clone(), values.clone()),
            (None, _) => return,
        };
        state.arguments = argument_fields::Typed::recalled(&command, &values);
    }

    /// Resets the search history — the Launcher page's "Reset search
    /// history" (#206): every recent query goes, its argument values with
    /// it. Whether anything was recorded to reset, and the future that
    /// records it, off the window's thread, one write at a time — `Err`
    /// when the record cannot be written, with everything put back as the
    /// record last held it. A record that cannot be read is never
    /// replaced; the entry says so while it cannot.
    pub fn reset_search_history(
        &self,
    ) -> (
        bool,
        impl Future<Output = Result<(), String>> + Send + 'static,
    ) {
        let mut guard = self.lock();
        let state = &mut *guard;
        // Nothing runs while the record cannot be read: it is never
        // replaced (see `choices`), and the page's entry says so.
        let refused = state.history.unreadable().map(str::to_owned);
        let reset = refused.is_none().then(|| state.history.chosen.forget_all());
        let launcher = self.clone();
        let recording = async move {
            if let Some(problem) = refused {
                return Err(problem);
            }
            if reset.is_none() {
                return Ok(());
            }
            let writer = launcher.clone();
            let written = off_thread(move || writer.save::<RecentQueries>(None)).await;
            if written.is_err() {
                // Everything goes back to what the record last held (see
                // `Record::revert`).
                launcher.lock().history.revert();
            }
            written
        };
        (reset.is_some(), recording)
    }

    /// Why the search history could not be read, if it could not: Up
    /// recalls nothing then, and the reset cannot run, until the record
    /// is readable again.
    pub fn history_problem(&self) -> Option<String> {
        self.lock().history.unreadable().map(str::to_owned)
    }

    /// Waits until every query recorded so far has been written; `false`
    /// if one has not within `limit`. For tests and development builds,
    /// which so wait for the record without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_history_recorded(&self, limit: std::time::Duration) -> bool {
        let saves = self.lock().history_saves.clone();
        saves.settled(limit)
    }

    /// Forgets the search history's entries that carry the uninstalled
    /// package's commands' values, as its aliases are forgotten. Returns
    /// what writes the record without them, to run off the window's
    /// thread; nothing to write if it had none.
    pub(super) fn forget_history_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.history.forget(identity) {
            return None;
        }
        let launcher = self.clone();
        Some(move || launcher.save::<RecentQueries>(None))
    }
}
