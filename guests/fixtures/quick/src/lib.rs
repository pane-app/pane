//! Test fixture: a root provider that answers any query at once with one
//! result. What the calculator's quick answers were before its sources
//! left the repository (#285): no sample computes an answer for every
//! query, and the tests that need a second answer arriving right behind a
//! slow one — close enough to coalesce with it (#201) — build this
//! fixture's `pane.json` (`"mode": "provider"`, `"rootResults": true`,
//! the component as `command.wasm`). A provider has no row of its own,
//! and Pane never opens or runs it; asked behind the slow fixture, its
//! call waits for the runtime thread to be free of the slow one and
//! answers right behind its answer.
#![no_std]

use pane_extension::alloc::{string::String, vec, vec::Vec};
use pane_extension::root::{RootAction, RootResult, WallTime};
use pane_extension::{Command, NoCustomView};

struct Quick;
pane_extension::export!(Quick);
pane_extension::root::export!(Quick);

/// A provider command never opens or runs, so its `Command` keeps the
/// defaults: `type CustomView = NoCustomView` and nothing else.
impl Command for Quick {
    type CustomView = NoCustomView;
}

impl pane_extension::root::Guest for Quick {
    /// One result for any query, copying the text it names: what runs is
    /// seen on the clipboard.
    async fn results_for(_query: String, _at: WallTime) -> Result<Vec<RootResult>, String> {
        Ok(vec![RootResult {
            id: "quick".into(),
            title: "Quick answer".into(),
            subtitle: None,
            action: RootAction::Copy("quick".into()),
            answer: None,
        }])
    }
}
