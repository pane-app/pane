//! The actions of an item in an open command's list (#137): what Enter,
//! Ctrl+Enter and Ctrl+Shift+Enter run, what the Actions panel lists for
//! the selected item, and which shortcuts run an action from the list.
//!
//! An item carries an ordered list of actions (`docs/list-tree.md`). The
//! first is its **primary action**: Enter and the footer's button run it,
//! and the footer names it. The second is its **secondary action**
//! (Ctrl+Enter) and the third runs with Ctrl+Shift+Enter; a missing one does
//! nothing. Choosing any of them hands its callback id to the command's
//! `handle-event`, shows the answer in the status line and draws the list
//! again, as the primary action always did. An item with no actions (and no
//! form or custom view) cannot be activated, and says so.
//!
//! An action's own shortcut runs it while the list has focus, without the
//! panel, when Pane binds it: modifiers match exactly, a per-system
//! shortcut binds only on its system, and a shortcut that is one of Pane's
//! own effective keys ([`PaneKeys`]: the Keyboard page's bindings with the
//! user's rebinds, Escape, Ctrl+K, the digit chords, Up and Down, the
//! action chords, the pin keys) is never bound. Neither is one held with no
//! Ctrl, Alt or Cmd (other than a function key), which would take a key a
//! search field types or moves with, one the tree gives in a shape Pane
//! cannot read, nor a second action's shortcut that an earlier action of
//! the same item already has. Such an action stays in the panel without
//! its shortcut, and while its package is being developed the status line
//! says which shortcuts Pane did not bind, and why.
//!
//! A held key's repeats and a double click's second click never run an
//! action again: the window ignores them, as it does for root search's
//! quick slots. Calls into one instance still run one after another.
//!
//! An action may open a submenu instead of calling the command back (#140,
//! see `submenus`): Enter, a chord or its shortcut then open the Actions
//! panel at that submenu, which the window does; the launcher runs nothing.
//!
//! The rows Pane lists itself with actions of its own (#150: a file of a
//! granted folder, in root search or Search Files, and a computed answer)
//! have them as an item has, on root search too, and Pane performs them
//! instead of calling a command (see `own_actions`).

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use super::{Entry, Launcher, Screen, State, Status, looks, own_actions, owner};
use crate::icons::Icon;
use crate::keyboard::{Binding, PaneKeys};
use crate::runtime::{Action, ActionStyle, SubmenuEntries};

/// What an action without a title is called.
const UNTITLED: &str = "Run item";

/// What the status line says when an item with no actions is activated.
pub(super) const NO_ACTIONS: &str = "This item has no actions, so it cannot be activated";

/// An item of the open command's list with its actions, as its entry keeps
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Listed {
    /// The item's id.
    pub(super) id: String,
    /// The item's title.
    pub(super) title: String,
    /// Its actions, in order; never empty.
    pub(super) actions: Vec<Action>,
}

impl Listed {
    /// What the primary action is called, as the footer names it.
    pub(super) fn primary(&self) -> String {
        title(&self.actions[0])
    }
}

/// One action of the selected item, as the Actions panel lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemAction {
    /// What it is called.
    pub title: String,
    /// The title of its section in the panel; `None` for an untitled one.
    /// Consecutive actions with the same section are one section.
    pub section: Option<String>,
    /// Whether it is drawn in the destructive style.
    pub destructive: bool,
    /// Its own shortcut, when Pane binds it: shown beside it in the panel,
    /// and it runs the action from the list.
    pub shortcut: Option<Binding>,
    /// Why its shortcut is not bound, when it has one Pane does not bind.
    pub unbound: Option<String>,
    /// Whether choosing it opens a submenu (#140) rather than calling the
    /// command back: the panel draws a chevron beside it.
    pub submenu: bool,
    /// The icon the panel draws beside it (#139), as it is now: a web image
    /// or a system icon once it loaded, its fallback until then (#142).
    pub icon: Option<Icon>,
}

