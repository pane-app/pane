//! The selected result's actions: what the launcher's Actions panel lists
//! for root search's selected row, and the flows its entries open.
//!
//! The list holds only what Pane can do for that row now: its primary
//! action (the footer's, the same definition and dispatch), then, for a
//! result a quick slot can hold, pinning it (see `quick_slots`: a slot's
//! own entries remove and move it), then, for an installed command, the
//! hotkey and alias configuration Manage extensions already offers.
//! Nothing is listed that has no working operation behind it (#100): no
//! quit, new window or hide. The same items describe a quick slot's own
//! entries and the slots a full set offers to replace (see
//! `quick_slots`).
//!
//! An alias or hotkey flow opened here returns to the search it came from
//! — the same rows, the target still selected, the outcome in the status —
//! where the same flows opened from Manage extensions return there.

use super::{
    Entry, Launcher, LauncherView, Screen, State, quick_slots, selected_action, shortcuts,
};

/// One kind of action on a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultAction {
    /// The result's primary action: what the footer's button and Enter do.
    Invoke,
    /// Records or changes the command's global hotkey.
    Hotkey,
    /// Sets or changes the command's alias.
    Alias,
    /// Pins the result to the first empty quick slot (see
    /// `quick_slots`); with all five taken, the slot to replace is chosen
    /// first ([`ResultAction::ReplaceSlot`]).
    Pin,
    /// Puts the result in the quick slot at this index, in place of what
    /// it holds: the explicit choice a full set of slots asks for.
    ReplaceSlot(usize),
    /// Empties the quick slot that holds the result.
    Unpin,
    /// Swaps the result's quick slot with the one to its left.
    MoveSlotLeft,
    /// Swaps the result's quick slot with the one to its right.
    MoveSlotRight,
}

impl ResultAction {
    /// The action's name in records and reports: "invoke", "hotkey",
    /// "alias", "pin", "replace-slot", "unpin", "move-slot-left",
    /// "move-slot-right".
    pub fn id(self) -> &'static str {
        match self {
            ResultAction::Invoke => "invoke",
            ResultAction::Hotkey => "hotkey",
            ResultAction::Alias => "alias",
            ResultAction::Pin => "pin",
            ResultAction::ReplaceSlot(_) => "replace-slot",
            ResultAction::Unpin => "unpin",
            ResultAction::MoveSlotLeft => "move-slot-left",
            ResultAction::MoveSlotRight => "move-slot-right",
        }
    }

    /// A quick slot entry's fixed label: "Pin to Quick Slot", "Remove from
    /// Quick Slot", "Move Slot Left", "Move Slot Right". `None` for the
    /// other actions, and for [`ResultAction::ReplaceSlot`], whose label
    /// names the slot and what it holds.
    pub fn quick_slot_label(self) -> Option<&'static str> {
        match self {
            ResultAction::Pin => Some("Pin to Quick Slot"),
            ResultAction::Unpin => Some("Remove from Quick Slot"),
            ResultAction::MoveSlotLeft => Some("Move Slot Left"),
            ResultAction::MoveSlotRight => Some("Move Slot Right"),
            ResultAction::Invoke
            | ResultAction::Hotkey
            | ResultAction::Alias
            | ResultAction::ReplaceSlot(_) => None,
        }
    }

    /// A configuration entry's label, by whether the command already has
    /// that configuration: "Assign Hotkey…" or "Change Hotkey…", "Add
    /// Alias…" or "Change Alias…". `None` for [`ResultAction::Invoke`],
    /// which is named by the result's own action, and for the quick slot
    /// entries (see [`ResultAction::quick_slot_label`]).
    pub fn configuration_label(self, configured: bool) -> Option<&'static str> {
        match (self, configured) {
            (
                ResultAction::Invoke
                | ResultAction::Pin
                | ResultAction::ReplaceSlot(_)
                | ResultAction::Unpin
                | ResultAction::MoveSlotLeft
                | ResultAction::MoveSlotRight,
                _,
            ) => None,
            (ResultAction::Hotkey, false) => Some("Assign Hotkey…"),
            (ResultAction::Hotkey, true) => Some("Change Hotkey…"),
            (ResultAction::Alias, false) => Some("Add Alias…"),
            (ResultAction::Alias, true) => Some("Change Alias…"),
        }
    }
}

/// One entry of the Actions panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultActionItem {
    pub action: ResultAction,
    /// What the entry says: "Open command", "Assign Hotkey…".
    pub label: String,
    /// Whether it can run now; the primary action of an unavailable result
    /// cannot.
    pub available: bool,
}

/// The actions of root search's selected row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultActions {
    /// The row's stable id: the target the panel opened for, which must
    /// still be the selected row when an entry runs.
    pub target: String,
    /// The row's title, as the panel's header names it.
    pub title: String,
    /// The primary action first, then the command's configuration.
    pub items: Vec<ResultActionItem>,
}

