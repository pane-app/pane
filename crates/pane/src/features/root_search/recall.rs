//! Root search's walk through the recent queries (#206): what Up does on
//! an empty query. The launcher owns the record and the restore (see the
//! core's `history`); the window owns the walk — which entry it is at,
//! and ending it — because the walk is the key press's, and the record
//! is the launcher's.
//!
//! Up, with the query empty, the first row (or no row) selected and the
//! press not the system's repeat of a key still held, restores the most
//! recent query with the argument values typed with it; while the
//! restored query is unchanged, Up again restores the one before. Any
//! other key ends the walk — the query changing (typing, Escape's clear),
//! the selection moving — so the history never hijacks navigation: when
//! a row other than the first is selected, or a query is typed that the
//! walk did not restore, Up moves the selection as it always did.
//!
//! The key reaches the walk through the selection's own handler
//! ([`crate::app::LauncherWindow::select_previous`]), which hands it on
//! while the walk could take it, and the key listener here — which sees
//! the press itself, so it can tell a held key's repeat from a press
//! ([`LauncherWindow::recall_key`]), as the item actions' listener does.

use gpui::{App, KeyDownEvent, Keystroke, Window, prelude::*};
use pane_core::Screen;

use crate::app::LauncherWindow;

/// One walk through the recent queries: which entry the query on screen
/// was restored from, counting back from the newest, and the query it
/// restored. Held by the window while the walk stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recall {
    /// How many entries back the restored query is.
    at: usize,
    /// The query the walk restored: the walk stands while it is the
    /// query on screen, unchanged.
    query: String,
}

/// The keystrokes that move the selection back a result — the Keyboard
/// page's binding for it, and the navigation set's previous key when a
/// set is chosen — which, on an empty query, are the history's instead
/// (#206).
fn previous_result_keys(cx: &App) -> Vec<Keystroke> {
    let mut keys = Vec::new();
    let binding = crate::settings::keyboard_of(cx)
        .binding(pane_core::KeyboardAction::PreviousResult)
        .id();
    if let Ok(key) = Keystroke::parse(&binding) {
        keys.push(key);
    }
    if let Some((previous, _)) = crate::settings::navigation_of(cx).bindings()
        && let Ok(key) = Keystroke::parse(previous)
    {
        keys.push(key);
    }
    keys
}

impl LauncherWindow {
    /// Whether the previous-result key is the history's on the screen now
    /// (#206): root search, with the query empty — a new walk — or with
    /// the query a standing walk restored, unchanged — the walk going on
    /// — and the first row (or no row) selected, so the key has no
    /// selection to move.
    pub(crate) fn recall_ready(&self) -> bool {
        let Screen::Root { query } = self.launcher.screen() else {
            return false;
        };
        // The walk stands while the query it restored is the query on
        // screen, unchanged.
        let walking = self
            .recall
            .as_ref()
            .is_some_and(|recall| recall.query == query);
        if !query.is_empty() && !walking {
            return false;
        }
        matches!(self.launcher.selected(), None | Some(0))
    }

    /// A key pressed while the previous-result key could be the history's
    /// (`recall_ready` held when the selection's own handler handed the
    /// key on), seen as the press itself: the previous-result key walks
    /// the recent queries (#206) — starting the walk, or walking it back
    /// while the query it restored stands — and the system's repeat of a
    /// key still held walks nothing. Any other key that reaches here is
    /// left to the field.
    pub(crate) fn recall_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only the keys that move the selection back walk the history;
        // every other key that reaches here is the field's.
        if !previous_result_keys(cx).contains(&event.keystroke) {
            return;
        }
        // The screen may have left the walk behind while the key was
        // dispatched: then it moves the selection, as it always did.
        if !self.recall_ready() {
            self.move_selection_back(window, cx);
            return;
        }
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        // Where the walk is: at its start the most recent query, then one
        // further back each time. Nothing that far back: the key does
        // nothing more — the selection has nowhere to move either.
        let at = self.recall.as_ref().map_or(0, |recall| recall.at + 1);
        let Some(query) = self.launcher.recent_query(at) else {
            return;
        };
        // The search runs as the field's own change does — the announcer
        // waits for its results and the keys held for the list it makes
        // are applied once it is published (#203) — and the query's
        // change ends any walk that stood. The replace's change reaches
        // the field's listener only at the end of the update this key
        // runs in, and the search it starts takes the values the query's
        // clear recorded with it (#205): the restore, and the walk's own
        // state after the change that would end it, are deferred to run
        // after that.
        self.query.replace(&query, cx);
        cx.defer_in(window, move |this, window, cx| {
            // The argument values the entry was recorded with follow the
            // query (#205), now that the search the replace ran has taken
            // the values the clear recorded.
            this.launcher.restore_recent_arguments(at);
            this.sync_arguments(window, cx);
            // The walk: at this entry, while this query stands. Any other
            // key than the walking one ends it.
            this.recall = Some(Recall { at, query });
            cx.notify();
        });
    }

    /// Moves the selection back a row, as the previous-result key does
    /// when the history is not the key's to walk (#206): the walk, if one
    /// stood, is over.
    fn move_selection_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.recall = None;
        self.launcher.move_selection(-1);
        self.announcer.user_moved();
        // The selected row's argument fields follow it (#205).
        self.sync_arguments(window, cx);
        cx.notify();
    }
}