/// The actions of the selected item in an open command's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemActions {
    /// The item's id: the target the panel opened for, which must still be
    /// the selected item when an action runs.
    pub target: String,
    /// The item's title, as the panel's header names it.
    pub title: String,
    /// Its actions, in order: the first is its primary action.
    pub actions: Vec<ItemAction>,
}

impl ItemActions {
    /// The indexes of the actions whose title holds `query`, ignoring case
    /// and the spaces around it; all of them for a blank one. Filtering
    /// flattens the sections: the panel shows what matches as one list.
    pub fn matching(&self, query: &str) -> Vec<usize> {
        matching(&self.actions, query)
    }

    /// The index of the action `binding` runs, if one of them has it bound.
    pub fn bound_to(&self, binding: &Binding) -> Option<usize> {
        bound_to(&self.actions, binding)
    }
}

/// The indexes of `actions` whose title holds `query`, ignoring case and
/// the spaces around it; all of them for a blank one.
pub(super) fn matching(actions: &[ItemAction], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    actions
        .iter()
        .enumerate()
        .filter(|(_, action)| action.title.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

/// The index of the action of `actions` that `binding` runs, if one of them
/// has it bound.
pub(super) fn bound_to(actions: &[ItemAction], binding: &Binding) -> Option<usize> {
    actions
        .iter()
        .position(|action| action.shortcut.as_ref() == Some(binding))
}

/// An action shortcut in the open command's list that Pane does not bind,
/// and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnboundShortcut {
    /// The title of the item the action belongs to.
    pub item: String,
    /// The action's title.
    pub action: String,
    /// The shortcut, as the user reads it ("Ctrl+K"); `None` when the tree
    /// gave one Pane cannot read.
    pub shortcut: Option<String>,
    /// Why it is not bound: "Ctrl+K opens the Actions panel in Pane".
    pub why: String,
}

impl Launcher {
    /// Tells the launcher which keys are Pane's own now: the Keyboard
    /// page's bindings as the user has them and the navigation bindings it
    /// adds. An action shortcut that is one of them is not bound (see the
    /// module docs). The window calls this whenever the bindings may have
    /// changed; until then Pane's default keys apply.
    pub fn set_pane_keys(&self, keys: PaneKeys) {
        let mut state = self.lock();
        if state.pane_keys != keys {
            state.pane_keys = keys;
        }
    }

    /// The actions of the selected item of an open command's list, with
    /// their shortcuts as Pane binds them now; `None` off a command's list,
    /// with nothing selected, or for a row that has no actions of its own
    /// (an item with none, a form, a custom view, a search result, a
    /// folder row). A row Pane lists with actions of its own (a file of a
    /// granted folder, a computed answer; see `own_actions`) has them on
    /// root search too.
    pub fn item_actions(&self) -> Option<ItemActions> {
        let state = self.lock();
        let listed = selected_listed(&state)?;
        let mut actions = item_actions(&listed, &state.pane_keys);
        // Loaded as the panel lists them, where the window draws only the
        // rows in view (#165).
        if looks::loads_as_shown(&state) {
            looks::want_action_icons(&state, &listed.actions);
        }
        actions.actions = looks::with_action_icons(&state, &listed.actions, actions.actions);
        Some(actions)
    }

    /// The shortcuts of the open command's list that Pane does not bind,
    /// and why, item by item in order.
    pub fn unbound_shortcuts(&self) -> Vec<UnboundShortcut> {
        let state = self.lock();
        unbound(&state)
    }

    /// Runs the action at `index` of the item `target`, as the Actions
    /// panel chooses it: only if `target` is still the selected item and it
    /// has that action, one that calls the command back (one that opens a
    /// submenu is [`Launcher::open_submenu`]'s). Await the returned future
    /// to apply the answer; nothing happens otherwise.
    pub fn run_item_action(
        &self,
        target: &str,
        index: usize,
    ) -> impl Future<Output = ()> + Send + 'static {
        let state = self.lock();
        let chosen = selected_listed(&state)
            .filter(|listed| listed.id == target)
            .and_then(|listed| callback_of(listed.actions.get(index)?));
        self.run_chosen(state, chosen)
    }

    /// Runs the action at `index` of the selected item: 1 for Ctrl+Enter
    /// (the secondary action), 2 for Ctrl+Shift+Enter, or the index of the
    /// action a shortcut runs ([`ItemActions::bound_to`]). Nothing happens
    /// for an action the item does not have, or one that opens a submenu
    /// (the window opens the Actions panel at it). (The primary action is
    /// [`Launcher::activate_selected`]'s, as Enter's.)
    pub fn run_selected_action(&self, index: usize) -> impl Future<Output = ()> + Send + 'static {
        let state = self.lock();
        let chosen =
            selected_listed(&state).and_then(|listed| callback_of(listed.actions.get(index)?));
        self.run_chosen(state, chosen)
    }

    /// Runs `chosen`, an action's callback and title, of the selected row:
    /// one of Pane's own on a row Pane acts on itself (see `own_actions`),
    /// else the open command's, as [`Launcher::run_callback`] does.
    pub(super) fn run_chosen(
        &self,
        mut state: std::sync::MutexGuard<'_, State>,
        chosen: Option<(String, String)>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        if let Some(own) = own_actions::selected(&state) {
            let work =
                chosen.and_then(|(callback, title)| own_actions::work(own, &callback, &title));
            if let Some(work) = &work {
                own_actions::begin(&mut state, work);
            }
            let epoch = state.screen_epoch;
            drop(state);
            let launcher = self.clone();
            return Box::pin(async move {
                if let Some(work) = work {
                    launcher.do_own(epoch, work).await;
                }
            });
        }
        Box::pin(self.run_callback(state, chosen.map(|(callback, _)| callback)))
    }

    /// Has the open command handle `callback`, if there is one, the way
    /// activating a row runs an action.
    pub(super) fn run_callback(
        &self,
        mut state: std::sync::MutexGuard<'_, State>,
        callback: Option<String>,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut run = None;
        if let (Some(callback), Some(component)) = (callback, state.open.clone()) {
            // The status line is about this action from now on.
            state.sent_from = None;
            state.view.status = Status::Running {
                since: Instant::now(),
            };
            let data = self.data_in(&state, &component);
            run = Some((state.screen_epoch, component, callback, data));
        }
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some((epoch, component, callback, data)) = run {
                launcher.run_action(epoch, component, callback, data).await;
            }
        }
    }

    /// Notes the open command's unbound shortcuts after its list was drawn,
    /// and, while its package is being developed, says in the status line
    /// which are not bound and why, when that changed since it last said.
    pub(super) fn report_unbound(&self, state: &mut State) {
        let unbound = unbound(state);
        let developed = state
            .open
            .as_ref()
            .and_then(|component| owner(&state.packages, component))
            .is_some_and(|package| self.is_developed(&package.identity));
        if developed && !unbound.is_empty() && unbound != state.reported_unbound {
            state.view.status = Status::Error(describe(&unbound));
        }
        state.reported_unbound = unbound;
    }
}