impl ResultActions {
    /// The entries whose label holds `query`, ignoring case and the
    /// spaces around it; all of them for a blank one.
    pub fn matching(&self, query: &str) -> Vec<&ResultActionItem> {
        let query = query.trim().to_lowercase();
        self.items
            .iter()
            .filter(|item| item.label.to_lowercase().contains(&query))
            .collect()
    }
}

/// The search to restore when an alias or hotkey flow opened from the
/// Actions panel ends.
pub(super) struct Return {
    view: LauncherView,
    entries: Vec<Entry>,
    /// The flow screen's epoch: a flow left any other way (Return to
    /// root, a hotkey pressed) moved past it, and this is stale then.
    epoch: u64,
}

impl Launcher {
    /// The actions of root search's selected row, or `None` off root
    /// search or with nothing selected. See the module docs for what is
    /// listed.
    pub fn result_actions(&self) -> Option<ResultActions> {
        let state = self.lock();
        result_actions(self, &state)
    }

    /// Whether `action` can run on `target` now: `target` is still root
    /// search's selected row, and the action is listed for it and
    /// available.
    pub fn result_action_ready(&self, target: &str, action: ResultAction) -> bool {
        let state = self.lock();
        ready(self, &state, target, action)
    }

    /// Opens the hotkey screen or the alias form of `target`, an installed
    /// command, when the action is [ready](Launcher::result_action_ready);
    /// the flow returns to this search when it ends. Whether it opened:
    /// nothing changes otherwise. Only [`ResultAction::Hotkey`] and
    /// [`ResultAction::Alias`] open a flow: [`ResultAction::Invoke`] is the
    /// window's primary action, and the quick slot entries change the
    /// slots ([`Launcher::change_quick_slots`]).
    pub fn open_result_action(&self, target: &str, action: ResultAction) -> bool {
        let mut state = self.lock();
        let flow = matches!(action, ResultAction::Hotkey | ResultAction::Alias);
        if !flow || !ready(self, &state, target, action) {
            return false;
        }
        let view = state.view.clone();
        let entries = state.entries.clone();
        if action == ResultAction::Hotkey {
            self.show_hotkey(&mut state, target);
        } else {
            self.show_alias_form(&mut state, target);
        }
        // Set after the flow opened: opening it clears what a visit from
        // Manage extensions would otherwise inherit.
        state.actions_return = Some(Return {
            view,
            entries,
            epoch: state.screen_epoch,
        });
        true
    }

    /// Ends an alias or hotkey flow back on the search the Actions panel
    /// opened it from, if it did and the flow is still the screen it
    /// opened: whether it did.
    pub(super) fn return_from_actions_flow(&self, state: &mut State) -> bool {
        let Some(back) = state
            .actions_return
            .take()
            .filter(|back| back.epoch == state.screen_epoch)
        else {
            return false;
        };
        state.next_screen();
        state.entries = back.entries;
        state.view = back.view;
        true
    }
}

fn result_actions(launcher: &Launcher, state: &State) -> Option<ResultActions> {
    if !matches!(state.view.screen, Screen::Root { .. }) {
        return None;
    }
    let index = state.view.selected?;
    let row = state.view.rows.get(index)?;
    let primary = selected_action(state);
    let mut items = vec![ResultActionItem {
        action: ResultAction::Invoke,
        label: primary.label,
        available: primary.available,
    }];
    // A result a quick slot can hold: pinning it (its slot's own panel
    // removes and moves it).
    if quick_slots::pin_of_selected(state).is_some() {
        items.push(quick_slots::pin_item(state));
    }
    // An installed command's own row — not one that sends text through
    // an alias, and not this build's samples, which take no configuration.
    let command = matches!(
        state.entries.get(index),
        Some(Entry::Open(_) | Entry::Unavailable(_))
    )
    .then(|| {
        shortcuts::catalog(launcher, state)
            .groups
            .into_iter()
            .flat_map(|group| group.commands)
            .find(|command| command.id == row.id)
    })
    .flatten();
    if let Some(command) = command {
        if command.hotkey_editable {
            items.push(configuration(
                ResultAction::Hotkey,
                command.hotkey.is_some(),
            ));
        }
        if command.editable {
            items.push(configuration(ResultAction::Alias, command.alias.is_some()));
        }
    }
    Some(ResultActions {
        target: row.id.clone(),
        title: row.title.clone(),
        items,
    })
}

/// A configuration entry, which can always be opened while listed.
fn configuration(action: ResultAction, configured: bool) -> ResultActionItem {
    ResultActionItem {
        action,
        label: action
            .configuration_label(configured)
            .expect("a configuration entry")
            .to_owned(),
        available: true,
    }
}

fn ready(launcher: &Launcher, state: &State, target: &str, action: ResultAction) -> bool {
    result_actions(launcher, state).is_some_and(|actions| {
        actions.target == target
            && actions
                .items
                .iter()
                .any(|item| item.action == action && item.available)
    })
}
