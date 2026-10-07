//! The selected result's actions: what the launcher's Actions panel lists
//! for root search's selected row, and the flows its entries open.
//!
//! The list holds only what Pane can do for that row now: its primary
//! action (the footer's, the same definition and dispatch), then, for a
//! result a quick slot can hold, pinning it (see `quick_slots`: a slot's
//! own entries remove and move it), then, for an installed command, the
//! hotkey and alias configuration Manage extensions already offers, and
//! "Configure Command…" and "Configure Extension…" when the command or its
//! package declares preferences (the window opens the extension's card in
//! Settings for them; see `setup`).
//! Nothing is listed that has no working operation behind it (#100): no
//! quit, new window or hide. The same items describe a quick slot's own
//! entries (see `quick_slots`).
//!
//! An alias or hotkey flow opened here returns to the search it came from
//! — the same rows, the target still selected, the outcome in the status —
//! where the same flows opened from Manage extensions return there.

use super::{
    Entry, Launcher, LauncherView, Mode, Screen, SelectedAction, State, Status, quick_slots,
    shortcuts,
};
use crate::packages::SavedData;

/// One kind of action on a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultAction {
    /// The result's primary action: what the footer's button and Enter do.
    Invoke,
    /// Records or changes the command's global hotkey.
    Hotkey,
    /// Sets or changes the command's alias.
    Alias,
    /// Pins the result: a quick slot at the end of the list (see
    /// `quick_slots`).
    Pin,
    /// Takes the quick slot that holds the result out of the list.
    Unpin,
    /// Swaps the result's quick slot with the one before it.
    MovePinUp,
    /// Swaps the result's quick slot with the one after it.
    MovePinDown,
    /// Opens the command's own preferences on its extension's card in
    /// Settings › Extensions (the window does; see
    /// [`Launcher::preferences_target`]).
    ConfigureCommand,
    /// Opens the extension's preferences on its card in Settings ›
    /// Extensions (the window does).
    ConfigureExtension,
}