/// The callback and the title of `action`, when it calls back rather than
/// open a submenu.
fn callback_of(action: &Action) -> Option<(String, String)> {
    Some((action.callback()?.to_owned(), title(action)))
}

/// The selected row's item with its actions: an item of an open command's
/// list, or a row Pane lists with actions of its own (see `own_actions`),
/// on root search too.
pub(super) fn selected_listed(state: &State) -> Option<Cow<'_, Listed>> {
    let index = state.view.selected?;
    if let Some(own) = own_actions::listed(state, index) {
        return Some(Cow::Owned(own));
    }
    if !matches!(
        state.view.screen,
        Screen::Command | Screen::CommandSearch { .. }
    ) {
        return None;
    }
    match state.entries.get(index)? {
        Entry::Actions(listed) => Some(Cow::Borrowed(listed)),
        _ => None,
    }
}

/// What action `action` is called.
pub(super) fn title(action: &Action) -> String {
    action
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| UNTITLED.to_owned())
}

/// `listed`'s actions, their shortcuts bound against `keys`.
fn item_actions(listed: &Listed, keys: &PaneKeys) -> ItemActions {
    ItemActions {
        target: listed.id.clone(),
        title: listed.title.clone(),
        actions: bind(&listed.actions, keys),
    }
}

/// `actions`, an item's or one submenu's entries, as the Actions panel
/// lists them: their shortcuts bound against `keys`, a shortcut an earlier
/// one of them already has left unbound.
pub(super) fn bind(actions: &[Action], keys: &PaneKeys) -> Vec<ItemAction> {
    let mut bound: Vec<(Binding, String)> = Vec::new();
    actions
        .iter()
        .map(|action| {
            let title = title(action);
            let (shortcut, unbound) = match &action.shortcut {
                None => (None, None),
                Some(Err(why)) => (None, Some(why.clone())),
                Some(Ok(binding)) => {
                    if let Some(does) = keys.taken(binding) {
                        (None, Some(format!("{binding} {does} in Pane")))
                    } else if types(binding) {
                        (
                            None,
                            Some(format!(
                                "{binding} has no Ctrl, Alt or Cmd, so it would take a key a \
                                 search field types or moves with"
                            )),
                        )
                    } else if let Some((_, first)) = bound.iter().find(|(held, _)| held == binding)
                    {
                        (None, Some(format!("{binding} already runs “{first}”")))
                    } else {
                        bound.push((binding.clone(), title.clone()));
                        (Some(binding.clone()), None)
                    }
                }
            };
            ItemAction {
                section: action.section.clone(),
                destructive: action.style == ActionStyle::Destructive,
                title,
                shortcut,
                unbound,
                submenu: action.submenu().is_some(),
                // Set by `looks::with_action_icons` where the panel lists
                // them.
                icon: None,
            }
        })
        .collect()
}

/// Whether `binding` is a key a search field types or moves with: one held
/// with no modifier but Shift, other than a function key.
pub(super) fn types(binding: &Binding) -> bool {
    let (control, alt, _shift, platform, function) = binding.modifiers();
    let key = binding.key();
    let function_key =
        key.len() > 1 && key.starts_with('f') && key[1..].bytes().all(|byte| byte.is_ascii_digit());
    !(control || alt || platform || function || function_key)
}

/// The unbound shortcuts of the list on screen, item by item.
fn unbound(state: &State) -> Vec<UnboundShortcut> {
    if !matches!(
        state.view.screen,
        Screen::Command | Screen::CommandSearch { .. }
    ) {
        return Vec::new();
    }
    state
        .entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Actions(listed) => Some(listed),
            _ => None,
        })
        .flat_map(|listed| {
            let mut found = Vec::new();
            unbound_in(
                &listed.title,
                "",
                &listed.actions,
                &state.pane_keys,
                &mut found,
            );
            found
        })
        .collect()
}

/// Adds the unbound shortcuts of `actions`, the item `item`'s actions or
/// the entries of one of its submenus given at once (named after `path`,
/// "Open With… › "), to `found`, in order, those of each submenu after the
/// action that opens it. A submenu the command is asked for when it opens
/// has no entries to look at until then.
fn unbound_in(
    item: &str,
    path: &str,
    actions: &[Action],
    keys: &PaneKeys,
    found: &mut Vec<UnboundShortcut>,
) {
    for (given, action) in actions.iter().zip(bind(actions, keys)) {
        let named = format!("{path}{}", action.title);
        if let Some(why) = action.unbound {
            found.push(UnboundShortcut {
                item: item.to_owned(),
                action: named.clone(),
                shortcut: given
                    .shortcut
                    .as_ref()
                    .and_then(|shortcut| shortcut.as_ref().ok())
                    .map(Binding::to_string),
                why,
            });
        }
        if let Some(submenu) = given.submenu()
            && let SubmenuEntries::Given(entries) = &submenu.entries
        {
            unbound_in(item, &format!("{named} › "), entries, keys, found);
        }
    }
}