impl ResultAction {
    /// The action's name in records and reports: "invoke", "hotkey",
    /// "alias", "pin", "unpin", "move-pin-up", "move-pin-down".
    pub fn id(self) -> &'static str {
        match self {
            ResultAction::Invoke => "invoke",
            ResultAction::Hotkey => "hotkey",
            ResultAction::Alias => "alias",
            ResultAction::Pin => "pin",
            ResultAction::Unpin => "unpin",
            ResultAction::MovePinUp => "move-pin-up",
            ResultAction::MovePinDown => "move-pin-down",
            ResultAction::ConfigureCommand => "configure-command",
            ResultAction::ConfigureExtension => "configure-extension",
        }
    }

    /// A quick slot entry's fixed label: "Pin", "Unpin", "Move Up", "Move
    /// Down" (a window that lays the pins out side by side may say left and
    /// right). `None` for the other actions.
    pub fn quick_slot_label(self) -> Option<&'static str> {
        match self {
            ResultAction::Pin => Some("Pin"),
            ResultAction::Unpin => Some("Unpin"),
            ResultAction::MovePinUp => Some("Move Up"),
            ResultAction::MovePinDown => Some("Move Down"),
            ResultAction::Invoke
            | ResultAction::Hotkey
            | ResultAction::Alias
            | ResultAction::ConfigureCommand
            | ResultAction::ConfigureExtension => None,
        }
    }

    /// A configuration entry's label, by whether the command already has
    /// that configuration: "Assign Hotkey…" or "Change Hotkey…", "Add
    /// Alias…" or "Change Alias…"; "Configure Command…" and "Configure
    /// Extension…" whatever is set. `None` for [`ResultAction::Invoke`],
    /// which is named by the result's own action, and for the quick slot
    /// entries (see [`ResultAction::quick_slot_label`]).
    pub fn configuration_label(self, configured: bool) -> Option<&'static str> {
        match (self, configured) {
            (
                ResultAction::Invoke
                | ResultAction::Pin
                | ResultAction::Unpin
                | ResultAction::MovePinUp
                | ResultAction::MovePinDown,
                _,
            ) => None,
            (ResultAction::Hotkey, false) => Some("Assign Hotkey…"),
            (ResultAction::Hotkey, true) => Some("Change Hotkey…"),
            (ResultAction::Alias, false) => Some("Add Alias…"),
            (ResultAction::Alias, true) => Some("Change Alias…"),
            (ResultAction::ConfigureCommand, _) => Some("Configure Command…"),
            (ResultAction::ConfigureExtension, _) => Some("Configure Extension…"),
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
        // Its preferences, and its package's, on the extension's card.
        let (key, id) = super::choices::split(&row.id);
        let manifest = state
            .packages
            .iter()
            .find(|package| package.identity.key() == key)
            .and_then(|package| package.manifest.as_ref().ok());
        if let Some(manifest) = manifest {
            let own = manifest
                .commands
                .iter()
                .any(|declared| declared.id == id && !declared.preferences.is_empty());
            if own {
                items.push(configuration(ResultAction::ConfigureCommand, false));
            }
            if !manifest.preferences.is_empty() {
                items.push(configuration(ResultAction::ConfigureExtension, false));
            }
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

/// The selected action (see [`Launcher::selected_action`]) for the
/// launcher's current state. The label comes from the selected entry's
/// identity — what activating that row does on that screen — never from a
/// display title; the availability comes from what can run now. Nothing is
/// selected, or the row's action cannot run, and the action is the
/// screen's own, unavailable: the window shows it disabled, and Enter
/// keeps the behavior it has today (nothing, or an explanation) instead of
/// an extension call.
pub(in crate::launcher) fn selected_action(state: &State) -> SelectedAction {
    // An action is already running: the status line reports it, and the
    // definition keeps the button from dispatching another one meanwhile.
    let busy = matches!(state.view.status, Status::Running);
    let acting = |label: &str| SelectedAction {
        label: label.into(),
        available: !busy,
    };
    let unusable = |label: &str| SelectedAction {
        label: label.into(),
        available: false,
    };
    let entry = state
        .view
        .selected
        .and_then(|index| state.entries.get(index));
    match (&state.view.screen, entry) {
        // A form submits: the form's own control keeps the label the
        // extension gave it, but Enter — and the footer's button with it —
        // submits the form.
        (Screen::Form(_), _) => acting("Submit"),
        // A custom view takes the keys itself, and the network and program
        // details screens have only Back: Enter does nothing, so there is
        // no primary action to show.
        (
            Screen::CustomView(_) | Screen::NetworkDetails { .. } | Screen::ProgramDetails { .. },
            _,
        ) => unusable(""),
        // A row is selected: what activating it does is the action.
        // A no-view command runs and opens no screen.
        (_, Some(Entry::Open(opening))) if opening.no_view => acting("Run command"),
        (_, Some(Entry::Open(_))) => acting("Open command"),
        (_, Some(Entry::Send(sending))) => match &sending.unavailable {
            Some(_) => unusable("Unavailable"),
            None => acting("Send query"),
        },
        (_, Some(Entry::Copy(_))) => acting("Copy answer"),
        (_, Some(Entry::OpenUrl(_))) => acting("Open link"),
        (_, Some(Entry::OpenFile { .. })) => acting("Open file"),
        (_, Some(Entry::OpenApplication { .. })) => acting("Open application"),
        (_, Some(Entry::OpenTarget { .. })) => acting("Open link"),
        (_, Some(Entry::Broken(_) | Entry::Unavailable(_))) => unusable("Unavailable"),
        (_, Some(Entry::InstallFromFolder)) => acting("Install from folder"),
        (_, Some(Entry::AskNpm)) => acting("Install from npm"),
        (_, Some(Entry::AskGit)) => acting("Install from Git"),
        (_, Some(Entry::Acquire(_))) => acting("Set up extension"),
        (_, Some(Entry::InstallUpdate)) => acting("Install update"),
        (_, Some(Entry::CheckUpdate)) => acting("Check for update"),
        (_, Some(Entry::Manage)) => acting("Manage extensions"),
        // Pane's Settings row opens the Settings window, exactly as its
        // ellipsis menu entry and the local shortcut do (the window, not
        // the launcher, acts; see [`Launcher::selected_opens_settings`]).
        (_, Some(Entry::Settings)) => acting("Open settings"),
        (_, Some(Entry::Run(_))) => acting("Run item"),
        // An item of a command's list: its primary action, by the title the
        // extension gave it (#137).
        (_, Some(Entry::Actions(listed))) => acting(&listed.primary()),
        (_, Some(Entry::NoActions)) => unusable("No actions"),
        (_, Some(Entry::Form(..))) => acting("Open form"),
        (_, Some(Entry::CustomView(..))) => acting("Open view"),
        (_, Some(Entry::ChooseFolder(_))) => acting("Choose folder"),
        (_, Some(Entry::StopSharingFolder(_))) => acting("Stop sharing"),
        (_, Some(Entry::Install(_, Mode::Install, _))) => acting("Install"),
        (_, Some(Entry::Install(_, Mode::Update(_), _))) => acting("Update"),
        // A confirmation's rows are its answers; the direction a toggle
        // turns in comes from the state it acts on, not from a title.
        (_, Some(Entry::Toggle(identity))) => {
            let enable = state
                .package(identity)
                .is_some_and(|package| !package.enabled);
            acting(if enable { "Enable" } else { "Disable" })
        }
        (_, Some(Entry::ToggleUpdates(None))) => acting(if state.update_controls.automatic {
            "Turn updates off"
        } else {
            "Turn updates on"
        }),
        (_, Some(Entry::ToggleUpdates(Some(identity)))) => {
            let off = state.update_controls.off.contains(&identity.key());
            acting(if off {
                "Turn updates on"
            } else {
                "Turn updates off"
            })
        }
        (_, Some(Entry::Reload(_))) => acting("Reload"),
        (_, Some(Entry::Retry(_))) => acting("Retry"),
        (_, Some(Entry::PauseDetails(_))) => acting("Show details"),
        (_, Some(Entry::NetworkDetails(_))) => acting("Show network use"),
        (_, Some(Entry::ProgramDetails(_))) => acting("Show programs run"),
        (_, Some(Entry::RuntimeDetails)) => acting("Show details"),
        (_, Some(Entry::RestartRuntime)) => acting("Restart runtime"),
        (_, Some(Entry::Develop(_))) => acting("Start developing"),
        (_, Some(Entry::StopDeveloping(_))) => acting("Stop developing"),
        (_, Some(Entry::BuildDetails(_))) => acting("Show details"),
        (_, Some(Entry::BuildAgain(_))) => acting("Build again"),
        (_, Some(Entry::AskClearCache(_))) => acting("Clear cache"),
        (_, Some(Entry::ResetConfirmations(_))) => acting("Reset confirmations"),
        (_, Some(Entry::AskHotkey(_))) => acting("Set hotkey"),
        (_, Some(Entry::RemoveHotkey(_))) => acting("Remove hotkey"),
        (_, Some(Entry::AskAlias(_))) => acting("Set alias"),
        (_, Some(Entry::ToggleFallback(command))) => {
            let fallback = state.aliases.chosen.is_fallback(command);
            acting(if fallback {
                "Stop offering as fallback"
            } else {
                "Offer as fallback"
            })
        }
        (_, Some(Entry::ForgetChoices(_))) => acting("Forget choices"),
        (_, Some(Entry::AskUninstall(_))) => acting("Uninstall"),
        (_, Some(Entry::AskDeleteRetained(_))) => acting("Delete retained data"),
        (_, Some(Entry::Uninstall(_, SavedData::Keep))) => acting("Uninstall"),
        (_, Some(Entry::Uninstall(_, SavedData::Delete))) => acting("Uninstall and delete data"),
        (_, Some(Entry::UninstallAll(_, _, SavedData::Keep))) => acting("Uninstall all"),
        (_, Some(Entry::UninstallAll(_, _, SavedData::Delete))) => {
            acting("Uninstall all and delete data")
        }
        (_, Some(Entry::DeleteRetained(_))) => acting("Delete retained data"),
        (_, Some(Entry::ClearCache(_))) => acting("Clear cache"),
        (_, Some(Entry::DisableAll(..))) => acting("Disable all"),
        (_, Some(Entry::Cancel)) => acting("Cancel"),
        // Nothing is selected: the screen's own action, which cannot run
        // without a row to run it on.
        (Screen::Root { .. }, None) => unusable("Open command"),
        (Screen::Command | Screen::CommandSearch { .. }, None) => unusable("Run item"),
        (Screen::Package { .. }, None) => unusable("Install"),
        (Screen::Extensions { .. }, None) => unusable("Choose"),
        (Screen::Confirm { .. }, None) => unusable("Choose"),
        // The hotkey screen without a row to remove has no primary action:
        // Enter does nothing there; the keys it records are the point.
        (Screen::Hotkey { .. }, None) => unusable(""),
        (Screen::PauseDetails { .. }, None) => unusable("Retry"),
        (Screen::RuntimeDetails { .. }, None) => unusable("Restart"),
        (Screen::BuildDetails { .. }, None) => unusable("Build again"),
    }
}