/// The status line's report of `unbound`, for a developed package.
fn describe(unbound: &[UnboundShortcut]) -> String {
    let each: Vec<String> = unbound
        .iter()
        .map(|shortcut| {
            format!(
                "“{}” of “{}”: {}",
                shortcut.action, shortcut.item, shortcut.why
            )
        })
        .collect();
    format!("Shortcuts Pane did not bind: {}", each.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(title: &str, shortcut: Option<&str>) -> Action {
        Action {
            title: Some(title.into()),
            kind: crate::runtime::ActionKind::Callback(title.into()),
            section: None,
            style: ActionStyle::Default,
            shortcut: shortcut.map(Binding::parse),
            icon: None,
        }
    }

    #[test]
    fn panes_keys_and_an_earlier_actions_shortcut_are_not_bound() {
        let listed = Listed {
            id: "a".into(),
            title: "A".into(),
            actions: vec![
                action("Open", None),
                action("Copy", Some("ctrl-shift-c")),
                action("Menu", Some("ctrl-k")),
                action("Again", Some("ctrl-shift-c")),
            ],
        };
        let actions = item_actions(&listed, &PaneKeys::default());
        let shortcuts: Vec<_> = actions
            .actions
            .iter()
            .map(|action| action.shortcut.as_ref().map(Binding::id))
            .collect();
        assert_eq!(
            shortcuts,
            [None, Some("ctrl-shift-c".to_owned()), None, None]
        );
        assert!(
            actions.actions[2]
                .unbound
                .as_ref()
                .unwrap()
                .contains("in Pane")
        );
        assert!(
            actions.actions[3]
                .unbound
                .as_ref()
                .unwrap()
                .contains("already runs “Copy”")
        );
        assert_eq!(
            actions.bound_to(&Binding::parse("ctrl-shift-c").unwrap()),
            Some(1)
        );
        assert_eq!(actions.bound_to(&Binding::parse("ctrl-c").unwrap()), None);
    }

    #[test]
    fn a_shortcut_without_ctrl_alt_or_cmd_is_not_bound_unless_a_function_key() {
        let listed = Listed {
            id: "a".into(),
            title: "A".into(),
            actions: vec![
                action("Open", None),
                action("Type", Some("shift-x")),
                action("Help", Some("f1")),
                action("Find", Some("alt-f")),
            ],
        };
        let actions = item_actions(&listed, &PaneKeys::default());
        assert_eq!(actions.actions[1].shortcut, None);
        assert!(actions.actions[1].unbound.is_some());
        assert_eq!(actions.actions[2].shortcut, Binding::parse("f1").ok());
        assert_eq!(actions.actions[3].shortcut, Binding::parse("alt-f").ok());
    }

    #[test]
    fn filtering_matches_titles_ignoring_case() {
        let listed = Listed {
            id: "a".into(),
            title: "A".into(),
            actions: vec![action("Open", None), action("Copy Link", None)],
        };
        let actions = item_actions(&listed, &PaneKeys::default());
        assert_eq!(actions.matching(" link "), [1]);
        assert_eq!(actions.matching(""), [0, 1]);
        assert!(actions.matching("zzz").is_empty());
    }

    #[test]
    fn an_untitled_action_is_called_run_item() {
        let mut untitled = action("", None);
        untitled.title = None;
        assert_eq!(title(&untitled), UNTITLED);
    }
}
